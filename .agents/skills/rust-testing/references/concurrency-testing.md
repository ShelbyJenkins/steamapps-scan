# Concurrency and execution-chaos testing

Test ordering, cancellation, retry, timeout, shutdown, resource cleanup, and recovery at the boundaries where concurrent work becomes observable.

Avoid real sleeps as synchronization. Use explicit channels, barriers, simulated time, deterministic schedulers, or bounded polling with a diagnostic failure. Keep thread counts and operation sequences small enough that a failure can be understood and reproduced.

Use scheduler-controlled or simulated execution for asynchronous services, network behavior, and race-prone state transitions. Vary wakeups, delays, failures, cancellation, and shutdown ordering while asserting stable safety and externally visible properties.

Control global state and isolate ports, files, clocks, and environment variables. Clean up processes and resources through guards where possible. A flaky test is an unresolved defect in the product, fixture, or test design; quarantine only with a named cause, owner, and removal condition.

Use Loom for small synchronization primitives and bounded concurrent state machines when exhaustive interleaving matters. Keep its model separate from a full application and assert invariants after meaningful transitions.

