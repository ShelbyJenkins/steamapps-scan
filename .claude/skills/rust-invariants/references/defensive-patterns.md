# Defensive patterns

The goal is not to prevent every crash. Make invalid states difficult to express, make incomplete refactors visible to the compiler, and make genuine failures explicit and diagnosable.

Treat `this should never happen` as a review signal. First try to make the state unrepresentable through a type, enum, constructor, pattern match, or visibility boundary. When that is impractical, preserve a loud failure with useful context rather than silently inventing data, clamping values, discarding errors, or continuing in a semantically different state.

Use semantic access instead of detached checks. Prefer `first`, `get`, iterators, and pattern matching over separate check-then-index logic. Use exact slice patterns when exact cardinality is itself a domain invariant; do not add exhaustive matching when only optional access is required.

Make closed cases exhaustive. Avoid wildcard enum arms when a new variant should force review. In equality, hashing, serialization, projections, and policy decisions, destructure the relevant fields so a newly added field produces a compile error. Name intentional omissions when their identity matters.

Use defaults deliberately. Use `..Default::default()` only when the call site intentionally accepts sensible defaults for both current and future fields; initialize or destructure explicitly when adding a field must trigger review.

Limit mutability to its purpose. Keep mutation local to construction or transformation, then return or shadow an immutable value when later mutation would be accidental.

Keep defenses proportional. Choose the smallest mechanism that materially strengthens a guarantee, and confirm the defensive mechanism is simpler and clearer than the bug it prevents. Useful review questions: which correctness assumption is still enforced only by convention, can a caller construct or mutate an invalid value, and does any fallback preserve the requested semantics or merely hide failure.
