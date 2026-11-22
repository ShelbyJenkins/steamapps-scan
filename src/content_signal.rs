//! steamapps-scan - Content signal
//! =================================
//!
//! Canonical build and depot identity reread from one discovered Steam app
//! manifest. The signal is independent of any particular game or app ID.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use super::report::{AppInstall, Diagnostic, DiscoveryStage, StateFlags};
use crate::internal::scan::read_metadata;

const SCHEME: &str = "steam-app-manifest-v1";

/// Canonical identity for the currently installed Steam app content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SteamContentSignal {
    value: String,
}

/// Failure to reread or validate an app manifest for a content signal.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ContentSignalErr {
    /// The bounded metadata read failed.
    #[error(transparent)]
    Metadata(#[from] Box<Diagnostic>),
    /// Valve `KeyValues` content could not be parsed.
    #[error("{}: could not parse app {app_id} content manifest: {message}", path.display())]
    Parse {
        /// Manifest path.
        path: PathBuf,
        /// Expected Steam app ID.
        app_id: u32,
        /// Parser detail.
        message: String,
    },
    /// Parsed manifest data violated a signal invariant.
    #[error("{}: app {app_id} content manifest is invalid: {message}", path.display())]
    Invalid {
        /// Manifest path.
        path: PathBuf,
        /// Expected Steam app ID.
        app_id: u32,
        /// Validation detail.
        message: String,
    },
}

#[derive(Debug, Deserialize)]
struct ContentManifest {
    appid: u32,
    buildid: u64,
    #[serde(rename = "StateFlags")]
    state_flags: u64,
    #[serde(rename = "InstalledDepots", default)]
    installed_depots: BTreeMap<String, InstalledDepot>,
}

#[derive(Debug, Deserialize)]
struct InstalledDepot {
    manifest: u64,
}

impl AppInstall {
    /// Reread this app's manifest and derive its canonical content signal.
    ///
    /// The manifest must still identify this app and report Steam's fully
    /// installed state. Workshop and locally modified content are outside the
    /// signal and require caller-supplied identity when relevant.
    pub fn content_signal(&self) -> Result<SteamContentSignal, ContentSignalErr> {
        SteamContentSignal::read(self)
    }
}

impl SteamContentSignal {
    /// Stable signal scheme.
    #[must_use]
    pub const fn scheme(&self) -> &'static str {
        SCHEME
    }

    /// Canonical app, build, and installed-depot value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    fn read(app: &AppInstall) -> Result<Self, ContentSignalErr> {
        let path = app.manifest_path().path();
        let text = read_metadata(path, DiscoveryStage::ReadManifest, Some(app.app_id()))?;
        Self::parse(app.app_id(), path, &text)
    }

    fn parse(expected_app_id: u32, path: &Path, text: &str) -> Result<Self, ContentSignalErr> {
        let (manifest, root_key): (ContentManifest, _) =
            keyvalues_serde::from_str_with_key(text).map_err(|source| ContentSignalErr::Parse {
                path: path.to_path_buf(),
                app_id: expected_app_id,
                message: source.to_string(),
            })?;
        if !root_key.eq_ignore_ascii_case("AppState") {
            return Err(ContentSignalErr::invalid(
                path,
                expected_app_id,
                format!("top-level key is '{root_key}', expected 'AppState'"),
            ));
        }
        if manifest.appid != expected_app_id {
            return Err(ContentSignalErr::invalid(
                path,
                expected_app_id,
                format!(
                    "manifest app ID is '{}', expected '{expected_app_id}'",
                    manifest.appid
                ),
            ));
        }
        if manifest.state_flags != StateFlags::FULLY_INSTALLED {
            return Err(ContentSignalErr::invalid(
                path,
                expected_app_id,
                format!(
                    "state flags are '{}', expected fully installed state '{}'; Steam may be updating, validating, or transitioning the install",
                    manifest.state_flags,
                    StateFlags::FULLY_INSTALLED
                ),
            ));
        }
        if manifest.buildid == 0 {
            return Err(ContentSignalErr::invalid(
                path,
                expected_app_id,
                "build ID is zero",
            ));
        }

        let mut depots = BTreeMap::new();
        for (raw_depot_id, depot) in manifest.installed_depots {
            let depot_id = raw_depot_id.parse::<u64>().map_err(|_| {
                ContentSignalErr::invalid(
                    path,
                    expected_app_id,
                    format!("installed depot ID '{raw_depot_id}' is not an unsigned integer"),
                )
            })?;
            if depot_id == 0 || depot.manifest == 0 {
                return Err(ContentSignalErr::invalid(
                    path,
                    expected_app_id,
                    format!("installed depot '{raw_depot_id}' has a zero depot or manifest ID"),
                ));
            }
            if depots.insert(depot_id, depot.manifest).is_some() {
                return Err(ContentSignalErr::invalid(
                    path,
                    expected_app_id,
                    format!(
                        "installed depot ID '{raw_depot_id}' duplicates another numeric depot ID"
                    ),
                ));
            }
        }

        let mut value = format!(
            "appid={expected_app_id};buildid={};depots=",
            manifest.buildid
        );
        for (index, (depot_id, manifest_id)) in depots.into_iter().enumerate() {
            if index != 0 {
                value.push(',');
            }
            let formatted = write!(value, "{depot_id}:{manifest_id}");
            debug_assert!(
                formatted.is_ok(),
                "formatting an installed depot into a String must be infallible"
            );
        }
        Ok(Self { value })
    }
}

impl ContentSignalErr {
    fn invalid(path: &Path, app_id: u32, message: impl Into<String>) -> Self {
        Self::Invalid {
            path: path.to_path_buf(),
            app_id,
            message: message.into(),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test setup and assertions stop immediately with fixture-specific context"
)]
mod tests {
    use std::{
        fs,
        num::NonZeroU32,
        path::PathBuf,
        sync::atomic::{AtomicU32, Ordering},
    };

    use super::{super::paths::LibraryRoot, *};

    const APP_ID: u32 = 42;

    fn manifest(installed_depots: &str) -> String {
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\"{APP_ID}\"\n\t\"StateFlags\"\t\"4\"\n\t\"buildid\"\t\"12345678\"\n\t\"LastUpdated\"\t\"999\"\n\t\"BytesToDownload\"\t\"123\"\n\t\"InstalledDepots\"\n\t{{\n{installed_depots}\t}}\n}}\n"
        )
    }

    fn fixture_path(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);

        std::env::temp_dir().join(format!(
            "steam-content-signal-{label}-{}-{}.acf",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn canonical_value_sorts_numeric_depot_ids_and_ignores_progress() {
        let text = manifest(
            "\t\t\"20\"\n\t\t{\n\t\t\t\"manifest\"\t\"900\"\n\t\t}\n\t\t\"3\"\n\t\t{\n\t\t\t\"manifest\"\t\"800\"\n\t\t}\n",
        );
        let signal = SteamContentSignal::parse(APP_ID, Path::new("fixture.acf"), &text)
            .expect("parse complete manifest");

        assert_eq!(signal.scheme(), "steam-app-manifest-v1");
        assert_eq!(
            signal.value(),
            "appid=42;buildid=12345678;depots=3:800,20:900"
        );
        assert!(!signal.value().contains("999"));
        assert!(!signal.value().contains("Bytes"));
    }

    #[test]
    fn no_depots_has_an_empty_canonical_suffix() {
        let signal = SteamContentSignal::parse(APP_ID, Path::new("fixture.acf"), &manifest(""))
            .expect("parse no-depots manifest");

        assert_eq!(signal.value(), "appid=42;buildid=12345678;depots=");
    }

    #[test]
    fn transitional_state_is_rejected() {
        let text = manifest("").replace("\"StateFlags\"\t\"4\"", "\"StateFlags\"\t\"1026\"");
        let error = SteamContentSignal::parse(APP_ID, Path::new("fixture.acf"), &text)
            .expect_err("transitional install must fail");

        assert!(error.to_string().contains("transitioning"));
    }

    #[test]
    fn app_install_reads_a_bounded_fixture_file() {
        let path = fixture_path("read");
        fs::write(&path, manifest("")).expect("write content manifest fixture");
        let root = path.parent().expect("fixture parent").to_path_buf();
        let app = AppInstall::from_scanned(
            NonZeroU32::new(APP_ID).expect("fixture app ID is nonzero"),
            None,
            "fixture".into(),
            super::super::paths::ManifestPath(path.clone()),
            LibraryRoot(root.clone()),
            super::super::paths::InstallDir(root),
            super::super::report::AppMetadata::default(),
        );

        let signal = app.content_signal().expect("read content signal");
        assert_eq!(signal.value(), "appid=42;buildid=12345678;depots=");

        fs::remove_file(path).expect("remove content manifest fixture");
    }

    #[test]
    fn duplicate_numeric_depot_ids_are_rejected() {
        let text = manifest(
            "\t\t\"3\"\n\t\t{\n\t\t\t\"manifest\"\t\"800\"\n\t\t}\n\t\t\"03\"\n\t\t{\n\t\t\t\"manifest\"\t\"900\"\n\t\t}\n",
        );
        let error = SteamContentSignal::parse(APP_ID, Path::new("fixture.acf"), &text)
            .expect_err("numeric duplicate must fail");

        assert!(error.to_string().contains("duplicates"));
    }
}
