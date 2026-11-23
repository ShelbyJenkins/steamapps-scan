//! steamapps-scan - CLI
//! =====================
//!
//! Command-line front end over the `steamapps_scan` library. The binary
//! consumes only the crate's public API, so everything it prints is
//! available to library consumers unchanged. `--json` emits the same
//! serialized reports the library produces; the human output is a
//! condensed view of the identical evidence.

use std::{
    fmt::Write as _,
    io::{self, Write},
    num::NonZeroU32,
    path::PathBuf,
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use steamapps_scan::{
    AppInstall, AttemptOutcome, Diagnostic, DiagnosticSeverity, Discovery, DiscoverySource,
    DiscoveryState, ExecutablePath, ExecutableQuery, FileVersion, LibraryLayout, MatchCardinality,
    MatchCompleteness, MatchReport,
};

#[derive(Parser)]
#[command(
    name = "steamapps-scan",
    version,
    about = "Locate Steam installations and query the installed-app inventory",
    after_help = "Exit codes: 0 success, 1 nothing found, 2 usage error, \
                  3 ambiguous result, 4 partial evidence, 5 platform unsupported, \
                  6 no result produced."
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

/// Process exit status, from the codes listed in the help text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Exit {
    /// The requested result was produced.
    Success,
    /// The run was valid but found nothing: no Steam, no apps, or no match.
    NothingFound,
    /// The command-line input was invalid.
    Usage,
    /// More than one result matched a request that needs exactly one.
    Ambiguous,
    /// A result was produced, but failures may have hidden evidence.
    Partial,
    /// Automatic discovery is unavailable on this platform.
    Unsupported,
    /// No result was produced: the requested value could not be derived, or
    /// the output could not be serialized or written.
    Failed,
}

impl Exit {
    const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::NothingFound => 1,
            Self::Usage => 2,
            Self::Ambiguous => 3,
            Self::Partial => 4,
            Self::Unsupported => 5,
            Self::Failed => 6,
        }
    }

    /// Apply the scan's own evidence on top of a command's result.
    fn after_discovery(self, discovery: &Discovery, evidence: Evidence) -> Self {
        let evidence = if discovery.has_errors() {
            Evidence::Partial
        } else {
            evidence
        };
        self.after_evidence(discovery.state(), evidence)
    }

    /// An unsupported platform outranks everything; partial evidence then
    /// outranks the command's own result.
    const fn after_evidence(self, state: DiscoveryState, evidence: Evidence) -> Self {
        if matches!(state, DiscoveryState::Unsupported) {
            Self::Unsupported
        } else if matches!(evidence, Evidence::Partial) {
            Self::Partial
        } else {
            self
        }
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        Self::from(exit.code())
    }
}

/// Whether every source a command relied on was inspected successfully.
#[derive(Clone, Copy)]
enum Evidence {
    Complete,
    Partial,
}

/// How a command renders its result on standard output.
#[derive(Clone, Copy)]
enum OutputFormat {
    Human,
    Json,
}

/// Why a command could not deliver its output.
enum OutputErr {
    Write(io::Error),
    Serialize(serde_json::Error),
}

impl From<io::Error> for OutputErr {
    fn from(error: io::Error) -> Self {
        Self::Write(error)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = if cli.json {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };
    let discovery = if cli.roots.is_empty() {
        Discovery::discover()
    } else {
        Discovery::discover_from(cli.roots.iter().cloned())
    };

    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    let result = match cli.command {
        Command::Scan => scan(&discovery, format, &mut out, &mut err),
        Command::Apps { app_id } => apps(&discovery, app_id, format, &mut out, &mut err),
        Command::Resolve {
            app_id,
            candidates,
            version,
        } => resolve(
            &discovery,
            app_id,
            &candidates,
            version,
            format,
            &mut out,
            &mut err,
        ),
        Command::Signal { app_id } => signal(&discovery, app_id, format, &mut out, &mut err),
    };
    let delivered = result.and_then(|exit| {
        out.flush()?;
        Ok(exit)
    });
    // Standard error is the last channel left, so a failure to report on it
    // is dropped; the exit code still says no result was produced.
    let exit = match delivered {
        Ok(exit) => exit,
        // The reader closed standard output early, as `| head` does. It
        // asked for less than the whole result, so there is nothing to tell.
        Err(OutputErr::Write(error)) if error.kind() == io::ErrorKind::BrokenPipe => Exit::Failed,
        Err(OutputErr::Write(error)) => {
            let _ = writeln!(err, "could not write output: {error}");
            Exit::Failed
        }
        Err(OutputErr::Serialize(error)) => {
            let _ = writeln!(err, "could not serialize the report: {error}");
            Exit::Failed
        }
    };
    ExitCode::from(exit)
}

fn scan(
    discovery: &Discovery,
    format: OutputFormat,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<Exit, OutputErr> {
    match format {
        OutputFormat::Json => print_json(out, discovery)?,
        OutputFormat::Human => {
            writeln!(out, "state: {}", describe_state(discovery.state()))?;
            for root in discovery.inventory().steam_roots() {
                writeln!(out, "steam root: {root}")?;
            }
            for library in discovery.inventory().libraries() {
                let layout = describe_layout(library.layout());
                writeln!(out, "library: {library} ({layout})")?;
            }
            for app in discovery.inventory().apps() {
                writeln!(out, "{}", describe_app(app))?;
            }
            writeln!(out, "attempts:")?;
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
                if let Some(detail) = attempt.detail() {
                    line.push_str(" - ");
                    line.push_str(detail);
                } else if let Some(diagnostic) = diagnostic {
                    line.push_str(" - ");
                    line.push_str(&diagnostic.message);
                }
                writeln!(out, "{line}")?;
            }
            print_diagnostics(err, discovery.diagnostics())?;
        }
    }
    let result = match discovery.state() {
        DiscoveryState::Complete => Exit::Success,
        DiscoveryState::NotFound => Exit::NothingFound,
        DiscoveryState::Unsupported => Exit::Unsupported,
        DiscoveryState::Partial | _ => Exit::Partial,
    };
    Ok(result.after_discovery(discovery, Evidence::Complete))
}

fn apps(
    discovery: &Discovery,
    app_id: Option<NonZeroU32>,
    format: OutputFormat,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<Exit, OutputErr> {
    let selected: Vec<&AppInstall> = discovery
        .inventory()
        .apps()
        .iter()
        .filter(|app| app_id.is_none_or(|wanted| app.app_id() == wanted))
        .collect();
    match format {
        OutputFormat::Json => print_json(out, &selected)?,
        OutputFormat::Human => {
            for app in &selected {
                writeln!(out, "{}", describe_app(app))?;
            }
        }
    }
    // The app list carries no diagnostics of its own; these explain a
    // partial exit code.
    print_diagnostics(err, discovery.diagnostics())?;
    let result = if selected.is_empty() {
        Exit::NothingFound
    } else {
        Exit::Success
    };
    Ok(result.after_discovery(discovery, Evidence::Complete))
}

fn resolve(
    discovery: &Discovery,
    app_id: NonZeroU32,
    candidates: &[PathBuf],
    version: bool,
    format: OutputFormat,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<Exit, OutputErr> {
    let query = match ExecutableQuery::any_of(app_id.get(), candidates.iter().cloned()) {
        Ok(query) => query,
        Err(error) => {
            writeln!(err, "invalid executable query: {error}")?;
            return Ok(Exit::Usage);
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

    match format {
        OutputFormat::Json => print_json(
            out,
            &ResolveOutput {
                report: &report,
                versions: &versions,
            },
        )?,
        OutputFormat::Human => {
            for found in report.matches() {
                writeln!(
                    out,
                    "app {} ({}): {}",
                    found.app().app_id(),
                    found.app().name().unwrap_or("unnamed"),
                    found.executable()
                )?;
            }
            for entry in &versions {
                writeln!(out, "{}", entry.describe())?;
            }
            print_diagnostics_iter(err, report.diagnostics())?;
        }
    }
    let result = match report.cardinality() {
        MatchCardinality::One => Exit::Success,
        MatchCardinality::None => Exit::NothingFound,
        MatchCardinality::Many => Exit::Ambiguous,
    };
    let evidence = if report.completeness() == MatchCompleteness::Complete
        && versions.iter().all(VersionOutput::is_readable)
    {
        Evidence::Complete
    } else {
        Evidence::Partial
    };
    Ok(result.after_discovery(discovery, evidence))
}

fn signal(
    discovery: &Discovery,
    app_id: NonZeroU32,
    format: OutputFormat,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<Exit, OutputErr> {
    // The signal carries no diagnostics of its own; these explain a missing
    // app or a partial exit code.
    print_diagnostics(err, discovery.diagnostics())?;
    let installs: Vec<&AppInstall> = discovery.inventory().apps_for(app_id).collect();
    let app = match installs.as_slice() {
        [app] => app,
        [] => {
            writeln!(err, "app {app_id} is not present in the loaded inventory")?;
            return Ok(Exit::NothingFound.after_discovery(discovery, Evidence::Complete));
        }
        many => {
            writeln!(
                err,
                "app {app_id} has {} installations; a signal needs exactly one",
                many.len()
            )?;
            return Ok(Exit::Ambiguous.after_discovery(discovery, Evidence::Complete));
        }
    };
    match app.content_signal() {
        Ok(signal) => {
            match format {
                OutputFormat::Json => print_json(
                    out,
                    &serde_json::json!({
                        "app_id": app_id,
                        "scheme": signal.scheme(),
                        "value": signal.value(),
                    }),
                )?,
                OutputFormat::Human => writeln!(out, "{} {}", signal.scheme(), signal.value())?,
            }
            Ok(Exit::Success.after_discovery(discovery, Evidence::Complete))
        }
        Err(error) => {
            writeln!(err, "could not derive a content signal: {error}")?;
            Ok(Exit::Failed)
        }
    }
}

#[derive(serde::Serialize)]
struct ResolveOutput<'a> {
    report: &'a MatchReport,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    versions: &'a [VersionOutput],
}

/// One match's version-resource read, serialized as `executable` plus
/// either `version` or `error`.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum VersionOutput {
    Found {
        executable: PathBuf,
        version: FileVersion,
    },
    Absent {
        executable: PathBuf,
    },
    Unreadable {
        executable: PathBuf,
        error: String,
    },
}

impl VersionOutput {
    fn read(executable: &ExecutablePath) -> Self {
        let path = executable.path().to_path_buf();
        match executable.version() {
            Ok(Some(version)) => Self::Found {
                executable: path,
                version,
            },
            Ok(None) => Self::Absent { executable: path },
            Err(error) => Self::Unreadable {
                executable: path,
                error: error.to_string(),
            },
        }
    }

    fn describe(&self) -> String {
        let (name, version) = match self {
            Self::Found {
                executable,
                version,
            } => (executable.display(), version),
            Self::Absent { executable } => {
                return format!("{}: no version resource", executable.display());
            }
            // The error message already starts with the executable path.
            Self::Unreadable { error, .. } => return error.clone(),
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

    const fn is_readable(&self) -> bool {
        match self {
            Self::Found { .. } | Self::Absent { .. } => true,
            Self::Unreadable { .. } => false,
        }
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

const fn describe_layout(layout: LibraryLayout) -> &'static str {
    match layout {
        LibraryLayout::Common => "apps in steamapps/common",
        LibraryLayout::ForceInstallDir => "force_install_dir: apps in the library itself",
        _ => "unknown layout",
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
            hive,
            key,
            value,
            view,
        } => {
            format!("registry {hive}\\{key}\\{value} ({view} view)")
        }
        DiscoverySource::Process { executable } => format!("running {executable}"),
        DiscoverySource::Environment { variable } => format!("environment {variable}"),
        DiscoverySource::Conventional { description } => format!("conventional {description}"),
        other => format!("{other:?}"),
    }
}

fn print_diagnostics(err: &mut impl Write, diagnostics: &[Diagnostic]) -> io::Result<()> {
    print_diagnostics_iter(err, diagnostics.iter())
}

fn print_diagnostics_iter<'a>(
    err: &mut impl Write,
    diagnostics: impl Iterator<Item = &'a Diagnostic>,
) -> io::Result<()> {
    for diagnostic in diagnostics {
        let severity = match diagnostic.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
            _ => "diagnostic",
        };
        writeln!(err, "[{severity}] {}", diagnostic.message)?;
    }
    Ok(())
}

fn print_json(out: &mut impl Write, value: &impl serde::Serialize) -> Result<(), OutputErr> {
    let text = serde_json::to_string_pretty(value).map_err(OutputErr::Serialize)?;
    writeln!(out, "{text}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_state_has_highest_exit_precedence() {
        assert_eq!(
            Exit::NothingFound.after_evidence(DiscoveryState::Unsupported, Evidence::Partial),
            Exit::Unsupported
        );
    }

    #[test]
    fn incomplete_evidence_precedes_a_subcommand_result() {
        assert_eq!(
            Exit::Success.after_evidence(DiscoveryState::Complete, Evidence::Partial),
            Exit::Partial
        );
        assert_eq!(
            Exit::NothingFound.after_evidence(DiscoveryState::Complete, Evidence::Partial),
            Exit::Partial
        );
    }
}
