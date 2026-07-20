import json
import sys


def send(value):
    print(json.dumps(value), flush=True)


def update(session_id, value):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": value
    }})


def option(current):
    return {"id": "model", "name": "Model", "category": "model", "type": "select",
            "currentValue": current, "options": [
                {"value": "from-response", "name": "Response"},
                {"value": "from-update", "name": "Update"}
            ]}


for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": 1,
            "agentInfo": {"name": "fixture-acp", "title": "Fixture ACP", "version": "1.2.3"},
            "agentCapabilities": {
                "loadSession": True,
                "promptCapabilities": {"image": True, "audio": True, "embeddedContext": True},
                "sessionCapabilities": {
                    "list": {}, "delete": {}, "fork": {}, "resume": {}, "close": {},
                    "additionalDirectories": {}
                },
                "auth": {"logout": {}},
                "providers": {},
                "mcpCapabilities": {"http": True, "sse": True, "acp": True}
            }
        }})
    elif method == "session/load":
        session_id = params.get("sessionId")
        update(session_id, {"sessionUpdate": "agent_message_chunk",
                            "content": {"type": "text", "text": "loaded history"}})
        update(session_id, {"sessionUpdate": "available_commands_update", "availableCommands": [
            {"name": "review", "description": "Review this workspace",
             "input": {"hint": "workspace path", "_meta": {"secret": "drop"}}}
        ]})
        update(session_id, {"sessionUpdate": "current_mode_update", "currentModeId": "plan"})
        update(session_id, {"sessionUpdate": "config_option_update", "configOptions": [option("from-update")]})
        update(session_id, {"sessionUpdate": "session_info_update", "title": "Loaded fixture"})
        update(session_id, {"sessionUpdate": "usage_update", "used": 42, "size": 100,
                            "cost": {"amount": 0.25, "currency": "USD", "_meta": {"secret": "drop"}},
                            "_meta": {"secret": "drop"}})
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "modes": {"currentModeId": "ask", "availableModes": [
                {"id": "ask", "name": "Ask", "description": "Answer questions"},
                {"id": "plan", "name": "Plan", "description": "Plan changes"}
            ]},
            "configOptions": [option("from-response")]
        }})
        update(session_id, {"sessionUpdate": "current_mode_update", "currentModeId": "ask"})
    elif method == "session/set_mode":
        session_id = params.get("sessionId")
        update(session_id, {"sessionUpdate": "current_mode_update",
                            "currentModeId": params.get("modeId")})
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "session/close":
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    else:
        send({"jsonrpc": "2.0", "id": mid,
              "error": {"code": -32601, "message": "method not found"}})
