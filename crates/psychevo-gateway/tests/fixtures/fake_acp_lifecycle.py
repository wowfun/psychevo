#!/usr/bin/env python3
import json
import os
import sys
import time


LOG_PATH = os.environ["ACP_LIFECYCLE_LOG"]
MODE = os.environ.get("ACP_LIFECYCLE_MODE", "all")
next_callback_id = 9000
cleanup_probes = []
session_config = {"model": "test/default", "mode": "build"}


def config_options():
    return [
        {
            "id": "model",
            "name": "Model",
            "category": "model",
            "type": "select",
            "currentValue": session_config["model"],
            "options": [
                {"value": "test/default", "name": "Default model"},
                {"value": "test/second", "name": "Second model"},
            ],
        },
        {
            "id": "mode",
            "name": "Session Mode",
            "category": "mode",
            "type": "select",
            "currentValue": session_config["mode"],
            "options": [
                {"value": "build", "name": "build"},
                {"value": "plan", "name": "plan"},
            ],
        },
    ]


def emit(value):
    print(json.dumps(value, separators=(",", ":")), flush=True)


def record(**value):
    with open(LOG_PATH, "a", encoding="utf-8") as handle:
        handle.write(json.dumps(value, separators=(",", ":")) + "\n")


def respond(message_id, result):
    emit({"jsonrpc": "2.0", "id": message_id, "result": result})


def fail(message_id, code, message, data=None):
    error = {"code": code, "message": message}
    if data is not None:
        error["data"] = data
    emit({"jsonrpc": "2.0", "id": message_id, "error": error})


def session_update(session_id, update):
    emit({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {"sessionId": session_id, "update": update},
    })


def probe_cleaned_context(session_id):
    global next_callback_id
    callback_id = next_callback_id
    next_callback_id += 1
    emit({
        "jsonrpc": "2.0",
        "id": callback_id,
        "method": "fs/read_text_file",
        "params": {
            "sessionId": session_id,
            "path": "/definitely-not-read-after-cleanup",
        },
    })
    record(event="cleanup_probe_sent", callbackId=callback_id, sessionId=session_id)


for raw_line in sys.stdin:
    if not raw_line.strip():
        continue
    message = json.loads(raw_line)
    method = message.get("method")
    message_id = message.get("id")
    params = message.get("params") or {}
    if method is None:
        record(event="callback_response", callbackId=message_id, response=message)
        continue

    record(event="request", method=method, params=params, hasId=message_id is not None)
    if method == "initialize":
        is_codex = MODE.startswith("codex-auth-")
        session_capabilities = {}
        if MODE != "none":
            session_capabilities = {
                "list": {},
                "delete": {},
                "fork": {},
                "resume": {},
                "close": {},
            }
        if MODE == "no-delete":
            session_capabilities.pop("delete", None)
        respond(message_id, {
            "protocolVersion": 2 if MODE == "protocol-v2" else 1,
            "agentInfo": {
                "name": "@agentclientprotocol/codex-acp" if is_codex else "fixture-lifecycle-acp",
                "title": "Codex ACP" if is_codex else "Lifecycle fixture",
                "version": (
                    "1.1.3" if MODE == "codex-auth-future"
                    else "1.1.2" if is_codex
                    else "1.0.0"
                ),
            },
            "agentCapabilities": {
                "loadSession": True,
                "promptCapabilities": {
                    "image": is_codex,
                    "embeddedContext": is_codex,
                },
                "sessionCapabilities": session_capabilities,
                "mcpCapabilities": {},
            },
        })
    elif method == "authentication/status":
        if MODE == "codex-auth-unauthenticated":
            respond(message_id, {"type": "unauthenticated"})
        elif MODE == "codex-auth-api-key":
            respond(message_id, {"type": "api-key"})
        elif MODE == "codex-auth-chat-gpt":
            respond(message_id, {"type": "chat-gpt", "email": "fixture@example.test"})
        elif MODE == "codex-auth-gateway":
            respond(message_id, {"type": "gateway", "name": "fixture-gateway"})
        else:
            fail(message_id, -32601, "authentication/status is unavailable")
    elif method == "session/new":
        respond(message_id, {
            "sessionId": "draft-native",
            "configOptions": config_options(),
        })
        # OpenCode schedules this notification after returning session/new.
        # Keep it after the response so the prepared receipt and resident
        # projection observe the same ordering as the real Agent.
        session_update("draft-native", {
            "sessionUpdate": "available_commands_update",
            "availableCommands": [{
                "name": "fixture_status",
                "description": "Show deterministic ACP fixture status",
            }],
        })
    elif method == "session/set_config_option":
        if params.get("configId") in session_config:
            session_config[params["configId"]] = params.get("value")
        respond(message_id, {"configOptions": config_options()})
        session_update(params["sessionId"], {
            "sessionUpdate": "session_info_update",
            "title": "Prepared fixture after control readback",
        })
        session_update(params["sessionId"], {
            "sessionUpdate": "usage_update",
            "used": 12,
            "size": 100,
        })
    elif method == "session/prompt":
        session_id = params["sessionId"]
        if MODE == "blocking-prompt":
            release_path = os.environ["ACP_LIFECYCLE_RELEASE"]
            record(event="prompt_blocked", sessionId=session_id)
            while not os.path.exists(release_path):
                time.sleep(0.01)
        session_update(session_id, {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "draft session response"},
        })
        respond(message_id, {"stopReason": "end_turn"})
    elif method == "session/load":
        respond(message_id, {"configOptions": config_options()})
    elif method == "session/resume":
        session_id = params["sessionId"]
        session_update(session_id, {
            "sessionUpdate": "current_mode_update",
            "currentModeId": "resume-mode",
        })
        respond(message_id, {
            "modes": {
                "currentModeId": "resume-mode",
                "availableModes": [{"id": "resume-mode", "name": "Resume mode"}],
            },
            "configOptions": [],
        })
    elif method == "session/fork":
        session_update("fork-native", {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "forked history"},
        })
        respond(message_id, {"sessionId": "fork-native", "configOptions": []})
    elif method == "session/list":
        if MODE == "auth-list":
            fail(
                message_id,
                -32000,
                "Authentication\nrequired",
                {"secret": "must-not-leak-from-agent-data"},
            )
            continue
        pending = list(cleanup_probes)
        cleanup_probes.clear()
        for session_id in pending:
            probe_cleaned_context(session_id)
        cwd = params.get("cwd") or "/fixture/workspace"
        respond(message_id, {
            "sessions": [{
                "sessionId": "listed-native",
                "cwd": cwd,
                "title": "Listed fixture",
            }],
            "nextCursor": "next-cursor",
        })
    elif method == "session/cancel":
        # Notifications have no response.
        pass
    elif method == "session/close":
        cleanup_probes.append(params["sessionId"])
        respond(message_id, {})
    elif method == "session/delete":
        if MODE == "delete-fails":
            fail(message_id, -32000, "fixture remote delete failed")
            continue
        cleanup_probes.append(params["sessionId"])
        respond(message_id, {})
    else:
        fail(message_id, -32601, f"unsupported fixture method: {method}")
