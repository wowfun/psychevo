from __future__ import annotations

import asyncio
import inspect
import math
import os
from collections.abc import AsyncIterator, Coroutine, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, cast
from uuid import uuid4

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
from ._transport import StdioTransport, Transport, WebSocketTransport
from ._types import (
    CompactionResult,
    ThreadPage,
    ThreadSnapshot,
    ThreadSummary,
    TurnEvent,
    TurnReceipt,
    TurnResult,
)
from .errors import ProtocolError, RequestTimeoutError, TransportError

_PROTOCOL_VERSION = 1
_EVENT_CAPACITY = 256
_EVENT_END = object()
_DEFAULT_REQUEST_TIMEOUT = 30.0
_DEFAULT_CLOSE_TIMEOUT = 10.0
_DEFAULT_CALLBACK_WORKERS = 8
_DEFAULT_CALLBACK_BACKLOG = 64
_USE_DEFAULT_TIMEOUT = object()


@dataclass(slots=True)
class _PendingRequest:
    method: str
    future: asyncio.Future[object]
    decoder: Callable[[object], object]


class _RpcClient:
    def __init__(
        self,
        transport: Transport,
        *,
        tools: Sequence[Tool],
        approval_handler: ApprovalHandler | None,
        clarify_handler: ClarifyHandler | None,
        request_timeout: float | None = _DEFAULT_REQUEST_TIMEOUT,
        callback_workers: int = _DEFAULT_CALLBACK_WORKERS,
        callback_backlog: int = _DEFAULT_CALLBACK_BACKLOG,
    ) -> None:
        _validate_timeout("request_timeout", request_timeout, allow_none=True)
        if callback_workers <= 0:
            raise ValueError("callback_workers must be greater than zero")
        if callback_backlog <= 0:
            raise ValueError("callback_backlog must be greater than zero")
        self._transport = transport
        self._request_timeout = request_timeout
        self._next_id = 0
        self._pending: dict[int, _PendingRequest] = {}
        self._turns: dict[str, TurnHandle] = {}
        self._reader: asyncio.Task[None] | None = None
        self._callback_worker_count = callback_workers
        self._callback_queue: asyncio.Queue[
            Coroutine[object, object, None] | None
        ] = asyncio.Queue(maxsize=callback_backlog)
        self._callbacks: set[asyncio.Task[None]] = set()
        self._terminal_error: TransportError | None = None
        self._tools = {tool.name: tool for tool in tools}
        if len(self._tools) != len(tools):
            raise ValueError("custom Tool names must be unique")
        self._approval_handler = approval_handler
        self._clarify_handler = clarify_handler

    async def start(self) -> None:
        self._reader = asyncio.create_task(self._read_loop())
        result = await self.request(
            "initialize",
            {
                "client": {"name": "psychevo-python", "version": _sdk_version()},
                "protocolMin": _PROTOCOL_VERSION,
                "protocolMax": _PROTOCOL_VERSION,
                "capabilities": {},
            },
        )
        if not isinstance(result, dict) or result.get("protocolVersion") != _PROTOCOL_VERSION:
            raise TransportError("App Server selected an unexpected protocol version")
        await self.notify("initialized", {})
        if (
            self._tools
            or self._approval_handler is not None
            or self._clarify_handler is not None
        ):
            await self.request(
                "tool/register",
                {
                    "tools": [tool.to_wire() for tool in self._tools.values()],
                    "approvalHandler": self._approval_handler is not None,
                    "clarifyHandler": self._clarify_handler is not None,
                },
            )

    async def request(
        self,
        method: str,
        params: object = None,
        *,
        timeout: float | None | object = _USE_DEFAULT_TIMEOUT,
    ) -> object:
        self._raise_if_terminal()
        effective_timeout = (
            self._request_timeout
            if timeout is _USE_DEFAULT_TIMEOUT
            else timeout
        )
        if effective_timeout is not None and not isinstance(
            effective_timeout, (int, float)
        ):
            raise TypeError("timeout must be a number or None")
        normalized_timeout = (
            None if effective_timeout is None else float(effective_timeout)
        )
        _validate_timeout("timeout", normalized_timeout, allow_none=True)
        self._next_id += 1
        request_id = self._next_id
        loop = asyncio.get_running_loop()
        future: asyncio.Future[object] = loop.create_future()
        self._pending[request_id] = _PendingRequest(
            method=method,
            future=future,
            decoder=_result_decoder(method),
        )
        message: dict[str, object] = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
        }
        if params is not None:
            message["params"] = params
        delivery_unknown = False
        try:
            async def send_and_wait() -> object:
                nonlocal delivery_unknown
                self._raise_if_terminal()
                delivery_unknown = True
                await self._send(message)
                return await future

            if normalized_timeout is None:
                return await send_and_wait()
            try:
                async with asyncio.timeout(normalized_timeout):
                    return await send_and_wait()
            except TimeoutError as error:
                if not future.done():
                    future.cancel()
                raise RequestTimeoutError(
                    method,
                    normalized_timeout,
                    delivery_unknown,
                ) from error
        except BaseException:
            if future.done() and not future.cancelled():
                future.exception()
            raise
        finally:
            self._pending.pop(request_id, None)

    async def notify(self, method: str, params: object = None) -> None:
        self._raise_if_terminal()
        message: dict[str, object] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            message["params"] = params
        self._raise_if_terminal()
        await self._send(message)

    def register_turn(self, turn: TurnHandle) -> None:
        turn_id = turn.receipt.turn_id
        if turn_id in self._turns:
            raise ProtocolError(-32602, f"Turn event sink is already registered: {turn_id}")
        self._turns[turn_id] = turn

    def unregister_turn(self, turn: TurnHandle) -> None:
        if self._turns.get(turn.receipt.turn_id) is turn:
            self._turns.pop(turn.receipt.turn_id, None)

    async def close(self) -> None:
        self._transition_terminal(TransportError("App Server connection closed"))
        if self._reader is not None:
            try:
                await self._reader
            except asyncio.CancelledError:
                pass
        if self._callbacks:
            await asyncio.gather(*self._callbacks, return_exceptions=True)
        await self._transport.close()

    def abort(self) -> None:
        self._transition_terminal(TransportError("App Server close deadline exceeded"))
        self._transport.abort()

    async def _read_loop(self) -> None:
        try:
            while True:
                message = await self._transport.receive()
                await self._receive_message(message)
        except asyncio.CancelledError:
            raise
        except BaseException as error:
            transport_error = (
                error
                if isinstance(error, TransportError)
                else TransportError(f"App Server reader failed: {error}")
            )
            self._transition_terminal(transport_error)

    def _raise_if_terminal(self) -> None:
        if self._terminal_error is not None:
            raise self._terminal_error

    async def _send(self, message: dict[str, object]) -> None:
        self._raise_if_terminal()
        try:
            await self._transport.send(message)
        except BaseException as error:
            transport_error = (
                error
                if isinstance(error, TransportError)
                else TransportError(f"App Server send failed: {error}")
            )
            self._transition_terminal(transport_error)
            raise transport_error

    def _transition_terminal(self, error: TransportError) -> None:
        if self._terminal_error is not None:
            return
        self._terminal_error = error
        for pending in self._pending.values():
            if not pending.future.done():
                pending.future.set_exception(error)
        for turn in self._turns.values():
            turn._close_events()
        current_task = asyncio.current_task()
        if self._reader is not None and self._reader is not current_task:
            self._reader.cancel()
        for task in self._callbacks:
            if task is not current_task:
                task.cancel()
        self._discard_queued_callbacks()
        self._turns.clear()

    def _forget_turn(self, turn_id: str) -> None:
        self._turns.pop(turn_id, None)

    async def _receive_message(self, message: dict[str, object]) -> None:
        if message.get("jsonrpc") != "2.0":
            raise TransportError('App Server message requires jsonrpc exactly "2.0"')
        has_method = "method" in message
        has_id = "id" in message
        if has_method:
            if "result" in message or "error" in message:
                raise TransportError(
                    "App Server request or notification cannot contain result or error"
                )
            method = message.get("method")
            if not isinstance(method, str):
                raise TransportError("App Server method must be a string")
            if "params" in message and not isinstance(
                message.get("params"), (dict, list)
            ):
                raise TransportError(
                    "App Server request or notification params must be an object or array"
                )
            if has_id:
                if not isinstance(message.get("id"), str):
                    raise TransportError("App Server callback id must be a string")
                await self._queue_callback_request(message)
            else:
                self._receive_notification(message)
            return
        if not has_id:
            raise TransportError("App Server message is not a JSON-RPC envelope")
        self._receive_response(message)

    def _receive_response(self, message: dict[str, object]) -> None:
        request_id = message.get("id")
        if type(request_id) is not int:
            raise TransportError("App Server response id must be an integer")
        has_result = "result" in message
        has_error = "error" in message
        if has_result == has_error:
            raise TransportError(
                "App Server response must contain exactly one of result or error"
            )
        protocol_error = (
            _decode_rpc_error(message["error"]) if has_error else None
        )
        pending = self._pending.get(request_id)
        if pending is None or pending.future.done():
            return
        if protocol_error is not None:
            pending.future.set_exception(protocol_error)
            return
        try:
            result = pending.decoder(message["result"])
        except Exception as error:
            if isinstance(error, TransportError):
                raise
            raise TransportError(
                f"{pending.method} returned an invalid result: {error}"
            ) from error
        pending.future.set_result(result)

    def _receive_notification(self, message: dict[str, object]) -> None:
        method = message["method"]
        if method == "server/error":
            error = _decode_rpc_error(message.get("params"))
            raise TransportError(
                f"App Server reported error {error.code}: {error}"
            )
        if method != "turn/event":
            return
        params = message.get("params")
        if not isinstance(params, dict):
            raise TransportError("turn/event params must be an object")
        thread_id = params.get("threadId")
        turn_id = params.get("turnId")
        event = params.get("event")
        if (
            not isinstance(thread_id, str)
            or not isinstance(turn_id, str)
            or not isinstance(event, dict)
        ):
            raise TransportError(
                "turn/event requires string threadId, string turnId, and object event"
            )
        _validate_turn_event(event, thread_id, turn_id)
        turn = self._turns.get(turn_id)
        if turn is None:
            return
        turn._receive_event(event)
        self._maybe_handle_clarify(thread_id, turn_id, event)
        if _terminal_event(event):
            self._forget_turn(turn_id)

    def _maybe_handle_clarify(
        self, thread_id: str, turn_id: str, event: Mapping[str, object]
    ) -> None:
        if (
            event.get("type") == "interaction_requested"
            and event.get("kind") == "clarify"
            and self._clarify_handler is not None
        ):
            interaction_id = event.get("interactionId")
            if isinstance(interaction_id, str):
                self._spawn_callback(
                    self._handle_clarify(
                        thread_id,
                        turn_id,
                        interaction_id,
                        event.get("payload"),
                    )
                )

    def _ensure_callback_workers(self) -> None:
        while len(self._callbacks) < self._callback_worker_count:
            task = asyncio.create_task(self._callback_worker())
            self._callbacks.add(task)
            task.add_done_callback(self._callbacks.discard)

    def _spawn_callback(self, coroutine: object) -> bool:
        if not inspect.iscoroutine(coroutine):
            raise TypeError("callback work must be a coroutine")
        self._ensure_callback_workers()
        try:
            self._callback_queue.put_nowait(coroutine)
            return True
        except asyncio.QueueFull:
            coroutine.close()
            asyncio.get_running_loop().call_exception_handler(
                {
                    "message": "Psychevo callback notification dropped: callback queue overloaded",
                    "exception": ProtocolError(
                        -32001,
                        "Python SDK callback queue is overloaded",
                    ),
                }
            )
            return False

    async def _queue_callback_request(self, message: dict[str, object]) -> None:
        coroutine = self._handle_callback_request(message)
        self._ensure_callback_workers()
        try:
            self._callback_queue.put_nowait(coroutine)
        except asyncio.QueueFull:
            coroutine.close()
            await self._send(
                {
                    "jsonrpc": "2.0",
                    "id": message.get("id"),
                    "error": {
                        "code": -32001,
                        "message": "Python SDK callback queue is overloaded",
                    },
                }
            )

    async def _callback_worker(self) -> None:
        while True:
            work = await self._callback_queue.get()
            try:
                if work is None:
                    return
                await work
            except asyncio.CancelledError:
                if work is not None:
                    work.close()
                raise
            except BaseException as error:
                transport_error = (
                    error
                    if isinstance(error, TransportError)
                    else TransportError(f"App Server callback worker failed: {error}")
                )
                self._transition_terminal(transport_error)
                asyncio.get_running_loop().call_exception_handler(
                    {
                        "message": "Psychevo callback worker failed",
                        "exception": transport_error,
                    }
                )
            finally:
                self._callback_queue.task_done()
            if self._terminal_error is not None:
                return

    def _discard_queued_callbacks(self) -> None:
        while True:
            try:
                work = self._callback_queue.get_nowait()
            except asyncio.QueueEmpty:
                return
            if work is not None:
                work.close()
            self._callback_queue.task_done()

    async def _handle_callback_request(self, message: dict[str, object]) -> None:
        request_id = message.get("id")
        method = message.get("method")
        params = message.get("params")
        assert isinstance(request_id, str)
        assert isinstance(method, str)
        try:
            if method == "tool/call":
                result = await self._call_tool(_callback_params(params))
            elif method == "approval/request":
                result = await self._call_approval(_callback_params(params))
            else:
                raise ProtocolError(-32601, f"callback method not found: {method}")
            response: dict[str, object] = {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": result,
            }
        except BaseException as error:
            response = {
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {
                    "code": error.code
                    if isinstance(error, ProtocolError)
                    else -32000,
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
        await self.request(
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


class Client:
    def __init__(
        self,
        *,
        executable: os.PathLike[str] | str | None = None,
        executable_args: Sequence[str] = (),
        remote_url: str | None = None,
        token: str | None = None,
        tools: Sequence[Tool] = (),
        approval_handler: ApprovalHandler | None = None,
        clarify_handler: ClarifyHandler | None = None,
        request_timeout: float | None = _DEFAULT_REQUEST_TIMEOUT,
        close_timeout: float = _DEFAULT_CLOSE_TIMEOUT,
    ) -> None:
        if executable is not None and remote_url is not None:
            raise ValueError("executable and remote_url are mutually exclusive")
        if remote_url is not None and not token:
            raise ValueError("remote_url requires an explicit bearer token")
        _validate_timeout("request_timeout", request_timeout, allow_none=True)
        _validate_timeout("close_timeout", close_timeout, allow_none=False)
        self._executable = executable
        self._executable_args = tuple(executable_args)
        self._remote_url = remote_url
        self._token = token
        self._tools = tuple(tools)
        self._approval_handler = approval_handler
        self._clarify_handler = clarify_handler
        self._request_timeout = request_timeout
        self._close_timeout = close_timeout
        self._rpc: _RpcClient | None = None
        self._local = remote_url is None

    async def __aenter__(self) -> Client:
        await self.connect()
        return self

    async def __aexit__(self, *_: object) -> None:
        await self.close()

    async def connect(self) -> None:
        if self._rpc is not None:
            return
        if self._remote_url is not None:
            transport: Transport = await WebSocketTransport.connect(
                self._remote_url, self._token or ""
            )
        else:
            executable = self._executable or _bundled_app_server()
            transport = await StdioTransport.start(executable, self._executable_args)
        rpc = _RpcClient(
            transport,
            tools=self._tools,
            approval_handler=self._approval_handler,
            clarify_handler=self._clarify_handler,
            request_timeout=self._request_timeout,
        )
        try:
            await rpc.start()
        except BaseException:
            await self._close_rpc(rpc, shutdown=False)
            raise
        self._rpc = rpc

    async def close(self) -> None:
        rpc, self._rpc = self._rpc, None
        if rpc is None:
            return
        await self._close_rpc(rpc, shutdown=self._local)

    async def _close_rpc(self, rpc: _RpcClient, *, shutdown: bool) -> None:
        async def graceful_close() -> None:
            if shutdown:
                try:
                    await rpc.request("shutdown", {})
                except (ProtocolError, RequestTimeoutError, TransportError):
                    pass
            await rpc.close()

        try:
            async with asyncio.timeout(self._close_timeout):
                await graceful_close()
        except TimeoutError:
            rpc.abort()

    async def start_thread(
        self,
        *,
        cwd: os.PathLike[str] | str,
        source: str = "python",
        metadata: Mapping[str, object] | None = None,
    ) -> Thread:
        result = await self._request(
            "thread/start",
            {
                "cwd": os.fspath(cwd),
                "source": source,
                "metadata": dict(metadata) if metadata is not None else None,
            },
        )
        return Thread(self, cast(ThreadSnapshot, result))

    async def resume_thread(self, thread_id: str) -> Thread:
        result = await self._request("thread/resume", {"threadId": thread_id})
        return Thread(self, cast(ThreadSnapshot, result))

    async def list_threads(
        self,
        *,
        cwd: os.PathLike[str] | str | None = None,
        archived: bool = False,
        sources: Sequence[str] = (),
        cursor: str | None = None,
        limit: int = 50,
    ) -> ThreadPage:
        return cast(
            ThreadPage,
            await self._request(
                "thread/list",
                {
                    "cwd": None if cwd is None else os.fspath(cwd),
                    "archived": archived,
                    "sources": list(sources),
                    "cursor": cursor,
                    "limit": limit,
                },
            ),
        )

    async def iter_threads(
        self,
        *,
        cwd: os.PathLike[str] | str | None = None,
        archived: bool = False,
        sources: Sequence[str] = (),
        page_size: int = 50,
    ) -> AsyncIterator[ThreadSummary]:
        cursor: str | None = None
        while True:
            page = await self.list_threads(
                cwd=cwd,
                archived=archived,
                sources=sources,
                cursor=cursor,
                limit=page_size,
            )
            for thread in page.threads:
                yield thread
            if page.next_cursor is None:
                return
            cursor = page.next_cursor

    async def resume_turn(self, turn_id: str) -> TurnHandle:
        return await self._request_turn(
            "turn/resume",
            {"turnId": turn_id},
            thread_id="",
            turn_id=turn_id,
            client_turn_id=None,
        )

    async def _request_turn(
        self,
        method: str,
        params: dict[str, object],
        *,
        thread_id: str,
        turn_id: str,
        client_turn_id: str | None,
    ) -> TurnHandle:
        rpc = self._rpc
        if rpc is None:
            raise RuntimeError("Client is not connected; use async with or connect()")
        turn = TurnHandle(
            self,
            TurnReceipt(
                accepted=True,
                thread_id=thread_id,
                turn_id=turn_id,
                client_turn_id=client_turn_id,
            ),
        )
        rpc.register_turn(turn)
        try:
            receipt = cast(TurnReceipt, await rpc.request(method, params))
            if (
                not receipt.accepted
                or receipt.turn_id != turn_id
                or (thread_id and receipt.thread_id != thread_id)
            ):
                raise ProtocolError(
                    -32603,
                    f"{method} returned a conflicting Turn receipt",
                )
            turn._accept_receipt(receipt)
            return turn
        except BaseException:
            rpc.unregister_turn(turn)
            turn._close_events()
            raise

    async def _request(
        self,
        method: str,
        params: object,
        *,
        timeout: float | None | object = _USE_DEFAULT_TIMEOUT,
    ) -> object:
        if self._rpc is None:
            raise RuntimeError("Client is not connected; use async with or connect()")
        return await self._rpc.request(method, params, timeout=timeout)


class Thread:
    def __init__(self, client: Client, snapshot: ThreadSnapshot) -> None:
        self._client = client
        self._snapshot = snapshot

    @property
    def id(self) -> str:
        return self._snapshot.id

    async def snapshot(self) -> ThreadSnapshot:
        result = await self._client._request("thread/read", {"threadId": self.id})
        self._snapshot = cast(ThreadSnapshot, result)
        return self._snapshot

    async def start_turn(
        self,
        prompt: str,
        *,
        client_turn_id: str | None = None,
        source: str = "python",
        model: str | None = None,
        reasoning_effort: str | None = None,
        no_agents: bool = False,
        no_skills: bool = False,
        inherited_env: Mapping[str, str] | None = None,
    ) -> TurnHandle:
        turn_id = str(uuid4())
        return await self._client._request_turn(
            "turn/start",
            {
                "threadId": self.id,
                "turnId": turn_id,
                "prompt": prompt,
                "clientTurnId": client_turn_id,
                "source": source,
                "model": model,
                "reasoningEffort": reasoning_effort,
                "noAgents": no_agents,
                "noSkills": no_skills,
                "inheritedEnv": None
                if inherited_env is None
                else dict(inherited_env),
                "useRegisteredApprovalHandler": self._client._approval_handler
                is not None,
                "useRegisteredClarifyHandler": self._client._clarify_handler
                is not None,
            },
            thread_id=self.id,
            turn_id=turn_id,
            client_turn_id=client_turn_id,
        )

    async def archive(self) -> None:
        await self._client._request("thread/archive", {"threadId": self.id})

    async def compact(
        self,
        *,
        model: str | None = None,
        reasoning_effort: str | None = None,
        instructions: str | None = None,
        force: bool = False,
    ) -> CompactionResult:
        result = await self._client._request(
            "thread/compact",
            {
                "threadId": self.id,
                "model": model,
                "reasoningEffort": reasoning_effort,
                "instructions": instructions,
                "force": force,
            },
        )
        return cast(CompactionResult, result)

    async def fork(self, *, before_session_seq: int | None = None) -> Thread:
        result = await self._client._request(
            "thread/fork",
            {
                "threadId": self.id,
                "beforeSessionSeq": before_session_seq,
            },
        )
        return Thread(self._client, cast(ThreadSnapshot, result))


class TurnHandle:
    def __init__(self, client: Client, receipt: TurnReceipt) -> None:
        self._client = client
        self.receipt = receipt
        self._events: asyncio.Queue[TurnEvent | object] = asyncio.Queue(
            maxsize=_EVENT_CAPACITY
        )
        self._closed = False
        self._missed = 0

    def _accept_receipt(self, receipt: TurnReceipt) -> None:
        self.receipt = receipt

    async def events(self) -> AsyncIterator[TurnEvent]:
        while True:
            event = await self._events.get()
            if self._missed:
                missed = self._missed
                self._missed = 0
                yield TurnEvent(
                    type="resync_required",
                    data={"type": "resync_required", "missed": missed},
                )
            if event is _EVENT_END:
                return
            if isinstance(event, TurnEvent):
                yield event

    async def wait(self, *, timeout: float | None = None) -> TurnResult:
        result = await self._client._request(
            "turn/wait",
            {"turnId": self.receipt.turn_id},
            timeout=timeout,
        )
        return cast(TurnResult, result)

    async def interrupt(self) -> None:
        await self._client._request(
            "turn/interrupt", {"turnId": self.receipt.turn_id}
        )

    async def steer(self, input: str) -> bool:
        return cast(
            bool,
            await self._client._request(
                "turn/steer",
                {"turnId": self.receipt.turn_id, "input": input},
            ),
        )

    async def respond(
        self,
        interaction_id: str,
        response: ApprovalDecision | Sequence[Sequence[str]] | None,
    ) -> bool:
        if isinstance(response, ApprovalDecision):
            wire_response: dict[str, object] = {
                "kind": "permission",
                **response.to_wire(),
            }
        elif response is None:
            wire_response = {"kind": "cancel"}
        else:
            wire_response = {
                "kind": "clarify",
                "answers": [list(answer) for answer in response],
            }
        return cast(
            bool,
            await self._client._request(
                "interaction/respond",
                {
                    "turnId": self.receipt.turn_id,
                    "interactionId": interaction_id,
                    "response": wire_response,
                },
            ),
        )

    def _receive_event(self, value: dict[str, Any]) -> None:
        if self._closed:
            return
        event = TurnEvent.from_wire(value)
        if self._events.full():
            try:
                self._events.get_nowait()
                self._missed += 1
            except asyncio.QueueEmpty:
                pass
        self._events.put_nowait(event)
        if event.type in {"completed", "failed"}:
            self._close_events()

    def _close_events(self) -> None:
        if self._closed:
            return
        self._closed = True
        if self._events.full():
            try:
                self._events.get_nowait()
                self._missed += 1
            except asyncio.QueueEmpty:
                pass
        self._events.put_nowait(_EVENT_END)


def _result_decoder(method: str) -> Callable[[object], object]:
    return {
        "initialize": _decode_initialize_result,
        "tool/register": _decode_tool_register_result,
        "thread/start": _decode_thread_snapshot_result,
        "thread/resume": _decode_thread_snapshot_result,
        "thread/read": _decode_thread_snapshot_result,
        "thread/list": _decode_thread_page_result,
        "thread/archive": _decode_thread_archive_result,
        "thread/compact": _decode_compaction_result,
        "thread/fork": _decode_thread_snapshot_result,
        "turn/start": _decode_turn_receipt_result,
        "turn/resume": _decode_turn_receipt_result,
        "turn/wait": _decode_turn_result,
        "turn/interrupt": _decode_turn_interrupt_result,
        "turn/steer": _decode_turn_steer_result,
        "interaction/respond": _decode_interaction_response_result,
        "shutdown": _decode_shutdown_result,
    }.get(method, _decode_object_result)


def _decode_rpc_error(value: object) -> ProtocolError:
    error = _wire_object(value, "JSON-RPC error")
    code = _wire_int(error, "code", "JSON-RPC error")
    message = _wire_string(error, "message", "JSON-RPC error")
    return ProtocolError(code, message, error.get("data"))


def _decode_initialize_result(value: object) -> dict[str, Any]:
    result = _wire_object(value, "initialize result")
    server = _wire_object(result.get("server"), "initialize result server")
    _wire_string(server, "name", "initialize result server")
    _wire_string(server, "version", "initialize result server")
    _wire_int(result, "protocolVersion", "initialize result")
    _wire_int(result, "protocolMin", "initialize result")
    _wire_int(result, "protocolMax", "initialize result")
    capabilities = _wire_object(
        result.get("capabilities"), "initialize result capabilities"
    )
    _wire_bool(capabilities, "threads", "initialize result capabilities")
    _wire_bool(capabilities, "turns", "initialize result capabilities")
    _wire_string(capabilities, "eventReplay", "initialize result capabilities")
    _wire_bool(capabilities, "interactions", "initialize result capabilities")
    _wire_bool(capabilities, "customTools", "initialize result capabilities")
    return result


def _decode_tool_register_result(value: object) -> dict[str, Any]:
    result = _wire_object(value, "tool/register result")
    _wire_bool(result, "registered", "tool/register result")
    _wire_int(result, "toolCount", "tool/register result")
    _wire_bool(result, "approvalHandler", "tool/register result")
    _wire_bool(result, "clarifyHandler", "tool/register result")
    return result


def _decode_thread_snapshot_result(value: object) -> ThreadSnapshot:
    result = _wire_object(value, "Thread snapshot")
    _validate_thread_summary(result, "Thread snapshot")
    for item in _wire_list(result.get("items", []), "Thread snapshot items"):
        item = _wire_object(item, "Thread item")
        _wire_int(item, "sessionSeq", "Thread item")
        _wire_present(item, "message", "Thread item")
    for interaction in _wire_list(
        result.get("pendingInteractions", []),
        "Thread snapshot pendingInteractions",
    ):
        interaction = _wire_object(interaction, "pending interaction")
        for key in ("interactionId", "threadId", "turnId", "kind", "status"):
            _wire_string(interaction, key, "pending interaction")
        _wire_present(interaction, "payload", "pending interaction")
        _wire_int(interaction, "requestedAtMs", "pending interaction")
        _wire_optional_int(interaction, "resolvedAtMs", "pending interaction")
    return ThreadSnapshot.from_wire(result)


def _decode_thread_page_result(value: object) -> ThreadPage:
    result = _wire_object(value, "thread/list result")
    threads = _wire_list(result.get("threads"), "thread/list result threads")
    decoded = []
    for value in threads:
        thread = _wire_object(value, "Thread summary")
        _validate_thread_summary(thread, "Thread summary")
        decoded.append(ThreadSummary.from_wire(thread))
    next_cursor = _wire_optional_string(
        result, "nextCursor", "thread/list result"
    )
    return ThreadPage(threads=tuple(decoded), next_cursor=next_cursor)


def _decode_thread_archive_result(value: object) -> dict[str, Any]:
    result = _wire_object(value, "thread/archive result")
    _wire_bool(result, "archived", "thread/archive result")
    _wire_string(result, "threadId", "thread/archive result")
    return result


def _decode_compaction_result(value: object) -> CompactionResult:
    result = _wire_object(value, "thread/compact result")
    for key in ("session_id", "reason", "message"):
        _wire_string(result, key, "thread/compact result")
    _wire_bool(result, "compacted", "thread/compact result")
    for key in (
        "checkpoint_id",
        "first_kept_session_seq",
        "tokens_before",
        "tokens_after",
    ):
        _wire_optional_int(result, key, "thread/compact result")
    for key in ("summary", "summary_provider", "summary_model"):
        _wire_optional_string(result, key, "thread/compact result")
    return CompactionResult.from_wire(result)


def _decode_turn_receipt_result(value: object) -> TurnReceipt:
    result = _wire_object(value, "Turn receipt")
    _wire_bool(result, "accepted", "Turn receipt")
    _wire_string(result, "threadId", "Turn receipt")
    _wire_string(result, "turnId", "Turn receipt")
    _wire_optional_string(result, "clientTurnId", "Turn receipt")
    return TurnReceipt.from_wire(result)


def _decode_turn_result(value: object) -> TurnResult:
    result = _wire_object(value, "turn/wait result")
    for key in ("threadId", "finalAnswer", "provider", "model"):
        _wire_string(result, key, "turn/wait result")
    outcome = _wire_string(result, "outcome", "turn/wait result")
    if outcome not in {"completed", "stopped", "failed", "interrupted"}:
        raise ValueError(f"turn/wait result outcome is invalid: {outcome}")
    _wire_optional_string(result, "reasoningEffort", "turn/wait result")
    _wire_int(result, "toolFailures", "turn/wait result")
    _wire_optional_int(result, "contextLimit", "turn/wait result")
    for key in (
        "contextSnapshot",
        "terminalReason",
        "terminalError",
        "selectedAgent",
    ):
        if key in result and result[key] is not None:
            _wire_object(result[key], f"turn/wait result {key}")
    for key in ("warnings", "selectedSkills"):
        for item in _wire_list(result.get(key, []), f"turn/wait result {key}"):
            _wire_object(item, f"turn/wait result {key} item")
    return TurnResult.from_wire(result)


def _decode_turn_interrupt_result(value: object) -> dict[str, Any]:
    result = _wire_object(value, "turn/interrupt result")
    _wire_bool(result, "interrupted", "turn/interrupt result")
    _wire_string(result, "turnId", "turn/interrupt result")
    return result


def _decode_turn_steer_result(value: object) -> bool:
    result = _wire_object(value, "turn/steer result")
    accepted = _wire_bool(result, "accepted", "turn/steer result")
    _wire_string(result, "turnId", "turn/steer result")
    return accepted


def _decode_interaction_response_result(value: object) -> bool:
    result = _wire_object(value, "interaction/respond result")
    accepted = _wire_bool(result, "accepted", "interaction/respond result")
    _wire_string(result, "turnId", "interaction/respond result")
    _wire_string(result, "interactionId", "interaction/respond result")
    return accepted


def _decode_shutdown_result(value: object) -> dict[str, Any]:
    result = _wire_object(value, "shutdown result")
    _wire_bool(result, "shutdown", "shutdown result")
    return result


def _decode_object_result(value: object) -> dict[str, Any]:
    return _wire_object(value, "App Server result")


def _validate_thread_summary(value: dict[str, Any], label: str) -> None:
    for key in ("id", "source", "cwd"):
        _wire_string(value, key, label)
    _wire_optional_string(value, "title", label)
    _wire_int(value, "startedAtMs", label)
    _wire_int(value, "updatedAtMs", label)
    _wire_bool(value, "archived", label)
    _wire_int(value, "messageCount", label)
    _wire_int(value, "toolCallCount", label)
    _wire_optional_string(value, "activeTurnId", label)


def _validate_turn_event(
    event: dict[str, Any],
    thread_id: str,
    turn_id: str,
) -> None:
    event_type = _wire_string(event, "type", "turn/event event")
    if event_type == "accepted":
        receipt = _decode_turn_receipt_result(event.get("receipt"))
        if receipt.thread_id != thread_id or receipt.turn_id != turn_id:
            raise TransportError("turn/event accepted receipt has conflicting identity")
    elif event_type in {"started", "completed", "failed"}:
        event_thread_id = _wire_string(event, "threadId", f"{event_type} event")
        event_turn_id = _wire_string(event, "turnId", f"{event_type} event")
        if event_thread_id != thread_id or event_turn_id != turn_id:
            raise TransportError(f"turn/event {event_type} has conflicting identity")
        if event_type == "completed":
            outcome = _wire_string(event, "outcome", "completed event")
            if outcome not in {"completed", "stopped", "failed", "interrupted"}:
                raise ValueError(f"completed event outcome is invalid: {outcome}")
        elif event_type == "failed":
            _wire_string(event, "message", "failed event")
    elif event_type == "message":
        _wire_stage(event, "message event")
        _wire_present(event, "message", "message event")
    elif event_type in {"message_delta", "reasoning_delta"}:
        _wire_string(event, "text", f"{event_type} event")
    elif event_type == "reasoning_completed":
        if "text" not in event or (
            event["text"] is not None and not isinstance(event["text"], str)
        ):
            raise ValueError("reasoning_completed event text must be a string or null")
    elif event_type == "tool":
        _wire_stage(event, "tool event")
        _wire_present(event, "data", "tool event")
    elif event_type == "interaction_requested":
        _wire_string(event, "interactionId", "interaction_requested event")
        _wire_string(event, "kind", "interaction_requested event")
        _wire_present(event, "payload", "interaction_requested event")
    elif event_type == "interaction_resolved":
        _wire_string(event, "interactionId", "interaction_resolved event")
        _wire_string(event, "reason", "interaction_resolved event")
    elif event_type == "warning":
        _wire_present(event, "data", "warning event")
    elif event_type == "resync_required":
        missed = _wire_int(event, "missed", "resync_required event")
        if missed < 0:
            raise ValueError("resync_required event missed must be non-negative")


def _wire_stage(value: dict[str, Any], label: str) -> str:
    stage = _wire_string(value, "stage", label)
    if stage not in {"started", "updated", "completed"}:
        raise ValueError(f"{label} stage is invalid: {stage}")
    return stage


def _wire_object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    return value


def _wire_list(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise ValueError(f"{label} must be an array")
    return value


def _wire_present(value: Mapping[str, object], key: str, label: str) -> object:
    if key not in value:
        raise ValueError(f"{label} requires {key}")
    return value[key]


def _wire_string(value: Mapping[str, object], key: str, label: str) -> str:
    field = _wire_present(value, key, label)
    if not isinstance(field, str):
        raise ValueError(f"{label} {key} must be a string")
    return field


def _wire_optional_string(
    value: Mapping[str, object], key: str, label: str
) -> str | None:
    field = value.get(key)
    if field is not None and not isinstance(field, str):
        raise ValueError(f"{label} {key} must be a string or null")
    return field


def _wire_int(value: Mapping[str, object], key: str, label: str) -> int:
    field = _wire_present(value, key, label)
    if type(field) is not int:
        raise ValueError(f"{label} {key} must be an integer")
    return field


def _wire_optional_int(
    value: Mapping[str, object], key: str, label: str
) -> int | None:
    field = value.get(key)
    if field is not None and type(field) is not int:
        raise ValueError(f"{label} {key} must be an integer or null")
    return field


def _wire_bool(value: Mapping[str, object], key: str, label: str) -> bool:
    field = _wire_present(value, key, label)
    if not isinstance(field, bool):
        raise ValueError(f"{label} {key} must be a boolean")
    return field


def _callback_params(value: object) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ProtocolError(-32602, "callback params must be an object")
    return value


def _validate_timeout(
    name: str,
    timeout: float | None,
    *,
    allow_none: bool,
) -> None:
    if timeout is None:
        if allow_none:
            return
        raise ValueError(f"{name} must be greater than zero")
    if isinstance(timeout, bool) or not isinstance(timeout, (int, float)):
        raise TypeError(f"{name} must be a number")
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError(f"{name} must be finite and greater than zero")


def _terminal_event(value: Mapping[str, object]) -> bool:
    return value.get("type") in {"completed", "failed"}


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


def _sdk_version() -> str:
    from . import __version__

    return __version__


def _bundled_app_server() -> Path:
    try:
        import psychevo_app_server_bin
    except ImportError as error:
        raise TransportError(
            "the exact-version psychevo-app-server-bin package is required"
        ) from error
    if getattr(psychevo_app_server_bin, "__version__", None) != _sdk_version():
        raise TransportError(
            "psychevo-app-server-bin version does not match the Psychevo SDK"
        )
    executable = Path(psychevo_app_server_bin.executable())
    if not executable.is_file():
        raise TransportError(f"bundled App Server is missing: {executable}")
    return executable
