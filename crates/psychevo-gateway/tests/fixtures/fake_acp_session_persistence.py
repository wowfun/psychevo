#!/usr/bin/env python3
import json
import os
import sqlite3
import sys

loaded_session = None
counter_path = sys.argv[0] + ".counter"
process_counter_path = sys.argv[0] + ".processes"
try:
    with open(process_counter_path, "r", encoding="utf-8") as process_counter_file:
        process_counter = int(process_counter_file.read())
except (FileNotFoundError, ValueError):
    process_counter = 0
with open(process_counter_path, "w", encoding="utf-8") as process_counter_file:
    process_counter_file.write(str(process_counter + 1))


def send(value):
    print(json.dumps(value), flush=True)


def update(session_id, update):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": update
    }})


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
        try:
            with open(counter_path, "r", encoding="utf-8") as counter_file:
                counter = int(counter_file.read())
        except (FileNotFoundError, ValueError):
            counter = 0
        counter += 1
        with open(counter_path, "w", encoding="utf-8") as counter_file:
            counter_file.write(str(counter))
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-" + str(counter)}})
    elif method == "session/load":
        loaded_session = params.get("sessionId")
        update(loaded_session, {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "old answer from loaded history"}
        })
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "session/prompt":
        session_id = params.get("sessionId") or "native-1"
        with sqlite3.connect(os.environ["PSYCHEVO_BINDING_DB"]) as connection:
            persisted = connection.execute(
                "SELECT native_session_id FROM gateway_runtime_bindings WHERE native_session_id = ?",
                (session_id,),
            ).fetchone()
        if persisted != (session_id,):
            raise RuntimeError("native session binding was not persisted before prompt")
        chunks = []
        for block in params.get("prompt") or []:
            if block.get("type") == "text":
                chunks.append(block.get("text") or "")
        prefix = "loaded:" + loaded_session if loaded_session else "new:" + session_id
        text = prefix + ":" + "\n".join(chunks)
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text}
            }
        }})
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
