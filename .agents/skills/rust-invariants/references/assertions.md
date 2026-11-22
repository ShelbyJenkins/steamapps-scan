# Assertions and invariant checks

Assertions expose programming defects; they do not replace fallible validation of command-line arguments, files, environment values, network data, deserialization, IPC, or FFI input.

Use `assert!` when continuing after an internal violation could corrupt persistent state, invalidate a safety argument, break an explicit programmer contract, or make the result untrustworthy. Use `debug_assert!` for development-time consistency checks only when omitting the check in release cannot make safe code unsound or silently corrupt state.

Place an internal precondition immediately before the operation that relies on it. Place a postcondition immediately after the mutation or transition that establishes it. Keep unsafe requirements at the safe-to-unsafe boundary and enforce every safety requirement in release builds. Write each invariant exactly once at its owning location; every execution that reaches it then checks it, and one well-placed assertion can replace a family of unit tests.

Prefer comparison assertion macros so failures display both values. Messages should name the invariant, operation, and relevant values when those values are safe to expose. Avoid vague messages such as `invalid state` when the code can identify the mismatched lengths, illegal transition, or broken index.

Keep checks visible at the call site. Extract a helper only when the logic is multi-line and repeated at four or more call sites, and make it preserve the domain context, the validated return type, and the specific error type. Keep the recognizable assertion or error return beside the operation it protects; do not create a helper that merely renames an assertion macro, hides a single conditional, or exists mostly to hide parsing, defaulting, or invariant construction. Extract cold error paths or tune inlining only when measurement shows the direct check harms a critical path.

Exercise assertions through ordinary tests, fuzzing, or model checks. Enable debug assertions in a fuzz build when the property depends on `debug_assert!`. If a test reaches an assertion, minimize and retain the reproducer, then fix the violated invariant rather than deleting the oracle.

