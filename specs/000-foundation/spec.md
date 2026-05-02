---
name: 000. Foundation
psychevo_self_edit: deny
---

Define the fundamental constraints and default assumptions of Psychevo as the upstream baseline for subsequent specs.

## Scope

- the purpose and design principles at the foundation level
- fundamental constraints

Out of scope:
- Rust workspace layout or crate boundaries
- runtime topology or dependency direction
- event, trace, provider, tool, or session schemas
- modes, UI, CLI, SDK, extension, or workflow features

## Principles

- Evolution needs substrate. Psychevo must first build a dependable execution substrate before adding self-evolution, workflow search, memory, skill generation, or other higher-level adaptive capabilities.
- Core stays minimal. The core should contain only the responsibilities required for stable execution and extension. Product surfaces, transport concerns, workflow opinions, and convenience features belong outside the core.
- Execution leaves evidence. Agent behavior should produce explicit records that make runs inspectable and replayable. Important behavior must not depend on hidden state that cannot be reconstructed from durable evidence.

## Related Topics

- [001 Architecture](../001-architecture/spec.md) defines Rust workspace layout, crate boundaries, runtime topology, and dependency direction.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines the durable evidence contract downstream of the execution evidence principle.
- [010 Memory System](../010-memory-system/spec.md) defines the optional memory boundary downstream of the execution substrate.
