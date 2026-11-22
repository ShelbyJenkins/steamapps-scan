---
name: rust-api-design
description: Use when adding or changing Rust APIs, public types, constructors, configuration types, boolean parameters, linked arguments, newtypes, typestates, or state transitions.
---

# Rust API design

Start from legal states and transitions, not from the convenient storage type. Identify which values are raw external input, which invariants a runtime value proves, and which invalid combinations the API should make impossible.

Use the focused references that match the change:

- [references/newtypes.md](references/newtypes.md) for distinct domain values and validated primitives.
- [references/typestates.md](references/typestates.md) for lifecycle-specific operations and ownership-consuming transitions.
- [references/two-phase-types.md](references/two-phase-types.md) for raw configuration, request, or serialized forms that must become resolved runtime values.
- [references/linked-parameters.md](references/linked-parameters.md) for mode-dependent fields, optional-value pairs, and positional booleans.

Keep public representation private unless it is intentionally stable. Expose only construction and operations that preserve the domain meaning. Avoid type-system machinery that does not remove a meaningful misuse case.

Prefer Rust ownership idioms over another language's object model: ownership-consuming transitions for one-way state changes, borrows for temporary access, RAII guards for cleanup and rollback, iterators over eagerly materialized intermediate collections, enums with exhaustive matching for closed sets, composition over inheritance-shaped trait hierarchies, and explicit capability values over ambient global access.

Model fallibility honestly. Provide an infallible conversion only when every input has a correct target representation, and keep a documented `Unknown` or fallback variant deliberate domain behavior rather than a place where invalid data hides. Implement `Default` only when a meaningful default state exists. Mark results, guards, builders, and configuration values with `#[must_use]` when discarding them likely indicates a bug.

Also load `rust-compatibility` for a public or semver-sensitive change. Finish by testing valid construction, rejected construction, and every changed state transition.

`pub(super)` and `pub(crate)` should almost never be used. Use `pub` and then set privacy through module organization and re-exports.
