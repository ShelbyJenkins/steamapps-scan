//! steamapps-scan - Scan pipeline
//! ===============================
//!
//! Steam root pipeline: candidate collection, library expansion, and
//! generic app-manifest loading. Root candidates come from the per-platform
//! sources in `providers`.

use std::{
    collections::{BTreeMap, HashSet},
    io::Read,
    num::{NonZeroU32, NonZeroU64},
    path::{Component, Path, PathBuf},
};

use normalize_path::NormalizePath;

use crate::{
    MAX_STEAM_METADATA_BYTES,
    paths::{
        FilePresence, InstallDir, LibraryRoot, ManifestPath, SteamRoot, ValidationErrKind,
        canonical_directory,
    },
    report::{
        AppInstall, AppMetadata, AttemptEvidence, AttemptOutcome, Diagnostic, DiagnosticKind,
        Discovery, DiscoveryAttempt, DiscoverySource, DiscoveryStage, InstalledDepot,
        MetadataReadErr, StateFlags, SteamInventory,
    },
};

const READ_LIMIT: u64 = MAX_STEAM_METADATA_BYTES + 1;

pub struct SteamScan {
    roots: Vec<SteamRoot>,
    libraries: Vec<LibraryRoot>,
    apps: Vec<AppInstall>,
    attempts: Vec<DiscoveryAttempt>,
    diagnostics: Vec<Diagnostic>,
}

impl SteamScan {
    pub fn automatic() -> Self {
        let mut collector = RootCollector::default();
        collector.automatic_roots();
        Self::from_root_scan(collector.finish())
    }

    pub fn explicit(candidates: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut collector = RootCollector::default();
        for candidate in candidates {
            collector.candidate(DiscoverySource::Explicit, candidate);
        }
        Self::from_root_scan(collector.finish())
    }

    fn from_root_scan(roots: RootScan) -> Self {
        let mut pipeline = ScanPipeline {
            diagnostics: roots.diagnostics,
        };
        let libraries = pipeline.libraries_from_roots(&roots.roots);
        let mut apps = Vec::new();
        for library in &libraries {
            apps.extend(pipeline.apps_from_library(library));
        }
        apps.sort_by(|left, right| {
            left.app_id().cmp(&right.app_id()).then_with(|| {
                left.manifest_path()
                    .path()
                    .cmp(right.manifest_path().path())
            })
        });

        Self {
            roots: roots.roots,
            libraries,
            apps,
            attempts: roots.attempts,
            diagnostics: pipeline.diagnostics,
        }
    }

    pub fn finish(self) -> Discovery {
        Discovery::from_parts(
            SteamInventory {
                steam_roots: self.roots,
                library_roots: self.libraries,
                apps: self.apps,
            },
            self.attempts,
            self.diagnostics,
        )
    }
}

/// Read one Steam metadata file, refusing anything that is not a regular
/// UTF-8 file within [`MAX_STEAM_METADATA_BYTES`].
pub fn read_metadata(path: &Path) -> Result<String, MetadataReadErr> {
    let metadata = std::fs::metadata(path).map_err(|source| MetadataReadErr::Inspect {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(MetadataReadErr::NotFile {
            path: path.to_path_buf(),
        });
    }
    if metadata.len() > MAX_STEAM_METADATA_BYTES {
        return Err(MetadataReadErr::TooLarge {
            path: path.to_path_buf(),
        });
    }

    let file = std::fs::File::open(path).map_err(|source| MetadataReadErr::Open {
        path: path.to_path_buf(),
        source,
    })?;
    // Capacity is only a hint; one extra byte may be read to detect a file
    // that grew after the metadata check.
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or_default());
    file.take(READ_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|source| MetadataReadErr::Read {
            path: path.to_path_buf(),
            source,
        })?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_STEAM_METADATA_BYTES) {
        return Err(MetadataReadErr::TooLarge {
            path: path.to_path_buf(),
        });
    }
    String::from_utf8(bytes).map_err(|error| MetadataReadErr::NotUtf8 {
        path: path.to_path_buf(),
        source: error.utf8_error(),
    })
}

/// Shared accumulator for the per-platform providers: every candidate,
/// rejection, and source failure lands here so the attempts ledger stays
/// non-lossy regardless of which platform produced it.
#[derive(Default)]
pub struct RootCollector {
    roots: Vec<SteamRoot>,
    attempts: Vec<DiscoveryAttempt>,
    diagnostics: Vec<Diagnostic>,
    seen: HashSet<PathKey>,
}

impl RootCollector {
    pub fn candidate(&mut self, source: DiscoverySource, candidate: PathBuf) {
        let rejection = match &source {
            DiscoverySource::Explicit => RejectionRecord::Diagnostic,
            DiscoverySource::Registry { .. }
            | DiscoverySource::Process { .. }
            | DiscoverySource::Environment { .. }
            | DiscoverySource::Conventional { .. } => RejectionRecord::AttemptDetail,
        };
        if !matches!(&source, DiscoverySource::Explicit) && !candidate.is_absolute() {
            self.attempts.push(DiscoveryAttempt {
                source,
                candidate: Some(candidate),
                outcome: AttemptOutcome::Rejected,
                evidence: Some(AttemptEvidence::Detail(
                    "automatic root candidates must be absolute so discovery is independent of the process directory"
                        .into(),
                )),
            });
            return;
        }
        match SteamRoot::from_dir(&candidate) {
            Ok(root) => {
                let key = PathKey::of(root.path());
                if self.seen.insert(key) {
                    self.attempts.push(DiscoveryAttempt {
                        source,
                        candidate: Some(candidate),
                        outcome: AttemptOutcome::Accepted,
                        evidence: None,
                    });
                    self.roots.push(root);
                } else {
                    self.attempts.push(DiscoveryAttempt {
                        source,
                        candidate: Some(candidate),
                        outcome: AttemptOutcome::Duplicate,
                        evidence: Some(AttemptEvidence::Detail(
                            "root was already accepted from another source".into(),
                        )),
                    });
                }
            }
            Err(error) => {
                let retain_diagnostic = matches!(rejection, RejectionRecord::Diagnostic)
                    || error.kind == ValidationErrKind::Io;
                let evidence = if retain_diagnostic {
                    let diagnostic_index = self.diagnostics.len();
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiscoveryStage::ValidateSteamRoot,
                            error.kind.into(),
                            error.to_string(),
                        )
                        .with_path(candidate.clone())
                        .with_source(source.clone()),
                    );
                    AttemptEvidence::DiagnosticIndex(diagnostic_index)
                } else {
                    AttemptEvidence::Detail(error.to_string())
                };
                self.attempts.push(DiscoveryAttempt {
                    source,
                    candidate: Some(candidate),
                    outcome: AttemptOutcome::Rejected,
                    evidence: Some(evidence),
                });
            }
        }
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub fn not_present(&mut self, source: DiscoverySource, detail: impl Into<String>) {
        self.attempts.push(DiscoveryAttempt {
            source,
            candidate: None,
            outcome: AttemptOutcome::NotPresent,
            evidence: Some(AttemptEvidence::Detail(detail.into())),
        });
    }

    #[cfg(any(windows, target_os = "linux"))]
    pub fn rejected_without_candidate(
        &mut self,
        source: DiscoverySource,
        detail: impl Into<String>,
    ) {
        self.attempts.push(DiscoveryAttempt {
            source,
            candidate: None,
            outcome: AttemptOutcome::Rejected,
            evidence: Some(AttemptEvidence::Detail(detail.into())),
        });
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    pub fn unsupported(&mut self, source: DiscoverySource, message: &str) {
        let diagnostic_index = self.diagnostics.len();
        self.diagnostics.push(
            Diagnostic::error(
                DiscoveryStage::LocateSteam,
                DiagnosticKind::Unsupported,
                message,
            )
            .with_source(source.clone()),
        );
        self.attempts.push(DiscoveryAttempt {
            source,
            candidate: None,
            outcome: AttemptOutcome::Unsupported,
            evidence: Some(AttemptEvidence::DiagnosticIndex(diagnostic_index)),
        });
    }

    #[cfg(windows)]
    pub fn failed(&mut self, source: DiscoverySource, detail: impl Into<String>) {
        let detail = detail.into();
        let diagnostic_index = self.diagnostics.len();
        self.diagnostics.push(
            Diagnostic::error(DiscoveryStage::LocateSteam, DiagnosticKind::Io, detail)
                .with_source(source.clone()),
        );
        self.attempts.push(DiscoveryAttempt {
            source,
            candidate: None,
            outcome: AttemptOutcome::Failed,
            evidence: Some(AttemptEvidence::DiagnosticIndex(diagnostic_index)),
        });
    }

    fn finish(self) -> RootScan {
        RootScan {
            roots: self.roots,
            attempts: self.attempts,
            diagnostics: self.diagnostics,
        }
    }
}

struct RootScan {
    roots: Vec<SteamRoot>,
    attempts: Vec<DiscoveryAttempt>,
    diagnostics: Vec<Diagnostic>,
}

/// How a candidate's validation failure is recorded.
#[derive(Clone, Copy)]
enum RejectionRecord {
    /// Escalate to an error-severity diagnostic: the candidate was supplied
    /// by the caller, so failing to validate it makes the result incomplete.
    Diagnostic,
    /// Note the failure as attempt detail only: the candidate was a
    /// speculative provider guess, and most missing or malformed guesses are
    /// expected. I/O failures still become diagnostics because they may hide
    /// a real root.
    AttemptDetail,
}

/// Library expansion and manifest loading over validated roots. Owns the
/// growing diagnostics list so every stage records degradation in one place;
/// mutation stays local to scan construction.
struct ScanPipeline {
    diagnostics: Vec<Diagnostic>,
}

impl ScanPipeline {
    fn libraries_from_roots(&mut self, steam_roots: &[SteamRoot]) -> Vec<LibraryRoot> {
        let mut libraries = Vec::new();
        for root in steam_roots {
            libraries.push(LibraryRoot(root.path().to_path_buf()));
            libraries.extend(self.library_folders(root));
        }

        let mut seen = HashSet::new();
        libraries.retain(|library| seen.insert(PathKey::of(library.path())));
        libraries
    }

    fn library_folders(&mut self, root: &SteamRoot) -> Vec<LibraryRoot> {
        let path = root
            .path()
            .join("steamapps")
            .join(SteamRoot::LIBRARY_FOLDERS_FILE);
        let path = match FilePresence::of(&path) {
            FilePresence::Missing => return Vec::new(),
            FilePresence::Present(path) => path,
            FilePresence::Invalid(error) => {
                let kind: DiagnosticKind = error.kind.into();
                self.diagnostics.push(
                    Diagnostic::error(DiscoveryStage::ReadLibraryFolders, kind, error.to_string())
                        .with_path(path),
                );
                return Vec::new();
            }
        };
        let text = match read_metadata(&path) {
            Ok(text) => text,
            Err(error) => {
                self.diagnostics
                    .push(error.into_diagnostic(DiscoveryStage::ReadLibraryFolders, None));
                return Vec::new();
            }
        };
        let parsed = match keyvalues_serde::parser::parse(&text) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiscoveryStage::ReadLibraryFolders,
                        DiagnosticKind::Parse,
                        format!(
                            "{}: could not parse Steam library metadata: {error}",
                            path.display()
                        ),
                    )
                    .with_path(path.clone()),
                );
                return Vec::new();
            }
        };
        if !parsed.key.eq_ignore_ascii_case("libraryfolders") {
            self.diagnostics.push(
                Diagnostic::error(
                    DiscoveryStage::ReadLibraryFolders,
                    DiagnosticKind::InvalidData,
                    format!(
                        "{}: top-level key is '{}', expected 'libraryfolders'",
                        path.display(),
                        parsed.key
                    ),
                )
                .with_path(path.clone()),
            );
            return Vec::new();
        }
        let Some(folders) = parsed.value.get_obj() else {
            self.diagnostics.push(
                Diagnostic::error(
                    DiscoveryStage::ReadLibraryFolders,
                    DiagnosticKind::InvalidData,
                    format!("{}: libraryfolders value is not an object", path.display()),
                )
                .with_path(path.clone()),
            );
            return Vec::new();
        };

        let mut libraries = Vec::new();
        for (slot, values) in folders.iter() {
            if slot.parse::<u32>().is_err() {
                continue;
            }
            let raw_path = values.first().and_then(|value| {
                value
                    .get_obj()
                    .and_then(|folder| folder.get("path"))
                    .and_then(|paths| paths.first())
                    .and_then(|path| path.get_str())
                    .or_else(|| value.get_str())
            });
            let Some(raw_path) = raw_path else {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiscoveryStage::ReadLibraryFolders,
                        DiagnosticKind::InvalidData,
                        format!(
                            "{}: library slot '{slot}' has no path string",
                            path.display()
                        ),
                    )
                    .with_path(path.clone()),
                );
                continue;
            };
            if let Some(library) = self.library_from_slot_path(slot, raw_path, &path) {
                libraries.push(library);
            }
        }
        libraries
    }

    fn library_from_slot_path(
        &mut self,
        slot: &str,
        raw_path: &str,
        vdf_path: &Path,
    ) -> Option<LibraryRoot> {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    DiscoveryStage::ReadLibraryFolders,
                    DiagnosticKind::InvalidData,
                    format!(
                        "{}: library slot '{slot}' has an empty path",
                        vdf_path.display()
                    ),
                )
                .with_path(vdf_path),
            );
            return None;
        }
        if !Path::new(trimmed).is_absolute() {
            self.diagnostics.push(
                Diagnostic::error(
                    DiscoveryStage::ValidateLibrary,
                    DiagnosticKind::InvalidData,
                    format!(
                        "{}: library slot '{slot}' path '{}' is not absolute",
                        vdf_path.display(),
                        trimmed
                    ),
                )
                .with_path(trimmed),
            );
            return None;
        }
        match LibraryRoot::from_dir(trimmed) {
            Ok(library) => Some(library),
            Err(error) => {
                let kind: DiagnosticKind = error.kind.into();
                self.diagnostics.push(
                    Diagnostic::error(DiscoveryStage::ValidateLibrary, kind, error.to_string())
                        .with_path(trimmed),
                );
                None
            }
        }
    }

    fn apps_from_library(&mut self, library: &LibraryRoot) -> Vec<AppInstall> {
        let steamapps = library.path().join("steamapps");
        let entries = match std::fs::read_dir(&steamapps) {
            Ok(entries) => entries,
            Err(error) => {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiscoveryStage::ReadManifest,
                        DiagnosticKind::Io,
                        format!(
                            "{}: could not enumerate app manifests: {error}",
                            steamapps.display()
                        ),
                    )
                    .with_path(steamapps),
                );
                return Vec::new();
            }
        };

        let mut manifests = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => {
                    if let Some(app_id) = ManifestPath::app_id_in_file_name(&entry.file_name()) {
                        manifests.push((app_id, entry.path()));
                    }
                }
                Err(error) => self.diagnostics.push(
                    Diagnostic::error(
                        DiscoveryStage::ReadManifest,
                        DiagnosticKind::Io,
                        format!(
                            "{}: could not inspect directory entry: {error}",
                            steamapps.display()
                        ),
                    )
                    .with_path(steamapps.clone()),
                ),
            }
        }
        manifests.sort_by(|left, right| left.1.cmp(&right.1));

        let mut apps = Vec::new();
        for (filename_app_id, manifest) in manifests {
            if let Some(app) = self.parse_manifest(library, filename_app_id, &manifest) {
                apps.push(app);
            }
        }
        apps
    }

    fn parse_manifest(
        &mut self,
        library: &LibraryRoot,
        filename_app_id: u32,
        manifest_path: &Path,
    ) -> Option<AppInstall> {
        let Some(app_id) = NonZeroU32::new(filename_app_id) else {
            self.diagnostics.push(
                Diagnostic::error(
                    DiscoveryStage::ReadManifest,
                    DiagnosticKind::InvalidData,
                    format!(
                        "{}: manifest file uses Steam's reserved invalid app ID 0",
                        manifest_path.display()
                    ),
                )
                .with_path(manifest_path),
            );
            return None;
        };
        let manifest_path = match FilePresence::of(manifest_path) {
            FilePresence::Missing => {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiscoveryStage::ReadManifest,
                        DiagnosticKind::NotFound,
                        format!(
                            "{}: manifest disappeared after directory enumeration",
                            manifest_path.display()
                        ),
                    )
                    .with_path(manifest_path)
                    .with_app_id(app_id),
                );
                return None;
            }
            FilePresence::Present(path) => ManifestPath(path),
            FilePresence::Invalid(error) => {
                let kind: DiagnosticKind = error.kind.into();
                self.diagnostics.push(
                    Diagnostic::error(DiscoveryStage::ReadManifest, kind, error.to_string())
                        .with_path(manifest_path)
                        .with_app_id(app_id),
                );
                return None;
            }
        };
        let text = match read_metadata(manifest_path.path()) {
            Ok(text) => text,
            Err(error) => {
                self.diagnostics
                    .push(error.into_diagnostic(DiscoveryStage::ReadManifest, Some(app_id)));
                return None;
            }
        };
        let manifest = self.manifest_identity(&text, &manifest_path, app_id)?;
        let (manifest_install_dir, install_dir) =
            self.resolve_install_dir(library, &manifest.installdir, &manifest_path, app_id)?;

        let metadata = self.app_metadata(&text, &manifest_path, app_id);
        Some(AppInstall::from_scanned(
            app_id,
            manifest.name.and_then(|name| {
                let trimmed = name.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }),
            manifest_install_dir,
            manifest_path,
            library.clone(),
            install_dir,
            metadata,
        ))
    }

    /// Strict identity parse: the manifest must be `AppState` and agree with
    /// the app ID in its filename.
    fn manifest_identity(
        &mut self,
        text: &str,
        manifest_path: &ManifestPath,
        app_id: NonZeroU32,
    ) -> Option<AppManifest> {
        let (manifest, root_key): (AppManifest, _) = match keyvalues_serde::from_str_with_key(text)
        {
            Ok(parsed) => parsed,
            Err(error) => {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiscoveryStage::ReadManifest,
                        DiagnosticKind::Parse,
                        format!(
                            "{}: could not parse app manifest for app {}: {error}",
                            manifest_path.path().display(),
                            app_id
                        ),
                    )
                    .with_path(manifest_path.path())
                    .with_app_id(app_id),
                );
                return None;
            }
        };
        if !root_key.eq_ignore_ascii_case("AppState") {
            self.diagnostics.push(
                Diagnostic::error(
                    DiscoveryStage::ReadManifest,
                    DiagnosticKind::InvalidData,
                    format!(
                        "{}: top-level key is '{root_key}', expected 'AppState'",
                        manifest_path.path().display()
                    ),
                )
                .with_path(manifest_path.path())
                .with_app_id(app_id),
            );
            return None;
        }
        if manifest.appid != app_id.get() {
            self.diagnostics.push(
                Diagnostic::error(
                    DiscoveryStage::ReadManifest,
                    DiagnosticKind::InvalidData,
                    format!(
                        "{}: manifest app ID is {}, expected {} from its filename",
                        manifest_path.path().display(),
                        manifest.appid,
                        app_id
                    ),
                )
                .with_path(manifest_path.path())
                .with_app_id(app_id),
            );
            return None;
        }
        Some(manifest)
    }

    /// Resolve and confine a manifest's `installdir` under the library's
    /// `steamapps/common`, rejecting lexical and symlink escapes separately.
    /// Returns the trimmed declared directory and the canonical installation
    /// directory.
    fn resolve_install_dir(
        &mut self,
        library: &LibraryRoot,
        manifest_install_dir: &str,
        manifest_path: &ManifestPath,
        app_id: NonZeroU32,
    ) -> Option<(String, InstallDir)> {
        let declared = manifest_install_dir.trim();
        if declared.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    DiscoveryStage::ValidateInstall,
                    DiagnosticKind::InvalidData,
                    format!(
                        "{}: manifest has an empty installdir",
                        manifest_path.path().display()
                    ),
                )
                .with_path(manifest_path.path())
                .with_app_id(app_id),
            );
            return None;
        }
        let raw_install = Path::new(declared);
        if raw_install.is_absolute()
            || raw_install.components().any(|component| {
                matches!(
                    component,
                    Component::Prefix(_)
                        | Component::RootDir
                        | Component::CurDir
                        | Component::ParentDir
                )
            })
        {
            self.reject_install_dir_escape(manifest_path, app_id, manifest_install_dir);
            return None;
        }

        let common = library.path().join("steamapps").join("common");
        let normalized_common = common.normalize();
        let candidate = common.join(raw_install).normalize();
        if !candidate.starts_with(&normalized_common) {
            self.reject_install_dir_escape(manifest_path, app_id, manifest_install_dir);
            return None;
        }
        let canonical_common = match canonical_directory(&common, "Steam common directory") {
            Ok(path) => path,
            Err(error) => {
                let kind: DiagnosticKind = error.kind.into();
                self.diagnostics.push(
                    Diagnostic::error(DiscoveryStage::ValidateInstall, kind, error.to_string())
                        .with_path(common)
                        .with_app_id(app_id),
                );
                return None;
            }
        };
        let resolved = match canonical_directory(&candidate, "Resolved app installation directory")
        {
            Ok(path) => path,
            Err(error) => {
                let kind: DiagnosticKind = error.kind.into();
                self.diagnostics.push(
                    Diagnostic::error(
                        DiscoveryStage::ValidateInstall,
                        kind,
                        format!(
                            "{}: resolved installation for app {} is unusable: {error}",
                            manifest_path.path().display(),
                            app_id
                        ),
                    )
                    .with_path(candidate)
                    .with_app_id(app_id),
                );
                return None;
            }
        };
        if !resolved.starts_with(&canonical_common) {
            self.diagnostics.push(
                Diagnostic::warning(
                    DiscoveryStage::ValidateInstall,
                    DiagnosticKind::OutsideBoundary,
                    format!(
                        "{}: app {} installation resolves outside '{}' to '{}'; accepted as a junction or symlinked install",
                        manifest_path.path().display(),
                        app_id,
                        canonical_common.display(),
                        resolved.display(),
                    ),
                )
                .with_path(candidate)
                .with_app_id(app_id),
            );
        }
        Some((declared.to_string(), InstallDir(resolved)))
    }

    fn reject_install_dir_escape(
        &mut self,
        manifest_path: &ManifestPath,
        app_id: NonZeroU32,
        manifest_install_dir: &str,
    ) {
        self.diagnostics.push(
            Diagnostic::error(
                DiscoveryStage::ValidateInstall,
                DiagnosticKind::OutsideBoundary,
                format!(
                    "{}: app {} installdir '{}' escapes steamapps/common",
                    manifest_path.path().display(),
                    app_id,
                    manifest_install_dir
                ),
            )
            .with_path(manifest_path.path())
            .with_app_id(app_id),
        );
    }

    fn app_metadata(
        &mut self,
        text: &str,
        manifest_path: &ManifestPath,
        app_id: NonZeroU32,
    ) -> AppMetadata {
        let raw = match keyvalues_serde::from_str_with_key::<RawMetadata>(text) {
            Ok((raw, _)) => raw,
            Err(error) => {
                self.diagnostics.push(
                    Diagnostic::warning(
                        DiscoveryStage::ReadManifest,
                        DiagnosticKind::Parse,
                        format!(
                            "{}: optional manifest metadata was skipped because it could not be \
                             parsed: {error}",
                            manifest_path.path().display()
                        ),
                    )
                    .with_path(manifest_path.path())
                    .with_app_id(app_id),
                );
                return AppMetadata::default();
            }
        };

        let mut installed_depots = BTreeMap::new();
        let mut ambiguous_depot_ids = HashSet::new();
        for (raw_depot_id, depot) in raw.installed_depots {
            let Ok(depot_id) = raw_depot_id.trim().parse::<NonZeroU64>() else {
                continue;
            };
            if ambiguous_depot_ids.contains(&depot_id) {
                continue;
            }
            let parsed = InstalledDepot {
                manifest: lenient_number(depot.manifest.as_ref()),
                size: lenient_number(depot.size.as_ref()),
                dlc_app_id: lenient_number(depot.dlcappid.as_ref()),
            };
            if installed_depots.insert(depot_id, parsed).is_some() {
                installed_depots.remove(&depot_id);
                ambiguous_depot_ids.insert(depot_id);
                self.diagnostics.push(
                    Diagnostic::warning(
                        DiscoveryStage::ReadManifest,
                        DiagnosticKind::InvalidData,
                        format!(
                            "{}: multiple installed-depot keys normalize to ID {depot_id}; omitted ambiguous advisory metadata",
                            manifest_path.path().display()
                        ),
                    )
                    .with_path(manifest_path.path())
                    .with_app_id(app_id),
                );
            }
        }
        debug_assert!(
            ambiguous_depot_ids
                .iter()
                .all(|depot_id| !installed_depots.contains_key(depot_id)),
            "ambiguous numeric depot IDs must not survive advisory parsing"
        );

        AppMetadata {
            state_flags: lenient_number(raw.state_flags.as_ref()).map(StateFlags::from_raw),
            build_id: lenient_number(raw.buildid.as_ref()),
            size_on_disk: lenient_number(raw.size_on_disk.as_ref()),
            last_updated: lenient_number(raw.last_updated.as_ref()),
            beta_key: raw.user_config.and_then(|config| {
                [config.beta_key, config.betakey]
                    .into_iter()
                    .flatten()
                    .find_map(|key| {
                        let trimmed = key.trim();
                        (!trimmed.is_empty()).then(|| trimmed.to_string())
                    })
            }),
            installed_depots,
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct AppManifest {
    appid: u32,
    installdir: String,
    #[serde(default)]
    name: Option<String>,
}

/// Advisory manifest fields, read in a second pass so a malformed value can
/// never cost the app its place in the inventory. Every field deserializes
/// as a string and converts leniently; failures become `None` rather than
/// errors. `SteamContentSignal` remains the strict reader of the same data.
#[derive(Debug, Default, serde::Deserialize)]
struct RawMetadata {
    #[serde(default, rename = "StateFlags")]
    state_flags: Option<String>,
    #[serde(default)]
    buildid: Option<String>,
    #[serde(default, rename = "SizeOnDisk")]
    size_on_disk: Option<String>,
    #[serde(default, rename = "LastUpdated")]
    last_updated: Option<String>,
    #[serde(default, rename = "InstalledDepots")]
    installed_depots: BTreeMap<String, RawDepot>,
    #[serde(default, rename = "UserConfig")]
    user_config: Option<RawUserConfig>,
}

#[derive(Debug, serde::Deserialize)]
struct RawDepot {
    #[serde(default)]
    manifest: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    dlcappid: Option<String>,
}

/// Steam has written the beta-branch key with both casings.
#[derive(Debug, Default, serde::Deserialize)]
struct RawUserConfig {
    #[serde(default, rename = "BetaKey")]
    beta_key: Option<String>,
    #[serde(default)]
    betakey: Option<String>,
}

fn lenient_number<T: std::str::FromStr>(value: Option<&String>) -> Option<T> {
    value.and_then(|value| value.trim().parse().ok())
}

/// Deduplication identity for a canonical root or library path.
#[cfg(any(windows, target_os = "macos"))]
#[derive(Eq, Hash, PartialEq)]
struct PathKey(String);

/// Deduplication identity for a canonical root or library path.
#[cfg(not(any(windows, target_os = "macos")))]
#[derive(Eq, Hash, PartialEq)]
struct PathKey(PathBuf);

impl PathKey {
    /// NTFS and default APFS are case-insensitive, so deduplication folds case
    /// on Windows and macOS. On a case-sensitive APFS volume two roots that
    /// differ only by case would merge; the attempts ledger still records both.
    #[cfg(any(windows, target_os = "macos"))]
    fn of(path: &Path) -> Self {
        Self(
            path.components()
                .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join("\0"),
        )
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn of(path: &Path) -> Self {
        Self(path.to_path_buf())
    }
}
