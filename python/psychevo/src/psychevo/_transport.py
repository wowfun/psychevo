from __future__ import annotations

import asyncio
import base64
import hashlib
import json
import os
import ssl
import struct
from abc import ABC, abstractmethod
from collections.abc import Sequence
from urllib.parse import urlsplit

from .errors import TransportError

_WEBSOCKET_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
_MAX_FRAME_BYTES = 16 * 1024 * 1024


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
        )
        return cls(process)

    async def send(self, value: dict[str, object]) -> None:
        payload = json.dumps(value, separators=(",", ":")).encode("utf-8") + b"\n"
        async with self._write_lock:
            self._stdin.write(payload)
            try:
                await self._stdin.drain()
            except (BrokenPipeError, ConnectionResetError) as error:
                raise TransportError("App Server stdin closed") from error

    async def receive(self) -> dict[str, object]:
        line = await self._stdout.readline()
        if not line:
            code = await self._process.wait()
            raise TransportError(f"App Server stdout closed with exit code {code}")
        try:
            value = json.loads(line)
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
    def __init__(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        self._reader = reader
        self._writer = writer
        self._write_lock = asyncio.Lock()

    @classmethod
    async def connect(cls, uri: str, token: str) -> WebSocketTransport:
        parsed = urlsplit(uri)
        if parsed.scheme not in {"ws", "wss"}:
            raise TransportError("remote URI must use ws:// or wss://")
        if not parsed.hostname:
            raise TransportError("remote WebSocket URI has no host")
        port = parsed.port or (443 if parsed.scheme == "wss" else 80)
        tls = ssl.create_default_context() if parsed.scheme == "wss" else None
        reader, writer = await asyncio.open_connection(
            parsed.hostname,
            port,
            ssl=tls,
            server_hostname=parsed.hostname if tls else None,
        )
        path = parsed.path or "/"
        if parsed.query:
            path = f"{path}?{parsed.query}"
        key = base64.b64encode(os.urandom(16)).decode("ascii")
        host = parsed.hostname if parsed.port is None else f"{parsed.hostname}:{parsed.port}"
        request = (
            f"GET {path} HTTP/1.1\r\n"
            f"Host: {host}\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n"
            f"Authorization: Bearer {token}\r\n"
            "\r\n"
        )
        writer.write(request.encode("ascii"))
        await writer.drain()
        status = await reader.readline()
        if not status.startswith(b"HTTP/1.1 101 "):
            writer.close()
            await writer.wait_closed()
            raise TransportError(
                f"WebSocket upgrade failed: {status.decode('ascii', 'replace').strip()}"
            )
        headers: dict[str, str] = {}
        while True:
            line = await reader.readline()
            if line in {b"\r\n", b"\n", b""}:
                break
            name, separator, value = line.decode("ascii", "replace").partition(":")
            if not separator:
                raise TransportError("invalid WebSocket upgrade header")
            headers[name.lower().strip()] = value.strip()
        expected = base64.b64encode(
            hashlib.sha1((key + _WEBSOCKET_GUID).encode("ascii")).digest()
        ).decode("ascii")
        if headers.get("sec-websocket-accept") != expected:
            writer.close()
            await writer.wait_closed()
            raise TransportError("WebSocket accept key mismatch")
        return cls(reader, writer)

    async def send(self, value: dict[str, object]) -> None:
        payload = json.dumps(value, separators=(",", ":")).encode("utf-8")
        await self._send_frame(0x1, payload)

    async def receive(self) -> dict[str, object]:
        chunks: list[bytes] = []
        message_opcode: int | None = None
        while True:
            fin, opcode, payload = await self._receive_frame()
            if opcode == 0x8:
                await self._send_frame(0x8, payload[:125])
                raise TransportError("remote WebSocket closed")
            if opcode == 0x9:
                await self._send_frame(0xA, payload)
                continue
            if opcode == 0xA:
                continue
            if opcode in {0x1, 0x2}:
                if message_opcode is not None:
                    raise TransportError("nested WebSocket data message")
                message_opcode = opcode
                chunks = [payload]
            elif opcode == 0x0:
                if message_opcode is None:
                    raise TransportError("unexpected WebSocket continuation")
                chunks.append(payload)
            else:
                raise TransportError(f"unsupported WebSocket opcode: {opcode}")
            if not fin:
                continue
            if message_opcode != 0x1:
                raise TransportError("App Server WebSocket message must be text")
            try:
                value = json.loads(b"".join(chunks))
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                raise TransportError("App Server emitted invalid WebSocket JSON") from error
            if not isinstance(value, dict):
                raise TransportError("App Server message must be a JSON object")
            return value

    async def close(self) -> None:
        if not self._writer.is_closing():
            try:
                await self._send_frame(0x8, struct.pack("!H", 1000))
            except (ConnectionError, TransportError):
                pass
            self._writer.close()
            await self._writer.wait_closed()

    async def _send_frame(self, opcode: int, payload: bytes) -> None:
        if len(payload) > _MAX_FRAME_BYTES:
            raise TransportError("WebSocket frame exceeds the SDK limit")
        first = 0x80 | opcode
        mask = os.urandom(4)
        length = len(payload)
        if length < 126:
            header = bytes((first, 0x80 | length))
        elif length <= 0xFFFF:
            header = bytes((first, 0x80 | 126)) + struct.pack("!H", length)
        else:
            header = bytes((first, 0x80 | 127)) + struct.pack("!Q", length)
        masked = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
        async with self._write_lock:
            self._writer.write(header + mask + masked)
            try:
                await self._writer.drain()
            except (BrokenPipeError, ConnectionResetError) as error:
                raise TransportError("remote WebSocket closed") from error

    async def _receive_frame(self) -> tuple[bool, int, bytes]:
        try:
            first, second = await self._reader.readexactly(2)
        except asyncio.IncompleteReadError as error:
            raise TransportError("remote WebSocket closed") from error
        fin = bool(first & 0x80)
        opcode = first & 0x0F
        masked = bool(second & 0x80)
        length = second & 0x7F
        if length == 126:
            length = struct.unpack("!H", await self._reader.readexactly(2))[0]
        elif length == 127:
            length = struct.unpack("!Q", await self._reader.readexactly(8))[0]
        if length > _MAX_FRAME_BYTES:
            raise TransportError("WebSocket frame exceeds the SDK limit")
        mask = await self._reader.readexactly(4) if masked else None
        payload = await self._reader.readexactly(length)
        if mask is not None:
            payload = bytes(
                byte ^ mask[index % 4] for index, byte in enumerate(payload)
            )
        return fin, opcode, payload
