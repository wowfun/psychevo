#!/usr/bin/env python3
import json
import sys


def send(value):
    print(json.dumps(value), flush=True)


def update(session_id, update):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": update
    }})


prompt_count = 0
for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": 1, "agentCapabilities": {}}})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-stream"}})
    elif method == "session/prompt":
        prompt_count += 1
        session_id = params.get("sessionId") or "native-stream"
        update(session_id, {"sessionUpdate": "session_info_update", "title": "ACP streamed title"})
        update(session_id, {"sessionUpdate": "available_commands_update", "availableCommands": [
            {"name": "research", "description": "Run peer research"}
        ]})
        update(session_id, {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "think "}})
        update(session_id, {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "first"}})
        update(session_id, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hello "}})
        update(session_id, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "world"}})
        update(session_id, {"sessionUpdate": "tool_call", "toolCallId": "call-echo", "title": "Run echo", "kind": "execute", "status": "pending", "rawInput": {"cmd": "echo done"}})
        update(session_id, {"sessionUpdate": "tool_call_update", "toolCallId": "call-echo", "status": "in_progress", "content": [
            {"type": "content", "content": {"type": "text", "text": "running\n"}}
        ]})
        update(session_id, {"sessionUpdate": "plan", "entries": [
            {"content": "Inspect repo", "priority": "high", "status": "completed"},
            {"content": "Patch bridge", "priority": "high", "status": "in_progress"}
        ]})
        update(session_id, {"sessionUpdate": "plan", "entries": [
            {"content": "Persist replacement plan", "priority": "high", "status": "completed"},
            {"content": "Verify terminal history", "priority": "high", "status": "in_progress"}
        ]})
        update(session_id, {"sessionUpdate": "tool_call_update", "toolCallId": "call-echo", "status": "completed", "content": [
            {"type": "content", "content": {"type": "text", "text": "done\n"}}
        ], "rawOutput": {"output": "done\n"}})
        usage = {
            "totalTokens": 144 if prompt_count == 1 else 200,
            "inputTokens": 100 if prompt_count == 1 else 140,
            "outputTokens": 44 if prompt_count == 1 else 60,
            "cachedReadTokens": 30 if prompt_count == 1 else 50,
            "thoughtTokens": 4 if prompt_count == 1 else 8
        }
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "stopReason": "end_turn",
            "usage": usage
        }})
        update(session_id, {"sessionUpdate": "agent_message_chunk", "content": {
            "type": "text", "text": "must remain after the response fence"
        }})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
