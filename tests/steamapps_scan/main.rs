//! steamapps-scan - Integration tests
//! ===================================
//!
//! One test binary, split by what each module controls.
//!
//! `cargo test --all-features` runs every hermetic test. Fixtures live in
//! temporary directories, and automatic discovery runs in a CLI child process
//! whose `HOME`, `XDG_DATA_HOME`, or `ProgramFiles(x86)` variable points into
//! one. The Windows registry and process sources still read the real machine,
//! so tests there assert only on the sources they control.
//!
//! `cargo test --all-features -- --ignored` runs the [`live`] checks instead,
//! against this machine's own Steam installation.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "integration fixtures and assertions intentionally fail fast with local context"
)]

#[cfg(any(windows, feature = "cli"))]
mod automatic;
#[cfg(feature = "cli")]
mod cli;
mod filesystem;
mod fixture;
mod inventory;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod live;
mod pe;
mod version;

use std::{
    num::{NonZeroU32, NonZeroU64},
    path::{Path, PathBuf},
};

use steamapps_scan::{
    AppInstall, AppMetadata, AttemptOutcome, DiagnosticKind, DiagnosticSeverity, Discovery,
    DiscoveryAttempt, DiscoverySource, DiscoveryStage, DiscoveryState, ExecutableMatch,
    ExecutablePath, ExecutableQuery, ExecutableTarget, LibraryLayout, MAX_STEAM_METADATA_BYTES,
    MatchCardinality, MatchCompleteness, MatchErr, MatchReport, QueryErr, StateFlags, SteamRoot,
};

#[cfg(feature = "cli")]
use fixture::CliScan;
use fixture::{TempTree, fixture_app_id};
use pe::{PeImage, VersionResource};
