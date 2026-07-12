#!/usr/bin/env python3
"""Deterministic stable-v1 ACP agent used by the shared Agent conformance driver."""

import json
import os
from pathlib import Path
import sqlite3
import sys
import threading
import time


LOG_PATH = Path(sys.argv[1])
BINDING_DB = Path(sys.argv[2])
RELEASE_DIR = Path(sys.argv[3])
RELEASE_DIR.mkdir(parents=True, exist_ok=True)

send_lock = threading.Lock()
state_lock = threading.Lock()
next_session_id = 0
pending_prompts = {}
pending_permissions = {}
fast_by_session = {}
closed = False


def send(value):
    with send_lock:
        print(json.dumps(value), flush=True)


def record(**value):
    with state_lock:
        with LOG_PATH.open("a", encoding="utf-8") as log_file:
            log_file.write(json.dumps(value, sort_keys=True) + "\n")


def update(session_id, text):
    send(
        {
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": text},
                },
            },
        }
    )


def complete_prompt(session_id, outcome="end_turn"):
    with state_lock:
        pending = pending_prompts.pop(session_id, None)
    if pending is None:
        return
    prompt_id, prompt_text = pending
    update(session_id, "answer:" + prompt_text)
    send(
        {
            "jsonrpc": "2.0",
            "id": prompt_id,
            "result": {"stopReason": outcome},
        }
    )
    record(event="prompt_completed", sessionId=session_id, prompt=prompt_text)


def binding_exists(session_id):
    with sqlite3.connect(BINDING_DB) as connection:
        persisted = connection.execute(
            "SELECT native_session_id FROM gateway_runtime_bindings "
            "WHERE native_session_id = ?",
            (session_id,),
        ).fetchone()
    return persisted == (session_id,)


def release_watcher():
    while True:
        with state_lock:
            if closed:
                return
            held = [
                (session_id, prompt_text.removeprefix("hold:"))
                for session_id, (_, prompt_text) in pending_prompts.items()
                if prompt_text.startswith("hold:")
            ]
        for session_id, token in held:
            if (RELEASE_DIR / ("release-" + token)).exists():
                complete_prompt(session_id)
        time.sleep(0.005)


threading.Thread(target=release_watcher, daemon=True).start()

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    message_id = message.get("id")
    params = message.get("params") or {}

    if method == "initialize":
        record(event="initialize")
        send(
            {
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {
                    "protocolVersion": 1,
                    "agentCapabilities": {
                        "loadSession": True,
                        "promptCapabilities": {
                            "image": False,
                            "embeddedContext": False,
                        },
                        "sessionCapabilities": {"close": {}},
                    },
                },
            }
        )
    elif method == "session/new":
        next_session_id += 1
        session_id = "conformance-session-" + str(next_session_id)
        fast_by_session[session_id] = False
        record(event="session_new", sessionId=session_id)
        send(
            {
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {
                    "sessionId": session_id,
                    "configOptions": [
                        {
                            "id": "fast",
                            "name": "Fast mode",
                            "type": "boolean",
                            "currentValue": False,
                        }
                    ],
                },
            }
        )
    elif method == "session/load":
        record(event="session_load", sessionId=params.get("sessionId"))
        send({"jsonrpc": "2.0", "id": message_id, "result": {}})
    elif method == "session/prompt":
        session_id = params.get("sessionId")
        text_blocks = [
            block.get("text") or ""
            for block in params.get("prompt") or []
            if block.get("type") == "text"
        ]
        prompt_text = text_blocks[-1] if text_blocks else ""
        bound = binding_exists(session_id)
        record(
            event="prompt",
            sessionId=session_id,
            prompt=prompt_text,
            bindingBeforePrompt=bound,
        )
        if not bound:
            raise RuntimeError("Agent session binding was not persisted before prompt")
        with state_lock:
            pending_prompts[session_id] = (message_id, prompt_text)
        if prompt_text == "crash-on-prompt":
            os._exit(0)
        if prompt_text == "permission":
            permission_id = "permission-request-" + session_id
            with state_lock:
                pending_permissions[permission_id] = session_id
            send(
                {
                    "jsonrpc": "2.0",
                    "id": permission_id,
                    "method": "session/request_permission",
                    "params": {
                        "sessionId": session_id,
                        "toolCall": {
                            "toolCallId": "permission-1",
                            "title": "Conformance tool",
                            "kind": "execute",
                            "status": "pending",
                        },
                        "options": [
                            {
                                "optionId": "allow-once",
                                "name": "Allow once",
                                "kind": "allow_once",
                            },
                            {
                                "optionId": "reject-once",
                                "name": "Reject once",
                                "kind": "reject_once",
                            },
                        ],
                    },
                }
            )
        elif not prompt_text.startswith("hold:"):
            complete_prompt(session_id)
    elif method == "session/cancel":
        session_id = params.get("sessionId")
        record(event="cancel", sessionId=session_id)
        complete_prompt(session_id)
    elif method == "session/set_config_option":
        session_id = params.get("sessionId")
        fast = params.get("value")
        fast_by_session[session_id] = fast
        record(
            event="set_control",
            sessionId=session_id,
            controlId=params.get("configId"),
            value=fast,
        )
        send(
            {
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {
                    "configOptions": [
                        {
                            "id": "fast",
                            "name": "Fast mode",
                            "type": "boolean",
                            "currentValue": fast,
                        }
                    ]
                },
            }
        )
    elif method == "session/close":
        session_id = params.get("sessionId")
        record(event="close", sessionId=session_id)
        send({"jsonrpc": "2.0", "id": message_id, "result": {}})
    elif method is None and message_id in pending_permissions:
        with state_lock:
            session_id = pending_permissions.pop(message_id)
        record(
            event="permission_response",
            sessionId=session_id,
            result=message.get("result"),
        )
        complete_prompt(session_id)
    elif message_id is not None:
        send(
            {
                "jsonrpc": "2.0",
                "id": message_id,
                "error": {"code": -32601, "message": "method not found"},
            }
        )

with state_lock:
    closed = True
