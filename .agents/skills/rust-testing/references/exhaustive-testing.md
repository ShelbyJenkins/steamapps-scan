# Stronger Rust verification

Choose a stronger technique according to the failure mode and state exactly what its harness covers.

Use Miri for unsafe operations, raw pointers, aliasing, initialization, custom allocators, and tricky interior mutability. Miri detects undefined behavior only on executed paths.

Use address, thread, leak, or related sanitizers when native execution, FFI, platform libraries, or cross-language behavior matters. Sanitizers complement Miri but also depend on workload coverage.

Use Loom for small synchronization primitives and bounded concurrent models. Replace the relevant synchronization primitives, keep operations bounded, and assert invariants at transitions. A passing model says nothing about behavior omitted from the model.

Use Kani for small algorithms and safety kernels where bounded symbolic exploration is tractable. State assumptions, assert the property, bound loops and collections realistically, and keep the harness independent of unrelated application complexity.

Use fuzzing for parser, normalization, codec, and state-machine properties over large byte or operation spaces. Configure panic behavior only when the harness requires it and retain minimized failures.

Report toolchain, target, harness, bounds or time budget, properties checked, and unsupported paths. Do not claim exhaustive verification beyond the model and bounds that actually ran.

