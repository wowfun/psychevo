#!/usr/bin/env python3
import json
import sys

log_path = sys.argv[1]
prompt_id = None


def send(value):
    print(json.dumps(value), flush=True)


def record(method):
    with open(log_path, "a", encoding="utf-8") as log_file:
        log_file.write(method + "\n")


for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    record(method or "response")
    if method == "initialize":
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {"protocolVersion": 1, "agentCapabilities": {}},
            }
        )
    elif method == "session/new":
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {"sessionId": "native-cancel"},
            }
        )
    elif method == "session/prompt":
        prompt_id = mid
    elif method == "session/cancel" and prompt_id is not None:
        send(
            {
                "jsonrpc": "2.0",
                "id": prompt_id,
                "result": {"stopReason": "end_turn"},
            }
        )
        prompt_id = None
