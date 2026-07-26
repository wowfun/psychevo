#!/usr/bin/env python3
import json, sys, time
for line in sys.stdin:
    req=json.loads(line)
    method=req.get("method")
    if method=="initialize":
        result={"ok": True}
        print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
    elif method=="contributions/list":
        time.sleep(30)
    else:
        print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":{}}), flush=True)
