# Linked parameters and boolean flags

Replace parameters whose legal values depend on each other with one enum or structured type. Common signals include `enabled: bool` plus `Option<T>`, a mode plus fields used only by that mode, an action plus action-specific fields, and a count plus allocation details.

Each variant should contain exactly the data required for that case. Avoid catch-all variants that recreate a collection of optional fields inside the enum. Match exhaustively and keep variant-specific behavior local.

When wire compatibility requires a flat external shape, deserialize into the raw form and validate it into the linked internal enum. Reject contradictory or incomplete combinations with field-specific context.

A positional boolean is acceptable only when its meaning is obvious and cannot be confused with another flag. Otherwise use a descriptive enum, a named options type, separate methods, or a linked enum. Multiple positional booleans are a strong redesign signal.

Test every variant, rejected external combination, and conversion boundary. For public APIs, review whether adding a future variant or field is a compatible change.

