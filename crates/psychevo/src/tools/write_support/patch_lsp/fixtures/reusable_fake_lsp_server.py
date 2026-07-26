#!/usr/bin/env python3
import json
import os
import sys


count_path = os.environ["PSYCHEVO_TEST_LSP_START_COUNT"]
with open(count_path, "a", encoding="utf-8") as count_file:
    count_file.write("start\n")


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
                "result": {"capabilities": {"textDocumentSync": 2}},
            }
        )
    elif method in ("textDocument/didOpen", "textDocument/didChange"):
        if method == "textDocument/didOpen":
            uri = msg["params"]["textDocument"]["uri"]
            text = msg["params"]["textDocument"].get("text", "")
        else:
            uri = msg["params"]["textDocument"]["uri"]
            text = msg["params"]["contentChanges"][0].get("text", "")
        diagnostics = []
        if "bad" in text:
            diagnostics.append(
                {
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 3},
                    },
                    "severity": 1,
                    "source": "fake",
                    "code": "E001",
                    "message": "bad token",
                }
            )
        if "worse" in text:
            diagnostics.append(
                {
                    "range": {
                        "start": {"line": 1, "character": 0},
                        "end": {"line": 1, "character": 5},
                    },
                    "severity": 1,
                    "source": "fake",
                    "code": "E002",
                    "message": "worse token",
                }
            )
        send(
            {
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {"uri": uri, "diagnostics": diagnostics},
            }
        )
    elif method == "shutdown":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": None})
    elif method == "exit":
        break
