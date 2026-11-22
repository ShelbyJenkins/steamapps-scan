# Property and differential testing

Use generated inputs when the valid and adversarial space is too large for representative examples. Prefer domain properties over arbitrary case counts.

Useful properties include:

- encode then decode preserves the value;
- normalization is idempotent;
- ordering is transitive and agrees with equality;
- a parser never panics on arbitrary bytes;
- rejected input causes no partial mutation;
- every operation preserves a structural invariant;
- an optimized implementation matches a smaller obviously correct model.

Compare custom parsers, schedulers, caches, codecs, collections, or numerical routines with a standard-library implementation, a specification model, a prior stable version, or an independent encoder or decoder when one exists. Compare observable state after each generated operation or defined checkpoint.

Control and print random seeds on failure. Minimize failing cases and retain them as ordinary deterministic regression tests. Do not add a property-testing dependency without reviewing its maintenance, transitive surface, and repository policy.

