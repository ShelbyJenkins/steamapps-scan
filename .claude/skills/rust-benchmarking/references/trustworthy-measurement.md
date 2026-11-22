# Trustworthy performance measurement

Represent the deployed workload: ordinary, pathological, and adversarial inputs; cold start, warm state, and steady state; small and large data; cache hits and misses; under-capacity, at-capacity, and overload behavior; and every target that can differ materially.

Measure the outcome that matters. Consider throughput, successful-work goodput, latency distributions and tails, memory average and peak, allocations and retention, CPU counters, startup and shutdown, queue depth, dropped work, recovery, and output quality for approximate systems.

Do not reduce a result to one mean when a distribution, percentile, maximum, confidence interval, or controlled comparison is needed. Wall-clock time is affected by scheduling, temperature, frequency scaling, and background load. Prefer low-noise counters, interleaved comparisons, or controlled hosts for regression gates when practical.

Model arrival correctly. A closed workload waits for responses and can hide overload because offered load falls as latency rises. An open workload arrives independently, and a partly open workload mixes client feedback with external pacing. Use the model that matches production and verify that data generation, clients, logging, and collection do not bottleneck the load generator.

Version important workloads and corpora, and record why each pathological case exists. Keep setup outside the measured interval unless setup is user-visible. Prevent compiler elimination and verify benchmark output for correctness. Revisit the suite when a production performance failure was not predicted.

Gate only on a stable metric with a justified threshold. Preserve raw results and environment details so a regression can be investigated rather than merely observed.

