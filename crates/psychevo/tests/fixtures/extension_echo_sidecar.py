#!/usr/bin/env python3
import json
import os
import pathlib
import sys
import threading


marker = pathlib.Path(os.environ["PSYCHEVO_EXTENSION_DATA"]) / "lifecycle.log"
connections = {}
state_lock = threading.Lock()
stdout_lock = threading.Lock()


def respond(request, result):
    with stdout_lock:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)


def respond_error(request, message):
    with stdout_lock:
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": request["id"],
            "error": {"code": -32601, "message": message}
        }), flush=True)


def handle(request):
    method = request.get("method")
    if method == "initialize":
        marker.parent.mkdir(parents=True, exist_ok=True)
        with marker.open("a", encoding="utf-8") as handle:
            handle.write("initialize\n")
        respond(request, {
            "protocol": "psychevo-extension/1",
            "extensionId": request["params"]["extensionId"],
            "capabilities": {}
        })
    elif method == "contributions/list":
        respond(request, {
            "commands": [],
            "mcpApps": [{
                "id": "dashboard",
                "resourceUri": "ui://example/dashboard.html",
                "fallback": "Use the dashboard text fallback.",
                "resourceUrl": "https://apps.example.test/dashboard.html",
                "resourceDomains": ["https://apps.example.test"],
                "connectDomains": [],
                "allowedTools": []
            }]
        })
    elif method == "command/run":
        respond(request, {
            "type": "bounded_text",
            "text": json.dumps(request["params"]["args"])
        })
    elif method == "channel/start":
        connection_id = request["params"]["connectionId"]
        configuration = request["params"].get("configuration", {})
        with state_lock:
            connections[connection_id] = {
                "lastSent": None,
                "pollGate": threading.Event(),
                "blockPollUntilSend": configuration.get("blockPollUntilSend", False)
            }
        respond(request, {})
    elif method == "channel/poll":
        connection_id = request["params"]["connectionId"]
        with state_lock:
            connection = connections[connection_id]
        if connection["blockPollUntilSend"]:
            connection["pollGate"].wait(timeout=10)
        respond(request, {"messages": [{
            "identity": {
                "connectionId": connection_id,
                "platform": "test",
                "chatId": "chat"
            },
            "messageId": "message",
            "text": "inbound",
            "attachments": []
        }]})
    elif method == "channel/send":
        connection_id = request["params"]["connectionId"]
        with state_lock:
            connection = connections[connection_id]
            connection["lastSent"] = request["params"]["message"]["text"]
            connection["pollGate"].set()
        respond(request, {})
    elif method == "channel/stop":
        with state_lock:
            connection = connections.pop(request["params"]["connectionId"], None)
            if connection is not None:
                connection["pollGate"].set()
        respond(request, {})
    elif method == "channel/test/status":
        connection_id = request["params"]["connectionId"]
        with state_lock:
            connection = connections.get(connection_id)
            result = {
                "started": connection is not None,
                "lastSent": None if connection is None else connection["lastSent"]
            }
        respond(request, result)
    elif method == "channel/test/hang":
        return
    elif method == "shutdown":
        with marker.open("a", encoding="utf-8") as handle:
            handle.write("shutdown\n")
        respond(request, {})
    else:
        respond_error(request, "method not found")


for line in sys.stdin:
    request = json.loads(line)
    if request.get("method") == "shutdown":
        handle(request)
        break
    threading.Thread(target=handle, args=(request,), daemon=True).start()
