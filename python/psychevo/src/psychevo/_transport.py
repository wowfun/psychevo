from __future__ import annotations

import asyncio
import json
import os
from abc import ABC, abstractmethod
from collections.abc import Sequence

from websockets.asyncio.client import ClientConnection, connect
from websockets.exceptions import WebSocketException

from .errors import TransportError

_MAX_MESSAGE_BYTES = 16 * 1024 * 1024
_CONNECT_TIMEOUT_SECONDS = 10
_CLOSE_TIMEOUT_SECONDS = 10


class Transport(ABC):
    @abstractmethod
    async def send(self, value: dict[str, object]) -> None: ...

    @abstractmethod
    async def receive(self) -> dict[str, object]: ...

    @abstractmethod
    async def close(self) -> None: ...


class StdioTransport(Transport):
    def __init__(self, process: asyncio.subprocess.Process) -> None:
        self._process = process
        if process.stdin is None or process.stdout is None:
            raise TransportError("App Server stdio pipes are unavailable")
        self._stdin = process.stdin
        self._stdout = process.stdout
        self._write_lock = asyncio.Lock()

    @classmethod
    async def start(
        cls,
        executable: os.PathLike[str] | str,
        args: Sequence[str] = (),
    ) -> StdioTransport:
        process = await asyncio.create_subprocess_exec(
            os.fspath(executable),
            *args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=None,
            limit=_MAX_MESSAGE_BYTES + 1,
        )
        return cls(process)

    async def send(self, value: dict[str, object]) -> None:
        payload = json.dumps(value, separators=(",", ":")).encode("utf-8")
        if len(payload) > _MAX_MESSAGE_BYTES:
            raise TransportError(
                f"App Server stdio message exceeds {_MAX_MESSAGE_BYTES} bytes"
            )
        async with self._write_lock:
            self._stdin.write(payload + b"\n")
            try:
                await self._stdin.drain()
            except (BrokenPipeError, ConnectionResetError) as error:
                raise TransportError("App Server stdin closed") from error

    async def receive(self) -> dict[str, object]:
        try:
            line = await self._stdout.readline()
        except ValueError as error:
            raise TransportError(
                f"App Server stdio message exceeds {_MAX_MESSAGE_BYTES} bytes"
            ) from error
        if not line:
            code = await self._process.wait()
            raise TransportError(f"App Server stdout closed with exit code {code}")
        payload = line[:-1] if line.endswith(b"\n") else line
        if len(payload) > _MAX_MESSAGE_BYTES:
            raise TransportError(
                f"App Server stdio message exceeds {_MAX_MESSAGE_BYTES} bytes"
            )
        try:
            value = json.loads(payload)
        except json.JSONDecodeError as error:
            raise TransportError("App Server emitted invalid JSON") from error
        if not isinstance(value, dict):
            raise TransportError("App Server message must be a JSON object")
        return value

    async def close(self) -> None:
        if not self._stdin.is_closing():
            self._stdin.close()
            await self._stdin.wait_closed()
        try:
            await asyncio.wait_for(self._process.wait(), timeout=10)
        except TimeoutError:
            self._process.terminate()
            try:
                await asyncio.wait_for(self._process.wait(), timeout=2)
            except TimeoutError:
                self._process.kill()
                await self._process.wait()


class WebSocketTransport(Transport):
    def __init__(self, connection: ClientConnection) -> None:
        self._connection = connection

    @classmethod
    async def connect(cls, uri: str, token: str) -> WebSocketTransport:
        try:
            connection = await connect(
                uri,
                additional_headers={"Authorization": f"Bearer {token}"},
                open_timeout=_CONNECT_TIMEOUT_SECONDS,
                close_timeout=_CLOSE_TIMEOUT_SECONDS,
                max_size=_MAX_MESSAGE_BYTES,
                proxy=None,
            )
        except (OSError, TimeoutError, ValueError, WebSocketException) as error:
            raise TransportError(f"remote WebSocket connection failed: {error}") from error
        return cls(connection)

    async def send(self, value: dict[str, object]) -> None:
        payload = json.dumps(value, separators=(",", ":"))
        try:
            await self._connection.send(payload)
        except (OSError, WebSocketException) as error:
            raise TransportError("remote WebSocket closed") from error

    async def receive(self) -> dict[str, object]:
        try:
            message = await self._connection.recv()
        except (OSError, WebSocketException) as error:
            raise TransportError("remote WebSocket closed") from error
        if not isinstance(message, str):
            raise TransportError("App Server WebSocket message must be text")
        try:
            value = json.loads(message)
        except json.JSONDecodeError as error:
            raise TransportError("App Server emitted invalid WebSocket JSON") from error
        if not isinstance(value, dict):
            raise TransportError("App Server message must be a JSON object")
        return value

    async def close(self) -> None:
        try:
            await self._connection.close()
        except (OSError, WebSocketException):
            pass
