//! steamapps-scan - Executable queries
//! ====================================
//!
//! Executable target queries evaluated against a loaded Steam inventory.

use std::{
    num::NonZeroU32,
    path::{Component, PathBuf},
    sync::Arc,
};

use super::{
    paths::{ExecutablePath, FilePresence, RelativeExecutablePath},
    report::{
        AppInstall, Diagnostic, DiagnosticKind, DiagnosticSeverity, Discovery, DiscoveryStage,
        SteamInventory,
    },
};

/// Invalid executable query input.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, thiserror::Error)]
#[non_exhaustive]
pub enum QueryErr {
    /// Steam reserves application ID zero as invalid.
    #[error("Steam app ID must be nonzero")]
    InvalidAppId,
    /// The ordered candidate set was empty.
    #[error("an executable query must contain at least one candidate")]
    NoCandidates,
    /// A candidate contained no path components.
    #[error("executable candidate is empty")]
    EmptyCandidate,
    /// A candidate was rooted or carried a platform path prefix.
    #[error("executable candidate must be relative: {path}")]
    AbsoluteCandidate {
        /// Rejected candidate path.
        path: PathBuf,
    },
    /// A candidate contained a current- or parent-directory component.
    #[error("executable candidate may not contain '.' or '..': {path}")]
    TraversalCandidate {
        /// Rejected candidate path.
        path: PathBuf,
    },
}

/// Runtime identity of an executable target.
///
/// Candidates are exact paths relative to the app installation and are tried
/// in order for each matching installation.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ExecutableQuery {
    app_id: NonZeroU32,
    /// Most preferred candidate; a query always has at least this one.
    primary: RelativeExecutablePath,
    /// Lower-priority candidates in preference order.
    fallbacks: Vec<RelativeExecutablePath>,
}

impl ExecutableQuery {
    /// Create a query with one exact installation-relative candidate.
    pub fn new(app_id: u32, candidate: impl Into<PathBuf>) -> Result<Self, QueryErr> {
        Self::any_of(app_id, [candidate])
    }

    /// Create a query with ordered fallback candidates.
    pub fn any_of<I, P>(app_id: u32, candidates: I) -> Result<Self, QueryErr>
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        let app_id = NonZeroU32::new(app_id).ok_or(QueryErr::InvalidAppId)?;
        Self::for_app(app_id, candidates)
    }

    /// Append a lower-priority candidate.
    pub fn fallback(mut self, candidate: impl Into<PathBuf>) -> Result<Self, QueryErr> {
        self.fallbacks
            .push(RelativeExecutablePath::validate(candidate.into())?);
        Ok(self)
    }

    /// Steam app ID evaluated by this query.
    #[must_use]
    pub const fn app_id(&self) -> NonZeroU32 {
        self.app_id
    }

    /// Validated candidates in preference order.
    pub fn candidates(&self) -> impl Iterator<Item = &RelativeExecutablePath> {
        std::iter::once(&self.primary).chain(&self.fallbacks)
    }

    /// Build a runtime query from a consumer-defined marker type.
    pub fn for_target<T: ExecutableTarget>() -> Result<Self, QueryErr> {
        Self::for_app(T::APP_ID, T::EXECUTABLES.iter().copied())
    }

    fn for_app<I, P>(app_id: NonZeroU32, candidates: I) -> Result<Self, QueryErr>
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        let mut candidates = candidates
            .into_iter()
            .map(|candidate| RelativeExecutablePath::validate(candidate.into()));
        let Some(primary) = candidates.next() else {
            return Err(QueryErr::NoCandidates);
        };
        Ok(Self {
            app_id,
            primary: primary?,
            fallbacks: candidates.collect::<Result<_, _>>()?,
        })
    }
}

impl serde::Serialize for ExecutableQuery {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(serde::Serialize)]
        struct SerializableExecutableQuery<'a> {
            app_id: NonZeroU32,
            candidates: Vec<&'a RelativeExecutablePath>,
        }

        serde::Serialize::serialize(
            &SerializableExecutableQuery {
                app_id: self.app_id,
                candidates: self.candidates().collect(),
            },
            serializer,
        )
    }
}

/// Optional compile-time adapter for projects with named executable roles.
///
/// Multiple marker types may intentionally use the same [`Self::APP_ID`] and
/// provide different [`Self::EXECUTABLES`].
pub trait ExecutableTarget {
    /// Steam app ID containing the target executable.
    const APP_ID: NonZeroU32;
    /// Exact installation-relative paths in preference order.
    const EXECUTABLES: &'static [&'static str];
}

impl RelativeExecutablePath {
    /// Validate one installation-relative candidate: non-empty, relative,
    /// and free of current- or parent-directory traversal.
    fn validate(path: PathBuf) -> Result<Self, QueryErr> {
        if path.as_os_str().is_empty() {
            return Err(QueryErr::EmptyCandidate);
        }
        if path.is_absolute() {
            return Err(QueryErr::AbsoluteCandidate { path });
        }
        for component in path.components() {
            match component {
                Component::Normal(_) => {}
                Component::Prefix(_) | Component::RootDir => {
                    return Err(QueryErr::AbsoluteCandidate { path });
                }
                Component::CurDir | Component::ParentDir => {
                    return Err(QueryErr::TraversalCandidate { path });
                }
            }
        }
        Ok(Self(path))
    }
}

/// One executable match, tied to the exact app installation that supplied it.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[non_exhaustive]
pub struct ExecutableMatch {
    /// Exact app installation that supplied the executable.
    app: AppInstall,
    /// Canonical, regular-file path confined to `app`.
    executable: ExecutablePath,
    /// First ordered candidate that matched this installation.
    matched_candidate: RelativeExecutablePath,
}

impl ExecutableMatch {
    /// Exact app installation that supplied this executable.
    #[must_use]
    pub const fn app(&self) -> &AppInstall {
        &self.app
    }

    /// Canonical executable path confined to [`Self::app`].
    #[must_use]
    pub const fn executable(&self) -> &ExecutablePath {
        &self.executable
    }

    /// First ordered candidate that matched this installation.
    #[must_use]
    pub const fn matched_candidate(&self) -> &RelativeExecutablePath {
        &self.matched_candidate
    }
}

/// Number of matches retained by an executable query.
///
/// Exhaustive by design: the three variants cover every possible count, so
/// callers can match without a wildcard arm.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchCardinality {
    /// No app installation satisfied the query.
    None,
    /// Exactly one app installation satisfied the query.
    One,
    /// More than one app installation satisfied the query.
    Many,
}

/// Whether all relevant installs and candidates could be inspected.
///
/// Exhaustive by design: evidence is either complete or it is not.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchCompleteness {
    /// Every relevant source and path was inspected successfully.
    Complete,
    /// At least one relevant scan or path inspection failed.
    Partial,
}

/// Failure to reduce a query report to exactly one match.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, thiserror::Error)]
#[non_exhaustive]
pub enum MatchErr {
    /// A report contained no matches.
    #[error("no executable matched app {app_id}")]
    NotFound {
        /// Steam app ID requested by the query.
        app_id: NonZeroU32,
    },
    /// A report contained more than one match.
    #[error("{matches} executables matched app {app_id}")]
    Ambiguous {
        /// Steam app ID requested by the query.
        app_id: NonZeroU32,
        /// Number of matches retained by the report.
        matches: usize,
    },
}

/// Non-lossy result of one executable query.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct MatchReport {
    query: ExecutableQuery,
    searched_installations: usize,
    matches: Vec<ExecutableMatch>,
    scan_diagnostics: Arc<[Diagnostic]>,
    query_diagnostics: Vec<Diagnostic>,
}

impl MatchReport {
    fn new(
        query: &ExecutableQuery,
        searched_installations: usize,
        matches: Vec<ExecutableMatch>,
        scan_diagnostics: Arc<[Diagnostic]>,
        query_diagnostics: Vec<Diagnostic>,
    ) -> Self {
        debug_assert!(
            matches.len() <= searched_installations,
            "a query cannot match more installs than it searched ({} matches, {searched_installations} searched)",
            matches.len()
        );
        debug_assert!(
            matches
                .iter()
                .all(|matched| matched.app.app_id() == query.app_id()),
            "every executable match must belong to queried app {}",
            query.app_id()
        );
        debug_assert!(
            matches.iter().all(|matched| query
                .candidates()
                .any(|candidate| candidate == matched.matched_candidate())),
            "every executable match must retain one of the query's validated candidates"
        );
        debug_assert!(
            matches.iter().all(|matched| matched
                .executable()
                .path()
                .starts_with(matched.app().path())),
            "every executable match must remain confined to its app installation"
        );
        Self {
            query: query.clone(),
            searched_installations,
            matches,
            scan_diagnostics,
            query_diagnostics,
        }
    }

    /// Query evaluated to produce this report.
    #[must_use]
    pub const fn query(&self) -> &ExecutableQuery {
        &self.query
    }

    /// Number-of-matches classification derived from retained matches.
    #[must_use]
    pub fn cardinality(&self) -> MatchCardinality {
        match self.matches.len() {
            0 => MatchCardinality::None,
            1 => MatchCardinality::One,
            _ => MatchCardinality::Many,
        }
    }

    /// Whether failures may have hidden additional matches.
    #[must_use]
    pub fn completeness(&self) -> MatchCompleteness {
        if self
            .diagnostics()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        {
            MatchCompleteness::Partial
        } else {
            MatchCompleteness::Complete
        }
    }

    /// Number of app installations whose IDs matched the query.
    #[must_use]
    pub const fn searched_installations(&self) -> usize {
        self.searched_installations
    }

    /// Every validated executable match.
    #[must_use]
    pub fn matches(&self) -> &[ExecutableMatch] {
        &self.matches
    }

    /// Diagnostics inherited from the underlying discovery scan.
    #[must_use]
    pub fn scan_diagnostics(&self) -> &[Diagnostic] {
        &self.scan_diagnostics
    }

    /// Diagnostics produced while evaluating this query.
    #[must_use]
    pub fn query_diagnostics(&self) -> &[Diagnostic] {
        &self.query_diagnostics
    }

    /// Iterate over inherited scan diagnostics followed by query diagnostics.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.scan_diagnostics
            .iter()
            .chain(self.query_diagnostics.iter())
    }

    /// Borrow the only match, or return a cardinality error.
    ///
    /// Query diagnostics remain available on this report when the conversion
    /// fails or the match is based on a partial scan.
    pub fn unique(&self) -> Result<&ExecutableMatch, MatchErr> {
        match self.matches.as_slice() {
            [matched] => Ok(matched),
            [] => Err(MatchErr::NotFound {
                app_id: self.query.app_id(),
            }),
            matches => Err(MatchErr::Ambiguous {
                app_id: self.query.app_id(),
                matches: matches.len(),
            }),
        }
    }
}

impl serde::Serialize for MatchReport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(serde::Serialize)]
        struct SerializableMatchReport<'a> {
            query: &'a ExecutableQuery,
            cardinality: MatchCardinality,
            completeness: MatchCompleteness,
            searched_installations: usize,
            matches: &'a [ExecutableMatch],
            diagnostics: Vec<&'a Diagnostic>,
        }

        serde::Serialize::serialize(
            &SerializableMatchReport {
                query: self.query(),
                cardinality: self.cardinality(),
                completeness: self.completeness(),
                searched_installations: self.searched_installations(),
                matches: self.matches(),
                diagnostics: self.diagnostics().collect(),
            },
            serializer,
        )
    }
}

impl SteamInventory {
    /// Resolve a query against every installation with the requested app ID.
    ///
    /// Use [`Discovery::resolve_executable`] when scan diagnostics should also
    /// be carried into the match report.
    #[must_use]
    pub fn resolve_executable(&self, query: &ExecutableQuery) -> MatchReport {
        self.resolve_executable_with(query, Arc::default())
    }

    /// Resolve several queries in input order without reloading manifests.
    pub fn resolve_all<'a, I: IntoIterator<Item = &'a ExecutableQuery>>(
        &self,
        queries: I,
    ) -> Vec<MatchReport> {
        queries
            .into_iter()
            .map(|query| self.resolve_executable(query))
            .collect()
    }

    pub(crate) fn resolve_executable_with(
        &self,
        query: &ExecutableQuery,
        scan_diagnostics: Arc<[Diagnostic]>,
    ) -> MatchReport {
        let apps = self.apps_for(query.app_id()).collect::<Vec<_>>();
        let searched_installations = apps.len();
        let mut matches = Vec::new();
        let mut query_diagnostics = Vec::new();

        if apps.is_empty() {
            query_diagnostics.push(
                Diagnostic::warning(
                    DiscoveryStage::ResolveExecutable,
                    DiagnosticKind::NotFound,
                    format!(
                        "Steam app {} is not present in the loaded inventory",
                        query.app_id()
                    ),
                )
                .with_app_id(query.app_id()),
            );
        }

        for app in apps {
            match app.resolve_executable(query) {
                AppResolution::Matched(found, mut app_diagnostics) => {
                    query_diagnostics.append(&mut app_diagnostics);
                    matches.push(*found);
                }
                AppResolution::NotFound(mut app_diagnostics) => {
                    query_diagnostics.append(&mut app_diagnostics);
                    let candidates = query
                        .candidates()
                        .map(|candidate| candidate.path().display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    query_diagnostics.push(
                        Diagnostic::warning(
                            DiscoveryStage::ResolveExecutable,
                            DiagnosticKind::NotFound,
                            format!(
                                "app {} installation '{}' contains none of the requested executables: {candidates}",
                                query.app_id(),
                                app.path().display(),
                            ),
                        )
                        .with_path(app.path())
                        .with_app_id(query.app_id()),
                    );
                }
            }
        }

        MatchReport::new(
            query,
            searched_installations,
            matches,
            scan_diagnostics,
            query_diagnostics,
        )
    }
}

impl Discovery {
    /// Resolve a query while retaining every diagnostic from the underlying scan.
    #[must_use]
    pub fn resolve_executable(&self, query: &ExecutableQuery) -> MatchReport {
        self.inventory()
            .resolve_executable_with(query, self.shared_diagnostics())
    }

    /// Resolve several queries in input order against the same loaded inventory.
    pub fn resolve_all<'a, I: IntoIterator<Item = &'a ExecutableQuery>>(
        &self,
        queries: I,
    ) -> Vec<MatchReport> {
        queries
            .into_iter()
            .map(|query| self.resolve_executable(query))
            .collect()
    }
}

enum AppResolution {
    Matched(Box<ExecutableMatch>, Vec<Diagnostic>),
    NotFound(Vec<Diagnostic>),
}

impl AppInstall {
    fn resolve_executable(&self, query: &ExecutableQuery) -> AppResolution {
        let mut diagnostics = Vec::new();
        for candidate in query.candidates() {
            let path = self.path().join(candidate.path());
            let executable = match FilePresence::of(&path) {
                FilePresence::Missing => continue,
                FilePresence::Present(executable) => executable,
                FilePresence::Invalid(error) => {
                    let kind: DiagnosticKind = error.kind.into();
                    diagnostics.push(
                        Diagnostic::error(
                            DiscoveryStage::ResolveExecutable,
                            kind,
                            format!(
                                "app {} executable candidate is unusable: {error}",
                                query.app_id()
                            ),
                        )
                        .with_path(path)
                        .with_app_id(query.app_id()),
                    );
                    continue;
                }
            };
            if !executable.starts_with(self.path()) {
                diagnostics.push(
                    Diagnostic::error(
                        DiscoveryStage::ResolveExecutable,
                        DiagnosticKind::OutsideBoundary,
                        format!(
                            "{}: app {} executable resolves outside installation '{}' to '{}'",
                            path.display(),
                            query.app_id(),
                            self.path().display(),
                            executable.display(),
                        ),
                    )
                    .with_path(path.clone())
                    .with_app_id(query.app_id()),
                );
                continue;
            }
            return AppResolution::Matched(
                Box::new(ExecutableMatch {
                    app: self.clone(),
                    executable: ExecutablePath(executable),
                    matched_candidate: candidate.clone(),
                }),
                diagnostics,
            );
        }
        AppResolution::NotFound(diagnostics)
    }
}
