from __future__ import annotations

import asyncio
import math
from collections.abc import Awaitable, Callable
from dataclasses import dataclass

from ._protocol import _decode_method_result
from .errors import ProtocolError, RequestTimeoutError

_DEFAULT_REQUEST_TIMEOUT = 30.0
_USE_DEFAULT_TIMEOUT = object()


@dataclass(slots=True)
class _PendingRequest:
    method: str
    future: asyncio.Future[object]


class _PendingRequests:
    def __init__(
        self,
        default_timeout: float | None = _DEFAULT_REQUEST_TIMEOUT,
    ) -> None:
        _validate_timeout("request_timeout", default_timeout, allow_none=True)
        self._default_timeout = default_timeout
        self._next_id = 0
        self._requests: dict[int, _PendingRequest] = {}

    async def request(
        self,
        method: str,
        params: object,
        *,
        send: Callable[[dict[str, object]], Awaitable[None]],
        ensure_open: Callable[[], None],
        timeout: float | None | object = _USE_DEFAULT_TIMEOUT,
    ) -> object:
        ensure_open()
        effective_timeout = (
            self._default_timeout if timeout is _USE_DEFAULT_TIMEOUT else timeout
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
        future = asyncio.get_running_loop().create_future()
        self._requests[request_id] = _PendingRequest(method, future)
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
                ensure_open()
                delivery_unknown = True
                await send(message)
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
            self._requests.pop(request_id, None)

    def resolve_result(self, request_id: int, value: object) -> None:
        pending = self._requests.get(request_id)
        if pending is None or pending.future.done():
            return
        pending.future.set_result(_decode_method_result(pending.method, value))

    def resolve_failure(self, request_id: int, error: ProtocolError) -> None:
        pending = self._requests.get(request_id)
        if pending is None or pending.future.done():
            return
        pending.future.set_exception(error)

    def fail_all(self, error: BaseException) -> None:
        for pending in self._requests.values():
            if not pending.future.done():
                pending.future.set_exception(error)


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
