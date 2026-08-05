from __future__ import annotations

import asyncio
from collections.abc import Sequence
from typing import Protocol

from ._callback_runtime import (
    _DEFAULT_CALLBACK_BACKLOG,
    _DEFAULT_CALLBACK_WORKERS,
    _CallbackRuntime,
)
from ._callbacks import ApprovalHandler, ClarifyHandler, Tool
from ._pending import (
    _DEFAULT_REQUEST_TIMEOUT,
    _USE_DEFAULT_TIMEOUT,
    _PendingRequests,
)
from ._protocol import (
    _RpcCallback,
    _RpcFailure,
    _RpcNotification,
    _RpcResult,
    _decode_rpc_envelope,
    _decode_server_error,
    _decode_turn_notification,
    _terminal_event,
)
from ._transport import Transport
from ._types import TurnEvent, TurnReceipt
from .errors import ProtocolError, TransportError

_PROTOCOL_VERSION = 1


class _TurnSink(Protocol):
    receipt: TurnReceipt

    def _receive_event(self, event: TurnEvent) -> None: ...

    def _close_events(self) -> None: ...


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
        self._transport = transport
        self._pending = _PendingRequests(request_timeout)
        self._turns: dict[str, _TurnSink] = {}
        self._reader: asyncio.Task[None] | None = None
        self._terminal_error: TransportError | None = None
        self._callback_runtime = _CallbackRuntime(
            send=self._send,
            request=self.request,
            transition_terminal=self._transition_terminal,
            tools=tools,
            approval_handler=approval_handler,
            clarify_handler=clarify_handler,
            workers=callback_workers,
            backlog=callback_backlog,
        )

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
        if (
            not isinstance(result, dict)
            or result.get("protocolVersion") != _PROTOCOL_VERSION
        ):
            raise TransportError("App Server selected an unexpected protocol version")
        await self.notify("initialized", {})
        if self._callback_runtime.has_registrations():
            await self.request(
                "tool/register",
                self._callback_runtime.registration_params(),
            )

    async def request(
        self,
        method: str,
        params: object = None,
        *,
        timeout: float | None | object = _USE_DEFAULT_TIMEOUT,
    ) -> object:
        return await self._pending.request(
            method,
            params,
            send=self._send,
            ensure_open=self._raise_if_terminal,
            timeout=timeout,
        )

    async def notify(self, method: str, params: object = None) -> None:
        self._raise_if_terminal()
        message: dict[str, object] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            message["params"] = params
        self._raise_if_terminal()
        await self._send(message)

    def register_turn(self, turn: _TurnSink) -> None:
        turn_id = turn.receipt.turn_id
        if turn_id in self._turns:
            raise ProtocolError(
                -32602,
                f"Turn event sink is already registered: {turn_id}",
            )
        self._turns[turn_id] = turn

    def unregister_turn(self, turn: _TurnSink) -> None:
        if self._turns.get(turn.receipt.turn_id) is turn:
            self._turns.pop(turn.receipt.turn_id, None)

    async def close(self) -> None:
        self._transition_terminal(TransportError("App Server connection closed"))
        if self._reader is not None:
            try:
                await self._reader
            except asyncio.CancelledError:
                pass
        await self._callback_runtime.wait_closed()
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
        self._pending.fail_all(error)
        for turn in self._turns.values():
            turn._close_events()
        current_task = asyncio.current_task()
        if self._reader is not None and self._reader is not current_task:
            self._reader.cancel()
        self._callback_runtime.cancel()
        self._turns.clear()

    async def _receive_message(self, message: dict[str, object]) -> None:
        envelope = _decode_rpc_envelope(message)
        if isinstance(envelope, _RpcResult):
            self._pending.resolve_result(envelope.request_id, envelope.value)
        elif isinstance(envelope, _RpcFailure):
            self._pending.resolve_failure(envelope.request_id, envelope.error)
        elif isinstance(envelope, _RpcCallback):
            await self._callback_runtime.queue_request(envelope)
        else:
            self._receive_notification(envelope)

    def _receive_notification(self, notification: _RpcNotification) -> None:
        if notification.method == "server/error":
            raise _decode_server_error(notification.params)
        if notification.method != "turn/event":
            return
        decoded = _decode_turn_notification(notification.params)
        turn = self._turns.get(decoded.turn_id)
        if turn is None:
            return
        turn._receive_event(decoded.event)
        self._callback_runtime.maybe_handle_clarify(
            decoded.thread_id,
            decoded.turn_id,
            decoded.event.data,
        )
        if _terminal_event(decoded.event.data):
            self._turns.pop(decoded.turn_id, None)


def _sdk_version() -> str:
    from . import __version__

    return __version__
