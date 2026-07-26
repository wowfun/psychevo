import json, sys
for line in sys.stdin:
    req=json.loads(line)
    method=req.get("method")
    if method=="initialize":
        result={"ok": True}
    elif method=="hooks/call":
        result={"feedback":"worker saw hook"}
    elif method=="shutdown":
        result={"ok": True}
    else:
        result={}
    print(json.dumps({"jsonrpc":"2.0","id":req.get("id"),"result":result}), flush=True)
