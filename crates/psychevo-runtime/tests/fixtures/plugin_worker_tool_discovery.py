#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    req=json.loads(line)
    method=req.get("method")
    if method=="initialize":
        result={"ok": True}
    elif method=="contributions/list":
        result={"tools":[{"name":"cleanup_status","description":"status","parameters":{"type":"object","properties":{}}}]}
    else:
        result={}
    print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
