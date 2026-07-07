---
name: 055. Skills
psychevo_self_edit: deny
---

Define first-class skill support for Psychevo.

Skills are filesystem-backed instruction packages discovered by runtime,
advertised through progressive disclosure, loaded by dedicated skill tools, and
managed through explicit CLI/TUI or default-mode model tool actions.

## Scope

- skill package shape, frontmatter compatibility, readiness, and preprocessing
- runtime discovery paths, precedence, disabled state, and collision behavior
- model-visible skill index, explicit `$skill` expansion, and dynamic TUI slash
- model-visible read/config/manage/hub tool semantics
- `pevo skill` command family behavior, hub state, bundles, scanner policy, and
  curator lifecycle semantics

Out of scope:

- live registry validation in default tests
- granting tool permissions from skill frontmatter
- automatic bundled skill seeding or sync
- storing secret values in model-visible outputs

## Skill Package

A directory skill contains `SKILL.md`. Supporting material may live under
`references/`, `scripts/`, `assets/`, and `templates/`. Psychevo-native skills
also allow direct root `.md` files as compatibility input, but new created or
installed skills are directory packages. `.agents/skills` locations load only
`SKILL.md` package directories.

`SKILL.md` uses YAML frontmatter followed by Markdown instructions. Required
model-visible fields are:

- `name`: optional; defaults to the parent directory or file stem.
- `description`: required for loading.

Runtime accepts common third-party skill package optional fields including
`license`, `compatibility`, `allowed-tools`, `metadata`, `tags`, `related`,
`platforms`, `required_environment_variables`, `required_credential_files`,
`setup`, and `disable-model-invocation`. Unknown fields are preserved as
metadata when practical. `allowed-tools` is metadata only and never grants or
pre-approves Psychevo tool access.

Psychevo recognizes Hermes-compatible prompt-visibility hints under
`metadata.hermes.requires_tools` and `metadata.hermes.fallback_for_tools`.
`requires_tools` means the skill is omitted from the automatic model-visible
skill index unless every named tool is present in the accepted invocation tool
surface. `fallback_for_tools` means the skill is omitted from that index when
any named preferred tool is present, and may appear only when those preferred
tools are absent. These fields affect automatic prompt visibility only; they
do not prevent explicit viewing or explicit skill selection.

Psychevo also recognizes `metadata.hermes.requires_toolsets` and
`metadata.hermes.fallback_for_toolsets`. These use accepted invocation toolset
facts from 007 Tool Surface. Runtime must not infer toolset availability from
configuration alone when deciding skill prompt visibility.

Plugin-provided skills join normal discovery by default. Runtime preserves the
plugin source fact for diagnostics and visibility decisions, but automatic
prompt indexing uses the same enabled, platform, hidden, tool, and toolset
rules as other skills unless a future manifest policy changes that default.

Runtime validates names using the skill naming shape: lowercase letters,
numbers, and hyphens; no leading, trailing, or consecutive hyphens; maximum
length 64. Invalid names are diagnostics but do not prevent loading. Missing or
empty descriptions are diagnostics and prevent loading. Descriptions longer than
1024 characters are diagnostics and are truncated for prompt display.

Management surfaces may replace a mutable Project or Profile(Global)
`SKILL.md` file by writing raw Markdown. The write must parse frontmatter,
validate the resulting skill name and required description, and reject content
that would make the skill undiscoverable. The write must target only skills
under `<cwd>/.psychevo/skills` or `$PSYCHEVO_HOME/skills`; plugin-provided,
configured external, explicit, `.agents/skills`, and system/built-in skills are
read-only from GUI management.

`platforms: [linux|macos|windows]` restricts normal activation to matching OSes.
Unsupported skills remain inspectable through explicit view/detail surfaces but
are omitted from automatic prompt indexes, dynamic slash, and marker activation.

## Discovery

Skill discovery is deterministic. The precedence order is:

1. explicit `--skill` paths or names
2. project `<cwd>/.psychevo/skills`
3. ancestor `.agents/skills` from nearest directory to git root
4. global `$PSYCHEVO_HOME/skills`
5. compatibility `$HOME/.agents/skills`
6. optional configured `skills.paths`

Psychevo keeps `skills.paths` as the external-directory config key. The legacy
`skills.external_dirs` key is not a supported config alias. `$HOME/.agents/skills`
is a compatibility discovery root, not a config alias, and loads only directory
packages with `SKILL.md`.

Discovery canonicalizes existing source roots before scanning and loads each
physical root once. If a root is reachable through multiple rules, the earliest
rule in the precedence order owns the source label and root-file behavior. This
prevents `$HOME/.agents/skills` from appearing once as an ancestor `agents` root
and again as an `agents_global` compatibility root while preserving collisions
for distinct enabled skills with the same name.

Discovery produces a management catalog before runtime activation filters are
applied. Valid discovered skills remain visible to management surfaces even
when they are disabled, unsupported on the current platform, hidden from model
invocation, or collision-ambiguous. Each catalog row exposes `enabled`,
`prompt_visible`, `readiness_status`, `supported_on_current_platform`, `source`,
`source_label`, `location`, `skill_dir`, `issues`, and, when applicable,
`collision_group`.

`source` is the raw discovery source for filtering, diagnostics, and mutation
target resolution. User-facing surfaces must prefer `source_label`, a coarse
display label that hides implementation-oriented source names: project-local
and ancestor `.agents/skills` roots are `Project`, user/configured/explicit
roots are `User`, and plugin or built-in/system roots are `System`. Unknown raw
sources do not fall back to visible snake_case labels.

Duplicate enabled skill names are surfaced as collisions. Name-based view,
marker selection, and dynamic slash invocation refuse ambiguous names instead of
silently choosing the precedence winner. Callers can disambiguate through a
category path, an explicit filesystem path, or CLI/Gateway path selectors where
available. Colliding rows stay visible in `skill/list`.

Disabled state lives in `config.toml` under `skills.disabled` and
`skills.platform_disabled.<platform>`. Runtime ignores legacy disabled sidecars.
`--no-skills` disables default and configured discovery but does not suppress
explicit `--skill` inputs.

Discovery facts that affect agent-invocation assembly are capability extension
facts. Runtime owns their acceptance into an invocation and should expose
diagnostics through CLI/TUI observation surfaces when practical.

## Model Visibility

Runtime appends a compact skill index to system instructions when at least one
available, non-hidden skill is discovered and skill discovery is enabled. The
index contains only name, description, and location. Skills marked
`disable-model-invocation: true`, disabled skills, unsupported skills, and
collision-ambiguous skills are omitted. The same strict visibility applies to
dynamic slash, marker activation, and name-only model `view_skill`.

The index is advisory: when the task matches a listed skill that has not
already been selected for the turn, the model is instructed to load the full
skill through `view_skill` before following it. Full skill content is not
included in the prompt automatically for index-only skills.

Explicit skill invocation uses editable `$skill-name` markers. Runtime parses
markers in every user entry surface, and also treats `--skill <name-or-path>` as
an explicit selected skill. Unknown markers remain ordinary text. Ambiguous
markers are rejected rather than resolved by precedence.

Explicitly selected skills are injected as separate hidden contextual-user
messages containing the full skill body. Relative references in injected skill
content are resolved against the skill directory. Injected fragments are the
already-loaded skill body for that turn, so the model should follow them
directly and only load supporting files when needed. Injected fragments are
durable context evidence anchored to the accepted user prompt.

Skill content preprocessing supports `${PSYCHEVO_SKILL_DIR}` and
`${PSYCHEVO_SESSION_ID}`. Runtime also accepts legacy aliases for imported skill
packages. Inline shell snippets written as ``!`cmd` `` are disabled by default.
When `skills.inline_shell` is enabled, snippets run before injection with
timeout/error/truncation markers inlined into the skill content.

## Tools

`list_skills` and `view_skill` are available in both plan and default runtime
modes when skills are enabled. They are adjunct skill tools, not part of the
required `coding-core` toolset.

`list_skills` is lightweight by default and accepts detail/filter/sort options.
Supported filters include category, source, enabled-only, platform, tag, and
readiness. Usage-based sorts read SQLite aggregate counters. Listing does not
increment usage telemetry.

`view_skill` reads a skill's main `SKILL.md` content, or a requested supporting
file within that skill's directory. Main `SKILL.md` reads keep `content` as the
processed model-facing body without YAML frontmatter, while product GUI callers
may use `preview_content` for the raw UTF-8 Markdown file preview. Supporting
file reads may return the same raw text for both fields. It rejects path
traversal and resolved paths outside the skill directory. Binary or invalid
UTF-8 files are reported as metadata rather than inlined. Missing supporting
files return available file choices. `view_skill` reports linked files,
raw `source`, display `source_label`, readiness, tags, related skills, setup
notes, missing required environment variables, missing credential files, and
platform status. Viewing increments aggregate view telemetry, including in Plan
Mode.

Default mode also exposes aggregate tools:

- `skill_manage`: `create`, `edit`, `patch`, `delete`, `write_file`,
  `remove_file`
- `skill_hub`: `browse`, `search`, `inspect`, `list`, `check`, `audit`,
  `install`, `update`, `uninstall`, `publish`
- `skill_config`: read status plus scoped enable/disable and `skills.config.*`
  edits

Plan Mode exposes `skill_hub` read-only actions only:
`browse`, `search`, `inspect`, `list`, `check`, and `audit`. Plan-mode audit is
read-only and does not append audit logs. Plan Mode exposes only read status for
`skill_config`. Plan Mode continues to withhold skill mutations except for
aggregate `view_skill` telemetry.

The old model tool names `create_skill`, `patch_skill`, `remove_skill`,
`enable_skill`, `disable_skill`, and `install_skill` are removed.

## CLI And TUI

`pevo skill` is the singular product command family; plural `pevo skills` is not
accepted. `pevo skill` with no subcommand shows help.

Primary command groups:

- `list`, `view`
- `browse`, `search`, `inspect`, `install`, `check`, `update`, `audit`,
  `uninstall`, `publish`
- `config`
- `bundle list|show|create|delete|reload`
- `snapshot export|import`
- `tap list|add|remove`
- `reset`

`pevo skill audit` absorbs the old standalone scan behavior. `reset` only
applies when a bundled manifest exists; this spec does not add bundled skill
seeding or syncing.

`pevo skill install` defaults to the current cwd `.psychevo/skills`;
`-g`/`--global` installs under `$PSYCHEVO_HOME/skills`. Installing a managed
skill as an editable copy uses `install --name <new-name>`, and the new name is
required. `--project` is not accepted as a scope alias.

Gateway exposes skill management methods for product surfaces:
`skill/list`, `skill/read`, `skill/install`, `skill/uninstall`, and
`skill/setEnabled`. These methods use the same discovery, scanner, install,
and scoped config helpers as the CLI/TUI. Workbench capability management writes
to the active profile by default. Dangerous scanner verdicts and overwrites
must require an explicit force request from the caller.

`skill/list` returns the management catalog and remains JSON-compatible with
older fields. It adds a stable path-derived `id`, `enabled`,
`prompt_visible`, `issues`, and optional `collision_group` per row.
`skill/read` accepts `{ name, path?, scope }`; `path` selects an exact
discovered row and bypasses name-collision ambiguity, while name-only reads
continue to refuse ambiguous names. `content` remains the processed body used by
model-facing views; `preview_content` is the raw UTF-8 file text intended for
GUI Markdown preview. `skill/uninstall` accepts
`{ name, path?, target?, scope }`; `path` removes that exact profile/project
installed skill when it is in a mutable Psychevo skills directory and refuses
non-mutable sources. `skill/setEnabled` remains name-scoped because persisted
disabled state is `skills.disabled`.

The TUI `/skills` command is a hub dispatcher. `/skills` with no arguments
shows a bounded hub dashboard/help block. Read subcommands include `list`,
`browse`, `search`, `inspect`, `check`, `audit`, and `reload`. Mutating
hub/config actions go through Psychevo permissions and remain blocked in Plan
Mode. TUI skill mutations that write scoped state use the same scope rule as
CLI: default current cwd `.psychevo`, `--local` explicit local, and
`-g`/`--global` global; legacy `--scope` and `--project` are not accepted.
Dynamic TUI slash uses `/<skill-or-bundle>` names and submits the slash line as
prompt input from Enter or mouse selection. Runtime still receives the
equivalent explicit `$skill` or `$bundle` marker text for skill expansion, but
the fullscreen composer is cleared and the transcript/history display keeps the
submitted slash line. Bundles and skills share marker syntax; bundles win over
same-name skills.

TUI mutating hub, bundle, config, and curator commands go through Psychevo
permissions.

## Bundles

Skill bundles are TOML files that name multiple skills to load together. Global
and project bundles are supported; project bundle slugs override global slugs.
Bundles are managed by `pevo skill bundle` and TUI `/bundles`, not by model
tools.

Bundle fields are `name`, `description`, `skills`, and optional `instruction`.
Invoking a bundle loads each available member skill once, skips and reports
missing members, and fails only when no member skill can be loaded. Bundle
invocation increments usage counters for successfully loaded member skills, not
for the bundle itself. Runtime reads only `skill-bundles/*.toml`; legacy
`*.yaml` or `*.yml` bundle files are ignored.

## Hub, Scanner, Curator, And Storage

Hub state uses `$PSYCHEVO_HOME/skills/.hub/{lock.json,taps.json,audit.log,index-cache}`.
Hub audit logs keep only non-sensitive summaries such as time, action, skill
name, identifier/source, scanner verdict, and status. Usage telemetry is stored
separately in SQLite aggregate records only: view/use/patch counts, last
timestamps, provenance, lifecycle state, and pinned flag. There is no per-event
usage log.

Scanner verdicts are `safe`, `caution`, and `dangerous`. Trusted sources allow
caution; community sources block any findings unless a CLI/TUI caller supplies
`--force`. Model tools can never force scanner results. Publishing blocks only
`dangerous`.

Publish uses a GitHub PR backend first. Auth resolves from `GITHUB_TOKEN`,
`GH_TOKEN`, then `gh auth token`. Published skills upload to `skills/<name>/` and
may update existing registry paths through PRs.

Curator is enabled by default. Its first automatic observation seeds state and
defers the first real pass by one interval. Automatic checks happen on
`pevo run`, `pevo tui`, and `pevo agent run`. The default curator scope is
global; project and all-scope runs require explicit `--scope`.

Curator never auto-deletes. It may archive or patch eligible local non-managed
skills, scanner still blocks dangerous output, and archives are recoverable.
Pinned local non-managed skills are skipped by curator and protected from
`skill_manage` edits/deletes.

## Permissions And Compatibility

Skill mutations default to permission `ask`. Secret capture input counts as user
authorization for writing `$PSYCHEVO_HOME/.env`. `skill_config` may only write
under `skills.disabled`, `skills.platform_disabled`, and `skills.config.*`.

Runtime ignores legacy disabled sidecars and old `.provenance.json`; no migration
or compatibility shim is required before the product is released.

## Related Topics

- [006 Context Assembly](../006-context-assembly/spec.md) owns model visibility
  for skill index and explicit skill expansion material.
- [050 Capability Extensions](../050-capability-extensions/spec.md) owns
  capability-extension source, declaration, and registry boundaries.
- [100 Coding Agent](../100-coding-agent/spec.md) defines the built-in
  coding-agent capability that may use skill adjunct tools.
- [110 Coding Core Tools](../110-coding-core-tools/spec.md) defines the required
  coding-core toolset, which skill tools do not join.
- [200 pevo CLI](../200-pevo-cli/spec.md) owns concrete command spelling.
- [210 pevo TUI Interaction](../210-pevo-tui/interaction.md) owns
  interactive slash projection.
