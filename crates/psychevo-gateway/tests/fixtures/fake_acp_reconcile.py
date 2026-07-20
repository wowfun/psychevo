#!/usr/bin/env python3
import json
import os
import sys

log_path = sys.argv[1]
state_path = sys.argv[2]


def load_state():
    try:
        with open(state_path, "r", encoding="utf-8") as state_file:
            return json.load(state_file)
    except (FileNotFoundError, json.JSONDecodeError):
        return {"promptCount": 0, "messages": []}


def save_state(state):
    with open(state_path, "w", encoding="utf-8") as state_file:
        json.dump(state, state_file)


def record(method, **fields):
    with open(log_path, "a", encoding="utf-8") as log_file:
        log_file.write(json.dumps({"method": method, **fields}) + "\n")


def send(value):
    print(json.dumps(value), flush=True)


def update(session_id, message_id, text):
    send(
        {
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "messageId": message_id,
                    "content": {"type": "text", "text": text},
                },
            },
        }
    )


for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        record(method)
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {
                    "protocolVersion": 1,
                    "agentCapabilities": {"loadSession": True},
                },
            }
        )
    elif method == "session/new":
        record(method)
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {"sessionId": "native-reconcile"},
            }
        )
    elif method == "session/load":
        state = load_state()
        record(method, sessionId=params.get("sessionId"))
        send(
            {
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": params.get("sessionId"),
                    "update": {
                        "sessionUpdate": "tool_call",
                        "toolCallId": "replayed-tool-only",
                        "title": "Replay tool-only fact",
                        "kind": "execute",
                        "status": "completed",
                    },
                },
            }
        )
        send(
            {
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": params.get("sessionId"),
                    "update": {
                        "sessionUpdate": "plan",
                        "entries": [
                            {
                                "content": "Replay replacement plan",
                                "priority": "high",
                                "status": "completed",
                            }
                        ],
                    },
                },
            }
        )
        for replay in state["messages"]:
            update(params.get("sessionId"), replay["id"], replay["text"])
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "session/prompt":
        state = load_state()
        state["promptCount"] += 1
        turn = state["promptCount"]
        prompt = "\n".join(
            block.get("text") or ""
            for block in params.get("prompt") or []
            if block.get("type") == "text"
        )
        answer = "reconciled answer " + str(turn)
        replay = {"id": "assistant-" + str(turn), "text": answer}
        state["messages"].append(replay)
        save_state(state)
        record(method, turn=turn, prompt=prompt)
        if turn == 1:
            os._exit(17)
        update(params.get("sessionId"), replay["id"], answer)
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {"stopReason": "end_turn"},
            }
        )
    else:
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "error": {"code": -32601, "message": "method not found"},
            }
        )
