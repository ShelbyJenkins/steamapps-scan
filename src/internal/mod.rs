//! Backend implementation the public API drives: root providers and the
//! scan pipeline. Nothing here is exported; callers interact only with the
//! types re-exported from the crate root.

mod providers;
// `internal` is private, so this remains crate-internal while allowing the
// public-report sibling module to drive the scan pipeline.
pub mod scan;
