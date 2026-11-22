# Mutation testing

Use mutation testing to learn whether the suite detects plausible implementation mistakes such as removed branches, reversed comparisons, altered constants, skipped validation, or weakened assertions.

Prioritize correctness-critical modules and rejection paths. A surviving mutant can mean that behavior is untested, an assertion is too weak, code is redundant, or the mutation is semantically equivalent. Classify the result before adding a test.

Do not chase a perfect score. Add tests only when the mutant exposes a meaningful contract or invariant. Remove redundant code when the mutation proves that it has no observable effect.

Run mutation work with a bounded scope and budget. Record the command, selected packages or files, surviving meaningful mutants, equivalent mutants, and follow-up decisions. Treat infrastructure failures as unverified work rather than as passing results.

