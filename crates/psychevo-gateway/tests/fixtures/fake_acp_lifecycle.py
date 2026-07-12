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


def legacy_models():
    return {
        "currentModelId": session_config["model"],
        "availableModels": [
            {
                "modelId": "test/default",
                "name": "Default model",
                "description": "Legacy default",
            },
            {
                "modelId": "test/second",
                "name": "Second model",
                "description": "Legacy second",
            },
        ],
    }


def uses_legacy_models():
    return MODE in {
        "legacy-models",
        "legacy-models-error",
        "legacy-models-and-config",
    }


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
                "loadSession": MODE != "resume-only",
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
        result = {"sessionId": "draft-native"}
        if MODE != "legacy-models" and MODE != "legacy-models-error":
            result["configOptions"] = config_options()
        if uses_legacy_models():
            result["models"] = legacy_models()
        respond(message_id, result)
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
    elif method == "session/set_model":
        if MODE == "legacy-models-error":
            fail(message_id, -32001, "legacy model switch rejected")
        else:
            session_config["model"] = params.get("modelId")
            respond(message_id, {})
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
        session_id = params["sessionId"]
        if session_id == "listed-native" and MODE == "history-replay-review":
            session_update(session_id, {
                "sessionUpdate": "user_message_chunk",
                "messageId": "history-user-reliable",
                "content": {"type": "text", "text": "Reliable imported question"},
            })
            session_update(session_id, {
                "sessionUpdate": "user_message_chunk",
                "content": {"type": "text", "text": "Unidentified imported question"},
            })
            session_update(session_id, {
                "sessionUpdate": "agent_message_chunk",
                "messageId": "history-assistant-ordered",
                "content": {"type": "text", "text": "Before tool"},
            })
            session_update(session_id, {
                "sessionUpdate": "tool_call",
                "toolCallId": "history-tool-ordered",
                "title": "Inspect ordered history",
                "kind": "execute",
                "status": "pending",
                "rawInput": {"cmd": "printf ordered"},
            })
            session_update(session_id, {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "history-tool-ordered",
                "status": "completed",
                "rawOutput": {"output": "ordered tool output\n"},
            })
            session_update(session_id, {
                "sessionUpdate": "agent_message_chunk",
                "messageId": "history-assistant-ordered",
                "content": {"type": "text", "text": "After tool"},
            })
            session_update(session_id, {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": "Unidentified imported answer"},
            })
            for content, status in [
                ("Inspect replay", "pending"),
                ("Implement replay", "in_progress"),
                ("Verify replay", "completed"),
            ]:
                session_update(session_id, {
                    "sessionUpdate": "plan",
                    "entries": [{
                        "content": content,
                        "priority": "high",
                        "status": status,
                    }],
                })
        elif session_id == "listed-native":
            session_update(session_id, {
                "sessionUpdate": "user_message_chunk",
                "messageId": "history-user-1",
                "content": {"type": "text", "text": "Imported user question"},
            })
            session_update(session_id, {
                "sessionUpdate": "agent_thought_chunk",
                "messageId": "history-assistant-1",
                "content": {"type": "text", "text": "Imported reasoning"},
            })
            session_update(session_id, {
                "sessionUpdate": "agent_message_chunk",
                "messageId": "history-assistant-1",
                "content": {"type": "text", "text": "Imported assistant answer"},
            })
            session_update(session_id, {
                "sessionUpdate": "tool_call",
                "toolCallId": "history-tool-1",
                "title": "Inspect imported history",
                "kind": "execute",
                "status": "pending",
                "rawInput": {"cmd": "printf imported"},
            })
            session_update(session_id, {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "history-tool-1",
                "status": "completed",
                "content": [{
                    "type": "content",
                    "content": {"type": "text", "text": "imported tool output\n"},
                }],
                "rawOutput": {"output": "imported tool output\n"},
            })
            session_update(session_id, {
                "sessionUpdate": "plan",
                "entries": [{
                    "content": "Verify imported replay",
                    "priority": "high",
                    "status": "completed",
                }],
            })
        result = {}
        if MODE != "legacy-models" and MODE != "legacy-models-error":
            result["configOptions"] = config_options()
        if uses_legacy_models():
            result["models"] = legacy_models()
        respond(message_id, result)
    elif method == "session/resume":
        session_id = params["sessionId"]
        session_update(session_id, {
            "sessionUpdate": "current_mode_update",
            "currentModeId": "resume-mode",
        })
        result = {
            "modes": {
                "currentModeId": "resume-mode",
                "availableModes": [{"id": "resume-mode", "name": "Resume mode"}],
            },
            "configOptions": [],
        }
        if uses_legacy_models():
            result["models"] = legacy_models()
        respond(message_id, result)
    elif method == "session/fork":
        session_update("fork-native", {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "forked history"},
        })
        result = {"sessionId": "fork-native", "configOptions": []}
        if uses_legacy_models():
            result["models"] = legacy_models()
        respond(message_id, result)
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
