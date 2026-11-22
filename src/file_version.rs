//! steamapps-scan - Executable version resource
//! =============================================
//!
//! Version strings read from a Windows executable's `VS_VERSIONINFO`
//! resource: the same data the Explorer property sheet shows, read from the
//! file on disk without running it.
//!
//! The resource is a Windows convention rather than anything app-specific,
//! and it is optional — plenty of shipped executables carry none.
//!
//! Two independent version records live in the resource and they routinely
//! disagree:
//!
//! - `VS_FIXEDFILEINFO`, four 16-bit integers, always structured.
//! - The `StringFileInfo` block, free text chosen by whoever built the
//!   binary.
//!
//! Neither can be derived from the other, and either may be absent while the
//! other is present. Observed on one machine: Arma Reforger publishes
//! `1.7.0.54` as a string while leaving the fixed record at `0.0.0.0`;
//! `BattlEye` publishes `1, 0, 0, 1`; Valve publishes `01.00.00.02`. A caller
//! wanting the version a vendor advertises wants the string, and must not
//! assume it parses as a dotted number.

use std::path::Path;

/// Version records read from one executable's version resource.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[non_exhaustive]
pub struct FileVersion {
    /// `ProductVersion` from the string block, exactly as the vendor wrote
    /// it. Not guaranteed to parse as a dotted number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_version: Option<String>,
    /// `FileVersion` from the string block, exactly as the vendor wrote it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_version: Option<String>,
    /// `ProductName` from the string block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_name: Option<String>,
    /// `CompanyName` from the string block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_name: Option<String>,
    /// The structured `VS_FIXEDFILEINFO` product version, when it is present
    /// and not all zeroes. Absent does not imply the strings are absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed_product_version: Option<[u16; 4]>,
}

impl FileVersion {
    /// Whether every record came back empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.product_version.is_none()
            && self.file_version.is_none()
            && self.product_name.is_none()
            && self.company_name.is_none()
            && self.fixed_product_version.is_none()
    }

    /// Read the version resource of the executable at `path`.
    ///
    /// Version resources are a PE concept, so non-Windows targets report
    /// `Ok(None)` rather than failing and callers stay cfg-free.
    #[cfg_attr(
        not(windows),
        expect(
            clippy::unnecessary_wraps,
            clippy::missing_const_for_fn,
            reason = "matches the Windows signature so callers need no cfg"
        )
    )]
    pub(crate) fn read(path: &Path) -> Result<Option<Self>, VersionErr> {
        #[cfg(windows)]
        {
            windows::read(path)
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Ok(None)
        }
    }
}

/// Failure to read an executable's version resource.
///
/// An executable with no version resource is not an error: reading reports
/// `Ok(None)`.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{}: could not read the version resource: {message}", path.display())]
#[non_exhaustive]
pub struct VersionErr {
    /// Executable that could not be read.
    pub path: std::path::PathBuf,
    /// Failure detail, including the OS error code when there was one.
    pub message: String,
}

#[cfg(windows)]
impl VersionErr {
    fn new(path: &Path, message: impl Into<String>) -> Self {
        Self {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path};

    use super::{FileVersion, VersionErr};

    // Declared directly rather than pulled from a bindings crate: three
    // calls with stable signatures, against a library already present on
    // every Windows installation.
    #[link(name = "version")]
    unsafe extern "system" {
        fn GetFileVersionInfoSizeW(filename: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoW(
            filename: *const u16,
            handle: u32,
            len: u32,
            data: *mut c_void,
        ) -> i32;
        fn VerQueryValueW(
            block: *const c_void,
            sub_block: *const u16,
            buffer: *mut *mut c_void,
            len: *mut u32,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLastError() -> u32;
    }

    /// The resource type or data is absent — the file simply carries no
    /// version resource.
    const ERROR_RESOURCE_DATA_NOT_FOUND: u32 = 1812;
    const ERROR_RESOURCE_TYPE_NOT_FOUND: u32 = 1813;

    /// `VS_FIXEDFILEINFO` starts with this, letting a bogus block be
    /// rejected rather than reinterpreted.
    const VS_FFI_SIGNATURE: u32 = 0xFEEF_04BD;

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub(super) fn read(path: &Path) -> Result<Option<FileVersion>, VersionErr> {
        let filename = wide_path(path);
        let mut handle = 0u32;

        // SAFETY: `filename` is a NUL-terminated UTF-16 buffer that outlives
        // the call, and `handle` is a valid writable u32.
        let size = unsafe { GetFileVersionInfoSizeW(filename.as_ptr(), &raw mut handle) };
        if size == 0 {
            // SAFETY: no intervening Windows call has run since the failure.
            let code = unsafe { GetLastError() };
            return match code {
                ERROR_RESOURCE_DATA_NOT_FOUND | ERROR_RESOURCE_TYPE_NOT_FOUND | 0 => Ok(None),
                code => Err(VersionErr::new(
                    path,
                    format!("GetFileVersionInfoSizeW failed with OS error {code}"),
                )),
            };
        }

        let block_len = usize::try_from(size).map_err(|_| {
            VersionErr::new(
                path,
                format!("version resource size {size} cannot be represented on this target"),
            )
        })?;
        let mut block = vec![0u8; block_len];
        // SAFETY: `block` has exactly `size` writable bytes, which is the
        // length the same API just reported as required.
        let ok = unsafe {
            GetFileVersionInfoW(
                filename.as_ptr(),
                handle,
                size,
                block.as_mut_ptr().cast::<c_void>(),
            )
        };
        if ok == 0 {
            // SAFETY: no intervening Windows call has run since the failure.
            let code = unsafe { GetLastError() };
            return Err(VersionErr::new(
                path,
                format!("GetFileVersionInfoW failed with OS error {code}"),
            ));
        }
        let block = VersionBlock(block);

        let mut version = FileVersion {
            fixed_product_version: block.fixed_product_version(),
            ..FileVersion::default()
        };

        // The string block is keyed by language and code page. Vendors do not
        // agree on which they use, so the file's own translation table picks
        // it; falling back to US-English/Unicode covers resources that omit
        // the table.
        let translations = block.translations();
        for (language, code_page) in translations
            .iter()
            .copied()
            .chain(std::iter::once((0x0409, 0x04B0)))
        {
            if version.product_version.is_none() {
                version.product_version = block.string_value(language, code_page, "ProductVersion");
            }
            if version.file_version.is_none() {
                version.file_version = block.string_value(language, code_page, "FileVersion");
            }
            if version.product_name.is_none() {
                version.product_name = block.string_value(language, code_page, "ProductName");
            }
            if version.company_name.is_none() {
                version.company_name = block.string_value(language, code_page, "CompanyName");
            }
            if version.product_version.is_some()
                && version.file_version.is_some()
                && version.product_name.is_some()
                && version.company_name.is_some()
            {
                break;
            }
        }

        if version.is_empty() {
            return Ok(None);
        }
        Ok(Some(version))
    }

    /// How `VerQueryValueW` counts the length of the value it points at.
    #[derive(Clone, Copy)]
    enum ValueLength {
        /// Binary values such as `\` and `\VarFileInfo\Translation`.
        Bytes,
        /// String-table values.
        Utf16Units,
    }

    /// A version-information block filled by a successful
    /// `GetFileVersionInfoW`. Only `read` builds one, so `VerQueryValueW`
    /// only ever walks headers the OS wrote.
    struct VersionBlock(Vec<u8>);

    impl VersionBlock {
        /// Return the bytes of one sub-block's value, or `None` when the
        /// value is absent or does not lie entirely inside this block.
        fn query(&self, sub_block: &str, length: ValueLength) -> Option<&[u8]> {
            let sub_block = wide(sub_block);
            let mut buffer: *mut c_void = std::ptr::null_mut();
            let mut len = 0u32;
            // SAFETY: `self.0` was filled by a successful GetFileVersionInfoW,
            // the only way a VersionBlock is built, `sub_block` is
            // NUL-terminated, and both out-params are valid writable
            // locations.
            let ok = unsafe {
                VerQueryValueW(
                    self.0.as_ptr().cast::<c_void>(),
                    sub_block.as_ptr(),
                    &raw mut buffer,
                    &raw mut len,
                )
            };
            if ok == 0 || buffer.is_null() || len == 0 {
                return None;
            }
            let len = usize::try_from(len).ok()?;
            let byte_len = match length {
                ValueLength::Bytes => len,
                ValueLength::Utf16Units => len.checked_mul(size_of::<u16>())?,
            };
            // Slice by offset rather than trusting the returned pointer, so a
            // value reaching outside the block is rejected in release builds.
            let offset = buffer.addr().checked_sub(self.0.as_ptr().addr())?;
            let end = offset.checked_add(byte_len)?;
            self.0.get(offset..end)
        }

        fn fixed_product_version(&self) -> Option<[u16; 4]> {
            let bytes = self.query("\\", ValueLength::Bytes)?;
            // VS_FIXEDFILEINFO is 13 u32 fields; product version occupies the
            // fifth and sixth.
            if bytes.len() < size_of::<[u32; 6]>() {
                return None;
            }
            let field = |index: usize| {
                let offset = index.checked_mul(size_of::<u32>())?;
                let end = offset.checked_add(size_of::<u32>())?;
                let raw = bytes.get(offset..end)?;
                Some(u32::from_ne_bytes(raw.try_into().ok()?))
            };
            if field(0)? != VS_FFI_SIGNATURE {
                return None;
            }
            let most = field(4)?;
            let least = field(5)?;
            let parts = [
                u16::try_from(most >> 16).ok()?,
                u16::try_from(most & 0xFFFF).ok()?,
                u16::try_from(least >> 16).ok()?,
                u16::try_from(least & 0xFFFF).ok()?,
            ];
            if parts == [0, 0, 0, 0] {
                // Reforger leaves this record zeroed while publishing real
                // strings; reporting 0.0.0.0 as a version would be a lie.
                return None;
            }
            Some(parts)
        }

        fn translations(&self) -> Vec<(u16, u16)> {
            let Some(bytes) = self.query("\\VarFileInfo\\Translation", ValueLength::Bytes) else {
                return Vec::new();
            };
            let chunks = bytes.chunks_exact(size_of::<[u16; 2]>());
            if !chunks.remainder().is_empty() {
                return Vec::new();
            }
            let expected_count = chunks.len();
            let translations = chunks
                .filter_map(|pair| <[u8; 4]>::try_from(pair).ok())
                .map(
                    |[language_low, language_high, code_page_low, code_page_high]| {
                        (
                            u16::from_ne_bytes([language_low, language_high]),
                            u16::from_ne_bytes([code_page_low, code_page_high]),
                        )
                    },
                )
                .collect::<Vec<_>>();
            debug_assert_eq!(
                translations.len(),
                expected_count,
                "every exact translation chunk must decode to one language/code-page pair"
            );
            translations
        }

        fn string_value(&self, language: u16, code_page: u16, name: &str) -> Option<String> {
            let sub_block = format!("\\StringFileInfo\\{language:04x}{code_page:04x}\\{name}");
            let bytes = self.query(&sub_block, ValueLength::Utf16Units)?;
            let chunks = bytes.chunks_exact(size_of::<u16>());
            if !chunks.remainder().is_empty() {
                return None;
            }
            let expected_count = chunks.len();
            let raw = chunks
                .filter_map(|unit| <[u8; 2]>::try_from(unit).ok())
                .map(u16::from_ne_bytes)
                .collect::<Vec<_>>();
            debug_assert_eq!(
                raw.len(),
                expected_count,
                "every exact UTF-16 byte chunk must decode to one code unit"
            );
            let trimmed = raw.split(|unit| *unit == 0).next().unwrap_or(&raw);
            let value = String::from_utf16_lossy(trimmed).trim().to_owned();
            if value.is_empty() { None } else { Some(value) }
        }
    }
}

#[cfg(all(test, windows))]
#[expect(
    clippy::expect_used,
    reason = "system-fixture tests stop immediately with contract-specific context"
)]
mod tests {
    use std::path::{Path, PathBuf};

    fn system32(name: &str) -> PathBuf {
        PathBuf::from(std::env::var_os("WINDIR").expect("WINDIR is set on Windows"))
            .join("System32")
            .join(name)
    }

    /// Every shipped Windows system library carries a version resource, so
    /// this exercises the full query path without needing a game installed.
    #[test]
    fn reads_a_system_library_version() {
        let path = system32("kernel32.dll");
        if !path.is_file() {
            return;
        }
        let version = super::FileVersion::read(&path)
            .expect("reading a system library version resource")
            .expect("kernel32.dll carries a version resource");

        let product = version
            .product_version
            .as_deref()
            .expect("kernel32.dll declares a product version");
        assert!(
            product.contains('.'),
            "expected a dotted product version, got '{product}'"
        );
        assert_eq!(
            version.company_name.as_deref(),
            Some("Microsoft Corporation")
        );
        assert!(
            version.fixed_product_version.is_some(),
            "Microsoft populates the fixed record; only the string block is optional"
        );
    }

    /// Reading never runs the target, so a version resource comes back for a
    /// library that was never loaded by this process.
    #[test]
    fn reading_does_not_require_execution() {
        let path = system32("shell32.dll");
        if !path.is_file() {
            return;
        }
        assert!(
            super::FileVersion::read(&path)
                .expect("reading a system library version resource")
                .is_some()
        );
    }

    /// A missing file is a read failure, distinct from a present file that
    /// carries no version resource.
    #[test]
    fn missing_file_is_an_error_not_an_absent_resource() {
        let path = Path::new(r"C:\definitely\not\here\nothing.exe");
        let error = super::FileVersion::read(path).expect_err("a missing file cannot be read");
        assert_eq!(error.path, path);
    }
}
