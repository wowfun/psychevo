from __future__ import annotations

import asyncio
import inspect
from collections.abc import Awaitable, Callable, Coroutine, Mapping, Sequence
from typing import Any

from ._callbacks import (
    ApprovalDecision,
    ApprovalHandler,
    ApprovalRequest,
    ClarifyHandler,
    ClarifyRequest,
    FilesystemApprovalRequest,
    McpStartupApprovalRequest,
    Tool,
    ToolCall,
    ToolResult,
)
from ._protocol import _RpcCallback
from .errors import ProtocolError, TransportError

_DEFAULT_CALLBACK_WORKERS = 8
_DEFAULT_CALLBACK_BACKLOG = 64


class _CallbackRuntime:
    def __init__(
        self,
        *,
        send: Callable[[dict[str, object]], Awaitable[None]],
        request: Callable[[str, object], Awaitable[object]],
        transition_terminal: Callable[[TransportError], None],
        tools: Sequence[Tool],
        approval_handler: ApprovalHandler | None,
        clarify_handler: ClarifyHandler | None,
        workers: int = _DEFAULT_CALLBACK_WORKERS,
        backlog: int = _DEFAULT_CALLBACK_BACKLOG,
    ) -> None:
        if workers <= 0:
            raise ValueError("callback_workers must be greater than zero")
        if backlog <= 0:
            raise ValueError("callback_backlog must be greater than zero")
        self._send = send
        self._request = request
        self._transition_terminal = transition_terminal
        self._worker_count = workers
        self._queue: asyncio.Queue[Coroutine[object, object, None]] = asyncio.Queue(
            maxsize=backlog
        )
        self._workers: set[asyncio.Task[None]] = set()
        self._closed = False
        self._tools = {tool.name: tool for tool in tools}
        if len(self._tools) != len(tools):
            raise ValueError("custom Tool names must be unique")
        self._approval_handler = approval_handler
        self._clarify_handler = clarify_handler

    @property
    def active_workers(self) -> int:
        return len(self._workers)

    @property
    def backlog_capacity(self) -> int:
        return self._queue.maxsize

    @property
    def queued(self) -> int:
        return self._queue.qsize()

    def has_registrations(self) -> bool:
        return bool(
            self._tools
            or self._approval_handler is not None
            or self._clarify_handler is not None
        )

    def registration_params(self) -> dict[str, object]:
        return {
            "tools": [tool.to_wire() for tool in self._tools.values()],
            "approvalHandler": self._approval_handler is not None,
            "clarifyHandler": self._clarify_handler is not None,
        }

    async def queue_request(self, callback: _RpcCallback) -> None:
        coroutine = self._handle_request(callback)
        self._ensure_workers()
        try:
            self._queue.put_nowait(coroutine)
        except asyncio.QueueFull:
            coroutine.close()
            await self._send(
                {
                    "jsonrpc": "2.0",
                    "id": callback.request_id,
                    "error": {
                        "code": -32001,
                        "message": "Python SDK callback queue is overloaded",
                    },
                }
            )

    def maybe_handle_clarify(
        self,
        thread_id: str,
        turn_id: str,
        event: Mapping[str, object],
    ) -> None:
        if (
            event.get("type") == "interaction_requested"
            and event.get("kind") == "clarify"
            and self._clarify_handler is not None
        ):
            interaction_id = event.get("interactionId")
            if isinstance(interaction_id, str):
                self.submit(
                    self._handle_clarify(
                        thread_id,
                        turn_id,
                        interaction_id,
                        event.get("payload"),
                    )
                )

    def submit(self, coroutine: object) -> bool:
        if not inspect.iscoroutine(coroutine):
            raise TypeError("callback work must be a coroutine")
        self._ensure_workers()
        try:
            self._queue.put_nowait(coroutine)
            return True
        except asyncio.QueueFull:
            coroutine.close()
            asyncio.get_running_loop().call_exception_handler(
                {
                    "message": (
                        "Psychevo callback notification dropped: "
                        "callback queue overloaded"
                    ),
                    "exception": ProtocolError(
                        -32001,
                        "Python SDK callback queue is overloaded",
                    ),
                }
            )
            return False

    def cancel(self) -> None:
        self._closed = True
        current_task = asyncio.current_task()
        for task in self._workers:
            if task is not current_task:
                task.cancel()
        self._discard_queued()

    async def wait_closed(self) -> None:
        if self._workers:
            await asyncio.gather(*self._workers, return_exceptions=True)

    def _ensure_workers(self) -> None:
        if self._closed:
            return
        while len(self._workers) < self._worker_count:
            task = asyncio.create_task(self._worker())
            self._workers.add(task)
            task.add_done_callback(self._workers.discard)

    async def _worker(self) -> None:
        while True:
            work = await self._queue.get()
            try:
                await work
            except asyncio.CancelledError:
                work.close()
                raise
            except BaseException as error:
                transport_error = (
                    error
                    if isinstance(error, TransportError)
                    else TransportError(
                        f"App Server callback worker failed: {error}"
                    )
                )
                self._transition_terminal(transport_error)
                asyncio.get_running_loop().call_exception_handler(
                    {
                        "message": "Psychevo callback worker failed",
                        "exception": transport_error,
                    }
                )
            finally:
                self._queue.task_done()
            if self._closed:
                return

    def _discard_queued(self) -> None:
        while True:
            try:
                work = self._queue.get_nowait()
            except asyncio.QueueEmpty:
                return
            work.close()
            self._queue.task_done()

    async def _handle_request(self, callback: _RpcCallback) -> None:
        try:
            if callback.method == "tool/call":
                result = await self._call_tool(_callback_params(callback.params))
            elif callback.method == "approval/request":
                result = await self._call_approval(_callback_params(callback.params))
            else:
                raise ProtocolError(
                    -32601,
                    f"callback method not found: {callback.method}",
                )
            response: dict[str, object] = {
                "jsonrpc": "2.0",
                "id": callback.request_id,
                "result": result,
            }
        except BaseException as error:
            response = {
                "jsonrpc": "2.0",
                "id": callback.request_id,
                "error": {
                    "code": error.code if isinstance(error, ProtocolError) else -32000,
                    "message": str(error),
                },
            }
        await self._send(response)

    async def _call_tool(self, params: dict[str, Any]) -> dict[str, object]:
        name = params.get("toolName")
        if not isinstance(name, str) or name not in self._tools:
            raise ProtocolError(-32602, f"unknown custom Tool: {name}")
        call = ToolCall(
            call_id=_string(params, "callId"),
            tool_name=name,
            arguments=params.get("arguments"),
            thread_id=_string(params, "threadId"),
            turn_id=_string(params, "turnId"),
        )
        result = self._tools[name].handler(call)
        if not inspect.isawaitable(result):
            raise TypeError("custom Tool handler must be async")
        value = await result
        if isinstance(value, ToolResult):
            return {
                "result": value.result,
                "modelContent": value.model_content,
                "isError": value.is_error,
            }
        return {"result": value, "modelContent": None, "isError": False}

    async def _call_approval(self, params: dict[str, Any]) -> dict[str, object]:
        if self._approval_handler is None:
            return ApprovalDecision("deny").to_wire()
        request = ApprovalRequest(
            call_id=_string(params, "callId"),
            thread_id=_string(params, "threadId"),
            turn_id=_string(params, "turnId"),
            tool_call_id=_string(params, "toolCallId"),
            tool_name=_string(params, "toolName"),
            summary=_string(params, "summary"),
            reason=_string(params, "reason"),
            matched_rule=_optional_string(params, "matchedRule"),
            suggested_rule=_optional_string(params, "suggestedRule"),
            allow_always=params.get("allowAlways") is True,
            filesystem=(
                FilesystemApprovalRequest.from_wire(params["filesystem"])
                if params.get("filesystem") is not None
                else None
            ),
            mcp_startup=(
                McpStartupApprovalRequest.from_wire(params["mcpStartup"])
                if params.get("mcpStartup") is not None
                else None
            ),
        )
        result = self._approval_handler(request)
        if not inspect.isawaitable(result):
            raise TypeError("approval handler must be async")
        decision = await result
        if not isinstance(decision, ApprovalDecision):
            raise TypeError("approval handler must return ApprovalDecision")
        return decision.to_wire()

    async def _handle_clarify(
        self,
        thread_id: str,
        turn_id: str,
        interaction_id: str,
        questions: object,
    ) -> None:
        if self._clarify_handler is None:
            return
        result = self._clarify_handler(
            ClarifyRequest(
                interaction_id=interaction_id,
                thread_id=thread_id,
                turn_id=turn_id,
                questions=questions,  # type: ignore[arg-type]
            )
        )
        if not inspect.isawaitable(result):
            return
        answers = await result
        if answers is None:
            return
        await self._request(
            "interaction/respond",
            {
                "turnId": turn_id,
                "interactionId": interaction_id,
                "response": {
                    "kind": "clarify",
                    "answers": [list(answer) for answer in answers],
                },
            },
        )


def _callback_params(value: object) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ProtocolError(-32602, "callback params must be an object")
    return value


def _string(value: Mapping[str, object], key: str) -> str:
    field = value.get(key)
    if not isinstance(field, str):
        raise ProtocolError(-32602, f"{key} must be a string")
    return field


def _optional_string(value: Mapping[str, object], key: str) -> str | None:
    field = value.get(key)
    if field is None or isinstance(field, str):
        return field
    raise ProtocolError(-32602, f"{key} must be a string or null")
