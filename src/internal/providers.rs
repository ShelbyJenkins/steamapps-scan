//! steamapps-scan - Automatic root providers
//! ==========================================
//!
//! Per-platform sources of Steam root candidates. Each provider records
//! every attempt in the shared [`RootCollector`], so a wrong guess costs a
//! ledger entry rather than a false root: candidates are validated by
//! `SteamRoot::from_dir` before acceptance.
//!
//! Windows asks the registry, enumerates running `steam.exe` processes, and
//! falls back to environment-derived and conventional install paths. Linux
//! and macOS have no registry; their sources are conventional locations plus
//! the pointer symlinks the Steam client maintains under `~/.steam`, which
//! serve the same role the registry serves on Windows and cover custom
//! install locations. Platforms outside these three record a single
//! unsupported attempt.

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::scan::RejectionRecord;
#[cfg(not(windows))]
use super::scan::RootCollector;
#[cfg(not(windows))]
use crate::report::DiscoverySource;

#[cfg(target_os = "linux")]
pub(super) fn automatic_roots(collector: &mut RootCollector) {
    xdg_data_home_candidate(collector);
    home_candidates(
        collector,
        &[
            ".local/share/Steam",
            ".steam/steam",
            ".steam/root",
            ".steam/debian-installation",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            ".var/app/com.valvesoftware.Steam/.steam/steam",
            ".var/app/com.valvesoftware.Steam/.steam/root",
            "snap/steam/common/.local/share/Steam",
            "snap/steam/common/.steam/steam",
            "snap/steam/common/.steam/root",
        ],
    );
}

#[cfg(target_os = "macos")]
pub(super) fn automatic_roots(collector: &mut RootCollector) {
    home_candidates(collector, &["Library/Application Support/Steam"]);
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn automatic_roots(collector: &mut RootCollector) {
    collector.unsupported(
        DiscoverySource::Conventional {
            description: std::env::consts::OS.to_string(),
        },
        "automatic Steam discovery supports Windows, Linux, and macOS; use \
         Discovery::discover_from on this platform",
    );
}

/// `$XDG_DATA_HOME/Steam`, honored only when the variable holds an absolute
/// path: the XDG base directory specification requires relative values to be
/// ignored.
#[cfg(target_os = "linux")]
fn xdg_data_home_candidate(collector: &mut RootCollector) {
    let source = DiscoverySource::Environment {
        variable: "XDG_DATA_HOME".into(),
    };
    match std::env::var_os("XDG_DATA_HOME") {
        Some(value) if !value.is_empty() => {
            let base = PathBuf::from(value);
            if base.is_absolute() {
                collector.candidate(source, base.join("Steam"), RejectionRecord::AttemptDetail);
            } else {
                collector.rejected_without_candidate(
                    source,
                    "value is relative; the XDG base directory specification requires \
                     relative values to be ignored",
                );
            }
        }
        _ => collector.not_present(source, "environment variable is not set"),
    }
}

/// Home-relative conventional candidates, recorded per path so the ledger
/// shows which convention produced each root. An unset `HOME` collapses to
/// one attempt covering all of them.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn home_candidates(collector: &mut RootCollector, relative_paths: &[&str]) {
    let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) else {
        collector.not_present(
            DiscoverySource::Environment {
                variable: "HOME".into(),
            },
            "environment variable is not set",
        );
        return;
    };
    let home = PathBuf::from(home);
    for relative in relative_paths {
        collector.candidate(
            DiscoverySource::Conventional {
                description: format!("~/{relative}"),
            },
            home.join(relative),
            RejectionRecord::AttemptDetail,
        );
    }
}

#[cfg(windows)]
pub(super) use windows::automatic_roots;

#[cfg(windows)]
mod windows {
    use std::{io::ErrorKind, path::PathBuf};

    use super::{
        super::scan::{RejectionRecord, RootCollector},
        process,
    };
    use crate::report::DiscoverySource;

    const STEAM_IMAGE_NAME: &str = "steam.exe";

    pub fn automatic_roots(collector: &mut RootCollector) {
        registry_candidates(collector);
        running_steam_candidates(collector);
        default_candidates(collector);
    }

    fn registry_candidates(collector: &mut RootCollector) {
        use winreg::{
            RegKey,
            enums::{
                HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
            },
        };

        let locations = [
            (
                HKEY_CURRENT_USER,
                "HKCU",
                r"Software\Valve\Steam",
                "SteamPath",
                KEY_READ,
                "default",
            ),
            (
                HKEY_LOCAL_MACHINE,
                "HKLM",
                r"SOFTWARE\Valve\Steam",
                "InstallPath",
                KEY_READ | KEY_WOW64_32KEY,
                "32-bit",
            ),
            (
                HKEY_LOCAL_MACHINE,
                "HKLM",
                r"SOFTWARE\Valve\Steam",
                "InstallPath",
                KEY_READ | KEY_WOW64_64KEY,
                "64-bit",
            ),
        ];

        for (hive, hive_name, key_name, value_name, flags, view) in locations {
            let source = DiscoverySource::Registry {
                hive: hive_name.into(),
                key: key_name.into(),
                value: value_name.into(),
                view: view.into(),
            };
            let key = match RegKey::predef(hive).open_subkey_with_flags(key_name, flags) {
                Ok(key) => key,
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    collector.not_present(source, "registry key is not present");
                    continue;
                }
                Err(error) => {
                    collector.failed(source, format!("could not open registry key: {error}"));
                    continue;
                }
            };
            let value: String = match key.get_value(value_name) {
                Ok(value) => value,
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    collector.not_present(source, "registry value is not present");
                    continue;
                }
                Err(error) => {
                    collector.failed(source, format!("could not read registry value: {error}"));
                    continue;
                }
            };
            let trimmed = value.trim();
            if trimmed.is_empty() {
                collector.rejected_without_candidate(source, "registry value is empty");
            } else {
                collector.candidate(
                    source,
                    PathBuf::from(trimmed),
                    RejectionRecord::AttemptDetail,
                );
            }
        }
    }

    /// A running Steam's own executable location is ground truth even when
    /// the registry is stale or absent, which is exactly the portable-install
    /// case the other sources miss.
    fn running_steam_candidates(collector: &mut RootCollector) {
        let source = DiscoverySource::Process {
            executable: STEAM_IMAGE_NAME.into(),
        };
        match process::executable_paths(STEAM_IMAGE_NAME) {
            Err(detail) => collector.failed(
                source,
                format!("could not enumerate running processes: {detail}"),
            ),
            Ok(processes) if processes.matched == 0 && processes.failures.is_empty() => {
                collector.not_present(source, "steam.exe is not running");
            }
            Ok(processes) => {
                for path in processes.paths {
                    if let Some(directory) = path.parent() {
                        collector.candidate(
                            source.clone(),
                            directory.to_path_buf(),
                            RejectionRecord::AttemptDetail,
                        );
                    } else {
                        collector.failed(
                            source.clone(),
                            format!(
                                "queried steam.exe path '{}' has no parent directory",
                                path.display()
                            ),
                        );
                    }
                }
                for failure in processes.failures {
                    collector.failed(source.clone(), failure);
                }
            }
        }
    }

    fn default_candidates(collector: &mut RootCollector) {
        for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
            let source = DiscoverySource::Environment {
                variable: variable.into(),
            };
            match std::env::var_os(variable) {
                Some(value) if !value.is_empty() => {
                    collector.candidate(
                        source,
                        PathBuf::from(value).join("Steam"),
                        RejectionRecord::AttemptDetail,
                    );
                }
                _ => collector.not_present(source, "environment variable is not set"),
            }
        }
        for candidate in [
            PathBuf::from(r"C:\Program Files (x86)\Steam"),
            PathBuf::from(r"C:\Program Files\Steam"),
        ] {
            collector.candidate(
                DiscoverySource::Conventional {
                    description: candidate.display().to_string(),
                },
                candidate,
                RejectionRecord::AttemptDetail,
            );
        }
    }
}

/// Process enumeration through the Tool Help snapshot API. Declared
/// directly rather than pulled from a bindings crate for the same reason as
/// the version-resource calls in `file_version`: a handful of calls with
/// stable signatures against a library present on every Windows
/// installation.
#[cfg(windows)]
mod process {
    use std::{os::windows::ffi::OsStringExt, path::PathBuf};

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> isize;
        fn Process32FirstW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        fn QueryFullProcessImageNameW(
            process: isize,
            flags: u32,
            exe_name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(object: isize) -> i32;
        fn GetLastError() -> u32;
    }

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const INVALID_HANDLE_VALUE: isize = -1;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const ERROR_NO_MORE_FILES: u32 = 18;

    /// Longest path the API can report: the extended-length limit in UTF-16
    /// units, comfortably above `MAX_PATH`.
    const IMAGE_PATH_CAPACITY: usize = 32 * 1024;

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "ProcessEntry32W is a fixed few hundred bytes"
    )]
    const ENTRY_SIZE: u32 = size_of::<ProcessEntry32W>() as u32;

    /// Owned kernel handle closed on drop.
    struct Handle(isize);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the wrapped value was returned open by the API that
            // produced it and is closed exactly once, here.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub(super) struct RunningProcesses {
        /// Full executable paths of the matching processes whose path could
        /// be queried.
        pub(super) paths: Vec<PathBuf>,
        /// Number of processes whose image name matched, whether or not a
        /// path could be queried for them.
        pub(super) matched: usize,
        /// Enumeration or path-query failures retained alongside successful
        /// paths so one readable process cannot hide an unreadable sibling.
        pub(super) failures: Vec<String>,
    }

    /// Enumerate running processes and report the executable paths of every
    /// one whose image name matches `image_name` (ASCII, case-insensitive).
    pub(super) fn executable_paths(image_name: &str) -> Result<RunningProcesses, String> {
        // SAFETY: the flag requests a process snapshot; the process id
        // argument is documented as ignored for that flag.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            // SAFETY: no intervening Windows call has run since the failure.
            let code = unsafe { GetLastError() };
            return Err(format!(
                "CreateToolhelp32Snapshot failed with OS error {code}"
            ));
        }
        let snapshot = Handle(snapshot);

        let mut entry = ProcessEntry32W {
            dw_size: ENTRY_SIZE,
            cnt_usage: 0,
            th32_process_id: 0,
            th32_default_heap_id: 0,
            th32_module_id: 0,
            cnt_threads: 0,
            th32_parent_process_id: 0,
            pc_pri_class_base: 0,
            dw_flags: 0,
            sz_exe_file: [0; 260],
        };

        // SAFETY: `snapshot` is a live snapshot handle and `entry.dw_size`
        // is initialized to the structure size, as the API requires.
        let mut has_entry = unsafe { Process32FirstW(snapshot.0, &raw mut entry) };
        if has_entry == 0 {
            // SAFETY: no intervening Windows call has run since the failure.
            let code = unsafe { GetLastError() };
            if code == ERROR_NO_MORE_FILES {
                return Ok(RunningProcesses {
                    paths: Vec::new(),
                    matched: 0,
                    failures: Vec::new(),
                });
            }
            return Err(format!("Process32FirstW failed with OS error {code}"));
        }

        let mut matched = 0usize;
        let mut paths = Vec::new();
        let mut failures = Vec::new();
        while has_entry != 0 {
            let name_units = entry
                .sz_exe_file
                .split(|unit| *unit == 0)
                .next()
                .unwrap_or(&[]);
            // Lossy conversion cannot create a false match against an ASCII
            // image name: replacement characters are not ASCII.
            let name = String::from_utf16_lossy(name_units);
            if name.eq_ignore_ascii_case(image_name) {
                matched = matched.checked_add(1).ok_or_else(|| {
                    "steam.exe match count exceeded the addressable process count".to_string()
                })?;
                match full_image_path(entry.th32_process_id) {
                    Ok(path) => paths.push(path),
                    Err(detail) => failures.push(detail),
                }
            }
            // SAFETY: `snapshot` and `entry` remain valid; the API advances
            // the snapshot cursor and rewrites `entry`.
            has_entry = unsafe { Process32NextW(snapshot.0, &raw mut entry) };
        }
        // SAFETY: no intervening Windows call has run since Process32NextW
        // returned false.
        let code = unsafe { GetLastError() };
        if code != ERROR_NO_MORE_FILES {
            failures.push(format!("Process32NextW failed with OS error {code}"));
        }

        debug_assert!(
            paths.len() <= matched,
            "every queried path must come from a matched process ({} paths, {matched} matched)",
            paths.len()
        );
        Ok(RunningProcesses {
            paths,
            matched,
            failures,
        })
    }

    /// Query one process's full executable path, retaining why a stale or
    /// protected process could not be inspected.
    fn full_image_path(process_id: u32) -> Result<PathBuf, String> {
        // SAFETY: limited query access against an id from the snapshot; a
        // stale or protected id fails cleanly with a null handle.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if process == 0 {
            // SAFETY: no intervening Windows call has run since OpenProcess
            // returned a null handle.
            let code = unsafe { GetLastError() };
            return Err(format!(
                "could not open steam.exe process {process_id} for path query: OS error {code}"
            ));
        }
        let process = Handle(process);

        let mut buffer = vec![0u16; IMAGE_PATH_CAPACITY];
        let mut length = u32::try_from(buffer.len()).map_err(|_| {
            format!(
                "path buffer of {} UTF-16 units exceeds the u32 size QueryFullProcessImageNameW accepts",
                buffer.len()
            )
        })?;
        // SAFETY: `length` is taken from `buffer.len()` just above, so the
        // call may write at most `length` UTF-16 units into `buffer`, and
        // `length` is a valid in/out size for the call.
        let ok = unsafe {
            QueryFullProcessImageNameW(process.0, 0, buffer.as_mut_ptr(), &raw mut length)
        };
        if ok == 0 {
            // SAFETY: no intervening Windows call has run since the query
            // failed.
            let code = unsafe { GetLastError() };
            return Err(format!(
                "could not query steam.exe process {process_id} path: OS error {code}"
            ));
        }
        let length = usize::try_from(length).map_err(|_| {
            format!("steam.exe process {process_id} returned an unrepresentable path length")
        })?;
        let written = buffer.get(..length).ok_or_else(|| {
            format!(
                "steam.exe process {process_id} returned path length {length} beyond buffer capacity {}",
                buffer.len()
            )
        })?;
        if written.is_empty() {
            return Err(format!(
                "steam.exe process {process_id} returned an empty executable path"
            ));
        }
        // Lossless: from_wide preserves ill-formed UTF-16 in the OsString.
        Ok(PathBuf::from(std::ffi::OsString::from_wide(written)))
    }
}
