# Typestates

Use typestate when available operations materially differ by lifecycle state and transitions can consume one state and produce another. Useful cases include uninitialized to initialized, disconnected to connected to authenticated, builder to validated to launched, and open transaction to committed or rolled back.

Expose state-specific methods only on the state in which they are valid. Make transitions ownership-consuming when reuse of the earlier state would be invalid. Keep common storage and behavior in one underlying representation so state modeling does not duplicate implementation logic.

Make fallible transitions return an error that preserves enough state for documented recovery. Do not imply that a transition completed when external work may have partially failed.

Prefer a checked runtime state machine when state is dynamic, shared by many owners, discovered only at runtime, or must be serialized as a stable enum. Typestate is valuable only when it removes meaningful illegal call sequences; do not add it for type-system novelty.

