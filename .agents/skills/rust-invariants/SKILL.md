---
name: rust-invariants
description: Use when changing Rust assertions, validation boundaries, internal invariants, panic behavior, lint policy, unsafe code, safety arguments, or other correctness-critical implementation details.
---

# Rust invariants

Classify each condition before changing code:

- External or recoverable invalidity returns a contextual error before trusted state is built.
- An internal invariant violation is a programming defect and may justify an adjacent assertion.
- A safety or integrity requirement must hold in release builds; never establish it only with `debug_assert!`.

Prefer a type that carries a proven invariant over repeated checks. Keep validation at the trust boundary and keep assertions beside the mutation, transition, or operation they protect. Assertion messages should name the failed invariant and relevant values when safe.

Read [references/assertions.md](references/assertions.md) before adding or changing assertions. Read [references/unsafe-invariants.md](references/unsafe-invariants.md) before changing unsafe code, panic behavior around unsafe code, or a safety argument. Read [references/defensive-patterns.md](references/defensive-patterns.md) for compiler-enforced alternatives to convention: semantic access, exhaustive matching, deliberate defaults, and proportional defenses. Read [references/lint-policy.md](references/lint-policy.md) before changing lint configuration, suppressions, or panic-path lint policy.

Verify both accepted and rejected inputs. Exercise the surrounding path so assertions are active verification oracles rather than dormant documentation. Report which invariant was added, preserved, or changed and which checks actually exercised it.

