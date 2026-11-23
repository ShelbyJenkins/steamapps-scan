//! steamapps-scan - Discovery report
//! ==================================
//!
//! Public inventory, discovery state, attempt ledger, and diagnostics.

use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
    path::{Path, PathBuf},
    sync::Arc,
};

use super::paths::{InstallDir, LibraryRoot, ManifestPath, SteamRoot, ValidationErrKind};
use crate::internal::scan::SteamScan;

/// A validated Steam application installation.
///
/// App IDs are intentionally not unique in an inventory: the same app can be
/// installed in libraries reached through different Steam roots. Each
/// installation directory appears once per app, though: of several manifests
/// that resolve to one directory, the one from the library holding it is kept
/// and each other leaves a [`DiagnosticKind::Duplicate`] warning.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[non_exhaustive]
pub struct AppInstall {
    /// Steam application ID parsed from and cross-checked with the manifest.
    app_id: NonZeroU32,
    /// Optional display name from the manifest.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Raw `installdir` value from the manifest.
    manifest_install_dir: String,
    /// Manifest that describes this installation.
    manifest_path: ManifestPath,
    /// Library containing the manifest.
    library_root: LibraryRoot,
    /// Canonical installation directory.
    install_dir: InstallDir,
    /// Advisory manifest metadata, parsed leniently.
    metadata: AppMetadata,
}

impl AppInstall {
    pub(crate) const fn from_scanned(
        app_id: NonZeroU32,
        name: Option<String>,
        manifest_install_dir: String,
        manifest_path: ManifestPath,
        library_root: LibraryRoot,
        install_dir: InstallDir,
        metadata: AppMetadata,
    ) -> Self {
        Self {
            app_id,
            name,
            manifest_install_dir,
            manifest_path,
            library_root,
            install_dir,
            metadata,
        }
    }

    /// Nonzero Steam application ID cross-checked with the manifest file name.
    #[must_use]
    pub const fn app_id(&self) -> NonZeroU32 {
        self.app_id
    }

    /// Optional display name from the manifest.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Raw, trimmed `installdir` value from the manifest.
    #[must_use]
    pub fn manifest_install_dir(&self) -> &str {
        &self.manifest_install_dir
    }

    /// Manifest that describes this installation.
    #[must_use]
    pub const fn manifest_path(&self) -> &ManifestPath {
        &self.manifest_path
    }

    /// Library containing the manifest.
    #[must_use]
    pub const fn library_root(&self) -> &LibraryRoot {
        &self.library_root
    }

    /// Canonical installation directory.
    #[must_use]
    pub const fn install_dir(&self) -> &InstallDir {
        &self.install_dir
    }

    /// Advisory metadata parsed from the manifest.
    #[must_use]
    pub const fn metadata(&self) -> &AppMetadata {
        &self.metadata
    }

    /// Return the app's canonical installation directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.install_dir.path()
    }
}

/// Advisory fields from an app manifest, present when the manifest carried
/// them in a readable form.
///
/// Unlike the identity fields on [`AppInstall`], nothing here is validated
/// or required: a missing or malformed value is `None` (or an absent depot
/// entry), never an inventory error. Callers that need these values
/// verified use [`AppInstall::content_signal`], which rereads the manifest
/// strictly.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[non_exhaustive]
pub struct AppMetadata {
    /// Steam install-state bitmask; see [`StateFlags`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_flags: Option<StateFlags>,
    /// Build currently installed, as Steam recorded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_id: Option<NonZeroU64>,
    /// Steam's accounting of the installation size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_on_disk: Option<u64>,
    /// Last manifest update, in Unix seconds as Steam recorded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<u64>,
    /// Selected beta branch, absent when the app follows the default branch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beta_key: Option<String>,
    /// Installed depots keyed by depot ID.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub installed_depots: BTreeMap<NonZeroU64, InstalledDepot>,
}

/// One installed depot from a manifest's `InstalledDepots` block.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[non_exhaustive]
pub struct InstalledDepot {
    /// Depot manifest ID pinning the exact content revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<NonZeroU64>,
    /// Depot size in bytes as Steam recorded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Owning DLC app when the depot belongs to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dlc_app_id: Option<NonZeroU32>,
}

/// The manifest `StateFlags` bitmask, kept raw with named predicates for
/// the commonly needed bits.
///
/// Steam sets combinations during downloads, updates, and validation; a
/// value of exactly `4` means fully installed with nothing else going on,
/// which is what [`AppInstall::content_signal`] requires.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(transparent)]
pub struct StateFlags(u64);

impl StateFlags {
    /// The exact bitmask of an idle, fully installed app; also the strict
    /// gate [`AppInstall::content_signal`] requires.
    pub(crate) const FULLY_INSTALLED: u64 = 4;
    const UPDATE_REQUIRED: u64 = 2;

    /// Wrap a raw manifest `StateFlags` value.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw bitmask exactly as the manifest carried it.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Whether the fully-installed bit is set. Other bits may also be set
    /// while Steam updates or validates the install.
    #[must_use]
    pub const fn is_fully_installed(self) -> bool {
        self.0 & Self::FULLY_INSTALLED != 0
    }

    /// Whether Steam has flagged the install as requiring an update.
    #[must_use]
    pub const fn update_required(self) -> bool {
        self.0 & Self::UPDATE_REQUIRED != 0
    }
}

/// Every valid root, library, and app retained by one scan.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[non_exhaustive]
pub struct SteamInventory {
    pub(crate) steam_roots: Vec<SteamRoot>,
    pub(crate) library_roots: Vec<LibraryRoot>,
    pub(crate) apps: Vec<AppInstall>,
}

impl SteamInventory {
    /// Validated Steam roots found by the scan.
    #[must_use]
    pub fn steam_roots(&self) -> &[SteamRoot] {
        &self.steam_roots
    }

    /// Deduplicated Steam libraries expanded from all roots.
    #[must_use]
    pub fn libraries(&self) -> &[LibraryRoot] {
        &self.library_roots
    }

    /// All valid app installations, including duplicate app IDs.
    #[must_use]
    pub fn apps(&self) -> &[AppInstall] {
        &self.apps
    }

    /// Iterate over every installation matching an app ID.
    pub fn apps_for(&self, app_id: NonZeroU32) -> impl Iterator<Item = &AppInstall> {
        self.apps.iter().filter(move |app| app.app_id() == app_id)
    }
}

/// Overall completeness of a discovery scan.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryState {
    /// At least one root was scanned and no error-severity diagnostics occurred.
    Complete,
    /// Useful inventory was retained, but one or more sources or metadata files failed.
    Partial,
    /// No viable Steam installation was found.
    NotFound,
    /// Automatic discovery is unavailable on this platform.
    Unsupported,
}

/// Severity of a retained diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiagnosticSeverity {
    /// Informational degradation that does not make the inventory incomplete.
    Warning,
    /// Failure that may have hidden a root, library, app, or executable.
    Error,
}

/// Pipeline stage that emitted a diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryStage {
    /// Querying registry, process, environment, or conventional root sources.
    LocateSteam,
    /// Confirming that a root resembles a Steam installation.
    ValidateSteamRoot,
    /// Reading or parsing `libraryfolders.vdf`.
    ReadLibraryFolders,
    /// Confirming a library, registered or a root's own, with its `steamapps`
    /// directory and layout.
    ValidateLibrary,
    /// Enumerating, reading, or parsing app manifests.
    ReadManifest,
    /// Resolving and confining a manifest's installation directory, and
    /// merging manifests that resolve to the same one.
    ValidateInstall,
    /// Resolving an executable query inside an app installation.
    ResolveExecutable,
}

/// Programmatic classification for a diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// A required path, app, or executable was absent.
    NotFound,
    /// An operating-system or filesystem operation failed.
    Io,
    /// Steam metadata could not be parsed.
    Parse,
    /// Parsed or inspected data violated an expected invariant.
    InvalidData,
    /// A path attempted to leave its permitted directory boundary.
    OutsideBoundary,
    /// Another manifest already describes the same installation.
    Duplicate,
    /// The requested automatic operation is unavailable on this platform.
    Unsupported,
}

impl From<ValidationErrKind> for DiagnosticKind {
    fn from(kind: ValidationErrKind) -> Self {
        match kind {
            ValidationErrKind::NotFound => Self::NotFound,
            ValidationErrKind::Io => Self::Io,
            ValidationErrKind::InvalidType | ValidationErrKind::NotSteamLayout => Self::InvalidData,
        }
    }
}

/// Provenance for an automatic or explicit root lookup.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoverySource {
    /// Root supplied directly by the caller.
    Explicit,
    /// Windows registry key/value lookup.
    Registry {
        /// Registry hive abbreviation, such as `HKCU` or `HKLM`.
        hive: String,
        /// Logical registry key path.
        key: String,
        /// Registry value read from the key.
        value: String,
        /// Registry view used for the lookup.
        view: String,
    },
    /// Executable path obtained from a running process.
    Process {
        /// Process image name used for the lookup.
        executable: String,
    },
    /// Root derived from an environment variable.
    Environment {
        /// Environment variable inspected.
        variable: String,
    },
    /// Hard-coded platform convention.
    Conventional {
        /// Human-readable description of the convention.
        description: String,
    },
}

/// One non-lossy attempt to locate or validate a Steam root.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[non_exhaustive]
pub struct DiscoveryAttempt {
    /// Provider or caller input responsible for the attempt.
    pub source: DiscoverySource,
    /// Candidate path, when the source produced one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<PathBuf>,
    /// Final disposition of the attempt.
    pub outcome: AttemptOutcome,
    /// Where the attempt's explanation lives, when it needs one.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<AttemptEvidence>,
}

impl DiscoveryAttempt {
    /// Index of this attempt's diagnostic in [`Discovery::diagnostics`].
    #[must_use]
    pub const fn diagnostic_index(&self) -> Option<usize> {
        match self.evidence {
            Some(AttemptEvidence::DiagnosticIndex(index)) => Some(index),
            Some(AttemptEvidence::Detail(_)) | None => None,
        }
    }

    /// Provider or validation detail kept on the attempt itself.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match &self.evidence {
            Some(AttemptEvidence::Detail(detail)) => Some(detail),
            Some(AttemptEvidence::DiagnosticIndex(_)) | None => None,
        }
    }
}

/// The explanation for one discovery attempt, held in exactly one place.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AttemptEvidence {
    /// Provider or validation detail, when useful.
    Detail(String),
    /// Index of the corresponding entry in [`Discovery::diagnostics`].
    ///
    /// This is present when the diagnostic owns the failure detail, avoiding a
    /// second copy of the same message in this attempt.
    DiagnosticIndex(usize),
}

/// Outcome of a root discovery attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AttemptOutcome {
    /// Candidate was valid and added to the inventory scan.
    Accepted,
    /// Candidate resolved to a root accepted from an earlier source.
    Duplicate,
    /// Source was available but did not contain or produce a value.
    NotPresent,
    /// Source produced a candidate that failed validation.
    Rejected,
    /// The source itself could not be queried successfully.
    Failed,
    /// The source cannot operate on this platform.
    Unsupported,
}

/// Failure to read one bounded Steam metadata file: `libraryfolders.vdf` or
/// an app manifest.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MetadataReadErr {
    /// The file's metadata could not be inspected.
    #[error("{}: could not inspect metadata file: {source}", path.display())]
    Inspect {
        /// Metadata file path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// The path is not a regular file.
    #[error("{}: metadata path is not a regular file", path.display())]
    NotFile {
        /// Metadata file path.
        path: PathBuf,
    },
    /// The file exceeds [`crate::MAX_STEAM_METADATA_BYTES`], or grew past it
    /// while being read.
    #[error(
        "{}: metadata exceeds the {}-byte limit",
        path.display(),
        crate::MAX_STEAM_METADATA_BYTES
    )]
    TooLarge {
        /// Metadata file path.
        path: PathBuf,
    },
    /// The file could not be opened.
    #[error("{}: could not open metadata file: {source}", path.display())]
    Open {
        /// Metadata file path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// The file could not be read.
    #[error("{}: could not read metadata file: {source}", path.display())]
    Read {
        /// Metadata file path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// The file is not valid UTF-8.
    #[error("{}: metadata is not valid UTF-8: {source}", path.display())]
    NotUtf8 {
        /// Metadata file path.
        path: PathBuf,
        /// Position of the first invalid byte sequence.
        source: std::str::Utf8Error,
    },
}

impl MetadataReadErr {
    /// Record this failure as an error diagnostic from `stage`.
    pub(crate) fn into_diagnostic(
        self,
        stage: DiscoveryStage,
        app_id: Option<NonZeroU32>,
    ) -> Diagnostic {
        let (kind, path) = match &self {
            Self::Inspect { path, .. } | Self::Open { path, .. } | Self::Read { path, .. } => {
                (DiagnosticKind::Io, path.clone())
            }
            Self::NotFile { path } | Self::TooLarge { path } | Self::NotUtf8 { path, .. } => {
                (DiagnosticKind::InvalidData, path.clone())
            }
        };
        Diagnostic::error(stage, kind, self.to_string())
            .with_path(path)
            .with_app_id(app_id)
    }
}

/// A retained warning or error from any discovery or query sibling.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[non_exhaustive]
pub struct Diagnostic {
    /// Whether the diagnostic makes the result incomplete.
    pub severity: DiagnosticSeverity,
    /// Pipeline stage that emitted the diagnostic.
    pub stage: DiscoveryStage,
    /// Programmatic failure category.
    pub kind: DiagnosticKind,
    /// Root-discovery provenance, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "source")]
    pub provenance: Option<DiscoverySource>,
    /// Most relevant filesystem path, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Relevant Steam app ID, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<NonZeroU32>,
    /// Human-readable diagnostic detail.
    pub message: String,
}

impl Diagnostic {
    /// Whether this diagnostic makes its enclosing result incomplete.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == DiagnosticSeverity::Error
    }

    /// Construct an error diagnostic for a failed scan or adapter validation.
    ///
    /// Optional context attaches through [`Self::with_path`] and
    /// [`Self::with_app_id`].
    pub fn error(stage: DiscoveryStage, kind: DiagnosticKind, message: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Error, stage, kind, message)
    }

    pub(crate) fn warning(
        stage: DiscoveryStage,
        kind: DiagnosticKind,
        message: impl Into<String>,
    ) -> Self {
        Self::new(DiagnosticSeverity::Warning, stage, kind, message)
    }

    fn new(
        severity: DiagnosticSeverity,
        stage: DiscoveryStage,
        kind: DiagnosticKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            stage,
            kind,
            provenance: None,
            path: None,
            app_id: None,
            message: message.into(),
        }
    }

    /// Attach the most relevant filesystem path.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Attach the relevant Steam app ID, when one is known.
    #[must_use]
    pub fn with_app_id(mut self, app_id: impl Into<Option<NonZeroU32>>) -> Self {
        self.app_id = app_id.into();
        self
    }

    pub(crate) fn with_source(mut self, source: DiscoverySource) -> Self {
        self.provenance = Some(source);
        self
    }
}

/// Full result of locating Steam and loading its installed-app inventory.
///
/// This is a report rather than a `Result`: useful paths remain available when
/// unrelated roots, libraries, or manifests fail.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct Discovery {
    inventory: SteamInventory,
    attempts: Vec<DiscoveryAttempt>,
    diagnostics: Arc<[Diagnostic]>,
}

impl Discovery {
    pub(crate) fn from_parts(
        inventory: SteamInventory,
        attempts: Vec<DiscoveryAttempt>,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        #[cfg(debug_assertions)]
        {
            for (attempt_index, attempt) in attempts.iter().enumerate() {
                let failed = matches!(
                    attempt.outcome,
                    AttemptOutcome::Rejected | AttemptOutcome::Failed | AttemptOutcome::Unsupported
                );
                match &attempt.evidence {
                    Some(AttemptEvidence::DiagnosticIndex(diagnostic_index)) => {
                        let diagnostic = diagnostics.get(*diagnostic_index);
                        debug_assert!(
                            diagnostic.is_some(),
                            "attempt {attempt_index} links diagnostic {diagnostic_index}, but only \
                             {} diagnostics were retained",
                            diagnostics.len()
                        );
                        if let Some(diagnostic) = diagnostic {
                            debug_assert!(
                                failed,
                                "attempt {attempt_index} links a diagnostic despite outcome {:?}",
                                attempt.outcome
                            );
                            debug_assert_eq!(
                                diagnostic.provenance.as_ref(),
                                Some(&attempt.source),
                                "attempt {attempt_index} must link a diagnostic from the same source"
                            );
                        }
                    }
                    Some(AttemptEvidence::Detail(_)) => {}
                    None => debug_assert!(
                        !failed,
                        "failed attempt {attempt_index} must retain local detail or link a diagnostic"
                    ),
                }
            }
        }
        Self {
            inventory,
            attempts,
            diagnostics: diagnostics.into(),
        }
    }

    /// Locate Steam through every supported automatic source.
    #[must_use]
    pub fn discover() -> Self {
        SteamScan::automatic().finish()
    }

    /// Load an inventory from explicit Steam roots on any platform.
    pub fn discover_from<I, P>(steam_roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        SteamScan::explicit(steam_roots.into_iter().map(Into::into)).finish()
    }

    /// Overall scan completeness derived from retained evidence.
    #[must_use]
    pub fn state(&self) -> DiscoveryState {
        if self.inventory.steam_roots.is_empty() {
            if !self.attempts.is_empty()
                && self
                    .attempts
                    .iter()
                    .all(|attempt| attempt.outcome == AttemptOutcome::Unsupported)
            {
                DiscoveryState::Unsupported
            } else {
                DiscoveryState::NotFound
            }
        } else if self.has_errors() {
            DiscoveryState::Partial
        } else {
            DiscoveryState::Complete
        }
    }

    /// Every valid root, library, and app retained by the scan.
    #[must_use]
    pub const fn inventory(&self) -> &SteamInventory {
        &self.inventory
    }

    /// Non-lossy root source and validation ledger.
    #[must_use]
    pub fn attempts(&self) -> &[DiscoveryAttempt] {
        &self.attempts
    }

    /// Warnings and errors from all scan siblings.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Iterate over root attempts together with their linked diagnostic.
    pub fn attempts_with_diagnostics(
        &self,
    ) -> impl Iterator<Item = (&DiscoveryAttempt, Option<&Diagnostic>)> {
        self.attempts.iter().map(|attempt| {
            let diagnostic = attempt
                .diagnostic_index()
                .and_then(|index| self.diagnostics.get(index));
            (attempt, diagnostic)
        })
    }

    /// Whether the scan retained at least one error-severity diagnostic.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }

    pub(crate) fn shared_diagnostics(&self) -> Arc<[Diagnostic]> {
        Arc::clone(&self.diagnostics)
    }
}

impl serde::Serialize for Discovery {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(serde::Serialize)]
        struct SerializableDiscovery<'a> {
            state: DiscoveryState,
            inventory: &'a SteamInventory,
            attempts: &'a [DiscoveryAttempt],
            diagnostics: &'a [Diagnostic],
        }

        serde::Serialize::serialize(
            &SerializableDiscovery {
                state: self.state(),
                inventory: self.inventory(),
                attempts: self.attempts(),
                diagnostics: self.diagnostics(),
            },
            serializer,
        )
    }
}
