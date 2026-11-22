//! steamapps-scan - CLI
//! =====================
//!
//! Command-line front end over the `steamapps_scan` library. The binary
//! consumes only the crate's public API, so everything it prints is
//! available to library consumers unchanged. `--json` emits the same
//! serialized reports the library produces; the human output is a
//! condensed view of the identical evidence.

#![expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "this binary's contract is to render reports and diagnostics on standard streams"
)]

use std::{fmt::Write as _, num::NonZeroU32, path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use steamapps_scan::{
    AppInstall, AttemptOutcome, Diagnostic, DiagnosticSeverity, Discovery, DiscoverySource,
    DiscoveryState, ExecutablePath, ExecutableQuery, FileVersion, MatchCardinality,
    MatchCompleteness, MatchReport,
};

/// The requested result was produced.
const EXIT_OK: u8 = 0;
/// The run was valid but found nothing: no Steam, no apps, or no match.
const EXIT_NONE: u8 = 1;
/// More than one result matched a request that needs exactly one.
const EXIT_AMBIGUOUS: u8 = 3;
/// A result was produced, but failures may have hidden evidence.
const EXIT_PARTIAL: u8 = 4;
/// Automatic discovery is unavailable on this platform.
const EXIT_UNSUPPORTED: u8 = 5;

#[derive(Parser)]
#[command(
    name = "steamapps-scan",
    version,
    about = "Locate Steam installations and query the installed-app inventory",
    after_help = "Exit codes: 0 success, 1 nothing found, 2 usage error, \
                  3 ambiguous result, 4 partial evidence, 5 platform unsupported."
)]
struct Cli {
    /// Scan these Steam roots instead of automatic platform discovery.
    #[arg(long = "root", global = true, value_name = "DIR")]
    roots: Vec<PathBuf>,

    /// Emit the full serialized report as JSON.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Locate Steam, load the inventory, and print the full report.
    Scan,
    /// List discovered app installations.
    Apps {
        /// Only installations of this app ID.
        #[arg(long, value_name = "APP_ID")]
        app_id: Option<NonZeroU32>,
    },
    /// Resolve executable candidates inside an app installation.
    Resolve {
        /// Steam app ID to search.
        app_id: NonZeroU32,
        /// Installation-relative candidate paths in preference order.
        #[arg(required = true, value_name = "RELATIVE_PATH")]
        candidates: Vec<PathBuf>,
        /// Also read each match's executable version resource.
        #[arg(long)]
        version: bool,
    },
    /// Print the content signal for an app with exactly one installation.
    Signal {
        /// Steam app ID to sign.
        app_id: NonZeroU32,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let discovery = if cli.roots.is_empty() {
        Discovery::discover()
    } else {
        Discovery::discover_from(cli.roots.iter().cloned())
    };

    let code = match cli.command {
        Command::Scan => scan(&discovery, cli.json),
        Command::Apps { app_id } => apps(&discovery, app_id, cli.json),
        Command::Resolve {
            app_id,
            candidates,
            version,
        } => resolve(&discovery, app_id, &candidates, version, cli.json),
        Command::Signal { app_id } => signal(&discovery, app_id, cli.json),
    };
    ExitCode::from(code)
}

fn scan(discovery: &Discovery, json: bool) -> u8 {
    let output_complete = if json {
        print_json(discovery)
    } else {
        println!("state: {}", describe_state(discovery.state()));
        for root in discovery.inventory().steam_roots() {
            println!("steam root: {root}");
        }
        for library in discovery.inventory().libraries() {
            println!("library: {library}");
        }
        for app in discovery.inventory().apps() {
            println!("{}", describe_app(app));
        }
        println!("attempts:");
        for (attempt, diagnostic) in discovery.attempts_with_diagnostics() {
            let source = describe_source(&attempt.source);
            let outcome = describe_outcome(attempt.outcome);
            let mut line = format!("  {source}: {outcome}");
            if let Some(candidate) = &attempt.candidate {
                let formatted = write!(line, " ({})", candidate.display());
                debug_assert!(
                    formatted.is_ok(),
                    "formatting a discovery candidate into a String must be infallible"
                );
            }
            if let Some(detail) = attempt.detail.as_deref() {
                line.push_str(" - ");
                line.push_str(detail);
            } else if let Some(diagnostic) = diagnostic {
                line.push_str(" - ");
                line.push_str(&diagnostic.message);
            }
            println!("{line}");
        }
        print_diagnostics(discovery.diagnostics());
        true
    };
    let result = match discovery.state() {
        DiscoveryState::Complete => EXIT_OK,
        DiscoveryState::NotFound => EXIT_NONE,
        DiscoveryState::Unsupported => EXIT_UNSUPPORTED,
        DiscoveryState::Partial | _ => EXIT_PARTIAL,
    };
    final_code(discovery, result, output_complete)
}

fn apps(discovery: &Discovery, app_id: Option<NonZeroU32>, json: bool) -> u8 {
    let selected: Vec<&AppInstall> = discovery
        .inventory()
        .apps()
        .iter()
        .filter(|app| app_id.is_none_or(|wanted| app.app_id() == wanted.get()))
        .collect();
    let output_complete = if json {
        print_json(&selected)
    } else {
        for app in &selected {
            println!("{}", describe_app(app));
        }
        true
    };
    let result = if selected.is_empty() {
        EXIT_NONE
    } else {
        EXIT_OK
    };
    final_code(discovery, result, output_complete)
}

fn resolve(
    discovery: &Discovery,
    app_id: NonZeroU32,
    candidates: &[PathBuf],
    version: bool,
    json: bool,
) -> u8 {
    let query = match ExecutableQuery::any_of(app_id.get(), candidates.iter().cloned()) {
        Ok(query) => query,
        Err(error) => {
            eprintln!("invalid executable query: {error}");
            return 2;
        }
    };
    let report = discovery.resolve_executable(&query);
    let versions = if version {
        report
            .matches()
            .iter()
            .map(|found| VersionOutput::read(found.executable()))
            .collect()
    } else {
        Vec::new()
    };

    let output_complete = if json {
        print_json(&ResolveOutput {
            report: &report,
            versions: &versions,
        })
    } else {
        for found in report.matches() {
            println!(
                "app {} ({}): {}",
                found.app().app_id(),
                found.app().name().unwrap_or("unnamed"),
                found.executable()
            );
        }
        for entry in &versions {
            println!("{}", entry.describe());
        }
        print_diagnostics_iter(report.diagnostics());
        true
    };
    let result = match report.cardinality() {
        MatchCardinality::One => EXIT_OK,
        MatchCardinality::None => EXIT_NONE,
        MatchCardinality::Many | _ => EXIT_AMBIGUOUS,
    };
    let evidence_complete = output_complete
        && report.completeness() == MatchCompleteness::Complete
        && versions.iter().all(VersionOutput::is_complete);
    final_code(discovery, result, evidence_complete)
}

fn signal(discovery: &Discovery, app_id: NonZeroU32, json: bool) -> u8 {
    let app_id = app_id.get();
    let installs: Vec<&AppInstall> = discovery.inventory().apps_for(app_id).collect();
    let app = match installs.as_slice() {
        [app] => app,
        [] => {
            eprintln!("app {app_id} is not present in the loaded inventory");
            return final_code(discovery, EXIT_NONE, true);
        }
        many => {
            eprintln!(
                "app {app_id} has {} installations; a signal needs exactly one",
                many.len()
            );
            return final_code(discovery, EXIT_AMBIGUOUS, true);
        }
    };
    match app.content_signal() {
        Ok(signal) => {
            let output_complete = if json {
                print_json(&serde_json::json!({
                    "app_id": app_id,
                    "scheme": signal.scheme(),
                    "value": signal.value(),
                }))
            } else {
                println!("{} {}", signal.scheme(), signal.value());
                true
            };
            final_code(discovery, EXIT_OK, output_complete)
        }
        Err(error) => {
            eprintln!("could not derive a content signal: {error}");
            EXIT_PARTIAL
        }
    }
}

#[derive(serde::Serialize)]
struct ResolveOutput<'a> {
    report: &'a MatchReport,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    versions: &'a [VersionOutput],
}

#[derive(serde::Serialize)]
struct VersionOutput {
    executable: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<FileVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl VersionOutput {
    fn read(executable: &ExecutablePath) -> Self {
        let (version, error) = match executable.version() {
            Ok(version) => (version, None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            executable: executable.path().to_path_buf(),
            version,
            error,
        }
    }

    fn describe(&self) -> String {
        let name = self.executable.display();
        if let Some(error) = &self.error {
            return format!("{name}: version unreadable - {error}");
        }
        let Some(version) = &self.version else {
            return format!("{name}: no version resource");
        };
        let mut parts = Vec::new();
        if let Some(product) = &version.product_version {
            parts.push(format!("product {product}"));
        }
        if let Some(file) = &version.file_version {
            parts.push(format!("file {file}"));
        }
        if let Some(fixed) = version.fixed_product_version {
            let [major, minor, patch, build] = fixed;
            parts.push(format!("fixed {major}.{minor}.{patch}.{build}"));
        }
        if parts.is_empty() {
            parts.push("version resource present but empty".to_string());
        }
        format!("{name}: {}", parts.join(", "))
    }

    const fn is_complete(&self) -> bool {
        self.error.is_none()
    }
}

fn describe_app(app: &AppInstall) -> String {
    let name = app.name().unwrap_or("unnamed");
    let mut line = format!("app {} ({name}): {}", app.app_id(), app.install_dir());
    let mut notes = Vec::new();
    if let Some(build_id) = app.metadata().build_id {
        notes.push(format!("build {build_id}"));
    }
    if let Some(flags) = app.metadata().state_flags {
        if flags.is_fully_installed() && !flags.update_required() {
            notes.push("fully installed".to_string());
        } else if flags.update_required() {
            notes.push("update required".to_string());
        } else {
            notes.push(format!("state {}", flags.raw()));
        }
    }
    if let Some(size) = app.metadata().size_on_disk {
        notes.push(format!("{} on disk", describe_bytes(size)));
    }
    if let Some(beta) = &app.metadata().beta_key {
        notes.push(format!("beta '{beta}'"));
    }
    if !notes.is_empty() {
        line.push_str(" [");
        line.push_str(&notes.join(", "));
        line.push(']');
    }
    line
}

fn describe_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= GIB {
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "floating-point conversion is used only for one-decimal display rounding"
        )]
        let gib = bytes as f64 / GIB as f64;
        format!("{gib:.1} GiB")
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} B")
    }
}

const fn describe_state(state: DiscoveryState) -> &'static str {
    match state {
        DiscoveryState::Complete => "complete",
        DiscoveryState::Partial => "partial",
        DiscoveryState::NotFound => "not found",
        DiscoveryState::Unsupported => "unsupported",
        _ => "unknown",
    }
}

const fn describe_outcome(outcome: AttemptOutcome) -> &'static str {
    match outcome {
        AttemptOutcome::Accepted => "accepted",
        AttemptOutcome::Duplicate => "duplicate",
        AttemptOutcome::NotPresent => "not present",
        AttemptOutcome::Rejected => "rejected",
        AttemptOutcome::Failed => "failed",
        AttemptOutcome::Unsupported => "unsupported",
        _ => "unknown",
    }
}

fn describe_source(source: &DiscoverySource) -> String {
    match source {
        DiscoverySource::Explicit => "explicit root".to_string(),
        DiscoverySource::Registry {
            hive, key, value, ..
        } => {
            format!("registry {hive}\\{key}\\{value}")
        }
        DiscoverySource::Process { executable } => format!("running {executable}"),
        DiscoverySource::Environment { variable } => format!("environment {variable}"),
        DiscoverySource::Conventional { description } => format!("conventional {description}"),
        other => format!("{other:?}"),
    }
}

fn print_diagnostics(diagnostics: &[Diagnostic]) {
    print_diagnostics_iter(diagnostics.iter());
}

fn print_diagnostics_iter<'a>(diagnostics: impl Iterator<Item = &'a Diagnostic>) {
    for diagnostic in diagnostics {
        let severity = match diagnostic.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
            _ => "diagnostic",
        };
        eprintln!("[{severity}] {}", diagnostic.message);
    }
}

fn final_code(discovery: &Discovery, result: u8, output_complete: bool) -> u8 {
    final_code_for(
        discovery.state(),
        discovery.has_errors(),
        result,
        output_complete,
    )
}

const fn final_code_for(
    state: DiscoveryState,
    has_errors: bool,
    result: u8,
    output_complete: bool,
) -> u8 {
    if matches!(state, DiscoveryState::Unsupported) {
        EXIT_UNSUPPORTED
    } else if !output_complete || has_errors {
        EXIT_PARTIAL
    } else {
        result
    }
}

fn print_json(value: &impl serde::Serialize) -> bool {
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            println!("{text}");
            true
        }
        Err(error) => {
            eprintln!("could not serialize the report: {error}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_state_has_highest_exit_precedence() {
        assert_eq!(
            final_code_for(DiscoveryState::Unsupported, true, EXIT_NONE, false),
            EXIT_UNSUPPORTED
        );
    }

    #[test]
    fn incomplete_evidence_precedes_a_subcommand_result() {
        assert_eq!(
            final_code_for(DiscoveryState::Complete, false, EXIT_OK, false),
            EXIT_PARTIAL
        );
        assert_eq!(
            final_code_for(DiscoveryState::Complete, true, EXIT_NONE, true),
            EXIT_PARTIAL
        );
    }
}
