# Panic and unsafe lint policy

Lint policy makes panic paths and unsafe assumptions visible during review. Lints are guardrails, not substitutes for understanding the domain invariant.

Centralize policy in `[workspace.lints]` in the workspace-root manifest and have every member crate opt in with `[lints] workspace = true`; Cargo does not apply workspace lint settings to members automatically. Keep member overrides narrow, documented, and justified by that crate's contract.

Deny `unsafe_op_in_unsafe_fn` so each unchecked operation stays inside an explicit unsafe block. Treat `unsafe_code = "forbid"` as a separate zero-unsafe policy for crates that must contain no unsafe code; do not enable it where reviewed unsafe code is intentional.

Enable the broad `pedantic` and `nursery` groups at warn with a lower priority so explicit settings override them. Nursery lints change between toolchains; review toolchain upgrades before promoting warnings into CI failures.

Keep panic-prone operations visible with warn-level lints for `unwrap_used`, `expect_used`, `panic`, `panic_in_result_fn`, `indexing_slicing`, `string_slice`, `unchecked_time_subtraction`, `unreachable`, `unimplemented`, `todo`, `exit`, `arithmetic_side_effects`, `as_conversions`, `dbg_macro`, `print_stdout`, `print_stderr`, and `should_panic_without_expect`. Each finding requires a local invariant or a recoverable alternative.

Keep suppressions and safety documentation reviewable with `allow_attributes`, `allow_attributes_without_reason`, `missing_safety_doc`, `undocumented_unsafe_blocks`, and `multiple_unsafe_ops_per_block`. Useful additional review aids include `fallible_impl_from`, `wildcard_enum_match_arm`, `unneeded_field_pattern`, `fn_params_excessive_bools`, and `must_use_candidate`.

Configure lint behavior in `clippy.toml` beside the workspace-root manifest. Keep test exemptions such as `allow-unwrap-in-tests` false when test code must follow the same panic-path review as production code. Add arithmetic, type, method, or macro exceptions only with a documented project-wide guarantee and a preferred replacement.

Link-time no-panic verification can prove that selected functions compile without a reachable panic handler. Apply it only to functions with an explicit no-panic contract, and verify in an optimized build that links with unwinding panics and keeps the function reachable; `cargo check` does not link, `panic = "abort"` prevents detection, and linker or optimization limits can fail functions whose panics are unreachable at the source level. Clippy remains the broad review layer; link-time verification is a stronger build-specific layer for critical parsers, storage operations, FFI boundaries, and real-time code.
