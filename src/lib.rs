//! steamapps-scan - Steam inventory
//! =================================
//!
//! Generic, report-oriented Steam installation discovery happens in two
//! phases:
//!
//! - [`Discovery`] locates every viable Steam root, expands its libraries,
//!   and loads every valid app manifest into a [`SteamInventory`].
//! - [`ExecutableQuery`] resolves an app ID together with exact,
//!   installation-relative executable paths.
//!
//! Keeping these phases separate matters when one Steam app contains several
//! programs. A game and its dedicated server can share an app ID while using
//! different executable queries.
//!
//! Automatic root location supports Windows and Linux, with macOS designed
//! in but not yet validated against a real installation; other platforms
//! report an unsupported state. [`Discovery::discover_from`] scans explicit
//! roots with portable filesystem operations on any platform.
//!
//! ```no_run
//! use steamapps_scan::{Discovery, ExecutableQuery};
//!
//! const APP_ID: u32 = 123_456;
//! let discovery = Discovery::discover();
//!
//! let game = ExecutableQuery::any_of(APP_ID, [
//!     "GameDiagnostic.exe",
//!     "Game.exe",
//! ])?;
//! let server = ExecutableQuery::any_of(APP_ID, [
//!     "DedicatedServerDiagnostic.exe",
//!     "DedicatedServer.exe",
//! ])?;
//!
//! let game_report = discovery.resolve_executable(&game);
//! let server_report = discovery.resolve_executable(&server);
//! # Ok::<(), steamapps_scan::QueryErr>(())
//! ```

#![warn(missing_docs)]

// API modules: every public type lives in one of these and is re-exported
// below, so the crate root is the entire surface callers interact with.
mod content_signal;
mod file_version;
mod paths;
mod query;
mod report;

// Backend the API drives; nothing inside is exported.
mod internal;

pub use content_signal::{ContentSignalErr, SteamContentSignal};
pub use file_version::{FileVersion, VersionErr};
pub use paths::{
    ExecutablePath, InstallDir, LibraryRoot, ManifestPath, RelativeExecutablePath, SteamRoot,
    ValidationErr, ValidationErrKind,
};
pub use query::{
    ExecutableMatch, ExecutableQuery, ExecutableTarget, MatchCardinality, MatchCompleteness,
    MatchErr, MatchReport, QueryErr,
};
pub use report::{
    AppInstall, AppMetadata, AttemptEvidence, AttemptOutcome, Diagnostic, DiagnosticKind,
    DiagnosticSeverity, Discovery, DiscoveryAttempt, DiscoverySource, DiscoveryStage,
    DiscoveryState, InstalledDepot, MetadataReadErr, StateFlags, SteamInventory,
};

/// Maximum size accepted for `libraryfolders.vdf` or an app manifest.
pub const MAX_STEAM_METADATA_BYTES: u64 = 4 * 1024 * 1024;
