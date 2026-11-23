//! steamapps-scan - Filesystem behavior tests
//! ===========================================
//!
//! Directory links, letter case, non-ASCII names, and long paths: the places
//! where Windows, Linux, and macOS filesystems disagree. Links are junctions
//! on Windows and symlinks elsewhere; case behavior is detected from the
//! temporary directory's filesystem rather than assumed from the platform.

use super::*;

#[test]
fn linked_roots_and_libraries_deduplicate() {
    const ROOT_APP: u32 = 301;
    const LIBRARY_APP: u32 = 302;
    let steam = TempTree::new("linked-root");
    let library = TempTree::new("linked-library");
    let links = TempTree::new("links");
    steam.install_app(ROOT_APP, "RootApp");
    library.install_app(LIBRARY_APP, "LibraryApp");
    let root_link = links.link_dir("Steam", steam.root());
    let library_link = links.link_dir("Library", library.root());
    steam.library_folders(&[steam.root(), library.root(), library_link.as_path()]);

    let discovery = Discovery::discover_from([steam.root(), root_link.as_path()]);

    assert_eq!(
        discovery.state(),
        DiscoveryState::Complete,
        "{:#?}",
        discovery.diagnostics()
    );
    let outcomes: Vec<_> = discovery
        .attempts()
        .iter()
        .map(|attempt| attempt.outcome)
        .collect();
    assert_eq!(
        outcomes,
        [AttemptOutcome::Accepted, AttemptOutcome::Duplicate]
    );
    assert_eq!(discovery.inventory().steam_roots().len(), 1);
    assert_eq!(discovery.inventory().libraries().len(), 2);
    assert_eq!(discovery.inventory().apps().len(), 2);
}

#[test]
fn linked_install_is_accepted_but_links_out_of_it_are_not() {
    const APP_ID: u32 = 70;
    let steam = TempTree::new("link-library");
    let physical_install = TempTree::new("link-install");
    let outside = TempTree::new("link-outside");
    physical_install.file("inside.exe", "");
    outside.file("outside.exe", "");
    steam.file(
        format!("steamapps/appmanifest_{APP_ID}.acf"),
        TempTree::manifest(APP_ID, APP_ID, "Linked", "Linked"),
    );
    steam.link_dir("steamapps/common/Linked", physical_install.root());
    physical_install.link_dir("escape", outside.root());

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

#[test]
fn one_install_reached_through_two_manifests_is_listed_once() {
    const COPY: u32 = 72;
    const FORCED: u32 = 73;
    // Each linking library's manifest sorts first, so only keeping the
    // library that holds the files picks the other manifest.
    let linking = TempTree::new("dup-alias");
    let holding = TempTree::new("dup-owner");
    let held = holding.install_app(COPY, "Shared");
    holding.install_executable("Shared", "app.exe");
    linking.file(
        format!("steamapps/appmanifest_{COPY}.acf"),
        TempTree::manifest(COPY, COPY, "Shared", "Shared"),
    );
    linking.link_dir("steamapps/common/Shared", &held);
    let steam = TempTree::new("dup-common");
    let forced = TempTree::new("dup-forced");
    forced.file(
        format!("steamapps/appmanifest_{FORCED}.acf"),
        TempTree::manifest(FORCED, FORCED, "Forced", "Forced"),
    );
    forced.file("Game.exe", "");
    steam.file(
        format!("steamapps/appmanifest_{FORCED}.acf"),
        TempTree::manifest(FORCED, FORCED, "Forced", "Forced"),
    );
    steam.link_dir("steamapps/common/Forced", forced.root());

    for (app_id, linking, holding, executable) in [
        (COPY, &linking, &holding, "app.exe"),
        (FORCED, &steam, &forced, "Game.exe"),
    ] {
        let discovery = Discovery::discover_from([linking.root(), holding.root()]);

        assert_eq!(
            discovery.state(),
            DiscoveryState::Complete,
            "{:#?}",
            discovery.diagnostics()
        );
        let installs: Vec<_> = discovery
            .inventory()
            .apps_for(fixture_app_id(app_id))
            .collect();
        assert_eq!(installs.len(), 1);
        let manifest = |tree: &TempTree| {
            dunce::canonicalize(
                tree.root()
                    .join(format!("steamapps/appmanifest_{app_id}.acf")),
            )
            .unwrap()
        };
        assert_eq!(installs[0].manifest_path().path(), manifest(holding));
        assert_eq!(installs[0].library_root().path(), holding.root());
        assert!(discovery.diagnostics().iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.stage == DiscoveryStage::ValidateInstall
                && diagnostic.kind == DiagnosticKind::Duplicate
                && diagnostic.app_id == Some(fixture_app_id(app_id))
                && diagnostic.path.as_deref() == Some(manifest(linking).as_path())
        }));
        let report =
            discovery.resolve_executable(&ExecutableQuery::new(app_id, executable).unwrap());
        assert!(report.unique().is_ok(), "{report:#?}");
    }
}

#[test]
fn install_link_to_a_vanished_target_is_reported() {
    const APP_ID: u32 = 71;
    let steam = TempTree::new("dangling-library");
    let vanished = TempTree::new("dangling-target");
    steam.file(
        format!("steamapps/appmanifest_{APP_ID}.acf"),
        TempTree::manifest(APP_ID, APP_ID, "Dangling", "Dangling"),
    );
    steam.link_dir("steamapps/common/Dangling", vanished.root());
    drop(vanished);

    let discovery = steam.discover();

    assert_eq!(discovery.state(), DiscoveryState::Partial);
    assert_eq!(discovery.inventory().apps(), []);
    assert!(discovery.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == DiscoveryStage::ValidateInstall
            && diagnostic.kind == DiagnosticKind::NotFound
            && diagnostic.app_id == Some(fixture_app_id(APP_ID))
    }));
}

#[test]
fn case_variant_spellings_follow_the_filesystem() {
    const APP_ID: u32 = 303;
    let steam = TempTree::new("case-variant");
    steam.install_app(APP_ID, "CaseApp");
    let executable = steam.install_executable("CaseApp", "Bin/Game.exe");
    let (variant, folds) = steam.case_variant();

    let discovery = Discovery::discover_from([steam.root(), variant.as_path()]);
    let report =
        discovery.resolve_executable(&ExecutableQuery::new(APP_ID, "bin/GAME.exe").unwrap());

    if folds {
        // One directory under two spellings: one root, and the executable
        // reported under its own spelling rather than the query's.
        assert_eq!(discovery.attempts()[1].outcome, AttemptOutcome::Duplicate);
        assert_eq!(
            report.unique().unwrap().executable().path(),
            dunce::canonicalize(&executable).unwrap()
        );
    } else {
        assert_eq!(discovery.attempts()[1].outcome, AttemptOutcome::Rejected);
        assert_eq!(report.cardinality(), MatchCardinality::None);
    }
}

#[test]
fn non_ascii_and_spaced_paths_resolve() {
    const APP_ID: u32 = 304;
    const NAME: &str = "Café Ω 游戏";
    let tree = TempTree::new("non-ascii");
    let root = tree.dir("Program Files (x86)/Stéam Ünïcode");
    let steamapps = "Program Files (x86)/Stéam Ünïcode/steamapps";
    tree.file(
        format!("{steamapps}/appmanifest_{APP_ID}.acf"),
        TempTree::manifest(APP_ID, APP_ID, NAME, NAME),
    );
    let executable = tree.file(format!("{steamapps}/common/{NAME}/Bin Dir/Gämé.exe"), "");

    let discovery = Discovery::discover_from([&root]);
    let report =
        discovery.resolve_executable(&ExecutableQuery::new(APP_ID, "Bin Dir/Gämé.exe").unwrap());

    assert_eq!(discovery.state(), DiscoveryState::Complete);
    assert_eq!(discovery.diagnostics(), []);
    let matched = report.unique().unwrap();
    assert_eq!(matched.app().name(), Some(NAME));
    assert_eq!(
        matched.executable().path(),
        dunce::canonicalize(&executable).unwrap()
    );
}

#[cfg(windows)]
#[test]
fn paths_beyond_max_path_resolve_like_any_other() {
    const DEEP_APP: u32 = 305;
    const LONG_APP: u32 = 306;
    let steam = TempTree::new("long-paths");
    // An executable nested past MAX_PATH inside an ordinary installation.
    steam.install_app(DEEP_APP, "Deep");
    let nested = (0..12)
        .map(|level| format!("level-{level:02}-of-a-deeply-nested-tree/"))
        .collect::<Vec<_>>()
        .concat();
    let deep_relative = format!("{nested}Game.exe");
    let deep = steam.install_executable("Deep", &deep_relative);
    // An installation directory whose own name carries it past MAX_PATH.
    let long_name = "L".repeat(240);
    steam.install_app(LONG_APP, &long_name);
    let long = steam.install_executable(&long_name, "Game.exe");
    assert!(deep.as_os_str().len() > 260 && long.as_os_str().len() > 260);

    let discovery = steam.discover();

    assert!(
        discovery.diagnostics().is_empty(),
        "{:#?}",
        discovery.diagnostics()
    );
    for (app_id, relative, file) in [
        (DEEP_APP, deep_relative.as_str(), &deep),
        (LONG_APP, "Game.exe", &long),
    ] {
        let report = discovery.resolve_executable(&ExecutableQuery::new(app_id, relative).unwrap());
        assert_eq!(
            report.cardinality(),
            MatchCardinality::One,
            "app {app_id}: {:#?}",
            report.diagnostics().collect::<Vec<_>>()
        );
        assert_eq!(
            report.unique().unwrap().executable().path(),
            dunce::canonicalize(file).unwrap()
        );
    }
}

/// Linux refuses a path of 4096 bytes or more, so a root of 4067 bytes puts
/// `steamapps/libraryfolders.vdf` one byte past the limit while its manifest
/// lands exactly on the longest allowed path: one marker cannot be inspected
/// while every other file stays reachable.
#[cfg(target_os = "linux")]
#[test]
fn an_uninspectable_marker_leaves_the_layout_to_the_others() {
    const APP_ID: u32 = 7;
    const ROOT_LEN: usize = 4067;
    let base = TempTree::new("long-marker");
    let mut root = base.root().to_path_buf();
    while ROOT_LEN.saturating_sub(root.as_os_str().len()) > 202 {
        root.push("d".repeat(100));
    }
    let last = ROOT_LEN
        .saturating_sub(root.as_os_str().len())
        .saturating_sub(1);
    root.push("d".repeat(last));
    TempTree::install_app_in(&root, APP_ID, "Game");
    let manifest = root.join(format!("steamapps/appmanifest_{APP_ID}.acf"));
    assert_eq!(manifest.as_os_str().len(), 4095);

    let discovery = Discovery::discover_from([root.as_path()]);

    let library = discovery
        .inventory()
        .libraries()
        .iter()
        .find(|library| library.path() == root.as_path())
        .expect("the root's own library is kept");
    assert_eq!(library.layout(), LibraryLayout::Common);
    assert_eq!(
        discovery
            .inventory()
            .apps_for(fixture_app_id(APP_ID))
            .count(),
        1,
        "{:#?}",
        discovery.diagnostics()
    );
}
