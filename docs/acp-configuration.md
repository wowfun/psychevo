# ACP Configuration Guide

This guide shows how to run Psychevo from an editor or other client that speaks
the Agent Client Protocol (ACP).

ACP clients start Psychevo as a child process and talk to it over stdio.
`pevo acp` is that stdio server. It is not an interactive terminal command to
run by hand.

## Prepare Psychevo

Initialize the Psychevo home:

```bash
pevo init
```

Check the configured model:

```bash
pevo model current
```

or list local configured and cached models:

```bash
pevo model list
```

Set provider credentials in one of these places:

- `~/.psychevo/.env`
- `<project>/.psychevo/.env`
- the environment inherited by the ACP client
- `pevo auth set <provider> --api-key-stdin`

Example:

```bash
printf '%s\n' "$DEEPSEEK_API_KEY" | pevo auth set deepseek --api-key-stdin
```

Before connecting an editor, verify that a normal runtime turn works:

```bash
pevo run "hello"
```

If this fails, fix model configuration or credentials first. ACP uses the same
runtime configuration path.

## Generic ACP Client

Configure the client to start Psychevo with:

- command: `pevo`
- args: `["acp"]`
- env: optional Psychevo paths and provider API-key variables

The working directory comes from the client session. Most editors set it to the
project root.

Generic shape:

```json
{
  "command": "pevo",
  "args": ["acp"],
  "env": {
    "PSYCHEVO_HOME": "~/.psychevo",
    "PSYCHEVO_DB": "~/.psychevo/state.db"
  }
}
```

Use `PSYCHEVO_CONFIG` when you want the ACP server to read one explicit TOML
config file:

```json
{
  "command": "pevo",
  "args": ["acp"],
  "env": {
    "PSYCHEVO_CONFIG": "/absolute/path/to/config.toml"
  }
}
```

You may also pass provider API-key environment variables through the client
configuration when the editor does not inherit your shell environment.

## Zed

Zed can run custom ACP agents from `agent_servers`. Add a Psychevo entry to
Zed's `settings.json`:

```json
{
  "agent_servers": {
    "Psychevo": {
      "type": "custom",
      "command": "pevo",
      "args": ["acp"],
      "env": {}
    }
  }
}
```

If Zed was launched from a desktop app and cannot find `pevo`, use an absolute
path:

```json
{
  "agent_servers": {
    "Psychevo": {
      "type": "custom",
      "command": "/home/me/.cargo/bin/pevo",
      "args": ["acp"],
      "env": {
        "PSYCHEVO_HOME": "/home/me/.psychevo"
      }
    }
  }
}
```

Open the Agent Panel, create a new external agent thread, and select
`Psychevo`. If startup fails, open `dev: open acp logs` from the Command
Palette.

Zed documents custom external agents at
<https://zed.dev/docs/ai/external-agents>. Its ACP client page is
<https://zed.dev/acp/editor/zed>. The protocol site is
<https://agentclientprotocol.com/>.

## Behavior Notes

Psychevo reads its own config and environment. Zed model/provider settings do
not configure Psychevo providers. Use Psychevo config, `.env` files, inherited
environment variables, or `pevo auth`.

ACP sessions can use model, mode, and session controls exposed by the client.
Changes apply to future prompts in that ACP session.

Use `/help` inside the ACP thread to see the currently advertised slash
commands. ACP hides commands whose useful behavior depends on a local terminal
surface, such as process exit, clipboard copy, image attachment syntax, or
renderer toggles. If you type an unknown slash-looking input, Psychevo sends it
as normal prompt text.

Permission prompts appear in the ACP client UI. Psychevo permission rules still
come from Psychevo config and project-local `.psychevo/config.toml`.

When the client sends MCP server declarations, Psychevo accepts supported stdio
and HTTP MCP inputs for that ACP session. MCP tools still run through Psychevo's
runtime permission policy.

## Troubleshooting

### Agent not shown in the editor

Check the client's agent-server configuration syntax. In Zed, the entry must be
under `agent_servers`, and a custom agent needs `"type": "custom"`, a
`"command"`, and optional `"args"`.

### Agent starts then exits or hangs

Run `pevo acp` only through the ACP client. To test Psychevo itself, use
`pevo run "hello"` instead. In Zed, open `dev: open acp logs` and check stderr
from the agent process.

### Credentials missing inside the editor

GUI-launched editors may not inherit your shell environment. Put the provider
key in `~/.psychevo/.env`, project `.psychevo/.env`, or the client `env` block.
Confirm with:

```bash
pevo auth status
```

### Wrong model or config scope

Run:

```bash
pevo config path
pevo model current
```

If the editor should use a separate config, pass `PSYCHEVO_CONFIG` in the ACP
client environment. If it should use a separate state database, pass
`PSYCHEVO_DB`.

### Slash command not advertised

Type `/help` in the ACP thread. Some commands are available only in the TUI or
CLI because they depend on terminal state, local clipboard access, or process
control.

### Permission prompts unexpected

ACP clients show permission requests from Psychevo at runtime. To inspect local
rules, run:

```bash
pevo config permissions list
```

Use the ACP client's approval UI for one-off decisions. Use Psychevo config for
durable project policy.

### MCP tools missing

Check whether the client sends MCP servers to external ACP agents. Psychevo can
use supported stdio and HTTP MCP declarations that arrive through ACP, and it
can also use MCP sources configured on the Psychevo side. Client-side MCP
configuration that is not forwarded to external agents will not appear in
Psychevo.
