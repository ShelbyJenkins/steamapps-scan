#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "integration fixtures and assertions intentionally fail fast with local context"
)]

use std::{
    num::{NonZeroU32, NonZeroU64},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

#[cfg(windows)]
use steamapps_scan::DiscoverySource;
use steamapps_scan::{
    AppMetadata, AttemptOutcome, DiagnosticKind, DiagnosticSeverity, Discovery, DiscoveryStage,
    DiscoveryState, ExecutableQuery, ExecutableTarget, MAX_STEAM_METADATA_BYTES, MatchCardinality,
    MatchCompleteness, MatchErr, QueryErr, StateFlags,
};

const fn fixture_app_id(raw: u32) -> NonZeroU32 {
    NonZeroU32::new(raw).expect("fixture app ID is nonzero")
}

struct TempTree(PathBuf);

impl TempTree {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);

        let path = std::env::temp_dir().join(format!(
            "steamapps-scan-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).expect("create temporary tree");
        Self(dunce::canonicalize(path).expect("canonicalize temporary tree"))
    }

    fn root(&self) -> &Path {
        &self.0
    }

    fn dir(&self, relative: impl AsRef<Path>) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(&path).expect("create test directory");
        path
    }

    fn file(&self, relative: impl AsRef<Path>, contents: &str) -> PathBuf {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create test file parent");
        }
        std::fs::write(&path, contents).expect("write test file");
        path
    }

    fn manifest(
        _filename_app_id: u32,
        manifest_app_id: u32,
        installdir: &str,
        name: &str,
    ) -> String {
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\"{manifest_app_id}\"\n\t\"name\"\t\"{name}\"\n\t\"installdir\"\t\"{installdir}\"\n}}\n",
        )
    }

    fn install_app(&self, app_id: u32, installdir: &str) -> PathBuf {
        self.file(
            format!("steamapps/appmanifest_{app_id}.acf"),
            &Self::manifest(app_id, app_id, installdir, &format!("Test App {app_id}")),
        );
        self.dir(format!("steamapps/common/{installdir}"))
    }

    fn install_executable(&self, installdir: &str, relative: &str) -> PathBuf {
        self.file(format!("steamapps/common/{installdir}/{relative}"), "")
    }

    fn discover(&self) -> Discovery {
        Discovery::discover_from([self.root()])
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn discovers_an_arbitrary_app_without_libraryfolders() {
    const APP_ID: u32 = 480;
    let steam = TempTree::new("arbitrary-app");
    let install = steam.install_app(APP_ID, "Spacewar");

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Complete);
    assert!(discovery.diagnostics().is_empty());
    assert_eq!(discovery.inventory().steam_roots().len(), 1);
    assert_eq!(discovery.inventory().libraries().len(), 1);
    let app = discovery
        .inventory()
        .apps_for(fixture_app_id(APP_ID))
        .next()
        .expect("arbitrary app should be inventoried");
    assert_eq!(app.path(), dunce::canonicalize(install).unwrap());
    assert_eq!(app.name(), Some("Test App 480"));

    let json = serde_json::to_value(&discovery).expect("serialize report");
    assert_eq!(json["state"], "complete");
    assert_eq!(json["inventory"]["apps"][0]["app_id"], APP_ID);
}

#[test]
fn same_app_id_resolves_different_executables_independently() {
    const APP_ID: u32 = 42;
    let steam = TempTree::new("same-id");
    steam.install_app(APP_ID, "SharedApp");
    let game = steam.install_executable("SharedApp", "Game.exe");
    let server = steam.install_executable("SharedApp", "DedicatedServer.exe");
    let discovery = steam.discover();

    let game_query = ExecutableQuery::new(APP_ID, "Game.exe").unwrap();
    let server_query = ExecutableQuery::new(APP_ID, "DedicatedServer.exe").unwrap();
    let reports = discovery.resolve_all([&game_query, &server_query]);

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].cardinality(), MatchCardinality::One);
    assert_eq!(reports[1].cardinality(), MatchCardinality::One);
    assert_eq!(
        reports[0].unique().unwrap().executable().path(),
        dunce::canonicalize(game).unwrap()
    );
    assert_eq!(
        reports[1].unique().unwrap().executable().path(),
        dunce::canonicalize(server).unwrap()
    );
    assert_eq!(
        reports[0].unique().unwrap().app(),
        reports[1].unique().unwrap().app()
    );
}

#[test]
fn nested_candidates_are_exact_and_ordered() {
    const APP_ID: u32 = 43;
    let steam = TempTree::new("ordered");
    steam.install_app(APP_ID, "Tools");
    let preferred = steam.install_executable("Tools", "Workbench/Preferred.exe");
    steam.install_executable("Tools", "Fallback.exe");
    let discovery = steam.discover();
    let query =
        ExecutableQuery::any_of(APP_ID, ["Workbench/Preferred.exe", "Fallback.exe"]).unwrap();

    let report = discovery.resolve_executable(&query);

    assert_eq!(report.cardinality(), MatchCardinality::One);
    let matched = report.unique().unwrap();
    assert_eq!(
        matched.executable().path(),
        dunce::canonicalize(preferred).unwrap()
    );
    assert_eq!(
        matched.matched_candidate().path(),
        Path::new("Workbench/Preferred.exe")
    );
}

#[test]
fn a_missing_preferred_candidate_uses_the_next_fallback() {
    const APP_ID: u32 = 1_874_911;
    let steam = TempTree::new("fallback");
    steam.install_app(APP_ID, "FallbackTools");
    let fallback = steam.install_executable("FallbackTools", "fallback.exe");
    let discovery = steam.discover();
    let query = ExecutableQuery::any_of(APP_ID, ["missing.exe", "fallback.exe"]).unwrap();

    let report = discovery.resolve_executable(&query);

    let matched = report.unique().unwrap();
    assert_eq!(
        matched.executable().path(),
        dunce::canonicalize(fallback).unwrap()
    );
    assert_eq!(
        matched.matched_candidate().path(),
        Path::new("fallback.exe")
    );
}

#[test]
fn marker_targets_can_share_an_app_id() {
    const SHARED_APP_ID: u32 = 42;

    struct Game;
    impl ExecutableTarget for Game {
        const APP_ID: NonZeroU32 = NonZeroU32::new(SHARED_APP_ID).unwrap();
        const EXECUTABLES: &'static [&'static str] = &["game.exe"];
    }

    struct Server;
    impl ExecutableTarget for Server {
        const APP_ID: NonZeroU32 = NonZeroU32::new(SHARED_APP_ID).unwrap();
        const EXECUTABLES: &'static [&'static str] = &["server.exe"];
    }

    let game = ExecutableQuery::for_target::<Game>().unwrap();
    let server = ExecutableQuery::for_target::<Server>().unwrap();
    assert_eq!(game.app_id(), server.app_id());
    assert!(!game.candidates().eq(server.candidates()));
}

#[test]
fn duplicate_app_installs_are_reported_as_ambiguous() {
    const APP_ID: u32 = 7;
    let first = TempTree::new("duplicate-first");
    let second = TempTree::new("duplicate-second");
    first.install_app(APP_ID, "SameApp");
    second.install_app(APP_ID, "SameApp");
    first.install_executable("SameApp", "app.exe");
    second.install_executable("SameApp", "app.exe");
    let discovery = Discovery::discover_from([first.root(), second.root()]);
    let query = ExecutableQuery::new(APP_ID, "app.exe").unwrap();

    let report = discovery.resolve_executable(&query);

    assert_eq!(report.cardinality(), MatchCardinality::Many);
    assert_eq!(report.matches().len(), 2);
    assert_eq!(
        report.unique(),
        Err(MatchErr::Ambiguous {
            app_id: fixture_app_id(APP_ID),
            matches: 2,
        })
    );
}

#[test]
fn missing_app_and_missing_executable_are_distinct() {
    const APP_ID: u32 = 10;
    let steam = TempTree::new("missing-targets");
    steam.install_app(APP_ID, "Installed");
    let discovery = steam.discover();

    let missing_exe =
        discovery.resolve_executable(&ExecutableQuery::new(APP_ID, "missing.exe").unwrap());
    let missing_app =
        discovery.resolve_executable(&ExecutableQuery::new(APP_ID + 1, "missing.exe").unwrap());

    assert_eq!(missing_exe.searched_installations(), 1);
    assert_eq!(missing_app.searched_installations(), 0);
    assert!(missing_exe.diagnostics().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::NotFound && diagnostic.path.is_some()
    }));
    assert!(missing_app.diagnostics().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::NotFound && diagnostic.path.is_none()
    }));
}

#[test]
fn malformed_sibling_manifest_yields_partial_inventory_without_hiding_matches() {
    const APP_ID: u32 = 20;
    let steam = TempTree::new("partial");
    steam.install_app(APP_ID, "Good");
    steam.install_executable("Good", "good.exe");
    steam.file("steamapps/appmanifest_21.acf", "\"AppState\" { garbage");

    let discovery = steam.discover();
    let report = discovery.resolve_executable(&ExecutableQuery::new(APP_ID, "good.exe").unwrap());

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert_eq!(report.cardinality(), MatchCardinality::One);
    assert_eq!(report.completeness(), MatchCompleteness::Partial);
    assert!(report.diagnostics().any(|diagnostic| {
        diagnostic.stage == DiscoveryStage::ReadManifest
            && diagnostic.kind == DiagnosticKind::Parse
            && diagnostic.app_id == Some(fixture_app_id(21))
    }));
}

#[test]
fn derived_match_summaries_serialize_with_shared_scan_diagnostics() {
    const APP_ID: u32 = 22;
    let steam = TempTree::new("shared-scan-diagnostics");
    steam.install_app(APP_ID, "SharedDiagnostics");
    steam.install_executable("SharedDiagnostics", "found.exe");
    steam.file("steamapps/appmanifest_23.acf", "\"AppState\" { garbage");
    let discovery = steam.discover();
    let found = ExecutableQuery::new(APP_ID, "found.exe").unwrap();
    let missing = ExecutableQuery::new(APP_ID, "missing.exe").unwrap();

    let reports = discovery.resolve_all([&found, &missing]);

    assert_eq!(reports[0].cardinality(), MatchCardinality::One);
    assert_eq!(reports[1].cardinality(), MatchCardinality::None);
    assert_eq!(reports[1].completeness(), MatchCompleteness::Partial);
    assert_eq!(reports[0].scan_diagnostics().len(), 1);
    assert!(std::ptr::eq(
        reports[0].scan_diagnostics().as_ptr(),
        reports[1].scan_diagnostics().as_ptr(),
    ));
    assert!(reports[1].query_diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == DiscoveryStage::ResolveExecutable
            && diagnostic.kind == DiagnosticKind::NotFound
    }));

    let json = serde_json::to_value(&reports[1]).expect("serialize match report");
    assert_eq!(json["query"]["app_id"], APP_ID);
    assert_eq!(json["cardinality"], "none");
    assert_eq!(json["completeness"], "partial");
    assert_eq!(
        json["diagnostics"].as_array().unwrap().len(),
        reports[1].diagnostics().count()
    );
}

#[test]
fn manifest_id_is_cross_checked_with_filename() {
    let steam = TempTree::new("id-mismatch");
    steam.file(
        "steamapps/appmanifest_30.acf",
        &TempTree::manifest(30, 31, "Mismatch", "Mismatch"),
    );
    steam.dir("steamapps/common/Mismatch");

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert!(discovery.inventory().apps().is_empty());
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::InvalidData
            && diagnostic.message.contains("expected 30 from its filename")
    }));
}

#[test]
fn reserved_zero_manifest_app_id_is_rejected() {
    let steam = TempTree::new("zero-app-id");
    steam.file(
        "steamapps/appmanifest_0.acf",
        &TempTree::manifest(0, 0, "InvalidApp", "Invalid App"),
    );
    steam.dir("steamapps/common/InvalidApp");

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert!(discovery.inventory().apps().is_empty());
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.app_id.is_none()
            && diagnostic.kind == DiagnosticKind::InvalidData
            && diagnostic.message.contains("reserved invalid app ID 0")
    }));
}

#[test]
fn manifest_install_dir_cannot_escape_common() {
    const APP_ID: u32 = 40;
    let steam = TempTree::new("manifest-traversal");
    steam.file(
        format!("steamapps/appmanifest_{APP_ID}.acf"),
        &TempTree::manifest(APP_ID, APP_ID, "../../outside", "Traversal"),
    );

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert!(discovery.inventory().apps().is_empty());
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::OutsideBoundary
            && diagnostic.message.contains("escapes steamapps/common")
    }));
}

#[test]
fn manifest_install_dir_cannot_claim_all_of_common() {
    const APP_ID: u32 = 41;
    let steam = TempTree::new("manifest-dot");
    steam.file(
        format!("steamapps/appmanifest_{APP_ID}.acf"),
        &TempTree::manifest(APP_ID, APP_ID, ".", "Dot"),
    );
    steam.dir("steamapps/common");

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert!(discovery.inventory().apps().is_empty());
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::OutsideBoundary
            && diagnostic.message.contains("escapes steamapps/common")
    }));
}

#[test]
fn executable_queries_reject_absolute_and_traversing_paths() {
    let absolute = PathBuf::from(std::path::MAIN_SEPARATOR.to_string()).join("outside.exe");
    assert_eq!(
        ExecutableQuery::new(0, "game.exe"),
        Err(QueryErr::InvalidAppId)
    );
    assert!(matches!(
        ExecutableQuery::new(1, absolute),
        Err(QueryErr::AbsoluteCandidate { .. })
    ));
    assert!(matches!(
        ExecutableQuery::new(1, "../outside.exe"),
        Err(QueryErr::TraversalCandidate { .. })
    ));
    assert_eq!(
        ExecutableQuery::any_of::<_, &str>(1, []),
        Err(QueryErr::NoCandidates)
    );
    assert_eq!(ExecutableQuery::new(1, ""), Err(QueryErr::EmptyCandidate));
    let valid = ExecutableQuery::new(1, "game.exe").unwrap();
    assert_eq!(valid.fallback(""), Err(QueryErr::EmptyCandidate));
}

#[test]
fn libraryfolders_expands_all_libraries_and_deduplicates_slots() {
    const APP_ID: u32 = 50;
    let steam = TempTree::new("library-owner");
    let library = TempTree::new("registered-library");
    library.install_app(APP_ID, "External");
    steam.file(
        "steamapps/libraryfolders.vdf",
        &format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n}}\n",
            library.root().display().to_string().replace('\\', r"\\"),
            library.root().display().to_string().replace('\\', r"\\"),
        ),
    );

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Complete);
    assert_eq!(discovery.inventory().libraries().len(), 2);
    assert_eq!(
        discovery
            .inventory()
            .apps_for(fixture_app_id(APP_ID))
            .count(),
        1
    );
}

#[test]
fn relative_library_paths_are_not_resolved_against_the_process_directory() {
    const APP_ID: u32 = 51;
    let steam = TempTree::new("relative-library");
    steam.install_app(APP_ID, "Local");
    steam.file(
        "steamapps/libraryfolders.vdf",
        "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\"relative/library\"\n\t}\n}\n",
    );

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert_eq!(discovery.inventory().libraries().len(), 1);
    assert_eq!(
        discovery
            .inventory()
            .apps_for(fixture_app_id(APP_ID))
            .count(),
        1
    );
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == DiscoveryStage::ValidateLibrary
            && diagnostic.kind == DiagnosticKind::InvalidData
            && diagnostic.message.contains("is not absolute")
    }));
}

#[test]
fn malformed_library_slot_does_not_hide_a_valid_sibling() {
    const APP_ID: u32 = 52;
    let steam = TempTree::new("malformed-library-owner");
    let library = TempTree::new("valid-library-sibling");
    library.install_app(APP_ID, "SiblingApp");
    steam.file(
        "steamapps/libraryfolders.vdf",
        &format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n}}\n",
            library.root().display().to_string().replace('\\', r"\\"),
        ),
    );

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert_eq!(
        discovery
            .inventory()
            .apps_for(fixture_app_id(APP_ID))
            .count(),
        1
    );
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == DiscoveryStage::ReadLibraryFolders
            && diagnostic.message.contains("slot '0' has no path")
    }));
}

#[test]
fn wrong_type_library_is_classified_as_invalid_data() {
    const APP_ID: u32 = 53;
    let steam = TempTree::new("wrong-library-type");
    steam.install_app(APP_ID, "StillValid");
    let not_a_library = steam.file("not-a-library", "");
    steam.file(
        "steamapps/libraryfolders.vdf",
        &format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n}}\n",
            not_a_library.display().to_string().replace('\\', r"\\"),
        ),
    );

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == DiscoveryStage::ValidateLibrary
            && diagnostic.kind == DiagnosticKind::InvalidData
    }));
}

#[test]
fn metadata_size_and_utf8_failures_are_reported_without_hiding_valid_apps() {
    const APP_ID: u32 = 80;
    let steam = TempTree::new("metadata-bounds");
    steam.install_app(APP_ID, "GoodMetadata");
    let oversized = steam.root().join("steamapps/appmanifest_81.acf");
    std::fs::write(
        &oversized,
        vec![b'x'; usize::try_from(MAX_STEAM_METADATA_BYTES + 1).expect("limit fits in usize")],
    )
    .expect("write oversized manifest");
    let invalid_utf8 = steam.root().join("steamapps/appmanifest_82.acf");
    std::fs::write(&invalid_utf8, [0xff]).expect("write invalid UTF-8 manifest");

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert_eq!(
        discovery
            .inventory()
            .apps_for(fixture_app_id(APP_ID))
            .count(),
        1
    );
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.app_id == Some(fixture_app_id(81)) && diagnostic.message.contains("exceeds the")
    }));
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.app_id == Some(fixture_app_id(82))
            && diagnostic.message.contains("not valid UTF-8")
    }));
}

#[test]
fn manifest_metadata_is_parsed_leniently_and_serialized_sparsely() {
    const APP_ID: u32 = 90;
    let steam = TempTree::new("metadata-lenient");
    steam.file(
        format!("steamapps/appmanifest_{APP_ID}.acf"),
        &format!(
            concat!(
                "\"AppState\"\n{{\n",
                "\t\"appid\"\t\"{app_id}\"\n",
                "\t\"name\"\t\"Meta App\"\n",
                "\t\"installdir\"\t\"MetaApp\"\n",
                "\t\"StateFlags\"\t\"6\"\n",
                "\t\"buildid\"\t\"24548213\"\n",
                "\t\"SizeOnDisk\"\t\"not-a-number\"\n",
                "\t\"LastUpdated\"\t\"1723756800\"\n",
                "\t\"InstalledDepots\"\n\t{{\n",
                "\t\t\"1874911\"\n\t\t{{\n",
                "\t\t\t\"manifest\"\t\"6518622583388653769\"\n",
                "\t\t\t\"size\"\t\"123456\"\n",
                "\t\t\t\"dlcappid\"\t\"1874910\"\n",
                "\t\t}}\n",
                "\t\t\"bogus\"\n\t\t{{\n",
                "\t\t\t\"manifest\"\t\"1\"\n",
                "\t\t}}\n",
                "\t}}\n",
                "\t\"UserConfig\"\n\t{{\n",
                "\t\t\"betakey\"\t\"profiling\"\n",
                "\t}}\n",
                "}}\n",
            ),
            app_id = APP_ID,
        ),
    );
    steam.dir("steamapps/common/MetaApp");

    let discovery = steam.discover();

    // Each malformed advisory value is omitted with its own warning, never an
    // inventory error, so the scan stays complete.
    assert_eq!(discovery.state(), DiscoveryState::Complete);
    let warnings: Vec<&str> = discovery
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
            assert_eq!(diagnostic.app_id, Some(fixture_app_id(APP_ID)));
            diagnostic.message.as_str()
        })
        .collect();
    assert_eq!(
        warnings.len(),
        2,
        "one warning per malformed value: {warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("advisory field SizeOnDisk value 'not-a-number'"))
    );
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("installed-depot key 'bogus' is not a depot ID"))
    );
    let app = discovery
        .inventory()
        .apps_for(fixture_app_id(APP_ID))
        .next()
        .expect("app with metadata should be inventoried");

    let flags = app.metadata().state_flags.expect("state flags parsed");
    assert_eq!(flags, StateFlags::from_raw(6));
    assert_eq!(flags.raw(), 6);
    assert!(flags.is_fully_installed());
    assert!(flags.update_required());
    assert_eq!(
        app.metadata().build_id.map(NonZeroU64::get),
        Some(24_548_213)
    );
    assert_eq!(app.metadata().size_on_disk, None);
    assert_eq!(app.metadata().last_updated, Some(1_723_756_800));
    assert_eq!(app.metadata().beta_key.as_deref(), Some("profiling"));
    assert_eq!(app.metadata().installed_depots.len(), 1);
    let depot = &app.metadata().installed_depots[&NonZeroU64::new(1_874_911).unwrap()];
    assert_eq!(
        depot.manifest.map(NonZeroU64::get),
        Some(6_518_622_583_388_653_769)
    );
    assert_eq!(depot.size, Some(123_456));
    assert_eq!(depot.dlc_app_id, Some(fixture_app_id(1_874_910)));

    let json = serde_json::to_value(&discovery).expect("serialize report");
    let metadata = &json["inventory"]["apps"][0]["metadata"];
    assert_eq!(metadata["build_id"], 24_548_213);
    assert!(
        metadata.get("size_on_disk").is_none(),
        "unparseable optional values must be omitted, not serialized as null"
    );
}

#[test]
fn ambiguous_numeric_depot_metadata_is_omitted_with_a_warning() {
    const APP_ID: u32 = 92;
    let steam = TempTree::new("metadata-ambiguous-depot");
    steam.file(
        format!("steamapps/appmanifest_{APP_ID}.acf"),
        &format!(
            concat!(
                "\"AppState\"\n{{\n",
                "\t\"appid\"\t\"{app_id}\"\n",
                "\t\"name\"\t\"Ambiguous Depot\"\n",
                "\t\"installdir\"\t\"AmbiguousDepot\"\n",
                "\t\"InstalledDepots\"\n\t{{\n",
                "\t\t\"3\"\n\t\t{{\n\t\t\t\"manifest\"\t\"800\"\n\t\t}}\n",
                "\t\t\"03\"\n\t\t{{\n\t\t\t\"manifest\"\t\"900\"\n\t\t}}\n",
                "\t}}\n",
                "}}\n",
            ),
            app_id = APP_ID,
        ),
    );
    steam.dir("steamapps/common/AmbiguousDepot");

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Complete);
    assert!(!discovery.has_errors());
    let app = discovery
        .inventory()
        .apps_for(fixture_app_id(APP_ID))
        .next()
        .expect("the valid app identity must remain inventoried");
    assert!(app.metadata().installed_depots.is_empty());
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Warning
            && diagnostic.kind == DiagnosticKind::InvalidData
            && diagnostic.message.contains("normalize to ID 3")
    }));
}

#[test]
fn unreadable_optional_metadata_keeps_the_app_with_a_warning() {
    const APP_ID: u32 = 91;
    let steam = TempTree::new("metadata-unreadable");
    steam.file(
        format!("steamapps/appmanifest_{APP_ID}.acf"),
        &format!(
            concat!(
                "\"AppState\"\n{{\n",
                "\t\"appid\"\t\"{app_id}\"\n",
                "\t\"name\"\t\"Broken Meta\"\n",
                "\t\"installdir\"\t\"BrokenMeta\"\n",
                "\t\"StateFlags\"\n\t{{\n\t\t\"nested\"\t\"4\"\n\t}}\n",
                "}}\n",
            ),
            app_id = APP_ID,
        ),
    );
    steam.dir("steamapps/common/BrokenMeta");

    let discovery = steam.discover();

    assert_eq!(
        discovery.state(),
        DiscoveryState::Complete,
        "a metadata warning must not make the inventory incomplete"
    );
    assert!(!discovery.has_errors());
    let app = discovery
        .inventory()
        .apps_for(fixture_app_id(APP_ID))
        .next()
        .expect("unreadable optional metadata must not evict the app");
    assert_eq!(app.metadata(), &AppMetadata::default());

    let warning = discovery
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.app_id == Some(fixture_app_id(APP_ID)))
        .expect("skipped metadata leaves a diagnostic");
    assert_eq!(warning.severity, DiagnosticSeverity::Warning);
    assert_eq!(warning.stage, DiscoveryStage::ReadManifest);
    assert_eq!(warning.kind, DiagnosticKind::Parse);
    assert!(warning.message.contains("optional manifest metadata"));
    let warning_path = warning.path.as_deref().expect("warning names the manifest");
    assert_eq!(
        warning_path.file_name().and_then(|name| name.to_str()),
        Some(format!("appmanifest_{APP_ID}.acf").as_str())
    );
}

#[test]
fn explicit_root_attempts_and_failures_are_retained() {
    const APP_ID: u32 = 60;
    let steam = TempTree::new("attempt-ledger");
    steam.install_app(APP_ID, "Good");
    let missing = steam.root().join("missing-root");

    let discovery = Discovery::discover_from([
        missing.clone(),
        steam.root().to_path_buf(),
        steam.root().to_path_buf(),
    ]);

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert_eq!(discovery.attempts().len(), 3);
    assert_eq!(discovery.attempts()[0].outcome, AttemptOutcome::Rejected);
    assert_eq!(discovery.attempts()[1].outcome, AttemptOutcome::Accepted);
    assert_eq!(discovery.attempts()[2].outcome, AttemptOutcome::Duplicate);
    assert!(discovery.attempts()[0].detail().is_none());
    let (_, linked_diagnostic) = discovery
        .attempts_with_diagnostics()
        .next()
        .expect("rejected attempt");
    let linked_diagnostic = linked_diagnostic.expect("linked rejection diagnostic");
    assert_eq!(linked_diagnostic.stage, DiscoveryStage::ValidateSteamRoot);
    assert_eq!(linked_diagnostic.path.as_deref(), Some(missing.as_path()));

    let json = serde_json::to_value(&discovery).expect("serialize linked attempt diagnostic");
    assert_eq!(json["attempts"][0]["diagnostic_index"], 0);
    assert!(json["attempts"][0].get("detail").is_none());
}

#[cfg(feature = "cli")]
#[test]
fn cli_rejects_zero_app_ids_as_usage_errors() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_steamapps-scan"))
        .args(["resolve", "0", "game.exe"])
        .output()
        .expect("run the steamapps-scan CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid value '0'"));
}

#[cfg(feature = "cli")]
#[test]
fn cli_reports_incomplete_explicit_discovery_with_exit_four() {
    let fixture = TempTree::new("cli-partial-exit");
    let missing_root = fixture.root().join("missing-root");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_steamapps-scan"))
        .arg("--root")
        .arg(&missing_root)
        .arg("scan")
        .output()
        .expect("run the steamapps-scan CLI");

    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("rejected"));
    assert!(stderr.contains(&missing_root.display().to_string()));
}

#[cfg(windows)]
#[test]
fn automatic_discovery_attempts_every_registry_location_and_view() {
    let discovery = Discovery::discover();
    let registry_sources = discovery
        .attempts()
        .iter()
        .filter_map(|attempt| match &attempt.source {
            DiscoverySource::Registry {
                hive, key, view, ..
            } => Some((hive.as_str(), key.as_str(), view.as_str())),
            DiscoverySource::Explicit
            | DiscoverySource::Process { .. }
            | DiscoverySource::Environment { .. }
            | DiscoverySource::Conventional { .. }
            | _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(registry_sources.len(), 3);
    assert!(registry_sources.contains(&("HKCU", r"Software\Valve\Steam", "default")));
    assert!(registry_sources.contains(&("HKLM", r"SOFTWARE\Valve\Steam", "32-bit")));
    assert!(registry_sources.contains(&("HKLM", r"SOFTWARE\Valve\Steam", "64-bit")));
}

#[cfg(windows)]
#[test]
fn junctioned_install_is_accepted_but_executable_escape_is_rejected() {
    const APP_ID: u32 = 70;
    let steam = TempTree::new("junction-library");
    let physical_install = TempTree::new("junction-install");
    let outside = TempTree::new("junction-outside");
    physical_install.file("inside.exe", "");
    outside.file("outside.exe", "");
    steam.file(
        format!("steamapps/appmanifest_{APP_ID}.acf"),
        &TempTree::manifest(APP_ID, APP_ID, "Junctioned", "Junctioned"),
    );
    let common = dunce::canonicalize(steam.dir("steamapps/common")).unwrap();
    let install_link = common.join("Junctioned");
    let install_status = std::process::Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            &install_link.to_string_lossy(),
            &physical_install.root().to_string_lossy(),
        ])
        .status()
        .expect("create install junction");
    assert!(install_status.success());
    let escape_link = physical_install.root().join("escape");
    let escape_status = std::process::Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            &escape_link.to_string_lossy(),
            &outside.root().to_string_lossy(),
        ])
        .status()
        .expect("create executable escape junction");
    assert!(escape_status.success());

    let discovery = steam.discover();
    assert_eq!(discovery.state(), DiscoveryState::Complete);
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::OutsideBoundary
            && diagnostic.message.contains("accepted as a junction")
    }));

    let inside = discovery.resolve_executable(&ExecutableQuery::new(APP_ID, "inside.exe").unwrap());
    assert_eq!(inside.cardinality(), MatchCardinality::One);

    let escaped =
        discovery.resolve_executable(&ExecutableQuery::new(APP_ID, "escape/outside.exe").unwrap());
    assert_eq!(escaped.cardinality(), MatchCardinality::None);
    assert_eq!(escaped.completeness(), MatchCompleteness::Partial);
    assert!(escaped.diagnostics().any(|diagnostic| {
        diagnostic.stage == DiscoveryStage::ResolveExecutable
            && diagnostic.kind == DiagnosticKind::OutsideBoundary
    }));
}
