from __future__ import annotations

import asyncio
import inspect
import os
from collections import defaultdict, deque
from collections.abc import AsyncIterator, Mapping, Sequence
from pathlib import Path
from typing import Any

from ._callbacks import (
    ApprovalDecision,
    ApprovalHandler,
    ApprovalRequest,
    ClarifyHandler,
    ClarifyRequest,
    Tool,
    ToolCall,
    ToolResult,
)
from ._transport import StdioTransport, Transport, WebSocketTransport
from ._types import (
    CompactionResult,
    ThreadSnapshot,
    ThreadSummary,
    TurnEvent,
    TurnReceipt,
    TurnResult,
)
from .errors import ProtocolError, TransportError

_PROTOCOL_VERSION = 1
_EVENT_CAPACITY = 256
_EVENT_END = object()


class _RpcClient:
    def __init__(
        self,
        transport: Transport,
        *,
        tools: Sequence[Tool],
        approval_handler: ApprovalHandler | None,
        clarify_handler: ClarifyHandler | None,
    ) -> None:
        self._transport = transport
        self._next_id = 0
        self._pending: dict[int, asyncio.Future[object]] = {}
        self._turns: dict[str, TurnHandle] = {}
        self._early_events: dict[str, deque[dict[str, Any]]] = defaultdict(
            lambda: deque(maxlen=_EVENT_CAPACITY)
        )
        self._early_missed: dict[str, int] = defaultdict(int)
        self._reader: asyncio.Task[None] | None = None
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

    async def request(self, method: str, params: object = None) -> object:
        self._raise_if_terminal()
        self._next_id += 1
        request_id = self._next_id
        loop = asyncio.get_running_loop()
        future: asyncio.Future[object] = loop.create_future()
        self._pending[request_id] = future
        message: dict[str, object] = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
        }
        if params is not None:
            message["params"] = params
        try:
            self._raise_if_terminal()
            await self._send(message)
            return await future
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
        self._turns[turn.receipt.turn_id] = turn
        missed = self._early_missed.pop(turn.receipt.turn_id, 0)
        if missed:
            turn._receive_event({"type": "resync_required", "missed": missed})
        for event in self._early_events.pop(turn.receipt.turn_id, ()):
            turn._receive_event(event)
            self._maybe_handle_clarify(turn.receipt.turn_id, event)
            if _terminal_event(event):
                self._forget_turn(turn.receipt.turn_id)
                break

    async def close(self) -> None:
        self._transition_terminal(TransportError("App Server connection closed"))
        await self._transport.close()
        if self._reader is not None:
            self._reader.cancel()
            try:
                await self._reader
            except asyncio.CancelledError:
                pass
        if self._callbacks:
            await asyncio.gather(*self._callbacks, return_exceptions=True)

    async def _read_loop(self) -> None:
        try:
            while True:
                message = await self._transport.receive()
                if "method" in message and "id" in message:
                    self._spawn_callback(self._handle_callback_request(message))
                elif "id" in message:
                    self._receive_response(message)
                else:
                    self._receive_notification(message)
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
        for future in self._pending.values():
            if not future.done():
                future.set_exception(error)
        for turn in self._turns.values():
            turn._close_events()
        current_task = asyncio.current_task()
        if self._reader is not None and self._reader is not current_task:
            self._reader.cancel()
        for task in self._callbacks:
            if task is not current_task:
                task.cancel()
        self._turns.clear()
        self._early_events.clear()
        self._early_missed.clear()

    def _forget_turn(self, turn_id: str) -> None:
        self._turns.pop(turn_id, None)
        self._early_events.pop(turn_id, None)
        self._early_missed.pop(turn_id, None)

    def _receive_response(self, message: dict[str, object]) -> None:
        request_id = message.get("id")
        if not isinstance(request_id, int):
            return
        future = self._pending.get(request_id)
        if future is None or future.done():
            return
        error = message.get("error")
        if isinstance(error, dict):
            future.set_exception(
                ProtocolError(
                    int(error.get("code", -32000)),
                    str(error.get("message", "App Server request failed")),
                    error.get("data"),
                )
            )
        else:
            future.set_result(message.get("result"))

    def _receive_notification(self, message: dict[str, object]) -> None:
        if message.get("method") != "turn/event":
            return
        params = message.get("params")
        if not isinstance(params, dict):
            return
        turn_id = params.get("turnId")
        event = params.get("event")
        if not isinstance(turn_id, str) or not isinstance(event, dict):
            return
        turn = self._turns.get(turn_id)
        if turn is None:
            early = self._early_events[turn_id]
            if len(early) == _EVENT_CAPACITY:
                self._early_missed[turn_id] += 1
            early.append(event)
        else:
            turn._receive_event(event)
            self._maybe_handle_clarify(turn_id, event)
            if _terminal_event(event):
                self._forget_turn(turn_id)

    def _maybe_handle_clarify(
        self, turn_id: str, event: Mapping[str, object]
    ) -> None:
        if (
            event.get("type") == "interaction_requested"
            and event.get("kind") == "clarify"
            and self._clarify_handler is not None
        ):
            interaction_id = event.get("interactionId")
            if isinstance(interaction_id, str):
                turn = self._turns.get(turn_id)
                self._spawn_callback(
                    self._handle_clarify(
                        turn.receipt.thread_id if turn is not None else "",
                        turn_id,
                        interaction_id,
                        event.get("payload"),
                    )
                )

    def _spawn_callback(self, coroutine: object) -> None:
        task = asyncio.create_task(coroutine)  # type: ignore[arg-type]
        self._callbacks.add(task)
        task.add_done_callback(self._callbacks.discard)

    async def _handle_callback_request(self, message: dict[str, object]) -> None:
        request_id = message.get("id")
        method = message.get("method")
        params = message.get("params")
        if not isinstance(request_id, str) or not isinstance(method, str):
            return
        try:
            if method == "tool/call":
                result = await self._call_tool(_object(params))
            elif method == "approval/request":
                result = await self._call_approval(_object(params))
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
            filesystem=params.get("filesystem"),
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
    ) -> None:
        if executable is not None and remote_url is not None:
            raise ValueError("executable and remote_url are mutually exclusive")
        if remote_url is not None and not token:
            raise ValueError("remote_url requires an explicit bearer token")
        self._executable = executable
        self._executable_args = tuple(executable_args)
        self._remote_url = remote_url
        self._token = token
        self._tools = tuple(tools)
        self._approval_handler = approval_handler
        self._clarify_handler = clarify_handler
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
        )
        try:
            await rpc.start()
        except BaseException:
            await rpc.close()
            raise
        self._rpc = rpc

    async def close(self) -> None:
        rpc, self._rpc = self._rpc, None
        if rpc is None:
            return
        if self._local:
            try:
                await rpc.request("shutdown", {})
            except (ProtocolError, TransportError):
                pass
        await rpc.close()

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
        return Thread(self, ThreadSnapshot.from_wire(_object(result)))

    async def resume_thread(self, thread_id: str) -> Thread:
        result = await self._request("thread/resume", {"threadId": thread_id})
        return Thread(self, ThreadSnapshot.from_wire(_object(result)))

    async def list_threads(
        self,
        *,
        cwd: os.PathLike[str] | str | None = None,
        archived: bool = False,
        sources: Sequence[str] = (),
    ) -> list[ThreadSummary]:
        result = _object(
            await self._request(
                "thread/list",
                {
                    "cwd": None if cwd is None else os.fspath(cwd),
                    "archived": archived,
                    "sources": list(sources),
                },
            )
        )
        threads = result.get("threads")
        if not isinstance(threads, list):
            raise TransportError("thread/list returned invalid threads")
        return [ThreadSummary.from_wire(_object(item)) for item in threads]

    async def resume_turn(self, turn_id: str) -> TurnHandle:
        result = await self._request("turn/resume", {"turnId": turn_id})
        receipt = TurnReceipt.from_wire(_object(result))
        turn = TurnHandle(self, receipt)
        if self._rpc is None:
            raise TransportError("Client disconnected while resuming a Turn")
        self._rpc.register_turn(turn)
        return turn

    async def _request(self, method: str, params: object) -> object:
        if self._rpc is None:
            raise RuntimeError("Client is not connected; use async with or connect()")
        return await self._rpc.request(method, params)


class Thread:
    def __init__(self, client: Client, snapshot: ThreadSnapshot) -> None:
        self._client = client
        self._snapshot = snapshot

    @property
    def id(self) -> str:
        return self._snapshot.id

    async def snapshot(self) -> ThreadSnapshot:
        result = await self._client._request("thread/read", {"threadId": self.id})
        self._snapshot = ThreadSnapshot.from_wire(_object(result))
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
        result = await self._client._request(
            "turn/start",
            {
                "threadId": self.id,
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
        )
        receipt = TurnReceipt.from_wire(_object(result))
        turn = TurnHandle(self._client, receipt)
        if self._client._rpc is None:
            raise TransportError("Client disconnected while accepting a Turn")
        self._client._rpc.register_turn(turn)
        return turn

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
        return CompactionResult.from_wire(_object(result))

    async def fork(self, *, before_session_seq: int | None = None) -> Thread:
        result = await self._client._request(
            "thread/fork",
            {
                "threadId": self.id,
                "beforeSessionSeq": before_session_seq,
            },
        )
        return Thread(self._client, ThreadSnapshot.from_wire(_object(result)))


class TurnHandle:
    def __init__(self, client: Client, receipt: TurnReceipt) -> None:
        self._client = client
        self.receipt = receipt
        self._events: asyncio.Queue[TurnEvent | object] = asyncio.Queue(
            maxsize=_EVENT_CAPACITY
        )
        self._closed = False
        self._missed = 0

    async def events(self) -> AsyncIterator[TurnEvent]:
        while True:
            event = await self._events.get()
            if event is _EVENT_END:
                return
            if isinstance(event, TurnEvent):
                yield event

    async def wait(self) -> TurnResult:
        result = await self._client._request(
            "turn/wait", {"turnId": self.receipt.turn_id}
        )
        return TurnResult.from_wire(_object(result))

    async def interrupt(self) -> None:
        await self._client._request(
            "turn/interrupt", {"turnId": self.receipt.turn_id}
        )

    async def steer(self, input: str) -> bool:
        result = _object(
            await self._client._request(
                "turn/steer",
                {"turnId": self.receipt.turn_id, "input": input},
            )
        )
        return result.get("accepted") is True

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
        result = _object(
            await self._client._request(
                "interaction/respond",
                {
                    "turnId": self.receipt.turn_id,
                    "interactionId": interaction_id,
                    "response": wire_response,
                },
            )
        )
        return result.get("accepted") is True

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
        if self._missed:
            resync = TurnEvent(
                type="resync_required",
                data={"type": "resync_required", "missed": self._missed},
            )
            try:
                self._events.put_nowait(resync)
                self._missed = 0
            except asyncio.QueueFull:
                pass
        if self._events.full():
            try:
                self._events.get_nowait()
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
            except asyncio.QueueEmpty:
                pass
        self._events.put_nowait(_EVENT_END)


def _object(value: object) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise TransportError("App Server result must be a JSON object")
    return value


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
