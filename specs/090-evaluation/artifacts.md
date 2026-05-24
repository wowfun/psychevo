---
name: 090. Evaluation Artifacts Attachment
psychevo_self_edit: deny
---

Define foundation rules for evaluation artifacts, trajectories, reports, and
privacy.

This attachment is part of [090 Evaluation](spec.md).

## Scope

- artifact classes and source-of-truth boundaries
- trajectory retention
- local raw data and shareable report privacy
- workspace retention principles

Out of scope:

- concrete report rendering
- CLI output paths and flags
- domain-specific patch, screenshot, or binary artifacts

## Artifact Classes

Evaluation artifacts are grouped into:

- run summary documents
- task or case result documents
- trajectory event streams
- scorer or harness logs
- environment diagnostics
- derived reports

Structured run and case result documents are the source of truth for automated
comparison. Reports are derived artifacts and must be reproducible from
structured result inputs plus the report renderer version.

## Trajectories

Trajectory artifacts record normalized events in execution order. Lower specs
may export additional trajectory formats for training or analysis, but such
exports are derived from the canonical event stream.

Trajectory events should identify source, timestamp or sequence order, case id,
candidate identity, task identity, and event kind. When an adapter cannot
provide full detail, it should emit a lossy event or collector diagnostic
instead of fabricating precise behavior.

## Privacy

Local run artifacts may retain raw prompts, model outputs, tool outputs, and
environment logs for debugging. Shareable reports must redact secrets,
credential values, HTTP headers, provider keys, and non-allowlisted environment
variables by default.

Raw public export must require an explicit opt-in in the concrete CLI or report
layer. Redaction must prefer omission or stable placeholders over partial
secret exposure.

## Retention

Lower specs define concrete artifact locations and retention defaults. The
foundation rule is that artifact retention must be explicit enough for a user
to clean local run state and to understand whether failed workspaces or raw
logs remain on disk.

Domain-specific code patches, screenshots, or final workspaces are not
foundation artifacts. A domain spec must opt into storing them.

## Related Topics

- [090 Schema](schema.md)
- [300 Reporting](../300-peval-cli/reporting.md)
- [350 Scoring](../350-coding-evaluation/scoring.md)
