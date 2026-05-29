---
name: 330. Harbor Benchmark Integration Attachment
psychevo_self_edit: deny
---

Define Harbor/Terminal-Bench style benchmark integration.

This attachment is part of [330 Benchmark Integrations](spec.md).

## Scope

- Harbor registry and task metadata bridge
- Rust-native Docker Compose execution for Terminal-Bench style tasks
- artifact import
- small-sample defaults

## Bridge Shape

The Harbor bridge reads official registry or task metadata through the official
tooling path when available. It translates task instructions, environment
requirements, timeouts, and scoring hooks into coding task cases.

Environment setup and scoring use a Harbor-compatible container lifecycle.
`psychevo-eval` may learn from Harbor's compose layering, mounted log
directories, resource limits, and no-network override, but the local bridge
does not depend on the Harbor Python package at runtime.

The container bridge starts a per-trial Docker Compose project. It uses the
task's prebuilt `environment.docker_image` or `environment/Dockerfile`, mounts
agent, verifier, artifact, and agent-state directories, and runs the ACP server
inside the main task container. `allow_internet = false` applies to the whole
main service, including agent setup, model calls, and verifier execution, by
adding a no-network compose override. Successful trials clean containers and
temporary volumes; failed trials retain the container and generated compose
files and report reproduction and cleanup commands.

Terminal-Bench tasks use `instruction.md` as the model-visible prompt. Hidden
tests, solution files, and verifier scripts are not included in the prompt.
After the agent phase, the bridge runs `tests/test.sh` in the same main
container and imports reward files or verifier exit status into canonical
results.

## Results

Harbor job artifacts and evaluator outputs are imported into canonical case
results. Native Harbor logs may be retained as diagnostic artifacts, but report
generation uses the imported structured result.

Real Harbor execution is a container-backed integration path. Default
validation uses fixture payloads and generated compose inspection without
starting real containers unless Docker validation is explicitly requested.

## Related Topics

- [095 Official Bridges](../095-evaluation-framework/official-bridges.md)
- [095 Sidecar](../095-evaluation-framework/sidecar.md)
- [330 Testing](testing.md)
