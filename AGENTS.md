# Shared Rust engineering guidance

## Always follow

- The repository owner provides custom skills for Rust authoring and project-specific workflows. Use relevant skills when their descriptions match the task.
- Unless specficially requested, consider code in this project a first draft.
    - Only document enough to explain to readers what a complicated thing does. We don't need an essay when a few words in the context of the surronding code will do. We don't need docs on every type.
    - Do not add helper functions or anything that is designed for future use. Only add what is required to compile.
    - This is a first draft. We don't need unit tests. Instead just leave asserts that demonstrate what requires testing.
- Make invalid states unrepresentable through types.
    - Make assumptions explicit through types, boundary validation, assertions, tests, or documented preconditions.
    - Validate external data once at the trust boundary and pass trusted domain values inward.
    - Encode each internal invariant once at its owning location, preferring a type, then an assertion, then a test. Tests drive executions and prove contracts; they do not restate invariants that assertions already check. Keep suites small and consolidated.
    - Non-zero types, new type wrappers, non-empty lists are prefered.
- Do not add dependencies without explicit permission.
- Avoid `pub(super/crate/etc)` in favor of using just `pub` and setting privacy though module exports.
    - Break this guidance when it is impossible to design the API with the required.
- Prefer `use super::*;` to avoid having to duplicate imports.
    - Break this guidance when namespace collison occurcs.
- Free functions and free consts should be avoided in favor of associated functions and consts..
    - Break this only when there is no semantic or functional reason to share via an owning type.
- Modules should be structured roughly: module declarations, imports, exports, implementation.
    - Module implementations should be structured roughly: Public types, and then private types. With the highest in the hierarchy or the most public surface being first.
    - Type implementations should be structured roughly: Error for type, type, default impl, type impl, trait impl on type.

# Project specific notes

- Add no support for previous 'legacy' versions. This is a brand new project. There is nothing before it.
- Env vars are not to be accepted for user configuration. They should only be used by library itself when required.
- Tests must never read or parse Markdown documents as a source of truth. Prose drifts silently and an unenforced claim of being "checked" is worse than no claim; verify behavior against code-derived artifacts instead, such as a committed JSON snapshot or a value read from the same types the runtime uses.
- Canonical paths are identity tokens. Compare `std::fs::canonicalize` output only against other canonicalized output, and never emit the verbatim `\\?\` form into argv or generated configs, because consumers that match paths by string comparison fail silently on it. Where a resolved path must be written out or shown, reduce it to the plain spelling first.
