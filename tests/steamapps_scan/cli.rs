//! steamapps-scan - CLI contract tests
//! ====================================
//!
//! Exit codes and messages of the `steamapps-scan` binary.

use super::*;

#[test]
fn cli_rejects_zero_app_ids_as_usage_errors() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_steamapps-scan"))
        .args(["resolve", "0", "game.exe"])
        .output()
        .expect("run the steamapps-scan CLI");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid value '0'"));
}

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
