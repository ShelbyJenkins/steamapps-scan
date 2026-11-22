# Unit and integration tests

Use colocated unit tests for private helpers, small algorithms, local invariants, precise error branches, and edge cases that need internal access. Keep them focused on behavior; do not couple them to a representation that can change without changing the contract.

Use integration tests for the public API as downstream code sees it, end-to-end validation and execution, persistent formats, protocols, CLI behavior, and compatibility expectations. An integration test should fail when the external contract changes, not when an internal collection is replaced.

Use compile-tested examples for public workflows that users should copy. Examples do not replace error-path and invariant testing.

Choose coverage by risk and value. Prioritize complex logic, public APIs, error-handling paths, state transitions, boundary and validation logic, and resource management. Weigh the consequence of failure, the complexity of the logic, how often the code runs, and whether the test protects future refactoring; do not chase a coverage metric. When an assertion inside the code proves the same property, prefer the assertion and exercise it through existing paths rather than duplicating it as a trivial test.

Build deterministic fixtures with process-specific temporary directories and explicit cleanup. Avoid developer paths, ambient configuration, locale, time zone, network access, and real sleeps unless the test owns those conditions. Prefer signaling, simulated time, or bounded polling for coordination.

Consolidate cases that exercise the same behavior through table-driven inputs, but keep distinct failure modes separate when their error categories or side effects are part of the contract. Avoid duplicate, trivial, and implementation-only tests that add maintenance without increasing confidence, and remove tests made redundant by assertions, broader tests, or refactoring.

