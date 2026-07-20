#!/usr/bin/env python3
import json
import sys

log_path = sys.argv[1]
values = {"model": "test/default-model", "effort": "low", "mode": "ask", "fast": False}
next_session_id = 0


def send(value):
    print(json.dumps(value), flush=True)


def record(value):
    with open(log_path, "a", encoding="utf-8") as log_file:
        log_file.write(json.dumps(value) + "\n")


def update(session_id, update_value):
    send(
        {
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {"sessionId": session_id, "update": update_value},
        }
    )


def config_options():
    return [
        {
            "id": "model",
            "name": "Model",
            "category": "model",
            "type": "select",
            "currentValue": values["model"],
            "options": [
                {"value": "test/default-model", "name": "Default"},
                {"value": "test/second-model", "name": "Second"},
            ],
        },
        {
            "id": "effort",
            "name": "Effort",
            "category": "thought_level",
            "type": "select",
            "currentValue": values["effort"],
            "options": [
                {"value": "low", "name": "Low"},
                {"value": "high", "name": "High"},
            ],
        },
        {
            "id": "mode",
            "name": "Mode",
            "category": "mode",
            "type": "select",
            "currentValue": values["mode"],
            "options": [
                {"value": "ask", "name": "Ask"},
                {"value": "code", "name": "Code"},
            ],
        },
        {"id": "fast", "name": "Fast", "type": "boolean", "currentValue": values["fast"]},
    ]


for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        record({"event": "initialize", "version": params.get("protocolVersion")})
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {
                    "protocolVersion": 1,
                    "agentCapabilities": {
                        "loadSession": True,
                        "promptCapabilities": {"image": True, "embeddedContext": True},
                        "sessionCapabilities": {"close": {}},
                    },
                },
            }
        )
    elif method == "session/new":
        next_session_id += 1
        record({"event": "new"})
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {
                    "sessionId": "native-v1-contract-" + str(next_session_id),
                    "configOptions": config_options(),
                },
            }
        )
    elif method == "session/set_config_option":
        config_id = params.get("configId")
        value = params.get("value")
        if isinstance(value, dict):
            value = value.get("value", value.get("boolean"))
        values[config_id] = value
        record({"event": "set", "id": config_id, "value": value})
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {"configOptions": config_options()},
            }
        )
    elif method == "session/prompt":
        blocks = params.get("prompt") or []
        types = [block.get("type") for block in blocks]
        resource = next(
            (
                block.get("resource") or {}
                for block in blocks
                if block.get("type") == "resource"
            ),
            {},
        )
        image = next((block for block in blocks if block.get("type") == "image"), {})
        record(
            {
                "event": "prompt",
                "types": types,
                "resourceText": resource.get("text"),
                "resourceMime": resource.get("mimeType"),
                "imageMime": image.get("mimeType"),
                "imageDataLength": len(image.get("data") or ""),
                "values": values,
            }
        )
        update(
            params.get("sessionId"),
            {"sessionUpdate": "_future_status", "label": "forward compatible"},
        )
        text = (
            "structured:"
            + ",".join(types)
            + ":"
            + values["model"]
            + ":"
            + values["effort"]
            + ":"
            + values["mode"]
            + ":"
            + str(values["fast"]).lower()
        )
        update(
            params.get("sessionId"),
            {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text},
            },
        )
        send(
            {
                "jsonrpc": "2.0",
                "id": mid,
                "result": {"stopReason": "end_turn"},
            }
        )
    elif method == "session/close":
        record({"event": "close", "sessionId": params.get("sessionId")})
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
