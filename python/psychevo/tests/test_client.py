from __future__ import annotations

import asyncio
import os
import stat
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1] / "src"))

from psychevo import (
    ApprovalDecision,
    Client,
    Tool,
    ToolResult,
    TransportError,
)
from psychevo._client import TurnHandle, _RpcClient
from psychevo._transport import Transport
from psychevo._types import TurnReceipt


_FAKE_SERVER = r"""
import json
import sys

thread = {
    "id": "thread-1",
    "source": "python",
    "cwd": "/tmp/work",
    "title": None,
    "startedAtMs": 1,
    "updatedAtMs": 1,
    "archived": False,
    "messageCount": 0,
    "toolCallCount": 0,
    "items": [{
        "sessionSeq": 1,
        "message": {"role": "assistant", "content": []},
        "usage": None,
        "metadata": None,
        "accounting": None,
    }],
}
thread_two = {**thread, "id": "thread-2", "updatedAtMs": 0}
registered = False
callback_answer = "hello"

for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    request_id = request.get("id")
    if method == "initialized":
        continue
    if method == "initialize":
        result = {
            "server": {"name": "fake", "version": "0.1.0"},
            "protocolVersion": 1,
            "protocolMin": 1,
            "protocolMax": 1,
            "capabilities": {},
        }
    elif method in {"thread/start", "thread/resume", "thread/read"}:
        result = thread
    elif method == "thread/list":
        if request["params"].get("cursor") == "page-2":
            result = {"threads": [thread_two], "nextCursor": None}
        else:
            result = {"threads": [thread], "nextCursor": "page-2"}
    elif method == "thread/archive":
        result = {"archived": True, "threadId": "thread-1"}
    elif method == "tool/register":
        registered = True
        result = {
            "registered": True,
            "toolCount": len(request["params"]["tools"]),
            "approvalHandler": request["params"]["approvalHandler"],
        }
    elif method == "turn/start":
        turn_id = request["params"]["turnId"]
        if registered and request["params"]["prompt"] == "callback":
            print(json.dumps({
                "jsonrpc": "2.0",
                "id": "server:tool-1",
                "method": "tool/call",
                "params": {
                    "callId": "call-1",
                    "toolName": "echo",
                    "arguments": {"text": "from server"},
                    "threadId": "thread-1",
                    "turnId": turn_id,
                },
            }), flush=True)
            callback = json.loads(sys.stdin.readline())
            callback_answer = callback["result"]["result"]["text"]
        if registered and request["params"]["prompt"] == "approval":
            print(json.dumps({
                "jsonrpc": "2.0",
                "id": "server:approval-1",
                "method": "approval/request",
                "params": {
                    "callId": "approval-1",
                    "threadId": "thread-1",
                    "turnId": turn_id,
                    "toolCallId": "tool-call-1",
                    "toolName": "exec",
                    "summary": "Run a command",
                    "reason": "The command changes files",
                    "matchedRule": None,
                    "suggestedRule": "exec:*",
                    "allowAlways": True,
                    "filesystem": {"write": ["/tmp/work"]},
                },
            }), flush=True)
            callback = json.loads(sys.stdin.readline())
            callback_answer = callback["result"]["outcome"]
        if request["params"]["prompt"] == "overflow":
            for index in range(300):
                print(json.dumps({
                    "jsonrpc": "2.0",
                    "method": "turn/event",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": turn_id,
                        "event": {
                            "type": "warning",
                            "data": {"index": index},
                        },
                    },
                }), flush=True)
        if request["params"]["prompt"] == "clarify":
            print(json.dumps({
                "jsonrpc": "2.0",
                "method": "turn/event",
                "params": {
                    "threadId": "thread-1",
                    "turnId": turn_id,
                    "event": {
                        "type": "interaction_requested",
                        "interactionId": "clarify-1",
                        "kind": "clarify",
                        "payload": [{"question": "Proceed?"}],
                    },
                },
            }), flush=True)
        result = {
            "accepted": True,
            "threadId": "thread-1",
            "turnId": turn_id,
            "clientTurnId": request["params"].get("clientTurnId"),
        }
        print(json.dumps({
            "jsonrpc": "2.0",
            "method": "turn/event",
            "params": {
                "threadId": "thread-1",
                "turnId": turn_id,
                "event": {
                    "type": "message",
                    "stage": "completed",
                    "message": {"text": "hello"},
                },
            },
        }), flush=True)
        print(json.dumps({
            "jsonrpc": "2.0",
            "method": "turn/event",
            "params": {
                "threadId": "thread-1",
                "turnId": turn_id,
                "event": {
                    "type": "completed",
                    "threadId": "thread-1",
                    "turnId": turn_id,
                    "outcome": "completed",
                },
            },
        }), flush=True)
    elif method == "turn/resume":
        if request["params"]["turnId"] == "turn-resume-clarify":
            print(json.dumps({
                "jsonrpc": "2.0",
                "method": "turn/event",
                "params": {
                    "threadId": "thread-1",
                    "turnId": request["params"]["turnId"],
                    "event": {
                        "type": "interaction_requested",
                        "interactionId": "clarify-resume-1",
                        "kind": "clarify",
                        "payload": [{"question": "Resume?"}],
                    },
                },
            }), flush=True)
        print(json.dumps({
            "jsonrpc": "2.0",
            "method": "turn/event",
            "params": {
                "threadId": "thread-1",
                "turnId": request["params"]["turnId"],
                "event": {
                    "type": "completed",
                    "threadId": "thread-1",
                    "turnId": request["params"]["turnId"],
                    "outcome": "completed",
                },
            },
        }), flush=True)
        result = {
            "accepted": True,
            "threadId": "thread-1",
            "turnId": request["params"]["turnId"],
            "clientTurnId": None,
        }
    elif method == "turn/wait":
        result = {
            "threadId": "thread-1",
            "outcome": "completed",
            "finalAnswer": callback_answer,
            "provider": "fake",
            "model": "fake",
            "reasoningEffort": None,
            "toolFailures": 0,
        }
    elif method == "interaction/respond":
        callback_answer = json.dumps(request["params"]["response"], sort_keys=True)
        result = {
            "accepted": True,
            "turnId": request["params"]["turnId"],
            "interactionId": request["params"]["interactionId"],
        }
    elif method == "shutdown":
        result = {"shutdown": True}
    else:
        result = {"accepted": True}
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)
"""


class ClientTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.server = Path(self.temp.name) / "fake-app-server"
        self.server.write_text(
            "#!/usr/bin/env python3\n" + textwrap.dedent(_FAKE_SERVER),
            encoding="utf-8",
        )
        self.server.chmod(self.server.stat().st_mode | stat.S_IXUSR)

    async def asyncTearDown(self) -> None:
        self.temp.cleanup()

    async def test_stdio_thread_turn_events_and_wait(self) -> None:
        async with Client(executable=self.server) as client:
            thread = await client.start_thread(cwd=self.temp.name)
            self.assertEqual(thread.id, "thread-1")
            turn = await thread.start_turn("hello", client_turn_id="client-1")
            self.assertEqual(turn.receipt.client_turn_id, "client-1")
            events = [event async for event in turn.events()]
            self.assertEqual([event.type for event in events], ["message", "completed"])
            result = await turn.wait()
            self.assertEqual(
                (await client.list_threads()).threads[0].id,
                "thread-1",
            )
            self.assertEqual((await thread.snapshot()).items[0].session_seq, 1)
        self.assertEqual(result.final_answer, "hello")

    async def test_resume_registers_its_event_sink_before_the_request(self) -> None:
        async with Client(executable=self.server) as client:
            turn = await client.resume_turn("turn-resume")
            events = [event async for event in turn.events()]
        self.assertEqual(turn.receipt.turn_id, "turn-resume")
        self.assertEqual([event.type for event in events], ["completed"])

    async def test_resume_clarify_before_receipt_uses_event_thread_identity(self) -> None:
        requests = []
        handled = asyncio.Event()

        async def clarify(request):
            requests.append(request)
            handled.set()
            return [["yes"]]

        async with Client(executable=self.server, clarify_handler=clarify) as client:
            turn = await client.resume_turn("turn-resume-clarify")
            await asyncio.wait_for(handled.wait(), timeout=1)

        self.assertEqual(turn.receipt.thread_id, "thread-1")
        self.assertEqual(requests[0].thread_id, "thread-1")
        self.assertEqual(requests[0].turn_id, "turn-resume-clarify")

    async def test_iter_threads_fetches_each_bounded_page(self) -> None:
        async with Client(executable=self.server) as client:
            self.assertEqual(
                [thread.id async for thread in client.iter_threads(page_size=1)],
                ["thread-1", "thread-2"],
            )

    async def test_explicit_remote_requires_token(self) -> None:
        with self.assertRaisesRegex(ValueError, "bearer token"):
            Client(remote_url="ws://127.0.0.1:1234/app-server")

    async def test_pre_registered_turn_sink_reports_resync_on_overflow(self) -> None:
        async with Client(executable=self.server) as client:
            thread = await client.start_thread(cwd=self.temp.name)
            turn = await thread.start_turn("overflow")
            events = [event async for event in turn.events()]
        self.assertIn("resync_required", [event.type for event in events])

    async def test_custom_tool_is_registered_and_routed_to_async_handler(self) -> None:
        calls = []

        async def echo(call):
            calls.append(call)
            return ToolResult({"text": call.arguments["text"]})

        tool = Tool(
            name="echo",
            description="Echo a text value",
            parameters={
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "required": ["text"],
            },
            handler=echo,
        )
        async with Client(
            executable=self.server,
            tools=[tool],
            approval_handler=lambda _request: _allow_once(),
        ) as client:
            thread = await client.start_thread(cwd=self.temp.name)
            turn = await thread.start_turn("callback")
            result = await turn.wait()
        self.assertEqual(result.final_answer, "from server")
        self.assertEqual(calls[0].thread_id, "thread-1")
        self.assertEqual(calls[0].turn_id, turn.receipt.turn_id)

    async def test_clarify_event_uses_the_pre_registered_turn_identity(self) -> None:
        requests = []
        handled = asyncio.Event()

        async def clarify(request):
            requests.append(request)
            handled.set()
            return [["yes"]]

        async with Client(executable=self.server, clarify_handler=clarify) as client:
            thread = await client.start_thread(cwd=self.temp.name)
            turn = await thread.start_turn("clarify")
            await asyncio.wait_for(handled.wait(), timeout=1)
        self.assertEqual(requests[0].thread_id, "thread-1")
        self.assertEqual(requests[0].turn_id, turn.receipt.turn_id)

    async def test_approval_is_routed_to_async_handler_and_returns_typed_decision(
        self,
    ) -> None:
        requests = []

        async def approve(request):
            requests.append(request)
            return ApprovalDecision("deny")

        async with Client(
            executable=self.server,
            approval_handler=approve,
        ) as client:
            thread = await client.start_thread(cwd=self.temp.name)
            turn = await thread.start_turn("approval")
            result = await turn.wait()
        self.assertEqual(result.final_answer, "deny")
        self.assertEqual(requests[0].tool_name, "exec")
        self.assertEqual(requests[0].thread_id, "thread-1")
        self.assertEqual(requests[0].turn_id, turn.receipt.turn_id)

    async def test_pending_permission_response_uses_typed_interaction_payload(self) -> None:
        async with Client(executable=self.server) as client:
            thread = await client.start_thread(cwd=self.temp.name)
            turn = await thread.start_turn("hello")
            accepted = await turn.respond(
                "permission-1",
                ApprovalDecision("allow_session", "/tmp/work"),
            )
            result = await turn.wait()
        self.assertTrue(accepted)
        self.assertEqual(
            result.final_answer,
            '{"filesystemDirectory": "/tmp/work", "kind": "permission", '
            '"outcome": "allow_session"}',
        )

    async def test_default_transport_never_searches_path(self) -> None:
        old_path = os.environ.get("PATH")
        os.environ["PATH"] = self.temp.name
        try:
            client = Client()
            with self.assertRaisesRegex(TransportError, "exact-version"):
                await client.connect()
        finally:
            if old_path is None:
                os.environ.pop("PATH", None)
            else:
                os.environ["PATH"] = old_path


class _FailingTransport(Transport):
    def __init__(self) -> None:
        self.sent: list[dict[str, object]] = []
        self.fail = asyncio.Event()
        self.closed = False
        self.send_error: TransportError | None = None

    async def send(self, value: dict[str, object]) -> None:
        self.sent.append(value)
        if self.send_error is not None:
            raise self.send_error

    async def receive(self) -> dict[str, object]:
        await self.fail.wait()
        raise TransportError("reader broken")

    async def close(self) -> None:
        self.closed = True


class RpcLifecycleTests(unittest.IsolatedAsyncioTestCase):
    async def test_request_send_failure_is_a_permanent_terminal_transition(
        self,
    ) -> None:
        transport = _FailingTransport()
        transport.send_error = TransportError("request send broken")
        rpc = _RpcClient(
            transport,
            tools=(),
            approval_handler=None,
            clarify_handler=None,
        )

        with self.assertRaisesRegex(TransportError, "request send broken"):
            await rpc.request("thread/archive", {"threadId": "thread-1"})
        sends_after_failure = len(transport.sent)
        with self.assertRaisesRegex(TransportError, "request send broken"):
            await rpc.request("thread/read", {"threadId": "thread-1"})

        self.assertEqual(len(transport.sent), sends_after_failure)
        self.assertFalse(rpc._pending)

    async def test_notification_send_failure_is_a_permanent_terminal_transition(
        self,
    ) -> None:
        transport = _FailingTransport()
        transport.send_error = TransportError("notification send broken")
        rpc = _RpcClient(
            transport,
            tools=(),
            approval_handler=None,
            clarify_handler=None,
        )

        with self.assertRaisesRegex(TransportError, "notification send broken"):
            await rpc.notify("initialized", {})
        sends_after_failure = len(transport.sent)
        with self.assertRaisesRegex(TransportError, "notification send broken"):
            await rpc.notify("initialized", {})

        self.assertEqual(len(transport.sent), sends_after_failure)

    async def test_callback_response_send_failure_is_terminal(self) -> None:
        transport = _FailingTransport()
        transport.send_error = TransportError("callback send broken")
        rpc = _RpcClient(
            transport,
            tools=(),
            approval_handler=None,
            clarify_handler=None,
        )

        with self.assertRaisesRegex(TransportError, "callback send broken"):
            await rpc._handle_callback_request(
                {
                    "jsonrpc": "2.0",
                    "id": "server:callback-1",
                    "method": "unknown/callback",
                    "params": {},
                }
            )
        sends_after_failure = len(transport.sent)
        with self.assertRaisesRegex(TransportError, "callback send broken"):
            await rpc.request("thread/read", {"threadId": "thread-1"})

        self.assertEqual(len(transport.sent), sends_after_failure)

    async def test_reader_failure_is_the_single_permanent_terminal_transition(
        self,
    ) -> None:
        transport = _FailingTransport()
        rpc = _RpcClient(
            transport,
            tools=(),
            approval_handler=None,
            clarify_handler=None,
        )
        rpc._reader = asyncio.create_task(rpc._read_loop())
        callback_started = asyncio.Event()
        callback_cancelled = asyncio.Event()

        async def callback() -> None:
            callback_started.set()
            try:
                await asyncio.Future()
            finally:
                callback_cancelled.set()

        rpc._spawn_callback(callback())
        await callback_started.wait()
        pending = asyncio.create_task(rpc.request("thread/list", {}))
        while not transport.sent:
            await asyncio.sleep(0)

        transport.fail.set()
        with self.assertRaisesRegex(TransportError, "reader broken"):
            await pending
        await rpc._reader
        await asyncio.wait_for(callback_cancelled.wait(), timeout=1)

        sends_after_failure = len(transport.sent)
        with self.assertRaisesRegex(TransportError, "reader broken"):
            await rpc.request("thread/read", {"threadId": "thread-1"})
        with self.assertRaisesRegex(TransportError, "reader broken"):
            await rpc.notify("initialized", {})
        self.assertEqual(len(transport.sent), sends_after_failure)
        self.assertFalse(rpc._pending)
        self.assertFalse(rpc._callbacks)

    async def test_terminal_turn_event_releases_all_connection_registry_state(
        self,
    ) -> None:
        transport = _FailingTransport()
        rpc = _RpcClient(
            transport,
            tools=(),
            approval_handler=None,
            clarify_handler=None,
        )
        receipt = TurnReceipt(
            accepted=True,
            thread_id="thread-1",
            turn_id="turn-1",
            client_turn_id=None,
        )
        turn = TurnHandle(None, receipt)  # type: ignore[arg-type]
        rpc.register_turn(turn)
        rpc._receive_notification(
            {
                "method": "turn/event",
                "params": {
                    "threadId": receipt.thread_id,
                    "turnId": receipt.turn_id,
                    "event": {
                        "type": "completed",
                        "threadId": receipt.thread_id,
                        "turnId": receipt.turn_id,
                        "outcome": "completed",
                    },
                },
            }
        )

        self.assertNotIn(receipt.turn_id, rpc._turns)
        self.assertEqual(
            [event.type async for event in turn.events()],
            ["completed"],
        )


if __name__ == "__main__":
    unittest.main()


async def _allow_once() -> ApprovalDecision:
    return ApprovalDecision("allow_once")
