#!/usr/bin/env python3
import json
import sys


def read_msg():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode("ascii").strip()
        if not line:
            break
        key, value = line.split(":", 1)
        headers[key.lower()] = value.strip()
    length = int(headers.get("content-length", "0"))
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))


def send(msg):
    body = json.dumps(msg).encode("utf-8")
    sys.stdout.buffer.write(
        b"Content-Length: "
        + str(len(body)).encode("ascii")
        + b"\r\n\r\n"
        + body
    )
    sys.stdout.buffer.flush()


while True:
    msg = read_msg()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        send(
            {
                "jsonrpc": "2.0",
                "id": msg["id"],
                "result": {"capabilities": {"textDocumentSync": 1}},
            }
        )
    elif method == "textDocument/didOpen":
        doc = msg["params"]["textDocument"]
        diagnostics = []
        if "bad" in doc.get("text", ""):
            diagnostics.append(
                {
                    "range": {
                        "start": {"line": 0, "character": 1},
                        "end": {"line": 0, "character": 4},
                    },
                    "severity": 1,
                    "source": "fake",
                    "code": "E001",
                    "message": "bad token",
                }
            )
        send(
            {
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {"uri": doc["uri"], "diagnostics": diagnostics},
            }
        )
    elif method == "shutdown":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": None})
    elif method == "exit":
        break
