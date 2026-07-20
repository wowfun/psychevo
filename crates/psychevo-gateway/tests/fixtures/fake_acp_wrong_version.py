#!/usr/bin/env python3
import json
import sys

log_path = sys.argv[1]


def send(value):
    print(json.dumps(value), flush=True)


for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    with open(log_path, "a", encoding="utf-8") as log_file:
        log_file.write(json.dumps({"method": message.get("method")}) + "\n")
    if message.get("method") == "initialize":
        send(
            {
                "jsonrpc": "2.0",
                "id": message.get("id"),
                "result": {"protocolVersion": 2, "agentCapabilities": {}},
            }
        )
