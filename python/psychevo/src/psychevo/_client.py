from __future__ import annotations

import asyncio
import os
from collections.abc import AsyncIterator, Mapping, Sequence
from pathlib import Path
from typing import Any, cast
from uuid import uuid4

from ._callbacks import (
    ApprovalDecision,
    ApprovalHandler,
    ClarifyHandler,
    Tool,
)
from ._pending import (
    _DEFAULT_REQUEST_TIMEOUT,
    _USE_DEFAULT_TIMEOUT,
    _validate_timeout,
)
from ._rpc import _RpcClient
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

_EVENT_CAPACITY = 256
_EVENT_END = object()
_DEFAULT_CLOSE_TIMEOUT = 10.0


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

    def _receive_event(self, event: TurnEvent) -> None:
        if self._closed:
            return
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


def _bundled_app_server() -> Path:
    from . import __version__

    try:
        import psychevo_app_server_bin
    except ImportError as error:
        raise TransportError(
            "the exact-version psychevo-app-server-bin package is required"
        ) from error
    if getattr(psychevo_app_server_bin, "__version__", None) != __version__:
        raise TransportError(
            "psychevo-app-server-bin version does not match the Psychevo SDK"
        )
    executable = Path(psychevo_app_server_bin.executable())
    if not executable.is_file():
        raise TransportError(f"bundled App Server is missing: {executable}")
    return executable
