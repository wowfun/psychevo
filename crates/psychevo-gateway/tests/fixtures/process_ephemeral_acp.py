import json
import sys

log_path = sys.argv[1]
for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    with open(log_path, "a", encoding="utf-8") as log:
        log.write(str(method) + "\n")
    if method == "initialize":
        response = {
            "protocolVersion": 1,
            "agentInfo": {"name": "ephemeral-test", "title": "Ephemeral", "version": "1.0.0"},
            "agentCapabilities": {"promptCapabilities": {"embeddedContext": True}},
            "authMethods": []
        }
        print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": response}), flush=True)
    else:
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": message.get("id"),
            "error": {"code": -32601, "message": "unexpected method"}
        }), flush=True)
