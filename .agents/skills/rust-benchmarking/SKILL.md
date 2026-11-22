---
name: rust-benchmarking
description: Use when changing performance-sensitive Rust code, designing or reviewing benchmarks, setting regression gates, or making claims about latency, throughput, goodput, memory, or capacity.
---

# Rust benchmarking

State the user-visible performance question before writing or changing a benchmark. Identify the target environment, workload shape, capacity limit, unacceptable regression, and whether the result is diagnostic, comparative, or a release gate.

Cover the relevant performance profile: ordinary and pathological inputs, cold and warm state, small and large data, under-capacity efficiency, at-capacity behavior, overload degradation, and recovery. Measure correctness and goodput so skipped validation, rejected work, or lower output quality cannot appear as an improvement.

Read [references/trustworthy-measurement.md](references/trustworthy-measurement.md) before selecting metrics, load models, comparison methods, or CI thresholds.

Report the workload, target, distributions or uncertainty, correctness checks, resource trade-offs, and material conditions not measured. Do not gate changes on a benchmark that is too noisy or unrepresentative to support the decision.

