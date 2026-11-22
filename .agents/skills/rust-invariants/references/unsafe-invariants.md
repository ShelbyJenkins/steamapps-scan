# Unsafe invariants and panic behavior

Keep each unchecked operation inside an explicit `unsafe` block even when the enclosing function is unsafe. Place a safety comment beside the block that explains every caller obligation and why the current values satisfy it.

Types and release checks must establish memory-safety requirements before the unsafe boundary. A comment, test, or `debug_assert!` can explain or exercise the argument but cannot be its only enforcement.

Review representation and ownership together with unsafe code. Identify aliasing, initialization, alignment, lifetime, provenance, thread-safety, bounds, and drop-order assumptions. State who establishes each assumption, which operation can invalidate it, and which check or type prevents that invalidation.

Panics should represent programming defects or broken internal invariants, not ordinary input, I/O, configuration, protocol, or environmental failures. Replace reachable `unwrap`, indexing, slicing, unchecked arithmetic, `todo!` or `unimplemented!` placeholders, and unreachable-state claims with fallible control flow unless a local invariant makes the panic path an intentional programmer contract.

Use Miri for executed paths involving raw pointers, aliasing, custom allocation, or interior mutability. Use native sanitizers when FFI, platform libraries, or native execution behavior matters. Neither technique proves unexecuted paths safe; keep harnesses focused on the unsafe operations and state their coverage limits.

Treat lint suppressions as reviewed exceptions. Prefer `#[expect(..., reason = "...")]` when supported so obsolete suppressions become visible. Do not weaken unsafe or panic lint policy merely to land a change.

