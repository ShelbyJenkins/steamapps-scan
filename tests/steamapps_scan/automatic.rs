//! steamapps-scan - Automatic discovery tests
//! ===========================================
//!
//! Each platform's automatic sources, driven through a CLI child process
//! whose environment points them at fixture trees. On Windows the registry
//! and process sources read the real machine, so they are checked only for
//! the attempts they record.

use super::*;

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

/// Only `ProgramFiles(x86)` can be driven: Windows sets `ProgramFiles`
/// itself in every 64-bit process at startup, whatever the parent passed.
#[cfg(all(windows, feature = "cli"))]
#[test]
fn program_files_x86_locates_a_root_and_its_absence_is_recorded() {
    const APP_ID: u32 = 201;
    const VARIABLE: &str = "ProgramFiles(x86)";
    let program_files = TempTree::new("program-files-x86");
    let expected = program_files.root().join("Steam");
    TempTree::install_app_in(&expected, APP_ID, "FromProgramFiles");

    let present = CliScan::run(|command| command.env(VARIABLE, program_files.root()));
    let absent = CliScan::run(|command| command.env_remove(VARIABLE));

    let attempt = present.attempt("variable", VARIABLE);
    assert_eq!(attempt["outcome"], "accepted", "{attempt:#}");
    assert_eq!(
        attempt["candidate"].as_str().map(Path::new),
        Some(expected.as_path())
    );
    assert!(present.app_ids().contains(&u64::from(APP_ID)));
    assert_eq!(
        absent.attempt("variable", VARIABLE)["outcome"],
        "not_present"
    );
}

#[cfg(all(target_os = "linux", feature = "cli"))]
#[test]
fn home_pointers_and_xdg_data_home_locate_roots() {
    let home = TempTree::new("linux-home");
    let native = home.dir(".local/share/Steam");
    TempTree::install_app_in(&native, 211, "Native");
    home.link_dir(".steam/steam", &native);
    home.link_dir(".steam/root", &native);
    let xdg = home.dir("xdg");
    TempTree::install_app_in(&xdg.join("Steam"), 212, "FromXdg");

    let scan = CliScan::run(|command| command.env("HOME", home.root()).env("XDG_DATA_HOME", &xdg));

    assert_eq!(scan.code, Some(0), "{:#}", scan.report);
    assert_eq!(scan.report["state"], "complete");
    assert_eq!(
        scan.attempt("variable", "XDG_DATA_HOME")["outcome"],
        "accepted"
    );
    for (description, outcome) in [
        ("~/.local/share/Steam", "accepted"),
        ("~/.steam/steam", "duplicate"),
        ("~/.steam/root", "duplicate"),
        ("~/.steam/debian-installation", "rejected"),
        (
            "~/.var/app/com.valvesoftware.Steam/.local/share/Steam",
            "rejected",
        ),
        ("~/snap/steam/common/.local/share/Steam", "rejected"),
    ] {
        assert_eq!(
            scan.attempt("description", description)["outcome"],
            outcome,
            "{description}"
        );
    }
    // A conventional location that is simply absent is expected and leaves
    // no diagnostic.
    assert_eq!(scan.report["diagnostics"], serde_json::json!([]));
    let mut apps = scan.app_ids();
    apps.sort_unstable();
    assert_eq!(apps, [211, 212]);
}

#[cfg(all(target_os = "macos", feature = "cli"))]
#[test]
fn application_support_locates_a_root() {
    let home = TempTree::new("macos-home");
    TempTree::install_app_in(
        &home.dir("Library/Application Support/Steam"),
        221,
        "MacApp",
    );

    let scan = CliScan::run(|command| command.env("HOME", home.root()));

    assert_eq!(scan.code, Some(0), "{:#}", scan.report);
    assert_eq!(scan.report["state"], "complete");
    assert_eq!(
        scan.attempt("description", "~/Library/Application Support/Steam")["outcome"],
        "accepted"
    );
    assert_eq!(scan.app_ids(), [221]);
}

#[cfg(all(any(target_os = "linux", target_os = "macos"), feature = "cli"))]
#[test]
fn missing_home_is_recorded_rather_than_guessed() {
    let scan = CliScan::run(|command| {
        command
            .env_remove("HOME")
            .env("XDG_DATA_HOME", "relative/data")
    });

    assert_eq!(scan.code, Some(1), "{:#}", scan.report);
    assert_eq!(scan.report["state"], "not_found");
    assert_eq!(scan.attempt("variable", "HOME")["outcome"], "not_present");
    #[cfg(target_os = "linux")]
    {
        // The XDG base directory specification requires a relative value to
        // be ignored rather than resolved against the process directory.
        let xdg = scan.attempt("variable", "XDG_DATA_HOME");
        assert_eq!(xdg["outcome"], "rejected");
        assert!(xdg.get("candidate").is_none());
    }
}
