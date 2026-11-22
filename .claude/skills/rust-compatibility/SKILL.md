---
name: rust-compatibility
description: Use when changing a Rust public API, trait implementation, serialized or protocol format, CLI contract, feature behavior, supported target, public dependency exposure, or other semver-sensitive behavior.
---

# Rust compatibility

Inventory the compatibility surface before changing it. Include public fields and variants, concrete return types, trait bounds and implementations, re-exports, macros, serialized forms, protocols, CLI flags and exit codes, environment and configuration keys, features, and supported targets.

Keep representation private when callers do not need to depend on it. Use `impl Trait` only when callers need behavior but should not name the concrete type. Use a named public type when callers must store, implement traits for, or otherwise rely on it.

Treat dependency types in public signatures, fields, bounds, implementations, macros, and wire formats as intentional commitments. Prefer private adapters or project-owned domain wrappers when that coupling is not part of the contract. Do not add convenience conversion impls that unintentionally become permanent API.

Use sealing or `#[non_exhaustive]` only when the extension policy is clear. Run available public-API or semver tools as signals, then review behavioral, coherence, serialization, feature, macro, platform, and dependency effects that tools cannot prove. Document intentional breaks and migration behavior rather than suppressing evidence. Record which behaviors and formats are intentionally stable and which are explicitly unstable, so accidental behavior does not become a permanent contract.

