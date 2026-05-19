---
name: 130. Permissions Testing
psychevo_self_edit: deny
---

# 130. Permissions Testing

## Acceptance Coverage

- Hardline/protected denies win over configured allow, session grants, and
  bypass modes.
- Configured precedence is deny, then ask, then allow, then default policy.
- Bash dangerous-command detection covers representative recursive delete,
  shell-pipe installer, interpreter inline execution, destructive git, process
  kill, service, permission, and SQL destructive commands.
- File path rules match both workdir-relative and canonical absolute patterns.
- `acceptEdits` auto-allows safe workdir write/edit asks for the current
  session only.
- Non-interactive runs allow prompt-level asks but deny hardline/protected
  actions, except `dontAsk`, which denies actions that would otherwise prompt.
- `allow always` writes project-local JSONC, preserves existing JSONC syntax,
  and skips exact duplicate rules.
- CLI permission config commands list and remove local allow/ask/deny rules.

## Validation

Prefer narrow deterministic tests in `psychevo-runtime` for policy evaluation
and config mutation. Use CLI smoke tests only for command surface behavior.
Do not use live providers, real API keys, network services, or host-global
state for permission validation.
