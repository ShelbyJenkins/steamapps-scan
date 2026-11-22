# Two-phase boundary types

Do not use a deserialization, request, CLI, or configuration type as the runtime model merely because its fields are already available.

The raw form mirrors the external source and may contain textual, optional, aliased, deprecated, partial, or unresolved values. The validated form contains normalized values required by runtime code: defaults are applied, required data is present, references are resolved, and cross-field constraints are proven.

Perform the transition once through an explicit validation or resolution operation. Return field names, source locations, bad values, and correction guidance when safe and useful. Aggregate independent validation errors when doing so materially reduces correction time.

Make runtime components accept the validated type. Do not retain `Option<T>` merely because an external field was optional when runtime behavior always requires a value. Keep meaningful absence explicit when not configured remains a legal runtime state.

Keep validated fields private unless they are intentionally stable API. Test defaults, normalization, independent errors, cross-field rejection, deprecated aliases, and the guarantee that a failed transition leaves no trusted partial state.

