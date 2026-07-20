#!/usr/bin/env python3
import json
import sys

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        result = {"ok": True}
    elif method == "contributions/list":
        result = {
            "tools": [{
                "name": "cleanup_status",
                "description": "Report cleanup status",
                "parameters": {"type": "object", "properties": {}}
            }]
        }
    elif method == "tools/call":
        result = {"json": {"status": "ok"}, "content": "cleanup ok"}
    else:
        result = {}
    print(json.dumps({
        "jsonrpc": "2.0",
        "id": request.get("id"),
        "result": result
    }), flush=True)
