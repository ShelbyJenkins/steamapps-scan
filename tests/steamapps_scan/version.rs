//! steamapps-scan - Version resource tests
//! ========================================
//!
//! Version reading over synthetic PE images, so the portable reader runs on
//! every platform rather than only against Windows system libraries.

use super::*;

/// Install each `(name, bytes)` file into one fixture app and resolve it,
/// keeping the tree alive for as long as the executables are read.
fn resolve_all(label: &str, files: &[(&str, &[u8])]) -> (TempTree, Vec<ExecutablePath>) {
    const APP_ID: u32 = 95;
    let steam = TempTree::new(label);
    steam.install_app(APP_ID, "Versioned");
    for (name, bytes) in files {
        steam.file(format!("steamapps/common/Versioned/{name}"), bytes);
    }
    let discovery = steam.discover();
    let executables = files
        .iter()
        .map(|(name, _)| {
            discovery
                .resolve_executable(&ExecutableQuery::new(APP_ID, name).unwrap())
                .unique()
                .expect("each fixture file resolves")
                .executable()
                .clone()
        })
        .collect();
    (steam, executables)
}

#[test]
fn version_records_are_read_from_the_file_itself() {
    let english = VersionResource {
        fixed_signature: VersionResource::SIGNATURE,
        fixed_product_version: [5, 7, 4, 0],
        translation: [0x0409, 0x04B0],
        strings: &[
            ("ProductVersion", "pioneer_2.03.x-CL-1450810"),
            ("FileVersion", "1, 0, 0, 1"),
            ("ProductName", "Fixture Game"),
            ("CompanyName", "Fixture Studio"),
        ],
    }
    .image();
    // A zeroed fixed record beside a German string table that only the
    // translation table names.
    let german = VersionResource {
        fixed_signature: VersionResource::SIGNATURE,
        fixed_product_version: [0, 0, 0, 0],
        translation: [0x0407, 0x04E4],
        strings: &[("ProductVersion", "01.00.00.02")],
    }
    .image();

    let (_steam, executables) = resolve_all(
        "version-records",
        &[("English.exe", &english), ("German.dll", &german)],
    );

    let english = executables[0]
        .version()
        .expect("read the English image")
        .expect("the English image carries a version resource");
    assert_eq!(
        english.product_version.as_deref(),
        Some("pioneer_2.03.x-CL-1450810")
    );
    assert_eq!(english.file_version.as_deref(), Some("1, 0, 0, 1"));
    assert_eq!(english.product_name.as_deref(), Some("Fixture Game"));
    assert_eq!(english.company_name.as_deref(), Some("Fixture Studio"));
    assert_eq!(english.fixed_product_version, Some([5, 7, 4, 0]));

    let german = executables[1]
        .version()
        .expect("read the German image")
        .expect("the German image carries a version resource");
    assert_eq!(german.product_version.as_deref(), Some("01.00.00.02"));
    assert_eq!(german.file_version, None);
    assert_eq!(german.fixed_product_version, None);
}

#[test]
fn absent_and_unreadable_version_resources_stay_distinct() {
    #[derive(Debug, PartialEq)]
    enum Outcome {
        Absent,
        Unreadable,
        Read,
    }

    let valid = VersionResource {
        fixed_signature: VersionResource::SIGNATURE,
        fixed_product_version: [1, 2, 3, 4],
        translation: [0x0409, 0x04B0],
        strings: &[("ProductVersion", "1.2.3.4")],
    };
    let mut truncated = valid.image();
    truncated.truncate(truncated.len().saturating_sub(16));
    let forged = VersionResource {
        fixed_signature: 0xDEAD_BEEF,
        ..valid
    }
    .image();
    let bare = PeImage::without_resources();

    let cases: [(&str, &[u8], Outcome); 6] = [
        ("launch.sh", b"#!/bin/sh\nexec ./game\n", Outcome::Absent),
        ("empty.exe", b"", Outcome::Absent),
        ("bare.exe", &bare, Outcome::Absent),
        ("truncated.exe", &truncated, Outcome::Unreadable),
        ("forged.exe", &forged, Outcome::Unreadable),
        ("deleted.exe", &bare, Outcome::Unreadable),
    ];
    let files: Vec<(&str, &[u8])> = cases
        .iter()
        .map(|(name, bytes, _)| (*name, *bytes))
        .collect();
    let (_steam, executables) = resolve_all("version-outcomes", &files);
    std::fs::remove_file(executables[5].path()).expect("delete a resolved executable");

    for ((name, _, expected), executable) in cases.iter().zip(&executables) {
        let outcome = match executable.version() {
            Ok(None) => Outcome::Absent,
            Ok(Some(_)) => Outcome::Read,
            Err(error) => {
                assert_eq!(error.path, executable.path(), "{name} error names the file");
                Outcome::Unreadable
            }
        };
        assert_eq!(&outcome, expected, "{name}");
    }
}
