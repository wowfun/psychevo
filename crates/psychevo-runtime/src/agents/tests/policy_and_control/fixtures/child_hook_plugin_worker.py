#!/usr/bin/env python3
import json
import os
import pathlib
import sys


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        result = {"ok": True}
    elif method == "contributions/list":
        result = {"tools": []}
    elif method == "hooks/call":
        data = pathlib.Path(os.environ["PSYCHEVO_PLUGIN_DATA"])
        data.mkdir(parents=True, exist_ok=True)
        with (data / "child-hook.jsonl").open("a", encoding="utf-8") as handle:
            handle.write(
                json.dumps(
                    {
                        "event": request.get("params", {})
                        .get("hook", {})
                        .get("event")
                    }
                )
                + "\n"
            )
        result = {"feedback": "plugin child hook ran"}
    elif method == "shutdown":
        result = {"ok": True}
    else:
        result = {}
    print(
        json.dumps({"jsonrpc": "2.0", "id": request.get("id"), "result": result}),
        flush=True,
    )
