from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import dataclass
from typing import Any, TypeAlias

from ._types import (
    CompactionResult,
    ThreadPage,
    ThreadSnapshot,
    ThreadSummary,
    TurnEvent,
    TurnReceipt,
    TurnResult,
)
from ._wire import (
    wire_bool as _wire_bool,
    wire_int as _wire_int,
    wire_list as _wire_list,
    wire_object as _wire_object,
    wire_optional_string as _wire_optional_string,
    wire_string as _wire_string,
)
from .errors import ProtocolError, TransportError


@dataclass(frozen=True, slots=True)
class _RpcResult:
    request_id: int
    value: object


@dataclass(frozen=True, slots=True)
class _RpcFailure:
    request_id: int
    error: ProtocolError


@dataclass(frozen=True, slots=True)
class _RpcNotification:
    method: str
    params: object


@dataclass(frozen=True, slots=True)
class _RpcCallback:
    request_id: str
    method: str
    params: object


@dataclass(frozen=True, slots=True)
class _TurnNotification:
    thread_id: str
    turn_id: str
    event: TurnEvent


_RpcEnvelope: TypeAlias = (
    _RpcResult | _RpcFailure | _RpcNotification | _RpcCallback
)


def _decode_rpc_envelope(message: dict[str, object]) -> _RpcEnvelope:
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
        params = message.get("params")
        if "params" in message and not isinstance(params, (dict, list)):
            raise TransportError(
                "App Server request or notification params must be an object or array"
            )
        if has_id:
            request_id = message.get("id")
            if not isinstance(request_id, str):
                raise TransportError("App Server callback id must be a string")
            return _RpcCallback(request_id, method, params)
        return _RpcNotification(method, params)
    if not has_id:
        raise TransportError("App Server message is not a JSON-RPC envelope")
    request_id = message.get("id")
    if type(request_id) is not int:
        raise TransportError("App Server response id must be an integer")
    has_result = "result" in message
    has_error = "error" in message
    if has_result == has_error:
        raise TransportError(
            "App Server response must contain exactly one of result or error"
        )
    if has_error:
        return _RpcFailure(request_id, _decode_rpc_error(message["error"]))
    return _RpcResult(request_id, message["result"])


def _decode_method_result(method: str, value: object) -> object:
    try:
        decoder = _RESULT_DECODERS.get(method, _decode_object_result)
        return decoder(value)
    except Exception as error:
        if isinstance(error, TransportError):
            raise
        raise TransportError(
            f"{method} returned an invalid result: {error}"
        ) from error


def _decode_turn_notification(params: object) -> _TurnNotification:
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
    return _TurnNotification(
        thread_id,
        turn_id,
        TurnEvent.from_wire(event, thread_id=thread_id, turn_id=turn_id),
    )


def _decode_server_error(params: object) -> TransportError:
    error = _decode_rpc_error(params)
    return TransportError(f"App Server reported error {error.code}: {error}")


def _terminal_event(value: Mapping[str, object]) -> bool:
    return value.get("type") in {"completed", "failed"}


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


def _decode_thread_page_result(value: object) -> ThreadPage:
    result = _wire_object(value, "thread/list result")
    threads = _wire_list(result.get("threads"), "thread/list result threads")
    decoded = []
    for value in threads:
        decoded.append(ThreadSummary.from_wire(value))
    next_cursor = _wire_optional_string(
        result, "nextCursor", "thread/list result"
    )
    return ThreadPage(threads=tuple(decoded), next_cursor=next_cursor)


def _decode_thread_archive_result(value: object) -> dict[str, Any]:
    result = _wire_object(value, "thread/archive result")
    _wire_bool(result, "archived", "thread/archive result")
    _wire_string(result, "threadId", "thread/archive result")
    return result


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


_RESULT_DECODERS: dict[str, Callable[[object], object]] = {
    "initialize": _decode_initialize_result,
    "tool/register": _decode_tool_register_result,
    "thread/start": ThreadSnapshot.from_wire,
    "thread/resume": ThreadSnapshot.from_wire,
    "thread/read": ThreadSnapshot.from_wire,
    "thread/list": _decode_thread_page_result,
    "thread/archive": _decode_thread_archive_result,
    "thread/compact": CompactionResult.from_wire,
    "thread/fork": ThreadSnapshot.from_wire,
    "turn/start": TurnReceipt.from_wire,
    "turn/resume": TurnReceipt.from_wire,
    "turn/wait": TurnResult.from_wire,
    "turn/interrupt": _decode_turn_interrupt_result,
    "turn/steer": _decode_turn_steer_result,
    "interaction/respond": _decode_interaction_response_result,
    "shutdown": _decode_shutdown_result,
}
