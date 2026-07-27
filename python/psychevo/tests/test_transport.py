from __future__ import annotations

import asyncio
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1] / "src"))

import psychevo._transport as transport_module
from psychevo import TransportError
from psychevo._transport import StdioTransport, WebSocketTransport
from websockets.asyncio.server import ServerConnection, serve


class WebSocketTransportTests(unittest.IsolatedAsyncioTestCase):
    async def test_stdio_lines_share_one_bounded_message_size(self) -> None:
        original_limit = transport_module._MAX_MESSAGE_BYTES
        transport_module._MAX_MESSAGE_BYTES = 64
        try:
            transport = await StdioTransport.start(
                sys.executable,
                ("-c", "import sys; sys.stdout.buffer.write(b'x' * 65 + b'\\n')"),
            )
            try:
                with self.assertRaisesRegex(TransportError, "exceeds 64 bytes"):
                    await transport.receive()
            finally:
                await transport.close()

            transport = await StdioTransport.start(
                sys.executable,
                ("-c", "import sys; sys.stdin.buffer.readline()"),
            )
            try:
                with self.assertRaisesRegex(TransportError, "exceeds 64 bytes"):
                    await transport.send({"data": "x" * 80})
            finally:
                await transport.close()
        finally:
            transport_module._MAX_MESSAGE_BYTES = original_limit

    async def test_authenticated_text_json_round_trip(self) -> None:
        authorization = asyncio.Future[str]()

        async def handler(connection: ServerConnection) -> None:
            authorization.set_result(connection.request.headers["Authorization"])
            request = await connection.recv()
            self.assertEqual(request, '{"method":"ping"}')
            await connection.send('{"result":{"pong":true}}')

        async with serve(handler, "127.0.0.1", 0) as server:
            port = server.sockets[0].getsockname()[1]
            transport = await WebSocketTransport.connect(
                f"ws://127.0.0.1:{port}", "secret"
            )
            await transport.send({"method": "ping"})
            self.assertEqual(await transport.receive(), {"result": {"pong": True}})
            await transport.close()
        self.assertEqual(await authorization, "Bearer secret")

    async def test_binary_and_invalid_json_messages_fail_at_the_transport_boundary(
        self,
    ) -> None:
        async def binary_handler(connection: ServerConnection) -> None:
            await connection.send(b"{}")

        async with serve(binary_handler, "127.0.0.1", 0) as server:
            port = server.sockets[0].getsockname()[1]
            transport = await WebSocketTransport.connect(
                f"ws://127.0.0.1:{port}", "secret"
            )
            with self.assertRaisesRegex(TransportError, "must be text"):
                await transport.receive()
            await transport.close()

        async def invalid_json_handler(connection: ServerConnection) -> None:
            await connection.send("not-json")

        async with serve(invalid_json_handler, "127.0.0.1", 0) as server:
            port = server.sockets[0].getsockname()[1]
            transport = await WebSocketTransport.connect(
                f"ws://127.0.0.1:{port}", "secret"
            )
            with self.assertRaisesRegex(TransportError, "invalid WebSocket JSON"):
                await transport.receive()
            await transport.close()

    async def test_fragmented_aggregate_is_bounded_by_message_size(self) -> None:
        original_limit = transport_module._MAX_MESSAGE_BYTES
        transport_module._MAX_MESSAGE_BYTES = 64

        async def handler(connection: ServerConnection) -> None:
            await connection.send(['{"data":"', "x" * 80, '"}'])
            await connection.wait_closed()

        try:
            async with serve(handler, "127.0.0.1", 0) as server:
                port = server.sockets[0].getsockname()[1]
                transport = await WebSocketTransport.connect(
                    f"ws://127.0.0.1:{port}", "secret"
                )
                with self.assertRaisesRegex(TransportError, "WebSocket closed"):
                    await transport.receive()
                await transport.close()
        finally:
            transport_module._MAX_MESSAGE_BYTES = original_limit

    async def test_open_timeout_and_invalid_upgrade_map_to_transport_error(self) -> None:
        original_timeout = transport_module._CONNECT_TIMEOUT_SECONDS
        transport_module._CONNECT_TIMEOUT_SECONDS = 0.05

        async def quiet_peer(
            _reader: asyncio.StreamReader, writer: asyncio.StreamWriter
        ) -> None:
            try:
                await asyncio.sleep(1)
            finally:
                writer.close()
                await writer.wait_closed()

        try:
            server = await asyncio.start_server(quiet_peer, "127.0.0.1", 0)
            port = server.sockets[0].getsockname()[1]
            with self.assertRaisesRegex(TransportError, "connection failed"):
                await WebSocketTransport.connect(
                    f"ws://127.0.0.1:{port}", "secret"
                )
            server.close()
            await server.wait_closed()
        finally:
            transport_module._CONNECT_TIMEOUT_SECONDS = original_timeout

        async def invalid_upgrade(
            _reader: asyncio.StreamReader, writer: asyncio.StreamWriter
        ) -> None:
            writer.write(b"HTTP/1.1 101 Switching Protocols\r\n\r\n")
            await writer.drain()
            writer.close()
            await writer.wait_closed()

        server = await asyncio.start_server(invalid_upgrade, "127.0.0.1", 0)
        port = server.sockets[0].getsockname()[1]
        with self.assertRaisesRegex(TransportError, "connection failed"):
            await WebSocketTransport.connect(f"ws://127.0.0.1:{port}", "secret")
        server.close()
        await server.wait_closed()


if __name__ == "__main__":
    unittest.main()
