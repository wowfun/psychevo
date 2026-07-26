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
        result = {"threads": [thread]}
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
                    "turnId": "turn-1",
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
                    "turnId": "turn-1",
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
                        "turnId": "turn-1",
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
                    "turnId": "turn-1",
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
            "turnId": "turn-1",
            "clientTurnId": request["params"].get("clientTurnId"),
        }
        print(json.dumps({
            "jsonrpc": "2.0",
            "method": "turn/event",
            "params": {
                "turnId": "turn-1",
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
                "turnId": "turn-1",
                "event": {
                    "type": "completed",
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "outcome": "completed",
                },
            },
        }), flush=True)
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
            self.assertEqual(result.final_answer, "hello")
            self.assertEqual((await client.list_threads())[0].id, "thread-1")
            self.assertEqual((await thread.snapshot()).items[0].session_seq, 1)

    async def test_explicit_remote_requires_token(self) -> None:
        with self.assertRaisesRegex(ValueError, "bearer token"):
            Client(remote_url="ws://127.0.0.1:1234/app-server")

    async def test_events_arriving_before_turn_receipt_report_resync_on_overflow(self) -> None:
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
        self.assertEqual(calls[0].turn_id, "turn-1")

    async def test_early_clarify_event_waits_for_turn_identity_before_callback(self) -> None:
        requests = []
        handled = asyncio.Event()

        async def clarify(request):
            requests.append(request)
            handled.set()
            return [["yes"]]

        async with Client(executable=self.server, clarify_handler=clarify) as client:
            thread = await client.start_thread(cwd=self.temp.name)
            await thread.start_turn("clarify")
            await asyncio.wait_for(handled.wait(), timeout=1)
        self.assertEqual(requests[0].thread_id, "thread-1")
        self.assertEqual(requests[0].turn_id, "turn-1")

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
        self.assertEqual(requests[0].turn_id, "turn-1")

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


if __name__ == "__main__":
    unittest.main()


async def _allow_once() -> ApprovalDecision:
    return ApprovalDecision("allow_once")
