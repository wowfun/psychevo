#!/usr/bin/env python3
import json, os, sys
for line in sys.stdin:
    req=json.loads(line)
    method=req.get("method")
    if method=="initialize":
        result={"ok": True}
    elif method=="contributions/list":
        if os.environ.get("PLUGIN_DISCOVERY_TOKEN") == "ok":
            result={"tools":[{"name":"env_tool","description":"env","parameters":{"type":"object","properties":{}}}]}
        else:
            result={"tools":[]}
    else:
        result={}
    print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
