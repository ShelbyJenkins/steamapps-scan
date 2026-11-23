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
    /// A validated Steam root: a Steam or steamcmd installation directory, or
    /// any directory whose `steamapps` holds app manifests, such as a folder
    /// filled by steamcmd's `force_install_dir`.
    SteamRoot
);

/// A validated Steam library directory.
///
/// Serializes as an object holding its `path` and `layout`.
#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize)]
pub struct LibraryRoot {
    pub(super) path: PathBuf,
    pub(super) layout: LibraryLayout,
}

/// Where a library keeps the apps its manifests describe.
///
/// A library that a `libraryfolders.vdf` registers is always `Common`. Only a
/// root that none registers can be `ForceInstallDir`, and only while it holds
/// none of the files Steam keeps in its own libraries. An install forced into
/// a folder that does hold them, such as steamcmd's own root, is therefore not
/// found: each of its manifests is reported as a missing `steamapps/common`
/// installation. Supporting it would take a layout decision per manifest,
/// worth making if such installs turn out to be common.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LibraryLayout {
    /// Each app in its own `steamapps/common/<installdir>`.
    Common,
    /// Every app directly in the library directory, as steamcmd's
    /// `force_install_dir` leaves it. Apps forced into one directory share it,
    /// and they count as installed only while it holds something besides
    /// `steamapps`.
    ForceInstallDir,
}

path_type!(
    /// A canonical app installation directory: the manifest's
    /// `steamapps/common/<installdir>`, or the library itself in the
    /// `force_install_dir` layout.
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

    /// Validate a caller-provided Steam root.
    ///
    /// The directory must contain `steamapps` and either
    /// `libraryfolders.vdf` or at least one `appmanifest_<id>.acf`-shaped
    /// directory entry. Manifest contents are validated during inventory scan.
    pub fn from_dir(path: impl AsRef<Path>) -> Result<Self, ValidationErr> {
        let path = path.as_ref();
        let root = canonical_directory(path, "Steam root")?;
        let steamapps = root.join(LibraryRoot::APPS_DIR);
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
    /// Directory under every library, Steam roots included, that holds app
    /// manifests.
    pub(super) const APPS_DIR: &str = "steamapps";
    /// Directory under `steamapps` that holds one directory per app in the
    /// common layout.
    pub(super) const COMMON_DIR: &str = "common";
    /// Marker Steam writes into each library it registers, though a Windows
    /// root's own library can lack it.
    const LIBRARY_FOLDER_FILE: &str = "libraryfolder.vdf";

    /// Validate a Steam library directory as a `libraryfolders.vdf` slot names
    /// it. Steam registered it, so its layout is [`LibraryLayout::Common`].
    ///
    /// Fails when the directory or its `steamapps` is missing, cannot be
    /// resolved, or is not a directory.
    pub fn from_dir(path: impl AsRef<Path>) -> Result<Self, ValidationErr> {
        let path = path.as_ref();
        let root = canonical_directory(path, "Steam library root")?;
        canonical_directory(&root.join(Self::APPS_DIR), "Steam library apps directory")?;
        Ok(Self {
            path: root,
            layout: LibraryLayout::Common,
        })
    }

    /// Return the validated path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Consume the wrapper and return the owned path.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }

    /// Where this library keeps its apps.
    #[must_use]
    pub const fn layout(&self) -> LibraryLayout {
        self.layout
    }
}

impl TryFrom<PathBuf> for LibraryRoot {
    type Error = ValidationErr;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Self::from_dir(path)
    }
}

impl AsRef<Path> for LibraryRoot {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl fmt::Display for LibraryRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.path.display().fmt(formatter)
    }
}

impl LibraryLayout {
    /// The layout of a root that no `libraryfolders.vdf` registers.
    ///
    /// Steam keeps `libraryfolders.vdf` in every root, `libraryfolder.vdf` in
    /// the libraries it registers, and `common` in any library it has
    /// installed into; steamcmd's `force_install_dir` leaves none of them, and
    /// any one of them means `Common`. A marker that cannot be inspected
    /// decides nothing, so it fails the probe, at that marker's path, only
    /// when no other marker is found.
    pub(super) fn of(root: &Path) -> Result<Self, ValidationErr> {
        let steamapps = root.join(LibraryRoot::APPS_DIR);
        let mut failure = None;
        for marker in [
            steamapps.join(SteamRoot::LIBRARY_FOLDERS_FILE),
            root.join(LibraryRoot::LIBRARY_FOLDER_FILE),
            steamapps.join(LibraryRoot::COMMON_DIR),
        ] {
            match std::fs::symlink_metadata(&marker) {
                Ok(_) => return Ok(Self::Common),
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    failure.get_or_insert_with(|| {
                        ValidationErr::new(
                            ValidationErrKind::Io,
                            &marker,
                            format!(
                                "{}: could not inspect Steam library file: {error}",
                                marker.display()
                            ),
                        )
                    });
                }
            }
        }
        failure.map_or(Ok(Self::ForceInstallDir), Err)
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
    // `Path::is_dir` reports every inspection failure as "not a directory";
    // reading the metadata keeps an I/O failure classified as one.
    match std::fs::metadata(&canonical) {
        Ok(metadata) if metadata.is_dir() => Ok(canonical),
        Ok(_) => Err(ValidationErr::new(
            ValidationErrKind::InvalidType,
            path,
            format!("{}: {role} is not a directory", path.display()),
        )),
        Err(error) => Err(ValidationErr::new(
            if error.kind() == ErrorKind::NotFound {
                ValidationErrKind::NotFound
            } else {
                ValidationErrKind::Io
            },
            path,
            format!("{}: {role} could not be inspected: {error}", path.display()),
        )),
    }
}

/// Whether canonical `path` lies at or under canonical `base`.
///
/// `dunce` keeps the verbatim `\\?\` prefix on a path too long for the plain
/// form, so two canonical paths under one directory can differ in prefix
/// alone. Prefixes are compared by the drive or share they name.
pub fn canonical_starts_with(path: &Path, base: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::{
            ffi::OsString,
            path::{Component, Prefix},
        };

        let identity = |component: Component<'_>| match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::VerbatimDisk(drive) | Prefix::Disk(drive) => {
                    OsString::from(format!("{}:", char::from(drive.to_ascii_uppercase())))
                }
                Prefix::VerbatimUNC(server, share) | Prefix::UNC(server, share) => {
                    let mut unc = OsString::from(r"\\");
                    unc.push(server);
                    unc.push(r"\");
                    unc.push(share);
                    unc
                }
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => prefix.as_os_str().to_owned(),
            },
            other @ (Component::RootDir
            | Component::CurDir
            | Component::ParentDir
            | Component::Normal(_)) => other.as_os_str().to_owned(),
        };
        let mut path_components = path.components().map(identity);
        base.components()
            .map(identity)
            .all(|component| path_components.next() == Some(component))
    }
    #[cfg(not(windows))]
    path.starts_with(base)
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
