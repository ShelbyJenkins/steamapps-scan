---
name: rust-conventions
description: Use when changing Rust source files, Cargo manifests, module layout, naming, function placement, where clauses, or error type design.
---

# Rust conventions

Use `edition = "2024"`, and `resolver = "3"` in workspaces, unless a documented MSRV or compatibility reason requires otherwise.

Keep `lib.rs` and `mod.rs` files for documentation, imports and exports, and const definitions. They should generally be free of behavior; using them as a temporary location for code during development is acceptable.

Avoid generic scaffolding: many tiny files, empty files, constants-only modules, and common directory structures without project-specific reasoning. Prefer small, idiomatic, boring code over over-engineered architecture.

Make function and variable names descriptive enough that documentation becomes unnecessary; being understandable from the name is preferable to a comment. If behavior is specific to a type, use an associated function rather than a free function. Free functions should be rare.

Do not use a `where` clause for a single bound; `fn serialize<S: serde::Serializer>` fits on one line, so use that form.

Name error types with an `Err` suffix to save space, and keep `error` out of variant names; the type already makes their purpose clear.
