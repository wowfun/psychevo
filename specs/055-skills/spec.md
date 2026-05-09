---
name: 055. Skills
psychevo_self_edit: deny
---

Define first-class skill support for Psychevo.

Skills are filesystem-backed instruction packages discovered by runtime,
advertised to the model through progressive disclosure, loaded through
dedicated skill tools, and managed through explicit CLI or build-mode tool
actions.

## Scope

- skill package shape and frontmatter validation
- runtime discovery paths, precedence, disabled state, and collisions
- model-visible skill index and explicit skill expansion
- verb-first skill tool semantics
- `pevo skills` command family behavior
- local and Git install boundaries
- scanner verdicts and provenance sidecar semantics

Out of scope:

- public registries, marketplaces, lockfiles, background curation,
  auto-skill generation, workflow mining, or self-evolution loops
- executing bundled skill scripts automatically
- remote provider validation, live service checks, or credential collection
- stable storage schemas, Rust APIs, JSON payload schemas, or UI layout details

## Skill Package

A directory skill contains `SKILL.md`. Everything else in the directory is
supporting material. Supported conventional supporting directories are
`references/`, `scripts/`, `assets/`, and `templates/`.

Psychevo-native skill directories also allow direct root `.md` files as
individual skills. `.agents/skills` locations load only `SKILL.md` package
directories.

`SKILL.md` uses YAML frontmatter followed by Markdown instructions. Required
model-visible fields are:

- `name`: optional; defaults to the parent directory or file stem.
- `description`: required for loading.

Optional fields include `disable-model-invocation`. Unknown frontmatter fields
are ignored by the first implementation slice.

Runtime validates names using the skill naming shape: lowercase
letters, numbers, and hyphens; no leading, trailing, or consecutive hyphens;
maximum length 64. Invalid names are diagnostics but do not prevent loading.
Missing or empty descriptions are diagnostics and prevent loading. Descriptions
longer than 1024 characters are diagnostics and are truncated for prompt
display.

## Discovery

Skill discovery is deterministic. The precedence order is:

1. explicit `--skill` paths or names
2. project `<workdir>/.psychevo/skills`
3. ancestor `.agents/skills` from nearest directory to git root
4. global `$PSYCHEVO_HOME/skills`
5. optional configured `skills.paths`

The first loaded skill for a name wins. Later skills with the same name are
omitted with a collision diagnostic. Explicit paths and project skills take
precedence over global skills.

`skills.disabled` or the runtime-managed disabled sidecar disables named skills
for the invocation unless the same skill is supplied by explicit path.
`--no-skills` disables default and configured discovery but does not suppress
explicit `--skill` inputs.

Discovery facts that affect agent-invocation assembly are capability extension
facts. Runtime owns their selection into an invocation and should expose
diagnostics through CLI/TUI observation surfaces when practical.

## Model Visibility

Runtime appends a compact skill index to system instructions when at least one
non-hidden skill is available and skill discovery is enabled. The index uses an
XML-shaped block containing only skill name, description, and location.

The index is advisory: when the task matches a skill description, the model is
instructed to load the full skill through `view_skill` before following it.
Full skill content is not included in the prompt automatically. Skills marked
`disable-model-invocation: true` are omitted from the index but remain available
for explicit invocation.

Explicit skill invocation uses editable `$skill-name` markers. A TUI
`/skill:<name>` menu selection inserts `$skill-name ` into the composer instead
of submitting the prompt. Runtime parses `$skill-name` markers in every user
entry surface, and also treats `--skill <name-or-path>` as an explicit selected
skill.

Explicitly selected skills are injected as separate non-persisted user-context
fragments containing the full skill body. The persisted user message and TUI
transcript keep the original user text, such as `$reviewer check this diff`.
Unknown `$name` markers remain ordinary text. Relative references in injected
skill content are resolved against the skill directory.

## Tools

`list_skills` and `view_skill` are available in both `plan` and `default`
runtime modes when skills are enabled. They are adjunct skill tools, not part of
the required `coding-core` toolset.

`list_skills` returns available skill metadata and diagnostics.

`view_skill` reads a skill's main `SKILL.md` content, or a requested supporting
file within that skill's directory. It must reject path traversal and resolved
paths outside the skill directory. Binary supporting files are reported as
binary metadata rather than inlined.

Mutating skill tools are available only in `default` mode and use verb-first
names: `create_skill`, `patch_skill`, `remove_skill`, `enable_skill`,
`disable_skill`, and `install_skill`. Model tool calls cannot force-install
scanner-blocked skills.

Plan mode remains hard read-only. It may list and view skills but must not
create, modify, remove, install, enable, or disable skills.

## CLI

`pevo skills` is the product command family for local skill management:

- `list [--json] [--all]`
- `view <name> [file_path]`
- `create <name> --description <text> [--global|--project]`
- `patch <name> --old <text> --new <text>`
- `remove <name>`
- `enable|disable <name> [--global|--project]`
- `install <local-path-or-git-url> [--name <name>|--all] [--global|--project] [--force]`
- `scan <path>`

The default create and install target is global `$PSYCHEVO_HOME/skills`.
`--project` targets `<workdir>/.psychevo/skills`.

`pevo run` and `pevo tui` accept repeatable `--skill <name-or-path>`. These
flags explicitly select and inject skills for the next prompt, while `$skill`
markers in the prompt select matching enabled skills by name.

Local installs copy a selected skill package or direct Markdown skill into the
target skills directory. Git installs clone with `git clone --depth 1` into a
temporary/cache location, then use the same discovery and install rules as
local paths. If multiple installable skills are found, the command requires
`--all` or `--name`.

## Scanner And Provenance

Installs scan candidate skill content before copying. The scanner reports
findings in these categories: prompt injection, exfiltration, destructive
commands, persistence, network/tunnel, and obfuscation.

Scanner verdicts are:

- `safe`
- `caution`
- `dangerous`

External and Git installs block `dangerous` verdicts unless the CLI caller
supplies `--force`. Model tool installs cannot force. Scanner findings are
diagnostics and do not rewrite skill content.

Runtime writes a minimal provenance sidecar under `$PSYCHEVO_HOME/skills` for
installed skills. The sidecar records source type, source path or URL, installed
timestamp, scanner verdict, and original skill name. This sidecar is operational
metadata, not part of `SKILL.md`.

## Related Topics

- [006 Context Assembly](../006-context-assembly/spec.md) owns model visibility
  for skill index and explicit skill expansion material.
- [050 Capability Extensions](../050-capability-extensions/spec.md) owns
  capability source and contribution selection boundaries.
- [100 Coding Agent](../100-coding-agent/spec.md) defines the built-in
  coding-agent capability that may use skill adjunct tools.
- [110 Coding Core Tools](../110-coding-core-tools/spec.md) defines the required
  coding-core toolset, which skill tools do not join.
- [200 pevo CLI](../200-pevo-cli/spec.md) owns concrete command spelling.
- [210 pevo TUI](../210-pevo-tui/spec.md) owns interactive slash projection.
