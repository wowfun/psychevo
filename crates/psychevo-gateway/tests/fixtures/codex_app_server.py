#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys
import time


config_path = Path(sys.argv[0]).with_suffix(".json")
with config_path.open(encoding="utf-8") as config_file:
    config = json.load(config_file)
scenario = config["scenario"]
state = {}


def send(value):
    print(json.dumps(value), flush=True)


def result(message, value):
    send({"jsonrpc": "2.0", "id": message["id"], "result": value})


def error(message, code, text):
    send({"jsonrpc": "2.0", "id": message["id"], "error": {"code": code, "message": text}})


def initialize(message, user_agent="fake", codex_home="/fake"):
    result(message, {
        "codexHome": os.environ.get("CODEX_HOME") if codex_home is None else codex_home,
        "platformFamily": "unix",
        "platformOs": "linux",
        "userAgent": user_agent
    })


def append_log(text):
    with open(config["log"], "a", encoding="utf-8") as log_file:
        log_file.write(text)


def catalog(plugins):
    marketplaces = [] if not plugins else [{"name": "openai", "path": None, "plugins": plugins}]
    return {"marketplaces": marketplaces, "marketplaceLoadErrors": [], "featuredPluginIds": []}


def reject_null_params(message):
    if message.get("params") is not None:
        return False
    error(message, -32602, "invalid params")
    return True


def private_home(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message, "codex_cli_rs/0.144.1", None)
    elif method == "initialized":
        pass
    elif method == "plugin/installed" and message.get("params") is not None:
        result(message, {"marketplaces": [], "marketplaceLoadErrors": []})
    elif message.get("id") is not None:
        error(message, -32602, "invalid params")


def preflight_missing_method(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message, "any-originator/0.144.1", None)
    elif method == "initialized":
        pass
    else:
        append_log(method + ":" + ("null" if message.get("params") is None else "normal") + "\n")
        code = -32601 if method == "app/list" else -32602
        error(message, code, "method not found" if code == -32601 else "invalid params")


def handshake(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message)
    elif method == "initialized":
        state["initialized"] = True
    elif method == "plugin/list":
        assert state.get("initialized")
        send({
            "jsonrpc": "2.0",
            "id": 900,
            "method": "mcpServer/elicitation/request",
            "params": {"request": {"mode": "form"}}
        })
        response = json.loads(sys.stdin.readline())
        assert response["result"]["action"] == "decline"
        result(message, {
            "marketplaces": [{
                "name": "openai",
                "path": None,
                "plugins": [{
                    "id": "review@openai",
                    "name": "review",
                    "installed": False,
                    "enabled": False
                }]
            }],
            "marketplaceLoadErrors": [],
            "featuredPluginIds": []
        })
    elif method == "plugin/installed":
        result(message, {"marketplaces": [], "marketplaceLoadErrors": []})
    elif method == "plugin/read":
        result(message, {"plugin": {
            "marketplaceName": "openai",
            "summary": {"name": "review", "installed": False, "enabled": False},
            "description": "Review",
            "skills": [{"name": "review"}],
            "hooks": [],
            "apps": [{"id": "review-app", "name": "Review"}],
            "mcpServers": []
        }})
    elif method == "plugin/install":
        result(message, {"authPolicy": "ON_USE", "appsNeedingAuth": []})
    elif method == "plugin/uninstall":
        result(message, {})
    elif method == "app/list":
        result(message, {"data": [], "nextCursor": None})


def elicitation_wait(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message)
    elif method == "initialized":
        pass
    elif method == "mcpServer/tool/call":
        state["pending_tool"] = message["id"]
        send({
            "jsonrpc": "2.0",
            "id": 900,
            "method": "mcpServer/elicitation/request",
            "params": {
                "threadId": "codex-thread",
                "turnId": "turn-a",
                "mode": "form",
                "message": "Continue?",
                "requestedSchema": {
                    "type": "object",
                    "properties": {"confirmed": {"type": "boolean"}},
                    "required": ["confirmed"]
                }
            }
        })
    elif method == "plugin/list":
        result(message, catalog([]))
    elif message.get("id") == 900:
        send({
            "jsonrpc": "2.0",
            "id": state["pending_tool"],
            "result": {"content": [{"type": "text", "text": "done"}], "isError": False}
        })


def inventory_single_flight(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message)
    elif method == "initialized":
        pass
    elif method == "plugin/installed":
        append_log("plugin-installed\n")
        time.sleep(0.1)
        result(message, {"marketplaces": [], "marketplaceLoadErrors": []})
    else:
        raise AssertionError("unexpected method: " + str(method))


def account_update(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message)
    elif method == "initialized":
        pass
    elif method == "plugin/installed":
        state["count"] = state.get("count", 0) + 1
        append_log("plugin-installed\n")
        if state["count"] == 1:
            send({"jsonrpc": "2.0", "method": "account/updated", "params": {"authMode": "chatgpt"}})
        result(message, {"marketplaces": [], "marketplaceLoadErrors": []})


def prewarm_inventory(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message, "codex_cli_rs/0.144.1", config["private_home"])
    elif method == "initialized":
        pass
    elif reject_null_params(message):
        pass
    elif method == "plugin/installed":
        append_log("plugin-installed\n")
        while not os.path.exists(config["release"]):
            time.sleep(0.005)
        result(message, {"marketplaces": [], "marketplaceLoadErrors": []})


def failed_inventory(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message)
    elif method == "initialized":
        pass
    elif method == "plugin/installed":
        append_log("plugin-installed\n")
        sys.exit(1)


def catalog_mutation(message):
    method = message.get("method")
    active = state.setdefault("active", "alpha")
    if method == "initialize":
        initialize(message)
    elif method == "initialized":
        pass
    elif method == "plugin/installed":
        append_log("installed:" + active + "\n")
        plugins = [] if active == "none" else [{
            "id": active + "@openai",
            "name": active,
            "installed": True,
            "enabled": True
        }]
        response = catalog(plugins)
        response.pop("featuredPluginIds")
        result(message, response)
    elif method == "plugin/list":
        result(message, {
            "marketplaces": [{
                "name": "openai",
                "path": None,
                "plugins": [{
                    "id": "beta@openai",
                    "name": "beta",
                    "installed": active == "beta",
                    "enabled": active == "beta"
                }]
            }],
            "marketplaceLoadErrors": [],
            "featuredPluginIds": []
        })
    elif method == "plugin/read":
        name = message["params"]["pluginName"]
        package = config["alpha"] if name == "alpha" else config["beta"]
        result(message, {"plugin": {
            "marketplaceName": "openai",
            "summary": {
                "id": name + "@openai",
                "name": name,
                "installed": True,
                "enabled": True,
                "source": {"type": "local", "path": package}
            },
            "description": name,
            "skills": [{
                "name": name,
                "path": package + "/skills/" + name + "/SKILL.md",
                "enabled": True
            }],
            "hooks": [],
            "apps": [],
            "mcpServers": []
        }})
    elif method == "plugin/install":
        state["active"] = "beta"
        append_log("install\n")
        result(message, {"authPolicy": "ON_USE", "appsNeedingAuth": []})
    elif method == "plugin/uninstall":
        state["active"] = "none"
        append_log("uninstall\n")
        result(message, {})
    else:
        raise AssertionError("unexpected method: " + str(method))


def install_materializes(message):
    method = message.get("method")
    installed = state.get("installed", False)

    def current_catalog():
        return {
            "marketplaces": [{
                "name": "openai",
                "path": None,
                "plugins": [{
                    "id": "review@openai",
                    "name": "review",
                    "installed": installed,
                    "enabled": installed,
                    "localVersion": "1.0.0"
                }]
            }],
            "marketplaceLoadErrors": [],
            "featuredPluginIds": []
        }

    if method == "initialize":
        initialize(message, "review-host/0.144.1", None)
    elif method == "initialized":
        pass
    elif reject_null_params(message):
        pass
    elif method == "plugin/list" or method == "plugin/installed":
        result(message, current_catalog())
    elif method == "plugin/install":
        state["installed"] = True
        result(message, {"authPolicy": "ON_USE", "appsNeedingAuth": []})
    elif method == "plugin/read":
        result(message, {"plugin": {
            "summary": {
                "installed": True,
                "enabled": True,
                "localVersion": "1.0.0",
                "source": {"type": "local", "path": config["package"]}
            },
            "skills": [],
            "hooks": [],
            "mcpServers": [],
            "apps": []
        }})
    else:
        error(message, -32601, "method not found")


def app_connect(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message)
    elif method == "initialized":
        pass
    elif method == "app/list":
        state["calls"] = state.get("calls", 0) + 1
        result(message, {"data": [{
            "id": "review-app",
            "isAccessible": state["calls"] > 1,
            "installUrl": "https://apps.example.test/install/review"
        }]})


def installed_package(message):
    method = message.get("method")
    if method == "initialize":
        assert message["params"]["capabilities"]["mcpServerOpenaiFormElicitation"] is True
        initialize(message, "fixture/0.144.1", None)
    elif method == "initialized":
        state["initialized"] = True
    elif reject_null_params(message):
        pass
    elif method == "plugin/installed":
        assert state.get("initialized")
        append_log("plugin-installed\n")
        result(message, catalog([{
            "id": "review@openai",
            "name": "review",
            "installed": True,
            "enabled": True
        }]))
    elif method == "plugin/list":
        time.sleep(2)
        append_log("plugin-list\n")
        result(message, catalog([{
            "id": "review@openai",
            "name": "review",
            "installed": True,
            "enabled": True
        }]))
    elif method == "plugin/install":
        result(message, {"authPolicy": "ON_USE", "appsNeedingAuth": []})
    elif method == "plugin/read":
        package = config["package"]
        result(message, {"plugin": {
            "marketplaceName": "openai",
            "summary": {
                "id": "review@openai",
                "name": "review",
                "installed": True,
                "enabled": True,
                "source": {"type": "local", "path": package}
            },
            "skills": [{
                "name": "review",
                "path": package + "/skills/review/SKILL.md",
                "enabled": True
            }],
            "hooks": [],
            "apps": [{"id": "review-app"}],
            "mcpServers": []
        }})
    elif method == "thread/start":
        assert message["params"]["ephemeral"] is True
        result(message, {"thread": {"id": "codex-thread-1"}})
    elif method == "mcpServerStatus/list":
        assert message["params"]["threadId"] == "codex-thread-1"
        append_log("mcp-status\n")
        result(message, {
            "data": [{
                "name": "codex_apps",
                "tools": {"review": {
                    "description": "Review app",
                    "inputSchema": {"type": "object", "properties": {}}
                }}
            }],
            "nextCursor": None
        })
    elif method == "mcpServer/tool/call":
        send({
            "jsonrpc": "2.0",
            "id": 9001,
            "method": "mcpServer/elicitation/request",
            "params": {
                "threadId": "codex-thread-1",
                "turnId": "turn-1",
                "serverName": "codex_apps",
                "mode": "form",
                "_meta": {"source": "test"},
                "message": "Continue?",
                "requestedSchema": {
                    "type": "object",
                    "properties": {"confirmed": {"type": "boolean"}},
                    "required": ["confirmed"]
                }
            }
        })
        answer = json.loads(sys.stdin.readline())
        assert answer["result"]["action"] == "accept"
        assert answer["result"]["content"]["confirmed"] is True
        result(message, {
            "content": [{"type": "text", "text": "done"}],
            "structuredContent": {"ok": True},
            "isError": False
        })
    elif method == "thread/archive":
        append_log(message["params"]["threadId"] + "\n")
        result(message, {})


def first_token_performance(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message, "performance-fixture/0.144.1", None)
    elif method == "initialized":
        pass
    elif reject_null_params(message):
        pass
    elif method == "plugin/installed":
        append_log("plugin-installed\n")
        result(message, {"marketplaces": [], "marketplaceLoadErrors": []})
    elif method == "plugin/read":
        result(message, {"plugin": {
            "summary": {"id": "review@openai", "name": "review", "installed": True, "enabled": True},
            "skills": [],
            "hooks": [],
            "apps": [{"id": "review-app"}],
            "mcpServers": []
        }})
    elif method == "plugin/list":
        time.sleep(2)
        append_log("plugin-list\n")
        result(message, catalog([]))
    elif method == "thread/start":
        state["thread_count"] = state.get("thread_count", 0) + 1
        result(message, {"thread": {"id": "codex-thread-" + str(state["thread_count"])}})
    elif method == "mcpServerStatus/list":
        append_log("mcp-status\n")
        result(message, {
            "data": [{
                "name": "codex_apps",
                "tools": {"review": {
                    "description": "Review app",
                    "inputSchema": {"type": "object", "properties": {}}
                }}
            }],
            "nextCursor": None
        })
    elif method == "thread/archive":
        result(message, {})


def gateway_plugin_rpcs(message):
    method = message.get("method")
    if method == "initialize":
        initialize(message, "gateway-fixture/0.144.1", None)
    elif method == "initialized":
        pass
    elif reject_null_params(message):
        pass
    elif method == "plugin/list":
        result(message, {
            "marketplaces": [{
                "name": "openai",
                "path": None,
                "plugins": [{
                    "id": "review@openai",
                    "name": "review",
                    "installed": False,
                    "enabled": False,
                    "interface": {"shortDescription": "Review"}
                }]
            }],
            "marketplaceLoadErrors": [],
            "featuredPluginIds": []
        })
    elif method == "plugin/installed":
        result(message, {"marketplaces": [], "marketplaceLoadErrors": []})
    elif method == "plugin/read":
        result(message, {"plugin": {
            "marketplaceName": "openai",
            "summary": {
                "id": "review@openai",
                "name": "review",
                "installed": False,
                "enabled": False
            },
            "description": "Review plugin",
            "skills": [],
            "hooks": [],
            "apps": [{"id": "review-app"}],
            "mcpServers": []
        }})
    elif method == "plugin/install":
        append_log("install\n")
        result(message, {"authPolicy": "ON_USE", "appsNeedingAuth": []})
    elif method == "plugin/uninstall":
        append_log("uninstall\n")
        result(message, {})
    elif method == "app/list":
        result(message, {"data": [], "nextCursor": None})


handlers = {
    "account_update": account_update,
    "app_connect": app_connect,
    "catalog_mutation": catalog_mutation,
    "elicitation_wait": elicitation_wait,
    "failed_inventory": failed_inventory,
    "first_token_performance": first_token_performance,
    "gateway_plugin_rpcs": gateway_plugin_rpcs,
    "handshake": handshake,
    "install_materializes": install_materializes,
    "installed_package": installed_package,
    "inventory_single_flight": inventory_single_flight,
    "preflight_missing_method": preflight_missing_method,
    "prewarm_inventory": prewarm_inventory,
    "private_home": private_home
}

if scenario == "private_home":
    with open(config["log"], "w", encoding="utf-8") as process_log:
        json.dump({"codexHome": os.environ.get("CODEX_HOME"), "argv": sys.argv[1:]}, process_log)

handler = handlers[scenario]
for line in sys.stdin:
    if line.strip():
        handler(json.loads(line))
