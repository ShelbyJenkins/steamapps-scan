---
name: rust-testing
description: Use when fixing Rust bugs or adding, changing, reviewing, planning, consolidating, or removing tests, error-path coverage, assertions as test oracles, property tests, mutation tests, concurrency tests, Miri, sanitizers, Loom, or Kani verification.
---

# Rust testing

Choose the cheapest oracle before writing any test: a type that makes the invalid state unrepresentable, then an assertion at the invariant's owning location, then a test that drives execution through that code. Read [references/assertions-as-oracles.md](references/assertions-as-oracles.md) before adding tests or assertions. Do not add a test that restates what an assertion already proves, and prefer a small consolidated suite over many near-duplicate tests.

Define the behavior, rejection paths, and invariants that the change must prove. Add a regression test for a reproducible bug and update error-path tests whenever validation or rejection changes.

Choose the smallest technique that can expose the relevant failure:

- [references/unit-and-integration-tests.md](references/unit-and-integration-tests.md) for test-layer boundaries and deterministic fixtures.
- [references/error-path-testing.md](references/error-path-testing.md) for rejection, partial-mutation, recovery, and error-quality checks.
- [references/property-testing.md](references/property-testing.md) for large input spaces, normalization, parser, and model-comparison properties.
- [references/mutation-testing.md](references/mutation-testing.md) for evaluating whether assertions and tests detect plausible implementation mistakes.
- [references/concurrency-testing.md](references/concurrency-testing.md) for ordering, cancellation, shutdown, retry, and scheduler-controlled tests.
- [references/exhaustive-testing.md](references/exhaustive-testing.md) for Miri, sanitizers, Loom, and Kani.

Keep tests independent of developer-specific paths, ambient network access, locale, time zone, and real sleeps unless the test explicitly owns those conditions. Control seeds, clean up resources, and retain minimized failures as ordinary regressions.

Do not add a testing dependency without following repository dependency policy. Report commands, targets, properties, error paths, and relevant heavy checks not run.

