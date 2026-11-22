---
name: rust-documentation
description: Use when adding or changing documentation, README content, design documents, Markdown, code comments, module docs, or API docs, or when documenting design decisions, rejected alternatives, accepted trade-offs, safety arguments, unsupported cases, or intentional omissions.
---

# Rust documentation

Read [references/documentation-style.md](references/documentation-style.md) before writing or revising README content, design documents, module docs, or comments.

Document non-obvious decisions near the code or in the repository's existing decision-record format. Preserve the problem and constraints, the chosen design, credible rejected alternatives, reasons for rejection, accepted downsides, consequences, and facts that should trigger reconsideration.

Record what code cannot show because it is intentionally absent: unsupported cases, deferred recovery, omitted trait implementations or conversions, validation performed elsewhere, deferred optimizations, and unsupported platforms. State what happens if the case occurs and what would justify implementing it later. Give a reachable `todo!` or `unimplemented!` placeholder the same treatment: name the exact unsupported case, whether external input can reach it, the owning issue or record, and the expected replacement; prefer a typed unsupported-case error when valid input can reach the path.

Keep safety arguments adjacent to unsafe operations. Name who establishes an invariant, which operations can change it, where release enforcement occurs, and which verification exercises it.

Update the nearest durable document when an invariant, unsupported case, public commitment, dependency boundary, benchmark assumption, or accepted trade-off changes. Avoid status narration, repeated implementation detail, and comments that merely restate the code.

