# Newtypes

Introduce a newtype when the same primitive or collection represents distinct domain concepts, units, namespaces, validated text, paths, ports, sizes, counts, versions, secrets, or capabilities.

Reach for a standard-library invariant type before writing a custom one. A `NonZero*` constructor is a validation gate: convert with `new` at the point where raw data becomes trusted so the check and the type change happen together, and let `let ... else` or `?` handle the zero branch. Checking zero early and unwrapping later separates the proof from the value and reintroduces the panic path. `Option<NonZero*>` expresses missing-or-valid without a sentinel and stores as compactly as the raw integer. Initialize a constant one with `NonZero*::MIN`.

Counts, sizes, limits, divisors, and identifiers that reserve zero as invalid are good non-zero candidates. Do not use one when zero is meaningful, such as disabled, unlimited, or a legal offset. When an empty collection is invalid, carry one required element plus a collection of the remaining elements instead of a plain collection whose non-empty state must be remembered.

Keep the inner field private when construction must preserve an invariant. Provide the smallest constructor surface that establishes validity and returns a contextual error for recoverable invalid input. Expose only operations that preserve the domain meaning.

Derive or implement traits deliberately. Do not mirror every trait or conversion of the inner type if doing so allows callers to bypass validation, mix units, expose secrets, or depend on representation. A type alias does not provide misuse resistance because it remains interchangeable with the underlying type.

Document units, normalization, range, equality and ordering meaning, and serialization stability when those properties are part of the contract. Test valid boundaries, rejected values, and round trips for any stable external representation.

