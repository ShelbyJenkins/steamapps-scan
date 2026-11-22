//! steamapps-scan - Validated paths
//! =================================
//!
//! Validated filesystem path roles used by discovery and executable queries.

use std::{
    fmt,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::file_version::{FileVersion, VersionErr};

macro_rules! path_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize)]
        #[serde(transparent)]
        pub struct $name(pub(super) PathBuf);

        impl $name {
            /// Return the validated path.
            pub fn path(&self) -> &Path {
                &self.0
            }

            /// Consume the wrapper and return the owned path.
            pub fn into_path(self) -> PathBuf {
                self.0
            }
        }

        impl AsRef<Path> for $name {
            fn as_ref(&self) -> &Path {
                self.path()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.display().fmt(formatter)
            }
        }
    };
}

path_type!(
    /// A validated Steam installation directory.
    SteamRoot
);

path_type!(
    /// A validated Steam library directory.
    LibraryRoot
);

path_type!(
    /// A canonical app installation directory resolved from a manifest.
    InstallDir
);

path_type!(
    /// A canonical `appmanifest_<id>.acf` path.
    ManifestPath
);

path_type!(
    /// A canonical executable contained by an app installation.
    ExecutablePath
);

impl ExecutablePath {
    /// Read this executable's Windows version resource.
    ///
    /// A plain read of the file on disk: nothing is executed. Reports
    /// `Ok(None)` when the executable carries no version resource, which is
    /// ordinary rather than exceptional.
    ///
    /// See [`FileVersion`] for why the string records and the fixed record
    /// are reported separately and why neither is parsed.
    pub fn version(&self) -> Result<Option<FileVersion>, VersionErr> {
        FileVersion::read(self.path())
    }
}

path_type!(
    /// A validated, installation-relative executable candidate.
    RelativeExecutablePath
);

/// Failure to construct a validated Steam or library root manually.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, thiserror::Error)]
#[error("{message}")]
#[non_exhaustive]
pub struct ValidationErr {
    /// Programmatic validation failure category.
    pub kind: ValidationErrKind,
    /// Path that failed validation.
    pub path: PathBuf,
    /// Human-readable failure detail.
    pub message: String,
}

/// Programmatic category for a validated-path construction failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ValidationErrKind {
    /// The target or one of its required children does not exist.
    NotFound,
    /// The filesystem object has the wrong type for its role.
    InvalidType,
    /// The candidate exists but does not resemble the requested Steam role.
    NotSteamLayout,
    /// The filesystem could not be inspected for another reason.
    Io,
}

impl ValidationErr {
    pub(super) fn new(kind: ValidationErrKind, path: &Path, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

impl SteamRoot {
    pub(super) const LIBRARY_FOLDERS_FILE: &str = "libraryfolders.vdf";

    /// Validate a caller-provided Steam installation directory.
    ///
    /// The directory must contain `steamapps` and either
    /// `libraryfolders.vdf` or at least one `appmanifest_<id>.acf`-shaped
    /// directory entry. Manifest contents are validated during inventory scan.
    pub fn from_dir(path: impl AsRef<Path>) -> Result<Self, ValidationErr> {
        let path = path.as_ref();
        let root = canonical_directory(path, "Steam root")?;
        let steamapps = root.join("steamapps");
        canonical_directory(&steamapps, "Steam apps directory")?;

        let library_folders = FilePresence::of(&steamapps.join(Self::LIBRARY_FOLDERS_FILE));
        let entries = std::fs::read_dir(&steamapps).map_err(|error| {
            ValidationErr::new(
                ValidationErrKind::Io,
                &steamapps,
                format!(
                    "{}: could not inspect Steam apps directory: {error}",
                    steamapps.display()
                ),
            )
        })?;
        let mut has_manifest = false;
        for entry in entries {
            let entry = entry.map_err(|error| {
                ValidationErr::new(
                    ValidationErrKind::Io,
                    &steamapps,
                    format!(
                        "{}: could not inspect Steam apps directory entry: {error}",
                        steamapps.display()
                    ),
                )
            })?;
            has_manifest |= ManifestPath::app_id_in_file_name(&entry.file_name()).is_some();
        }

        // A manifest is enough to accept the root; the inventory scan then
        // reports an unusable libraryfolders.vdf as its own diagnostic.
        if has_manifest {
            return Ok(Self(root));
        }
        match library_folders {
            FilePresence::Present(_) => Ok(Self(root)),
            // Without a manifest, the unusable file is why the root fails.
            FilePresence::Invalid(error) => Err(error),
            FilePresence::Missing => Err(ValidationErr::new(
                ValidationErrKind::NotSteamLayout,
                &root,
                format!(
                    "{}: Steam apps directory contains neither {} nor an appmanifest_<id>.acf file",
                    steamapps.display(),
                    Self::LIBRARY_FOLDERS_FILE,
                ),
            )),
        }
    }
}

impl TryFrom<PathBuf> for SteamRoot {
    type Error = ValidationErr;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Self::from_dir(path)
    }
}

impl LibraryRoot {
    /// Validate a caller-provided Steam library directory.
    pub fn from_dir(path: impl AsRef<Path>) -> Result<Self, ValidationErr> {
        let path = path.as_ref();
        let root = canonical_directory(path, "Steam library root")?;
        canonical_directory(&root.join("steamapps"), "Steam library apps directory")?;
        Ok(Self(root))
    }
}

impl TryFrom<PathBuf> for LibraryRoot {
    type Error = ValidationErr;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Self::from_dir(path)
    }
}

pub enum FilePresence {
    Missing,
    Present(PathBuf),
    Invalid(ValidationErr),
}

impl FilePresence {
    /// Classify one path as a missing, canonical regular file, or unusable
    /// filesystem object, distinguishing absence from inspection failure.
    pub fn of(path: &Path) -> Self {
        match std::fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => match dunce::canonicalize(path) {
                Ok(canonical) => Self::Present(canonical),
                Err(error) => Self::Invalid(ValidationErr::new(
                    ValidationErrKind::Io,
                    path,
                    format!("{}: file cannot be resolved: {error}", path.display()),
                )),
            },
            Ok(_) => Self::Invalid(ValidationErr::new(
                ValidationErrKind::InvalidType,
                path,
                format!("{}: path is not a regular file", path.display()),
            )),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                match std::fs::symlink_metadata(path) {
                    Ok(_) => Self::Invalid(ValidationErr::new(
                        ValidationErrKind::NotFound,
                        path,
                        format!(
                            "{}: file exists but its symlink target cannot be resolved",
                            path.display()
                        ),
                    )),
                    Err(link_error) if link_error.kind() == ErrorKind::NotFound => Self::Missing,
                    Err(link_error) => Self::Invalid(ValidationErr::new(
                        ValidationErrKind::Io,
                        path,
                        format!("{}: could not inspect file: {link_error}", path.display()),
                    )),
                }
            }
            Err(error) => Self::Invalid(ValidationErr::new(
                ValidationErrKind::Io,
                path,
                format!("{}: could not inspect file: {error}", path.display()),
            )),
        }
    }
}

pub fn canonical_directory(path: &Path, role: &str) -> Result<PathBuf, ValidationErr> {
    let canonical = dunce::canonicalize(path).map_err(|error| {
        ValidationErr::new(
            if error.kind() == ErrorKind::NotFound {
                ValidationErrKind::NotFound
            } else {
                ValidationErrKind::Io
            },
            path,
            format!(
                "{}: {role} does not exist or cannot be resolved: {error}",
                path.display()
            ),
        )
    })?;
    if !canonical.is_dir() {
        return Err(ValidationErr::new(
            ValidationErrKind::InvalidType,
            path,
            format!("{}: {role} is not a directory", path.display()),
        ));
    }
    Ok(canonical)
}

impl ManifestPath {
    /// Parse the app ID out of an `appmanifest_<id>.acf` file name, or
    /// `None` when the name does not follow the manifest convention.
    pub(crate) fn app_id_in_file_name(name: &std::ffi::OsStr) -> Option<u32> {
        let name = name.to_str()?;
        name.strip_prefix("appmanifest_")?
            .strip_suffix(".acf")?
            .parse()
            .ok()
    }
}
