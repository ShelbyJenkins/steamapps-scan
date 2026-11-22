//! steamapps-scan - Live installation tests
//! =========================================
//!
//! Checks against this machine's own Steam installation, ignored by default
//! because they depend on what is installed. Every check compares the report
//! with what the filesystem shows directly, so it holds whatever games are
//! installed, and each fails when no Steam installation is found.

use super::*;

/// This machine's Steam installation, found by automatic discovery.
struct LiveSteam(Discovery);

impl LiveSteam {
    /// Directory levels walked below an installation for sample files.
    const WALK_DEPTH: usize = 3;

    fn discover() -> Self {
        let discovery = Discovery::discover();
        assert!(
            !discovery.inventory().steam_roots().is_empty(),
            "no Steam installation was found: {:#?}",
            discovery.attempts()
        );
        Self(discovery)
    }

    /// Whether `source` is the one the Steam client itself keeps current on
    /// this platform: the registry on Windows, the `~/.steam` pointers on
    /// Linux, and the one data location on macOS.
    #[cfg(windows)]
    const fn client_maintained(source: &DiscoverySource) -> bool {
        matches!(source, DiscoverySource::Registry { .. })
    }

    #[cfg(target_os = "linux")]
    fn client_maintained(source: &DiscoverySource) -> bool {
        matches!(source, DiscoverySource::Conventional { description } if description.contains(".steam/"))
    }

    #[cfg(target_os = "macos")]
    fn client_maintained(source: &DiscoverySource) -> bool {
        matches!(source, DiscoverySource::Conventional { description } if description == "~/Library/Application Support/Steam")
    }

    /// Up to `limit` regular files in `install`, walked breadth first to
    /// [`Self::WALK_DEPTH`] levels, that `wanted` accepts. Links are not
    /// followed, so every file lies inside the installation.
    #[expect(
        clippy::panic,
        reason = "an installation the walk cannot read must name the path it failed on"
    )]
    fn files_in(install: &Path, limit: usize, wanted: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
        let unreadable = |path: &Path, error: std::io::Error| -> ! {
            panic!("{} could not be read: {error}", path.display())
        };
        let mut files = Vec::new();
        let mut level = vec![install.to_path_buf()];
        for _ in 0..Self::WALK_DEPTH {
            let mut next = Vec::new();
            for directory in level {
                let entries = std::fs::read_dir(&directory)
                    .unwrap_or_else(|error| unreadable(&directory, error));
                for entry in entries {
                    let entry = entry.unwrap_or_else(|error| unreadable(&directory, error));
                    let file_type = entry
                        .file_type()
                        .unwrap_or_else(|error| unreadable(&entry.path(), error));
                    if file_type.is_dir() {
                        next.push(entry.path());
                    } else if file_type.is_file() && wanted(&entry.path()) {
                        files.push(entry.path());
                        if files.len() == limit {
                            return files;
                        }
                    }
                }
            }
            level = next;
        }
        files
    }

    /// The installation's own match for `file`, which must lie inside it.
    fn resolve<'a>(report: &'a MatchReport, app: &AppInstall) -> Option<&'a ExecutableMatch> {
        report.matches().iter().find(|found| found.app() == app)
    }
}

#[test]
#[ignore = "reads this machine's Steam installation"]
fn discovery_is_complete_and_every_accepted_candidate_is_a_root() {
    let LiveSteam(discovery) = LiveSteam::discover();

    assert_eq!(
        discovery.state(),
        DiscoveryState::Complete,
        "{:#?}",
        discovery.diagnostics()
    );
    let roots: Vec<&Path> = discovery
        .inventory()
        .steam_roots()
        .iter()
        .map(SteamRoot::path)
        .collect();
    let found_root = |attempt: &&DiscoveryAttempt| {
        matches!(
            attempt.outcome,
            AttemptOutcome::Accepted | AttemptOutcome::Duplicate
        )
    };
    for attempt in discovery.attempts().iter().filter(found_root) {
        let candidate = attempt
            .candidate
            .as_deref()
            .expect("an attempt that found a root names its candidate");
        let resolved = dunce::canonicalize(candidate).expect("the candidate still resolves");
        assert!(
            roots.contains(&resolved.as_path()),
            "{} resolves to {}, which is none of the accepted roots {roots:?}",
            candidate.display(),
            resolved.display()
        );
    }
    assert!(
        discovery
            .attempts()
            .iter()
            .filter(found_root)
            .any(|attempt| LiveSteam::client_maintained(&attempt.source)),
        "the location the Steam client maintains led to no root: {:#?}",
        discovery.attempts()
    );
}

#[test]
#[ignore = "reads this machine's Steam installation"]
fn every_manifest_on_disk_is_inventoried() {
    let LiveSteam(discovery) = LiveSteam::discover();

    let mut on_disk = Vec::new();
    for library in discovery.inventory().libraries() {
        let steamapps = library.path().join("steamapps");
        for entry in std::fs::read_dir(&steamapps).expect("read a library's steamapps directory") {
            let entry = entry.expect("read a steamapps entry");
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("appmanifest_") && name.ends_with(".acf") {
                on_disk.push(dunce::canonicalize(entry.path()).expect("resolve a manifest"));
            }
        }
    }
    let mut inventoried: Vec<PathBuf> = discovery
        .inventory()
        .apps()
        .iter()
        .map(|app| app.manifest_path().path().to_path_buf())
        .collect();
    on_disk.sort();
    inventoried.sort();

    assert_eq!(on_disk, inventoried, "{:#?}", discovery.diagnostics());
}

#[test]
#[ignore = "reads this machine's Steam installation"]
fn sampled_installation_files_resolve_to_themselves() {
    let LiveSteam(discovery) = LiveSteam::discover();

    let mut failures = Vec::new();
    let mut sampled = Vec::new();
    for app in discovery.inventory().apps() {
        for file in LiveSteam::files_in(app.path(), 4, |_| true) {
            let relative = file
                .strip_prefix(app.path())
                .expect("walked files lie inside the installation");
            let query = ExecutableQuery::new(app.app_id().get(), relative)
                .expect("a walked relative path is a valid candidate");
            let report = discovery.resolve_executable(&query);
            let expected = dunce::canonicalize(&file).expect("resolve a sampled file");
            let found = LiveSteam::resolve(&report, app).map(|found| found.executable().path());
            if found != Some(expected.as_path()) {
                failures.push(format!(
                    "{} resolved to {found:?}: {:#?}",
                    file.display(),
                    report.diagnostics().collect::<Vec<_>>()
                ));
            }
            sampled.push(file);
        }
    }

    assert!(
        !sampled.is_empty(),
        "no installed files were found to sample"
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
#[ignore = "reads this machine's Steam installation"]
fn windows_binaries_read_their_version_resources() {
    let LiveSteam(discovery) = LiveSteam::discover();
    let named_like_pe = |path: &Path| {
        path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("dll")
        })
    };

    let mut outcomes = Vec::new();
    for app in discovery.inventory().apps() {
        for file in LiveSteam::files_in(app.path(), 25, named_like_pe) {
            let relative = file
                .strip_prefix(app.path())
                .expect("walked files lie inside the installation");
            let report = discovery.resolve_executable(
                &ExecutableQuery::new(app.app_id().get(), relative)
                    .expect("a walked relative path is a valid candidate"),
            );
            let found = LiveSteam::resolve(&report, app).expect("a sampled binary resolves");
            outcomes.push(found.executable().version());
        }
    }

    let failures: Vec<_> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().err())
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
    #[cfg(windows)]
    assert!(
        outcomes
            .iter()
            .any(|outcome| matches!(outcome, Ok(Some(_)))),
        "no installed Windows binary carried a version resource"
    );
}

#[test]
#[ignore = "reads this machine's Steam installation"]
fn fully_installed_apps_yield_content_signals() {
    let LiveSteam(discovery) = LiveSteam::discover();

    let settled: Vec<&AppInstall> = discovery
        .inventory()
        .apps()
        .iter()
        .filter(|app| app.metadata().state_flags == Some(StateFlags::from_raw(4)))
        .collect();
    assert!(
        !settled.is_empty(),
        "no app is in the fully installed state"
    );
    let mut failures = Vec::new();
    for app in settled {
        let expected_prefix = format!(
            "appid={};buildid={};",
            app.app_id(),
            app.metadata().build_id.map_or(0, std::num::NonZeroU64::get)
        );
        match app.content_signal() {
            Ok(signal) if signal.value().starts_with(&expected_prefix) => {}
            Ok(signal) => failures.push(format!(
                "app {}: signal '{}' disagrees with the advisory metadata '{expected_prefix}'",
                app.app_id(),
                signal.value()
            )),
            Err(error) => failures.push(error.to_string()),
        }
    }

    assert!(failures.is_empty(), "{failures:#?}");
}

#[cfg(feature = "cli")]
#[test]
#[ignore = "reads this machine's Steam installation"]
fn cli_reports_the_same_discovery_as_the_library() {
    let LiveSteam(discovery) = LiveSteam::discover();

    let scan = CliScan::run(|command| command);

    assert_eq!(scan.code, Some(0), "{:#}", scan.report);
    assert_eq!(
        scan.report,
        serde_json::to_value(&discovery).expect("serialize the library's report")
    );
}
