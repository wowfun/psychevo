#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    req=json.loads(line)
    mid=req.get("method")
    if mid=="initialize":
        result={"ok": True}
    elif mid=="contributions/list":
        result={"tools":[{"name":"cleanup_status","description":"status","parameters":{"type":"object","properties":{}}}]}
    elif mid=="tools/call":
        result={"json":{"status":"ok","plugin":req["params"]["name"]},"content":"ok"}
    else:
        result={}
    print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
