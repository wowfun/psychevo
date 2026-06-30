---
name: 009. Resource Surface
psychevo_self_edit: deny
---

Define the runtime-owned resource surface for non-model resources used by context assembly and tool execution.

## Scope

- runtime-owned candidate surface for non-model resources
- resource facts that may become context source candidates
- resource operations that may be attempted by tool execution
- resource boundaries around facts and operations
- access gates before model visibility or resource use
- allow, deny, and defer resource decisions
- observable resource bounds, timeouts, and aborts
- evidence relationship for resource decisions that affect model visibility or tool execution

Out of scope:
- named resource families such as filesystem, network, process, or secrets
- permission schemas, policy rule languages, approval UX, sandbox behavior, path rules, concrete enforcement mechanics, or security guarantees
- concrete tools, tool schemas, provider wire formats, model-provider authentication, or tool result formats
- resource discovery, selection precedence, provisioning, lifecycle, scheduling, cleanup, or plugin mechanics
- credential storage, secret handling, redaction, privacy, retention, or data governance policy
- storage schemas, trace formats, replay formats, session formats, migrations, indexes, or search
- CLI rendering, terminal behavior, SDK APIs, Rust APIs, payload schemas, or event names

## Resource Surface

A resource is a non-model fact, capability, or external target that may be used by context assembly or tool execution.

A resource fact is resource-derived material that may become a context source candidate.

A resource operation is attempted access, use, mutation, execution, or external effect involving a resource.

The resource surface is the runtime-supplied candidate surface of resource facts and resource operations. Candidate means available for consideration, not authorized for model visibility or execution.

Runtime supplies the resource surface. This spec does not define how resources are discovered, selected, prioritized, provisioned, scheduled, cleaned up, or configured.

Capability extensions may declare resource candidates or resource gates. [050 Capability Extensions](../050-capability-extensions/spec.md) defines source, declaration, activation, availability, and conflict boundaries for those candidates. This spec owns resource surface and resource decision semantics after candidates reach the resource boundary.

Resource facts are not automatically model-visible. [006 Context Assembly](../006-context-assembly/spec.md) owns context projection and visibility.

Tool requests are not resource authorization. [007 Tool Surface](../007-tool-surface/spec.md) owns tool declarations and execution bindings; this spec owns resource gates around resource use.

## Resource Boundary

A resource boundary is the runtime-wired perimeter around the resource surface.

The boundary separates lower-layer execution and AI protocol semantics from resource policy. `psychevo-agent-core` and `psychevo-ai` must not own resource policy semantics.

The boundary applies to both resource facts and resource operations. A resource fact may be available to runtime without becoming model-visible. A resource operation may be available to a tool binding without being allowed to proceed.

This spec does not define sandbox isolation, path containment, network policy, process policy, credential policy, or any concrete enforcement mechanism.

## Access Gates

A resource gate is a decision point before resource facts become model-visible or before or during resource operations.

Access gates may apply during context assembly, tool request handling, tool execution, or other runtime wiring that touches resources.

Access gates produce resource decisions. They do not define permission schemas, approval UX, human confirmation flows, policy rule languages, or failure presentation.

The concrete runtime permission policy is one specialization of resource gates.
[041 Permissions](../041-permissions/spec.md) owns permission modes, approval
semantics, rule precedence, and the first dangerous-action policy. This spec
continues to own the generic allow, deny, and defer decision model.

## Resource Decisions

A resource decision is allow, deny, or defer.

Allow means the gated fact or operation may proceed through the resource boundary.

Deny means the gated fact or operation may not proceed through the resource boundary.

Defer means an external runtime-level decision is needed before the fact or operation can proceed. Defer does not define approval UX, human confirmation, asynchronous policy, or final failure behavior.

Resource decisions may carry optional reason or metadata. Reason and metadata must not redefine allow, deny, or defer.

Deny or defer projection into context omission, tool-result artifacts, terminal rendering, or user-facing output is owned by adjacent specs and future implementation specs, not by this spec.

Resource operations may also encounter runtime bounds, timeout, or abort conditions. When those conditions affect model visibility or tool execution, they must be observable as resource facts, tool-result material, or before-agent-start rejection through the adjacent owning spec.

Adjacent capability or tool specs may require a denied, deferred, bounded, timed-out, or aborted operation to appear as a structured tool-result error. This spec owns that the resource decision or boundary condition exists and is observable; it does not define the tool-result format.

## Evidence Relationship

Resource decisions that affect model visibility or tool execution must be observable as durable evidence facts.

Durable evidence may represent that a resource fact was allowed, denied, or deferred before context projection, or that a resource operation was allowed, denied, or deferred before or during tool execution.

[005 Durable Evidence](../005-durable-evidence/spec.md) owns durable evidence semantics. This spec does not define record shape, serialization, storage, traces, replay, or retention.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and resource surface wiring.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for inspectable agent-invocation facts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model visibility and projection for resource facts.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool declarations and execution bindings that may use resource operations.
- [041 Permissions](../041-permissions/spec.md) defines the concrete runtime
  permission policy that specializes resource gates for local operations.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines how resource facts relate to other state families.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  how extension declarations may provide resource candidates or gates.
