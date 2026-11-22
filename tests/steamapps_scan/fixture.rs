//! steamapps-scan - Test fixtures
//! ===============================
//!
//! Temporary Steam trees, directory links, and CLI child processes shared by
//! every test module.

use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use steamapps_scan::Discovery;

pub const fn fixture_app_id(raw: u32) -> NonZeroU32 {
    NonZeroU32::new(raw).expect("fixture app ID is nonzero")
}

/// A process-unique temporary directory, removed on drop.
pub struct TempTree(PathBuf);

impl TempTree {
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);

        let path = std::env::temp_dir().join(format!(
            "steamapps-scan-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).expect("create temporary tree");
        Self(dunce::canonicalize(path).expect("canonicalize temporary tree"))
    }

    pub fn root(&self) -> &Path {
        &self.0
    }

    pub fn dir(&self, relative: impl AsRef<Path>) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(&path).expect("create test directory");
        path
    }

    pub fn file(&self, relative: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create test file parent");
        }
        std::fs::write(&path, contents).expect("write test file");
        path
    }

    pub fn manifest(
        _filename_app_id: u32,
        manifest_app_id: u32,
        installdir: &str,
        name: &str,
    ) -> String {
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\"{manifest_app_id}\"\n\t\"name\"\t\"{name}\"\n\t\"installdir\"\t\"{installdir}\"\n}}\n",
        )
    }

    pub fn install_app(&self, app_id: u32, installdir: &str) -> PathBuf {
        Self::install_app_in(&self.0, app_id, installdir)
    }

    /// Install an app with an empty installation directory into the Steam
    /// root or library at `root`, which may lie anywhere, and return the
    /// installation directory.
    pub fn install_app_in(root: &Path, app_id: u32, installdir: &str) -> PathBuf {
        let steamapps = root.join("steamapps");
        let install = steamapps.join("common").join(installdir);
        std::fs::create_dir_all(&install).expect("create fixture installation");
        std::fs::write(
            steamapps.join(format!("appmanifest_{app_id}.acf")),
            Self::manifest(app_id, app_id, installdir, &format!("Test App {app_id}")),
        )
        .expect("write fixture manifest");
        install
    }

    pub fn install_executable(&self, installdir: &str, relative: &str) -> PathBuf {
        self.file(format!("steamapps/common/{installdir}/{relative}"), "")
    }

    /// Register each library in this root's `libraryfolders.vdf`, one slot
    /// apiece in order.
    pub fn library_folders(&self, libraries: &[&Path]) {
        let slots = libraries
            .iter()
            .enumerate()
            .map(|(slot, library)| {
                format!(
                    "\t\"{slot}\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n",
                    library.display().to_string().replace('\\', r"\\")
                )
            })
            .collect::<Vec<_>>()
            .concat();
        self.file(
            "steamapps/libraryfolders.vdf",
            format!("\"libraryfolders\"\n{{\n{slots}}}\n"),
        );
    }

    /// Create a directory link at `relative` pointing at `target`: a
    /// junction on Windows, which needs no privilege, and a symlink
    /// elsewhere.
    pub fn link_dir(&self, relative: impl AsRef<Path>, target: &Path) -> PathBuf {
        // Rebuilt from components so Windows gets native separators, which
        // `mklink` needs: it reads a forward slash as a switch.
        let link: PathBuf = self.0.join(relative).components().collect();
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).expect("create link parent");
        }
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(target)
                .output()
                .expect("run mklink");
            assert!(
                output.status.success(),
                "mklink /J failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &link).expect("create directory symlink");
        link
    }

    /// This root spelled with its final component upper-cased, and whether
    /// that spelling names the same directory, which it does exactly when
    /// the temporary directory's filesystem ignores case.
    pub fn case_variant(&self) -> (PathBuf, bool) {
        let name = self
            .0
            .file_name()
            .expect("temporary tree has a name")
            .to_string_lossy()
            .to_uppercase();
        let variant = self.0.with_file_name(name);
        assert_ne!(variant, self.0, "temporary tree names contain lowercase");
        let folds = variant.is_dir();
        (variant, folds)
    }

    pub fn discover(&self) -> Discovery {
        Discovery::discover_from([self.root()])
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One `steamapps-scan scan --json` run in a child process, so a test can
/// set the environment variables automatic discovery reads without touching
/// its own.
#[cfg(feature = "cli")]
pub struct CliScan {
    pub code: Option<i32>,
    pub report: serde_json::Value,
}

#[cfg(feature = "cli")]
impl CliScan {
    pub fn run(
        configure: impl FnOnce(&mut std::process::Command) -> &mut std::process::Command,
    ) -> Self {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_steamapps-scan"));
        command.args(["scan", "--json"]);
        configure(&mut command);
        let output = command.output().expect("run the steamapps-scan CLI");
        let report = serde_json::from_slice::<serde_json::Value>(&output.stdout);
        assert!(
            report.is_ok(),
            "scan --json printed no JSON report ({:?}); stderr: {}",
            report.as_ref().err(),
            String::from_utf8_lossy(&output.stderr)
        );
        Self {
            code: output.status.code(),
            report: report.unwrap(),
        }
    }

    /// The one attempt whose source has `field` set to `value`.
    pub fn attempt(&self, field: &str, value: &str) -> &serde_json::Value {
        let matching: Vec<_> = self.report["attempts"]
            .as_array()
            .expect("the report lists its attempts")
            .iter()
            .filter(|attempt| attempt["source"][field] == value)
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "expected one attempt from {field} '{value}': {:#}",
            self.report
        );
        matching[0]
    }

    /// Inventoried app IDs, in report order.
    pub fn app_ids(&self) -> Vec<u64> {
        self.report["inventory"]["apps"]
            .as_array()
            .expect("the report lists its apps")
            .iter()
            .map(|app| app["app_id"].as_u64().expect("app IDs are integers"))
            .collect()
    }
}
