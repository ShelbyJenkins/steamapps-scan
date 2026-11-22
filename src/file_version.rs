//! steamapps-scan - Executable version resource
//! =============================================
//!
//! Version strings read from a Windows executable's `VS_VERSIONINFO`
//! resource: the same data the Explorer property sheet shows, read from the
//! file on disk without running it.
//!
//! The resource is a Windows convention rather than anything app-specific,
//! and it is optional: plenty of shipped executables carry none.
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

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

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

impl VersionErr {
    fn new(path: &Path, message: impl Into<String>) -> Self {
        Self {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

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
    /// The resource is parsed from the PE file's own bytes, so every target
    /// reads it the same way, including Windows executables installed on
    /// Linux. A file that is not a PE image carries no version resource and
    /// reports `Ok(None)`, as `GetFileVersionInfoW` does. A PE image whose
    /// headers point outside the file is an error: the resource may exist
    /// but cannot be read. 16-bit executables are not read and report
    /// `Ok(None)`.
    pub(crate) fn read(path: &Path) -> Result<Option<Self>, VersionErr> {
        let fail = |err: PeErr| VersionErr::new(path, err.to_string());
        let Some(mut resources) = PeResources::open(path).map_err(fail)? else {
            return Ok(None);
        };
        let Some(resource) = resources.version_resource().map_err(fail)? else {
            return Ok(None);
        };
        let block = VersionBlock::parse(&resource).map_err(fail)?;

        let mut version = Self {
            fixed_product_version: block.fixed_product_version(),
            ..Self::default()
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
}

/// Failure to find the version resource of a file that identifies itself as
/// a PE image.
#[derive(Debug, thiserror::Error)]
enum PeErr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0} lies outside the file")]
    Outside(&'static str),
    #[error("{0}")]
    Malformed(&'static str),
}

/// The resource tree of a PE image, read from the file on demand so that
/// only headers, resource directories and the version resource are loaded.
struct PeResources {
    file: BoundedFile,
    sections: Vec<Section>,
    /// RVA of the root resource directory.
    root: u32,
}

impl PeResources {
    const DOS_HEADER_LEN: usize = 0x40;
    const NT_HEADERS_OFFSET_FIELD: usize = 0x3C;
    /// The `PE\0\0` signature followed by the COFF file header.
    const NT_HEADERS_LEN: u16 = 24;
    const PE32_MAGIC: u16 = 0x10B;
    const PE32_PLUS_MAGIC: u16 = 0x20B;
    const RESOURCE_TABLE_INDEX: u32 = 2;
    const DIRECTORY_HEADER_LEN: u16 = 16;
    const DATA_ENTRY_LEN: usize = 16;
    const RT_VERSION: u32 = 16;
    const VS_VERSION_INFO: u32 = 1;
    /// Language-neutral, then US English, ahead of the first language.
    /// Windows takes the neutral resource and then the reader's UI language;
    /// a fixed order matches an English-language Windows on every machine.
    const LANGUAGE_PREFERENCE: [u32; 2] = [0x0000, 0x0409];

    /// Open the resource tree of the PE image at `path`.
    ///
    /// `None` when the file is not a PE image or the image declares no
    /// resources; either way it carries no version resource.
    fn open(path: &Path) -> Result<Option<Self>, PeErr> {
        let mut file = BoundedFile::open(path)?;
        let Some(dos_header) = file.read_at(0, Self::DOS_HEADER_LEN)? else {
            return Ok(None);
        };
        let (Some(b"MZ"), Some(nt_headers_offset)) = (
            dos_header.first_chunk::<2>(),
            LeBytes(&dos_header).u32(Self::NT_HEADERS_OFFSET_FIELD),
        ) else {
            return Ok(None);
        };
        // Only a PE image fills in this field, so in a DOS program it can
        // point anywhere, including past the end of the file.
        let nt_headers_offset = u64::from(nt_headers_offset);
        let Some(nt_headers) =
            file.read_at(nt_headers_offset, usize::from(Self::NT_HEADERS_LEN))?
        else {
            return Ok(None);
        };
        let Some((b"PE\0\0", coff)) = nt_headers.split_first_chunk::<4>() else {
            return Ok(None);
        };

        let coff = LeBytes(coff);
        let (Some(section_count), Some(optional_len)) = (coff.u16(2), coff.u16(16)) else {
            return Err(PeErr::Malformed("COFF file header is truncated"));
        };
        let optional_len = usize::from(optional_len);
        let headers_offset = nt_headers_offset
            .checked_add(u64::from(Self::NT_HEADERS_LEN))
            .ok_or(PeErr::Outside("optional header"))?;
        let headers_len = usize::from(section_count)
            .checked_mul(Section::HEADER_LEN)
            .and_then(|sections_len| sections_len.checked_add(optional_len))
            .ok_or(PeErr::Outside("section table"))?;
        let headers = file
            .read_at(headers_offset, headers_len)?
            .ok_or(PeErr::Outside("optional header or section table"))?;
        let (optional, section_table) = headers
            .split_at_checked(optional_len)
            .ok_or(PeErr::Outside("section table"))?;

        let optional = LeBytes(optional);
        // Offsets of NumberOfRvaAndSizes and of the resource table, the third
        // data directory entry.
        let (directory_count_field, resource_table_field) = match optional.u16(0) {
            Some(Self::PE32_MAGIC) => (92, 112),
            Some(Self::PE32_PLUS_MAGIC) => (108, 128),
            _ => {
                return Err(PeErr::Malformed(
                    "optional header magic is neither PE32 nor PE32+",
                ));
            }
        };
        let directory_count = optional.u32(directory_count_field).ok_or(PeErr::Malformed(
            "optional header ends before its data directories",
        ))?;
        if directory_count <= Self::RESOURCE_TABLE_INDEX {
            return Ok(None);
        }
        let root = optional.u32(resource_table_field).ok_or(PeErr::Malformed(
            "optional header ends before its resource table",
        ))?;
        if root == 0 {
            return Ok(None);
        }

        let sections = section_table
            .chunks_exact(Section::HEADER_LEN)
            .map(Section::parse)
            .collect::<Option<Vec<_>>>()
            .ok_or(PeErr::Malformed("section header is truncated"))?;
        Ok(Some(Self {
            file,
            sections,
            root,
        }))
    }

    /// Read the `RT_VERSION` resource with ID `VS_VERSION_INFO`, the one
    /// `GetFileVersionInfoW` reads, or `None` when the image has none.
    fn version_resource(&mut self) -> Result<Option<Vec<u8>>, PeErr> {
        let Some(names) = self.subdirectory(0, Self::RT_VERSION)? else {
            return Ok(None);
        };
        let Some(languages) = self.subdirectory(names, Self::VS_VERSION_INFO)? else {
            return Ok(None);
        };
        let languages = self.directory(languages)?;
        let Some(language) = Self::LANGUAGE_PREFERENCE
            .iter()
            .find_map(|preferred| languages.iter().find(|entry| entry.id == *preferred))
            .or_else(|| languages.first())
            .copied()
        else {
            return Ok(None);
        };
        let ResourceTarget::Data(data_entry) = language.target else {
            return Err(PeErr::Malformed(
                "version resource language entry is a directory rather than data",
            ));
        };

        let data_entry =
            self.read_resource(data_entry, Self::DATA_ENTRY_LEN, "resource data entry")?;
        let data_entry = LeBytes(&data_entry);
        let (Some(rva), Some(size)) = (data_entry.u32(0), data_entry.u32(4)) else {
            return Err(PeErr::Malformed("resource data entry is truncated"));
        };
        // The root node's length is a u16, so nothing past 64 KiB belongs to
        // the version tree.
        let len = usize::from(u16::try_from(size).unwrap_or(u16::MAX));
        self.read_rva(rva, len, "version resource").map(Some)
    }

    /// Find the entry with integer `id` in the directory at `directory` and
    /// return the subdirectory it leads to.
    fn subdirectory(&mut self, directory: u32, id: u32) -> Result<Option<u32>, PeErr> {
        let Some(entry) = self
            .directory(directory)?
            .into_iter()
            .find(|entry| entry.id == id)
        else {
            return Ok(None);
        };
        match entry.target {
            ResourceTarget::Directory(offset) => Ok(Some(offset)),
            ResourceTarget::Data(_) => Err(PeErr::Malformed(
                "resource type or name entry is data rather than a directory",
            )),
        }
    }

    /// Read every entry of the resource directory `offset` bytes past the
    /// root directory.
    fn directory(&mut self, offset: u32) -> Result<Vec<ResourceEntry>, PeErr> {
        let header = self.read_resource(
            offset,
            usize::from(Self::DIRECTORY_HEADER_LEN),
            "resource directory",
        )?;
        let header = LeBytes(&header);
        let (Some(named), Some(ids)) = (header.u16(12), header.u16(14)) else {
            return Err(PeErr::Malformed("resource directory header is truncated"));
        };
        let count = usize::from(named)
            .checked_add(usize::from(ids))
            .ok_or(PeErr::Outside("resource directory entries"))?;
        if count == 0 {
            return Ok(Vec::new());
        }
        let entries_offset = offset
            .checked_add(u32::from(Self::DIRECTORY_HEADER_LEN))
            .ok_or(PeErr::Outside("resource directory entries"))?;
        let entries_len = count
            .checked_mul(ResourceEntry::LEN)
            .ok_or(PeErr::Outside("resource directory entries"))?;
        let entries =
            self.read_resource(entries_offset, entries_len, "resource directory entries")?;
        entries
            .chunks_exact(ResourceEntry::LEN)
            .map(ResourceEntry::parse)
            .collect::<Option<Vec<_>>>()
            .ok_or(PeErr::Malformed("resource directory entry is truncated"))
    }

    /// Read `len` bytes located `offset` bytes past the root directory.
    fn read_resource(
        &mut self,
        offset: u32,
        len: usize,
        what: &'static str,
    ) -> Result<Vec<u8>, PeErr> {
        let rva = self.root.checked_add(offset).ok_or(PeErr::Outside(what))?;
        self.read_rva(rva, len, what)
    }

    /// Read `len` bytes at `rva`, all of which must be stored in the file as
    /// part of one section.
    fn read_rva(&mut self, rva: u32, len: usize, what: &'static str) -> Result<Vec<u8>, PeErr> {
        let (offset, available) = self
            .sections
            .iter()
            .find_map(|section| section.file_span(rva))
            .ok_or(PeErr::Outside(what))?;
        if !u32::try_from(len).is_ok_and(|len| len <= available) {
            return Err(PeErr::Outside(what));
        }
        self.file.read_at(offset, len)?.ok_or(PeErr::Outside(what))
    }
}

/// One section header: where a range of RVAs is stored in the file.
#[derive(Clone, Copy)]
struct Section {
    virtual_address: u32,
    raw_size: u32,
    raw_offset: u32,
}

impl Section {
    const HEADER_LEN: usize = 40;

    fn parse(header: &[u8]) -> Option<Self> {
        let header = LeBytes(header);
        Some(Self {
            virtual_address: header.u32(12)?,
            raw_size: header.u32(16)?,
            raw_offset: header.u32(20)?,
        })
    }

    /// The file offset of `rva` and how many stored bytes follow it, when
    /// `rva` falls inside this section's stored data.
    fn file_span(self, rva: u32) -> Option<(u64, u32)> {
        let delta = rva.checked_sub(self.virtual_address)?;
        let available = self
            .raw_size
            .checked_sub(delta)
            .filter(|available| *available > 0)?;
        let offset = u64::from(self.raw_offset).checked_add(u64::from(delta))?;
        Some((offset, available))
    }
}

/// One resource directory entry.
#[derive(Clone, Copy)]
struct ResourceEntry {
    /// The integer ID, or a name offset with the high bit set, which no
    /// integer ID equals.
    id: u32,
    target: ResourceTarget,
}

impl ResourceEntry {
    const LEN: usize = 8;
    const SUBDIRECTORY_FLAG: u32 = 0x8000_0000;

    fn parse(entry: &[u8]) -> Option<Self> {
        let entry = LeBytes(entry);
        let (id, target) = (entry.u32(0)?, entry.u32(4)?);
        let target = if target & Self::SUBDIRECTORY_FLAG == 0 {
            ResourceTarget::Data(target)
        } else {
            ResourceTarget::Directory(target & !Self::SUBDIRECTORY_FLAG)
        };
        Some(Self { id, target })
    }
}

/// What a resource directory entry leads to, as an offset from the root
/// directory.
#[derive(Clone, Copy)]
enum ResourceTarget {
    Directory(u32),
    Data(u32),
}

/// A file whose reads are checked against its length before they run, so a
/// structure claiming bytes the file lacks is refused rather than read.
struct BoundedFile {
    file: File,
    len: u64,
}

impl BoundedFile {
    fn open(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self { file, len })
    }

    /// Read `len` bytes at `offset`, or `None` when they do not all lie
    /// inside the file.
    fn read_at(&mut self, offset: u64, len: usize) -> std::io::Result<Option<Vec<u8>>> {
        let end = u64::try_from(len)
            .ok()
            .and_then(|len| offset.checked_add(len));
        if end.is_none_or(|end| end > self.len) {
            return Ok(None);
        }
        self.file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; len];
        self.file.read_exact(&mut bytes)?;
        Ok(Some(bytes))
    }
}

/// The root `VS_VERSIONINFO` node of a version resource, answering the three
/// lookups this module makes the way `VerQueryValueW` answers them.
struct VersionBlock<'a>(VersionNode<'a>);

impl<'a> VersionBlock<'a> {
    /// `VS_FIXEDFILEINFO` starts with this, letting a bogus block be
    /// rejected rather than reinterpreted.
    const FIXED_INFO_SIGNATURE: u32 = 0xFEEF_04BD;

    /// Validate the root node and its fixed record. A resource without a
    /// fixed record keeps its strings; one whose fixed record lacks the
    /// signature is malformed, which `GetFileVersionInfoW` also refuses.
    fn parse(resource: &'a [u8]) -> Result<Self, PeErr> {
        let root = VersionNode::parse(resource).ok_or(PeErr::Malformed(
            "version resource root node does not fit the resource",
        ))?;
        if root.value_length != 0
            && root.binary_value().and_then(|value| LeBytes(value).u32(0))
                != Some(Self::FIXED_INFO_SIGNATURE)
        {
            return Err(PeErr::Malformed("VS_FIXEDFILEINFO signature is wrong"));
        }
        Ok(Self(root))
    }

    /// The node at `path` below the root, matching each key as
    /// `VerQueryValueW` does.
    fn query(&self, path: &[&str]) -> Option<VersionNode<'a>> {
        path.iter().try_fold(self.0, |node, key| node.child(key))
    }

    fn fixed_product_version(&self) -> Option<[u16; 4]> {
        // VS_FIXEDFILEINFO is 13 u32 fields; product version occupies the
        // fifth and sixth.
        let fields = LeBytes(self.0.binary_value()?);
        debug_assert!(
            self.0.value_length == 0 || fields.u32(0) == Some(Self::FIXED_INFO_SIGNATURE),
            "parse admits a fixed record only when it carries the signature"
        );
        let most = fields.u32(16)?;
        let least = fields.u32(20)?;
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
        let Some(bytes) = self
            .query(&["VarFileInfo", "Translation"])
            .and_then(|node| node.binary_value())
        else {
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
                        u16::from_le_bytes([language_low, language_high]),
                        u16::from_le_bytes([code_page_low, code_page_high]),
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
        let table = format!("{language:04x}{code_page:04x}");
        let units = self.query(&["StringFileInfo", &table, name])?.text_units();
        let value = String::from_utf16_lossy(&units).trim().to_owned();
        if value.is_empty() { None } else { Some(value) }
    }
}

/// One node of a `VS_VERSIONINFO` tree: a `wLength`, `wValueLength`,
/// `wType` header, a NUL-terminated UTF-16 key, a value, and child nodes,
/// each part aligned to 4 bytes from the node's start.
#[derive(Clone, Copy)]
struct VersionNode<'a> {
    /// Exactly `wLength` bytes, starting at the header.
    bytes: &'a [u8],
    /// UTF-16LE key without its NUL.
    key: &'a [u8],
    /// `wValueLength`: UTF-16 units for text values, bytes otherwise.
    value_length: u16,
    value_start: usize,
    children_start: usize,
}

impl<'a> VersionNode<'a> {
    const HEADER_LEN: usize = 6;

    /// Parse the node at the start of `bytes`, which runs to the end of the
    /// enclosing node. `None` when the node does not fit there or its key is
    /// unterminated.
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        let header = LeBytes(bytes);
        let length = usize::from(header.u16(0)?);
        let value_length = header.u16(2)?;
        let is_text = header.u16(4)? != 0;
        if length < Self::HEADER_LEN {
            return None;
        }
        let bytes = bytes.get(..length)?;
        let key_units = bytes
            .get(Self::HEADER_LEN..)?
            .chunks_exact(size_of::<u16>())
            .position(|unit| *unit == [0, 0])?;
        let key_end = key_units
            .checked_mul(size_of::<u16>())?
            .checked_add(Self::HEADER_LEN)?;
        let key = bytes.get(Self::HEADER_LEN..key_end)?;
        let value_start = Self::align(key_end.checked_add(size_of::<u16>())?)?;
        let value_bytes = usize::from(value_length).checked_mul(if is_text { 2 } else { 1 })?;
        let children_start = Self::align(value_start.checked_add(value_bytes)?)?;
        Some(Self {
            bytes,
            key,
            value_length,
            value_start,
            children_start,
        })
    }

    const fn align(offset: usize) -> Option<usize> {
        match offset.checked_add(3) {
            Some(padded) => Some(padded & !3),
            None => None,
        }
    }

    /// The first child whose key equals `key` ignoring ASCII case, as
    /// `VerQueryValueW` matches. A malformed child ends the search, since
    /// nothing after it can be located.
    fn child(&self, key: &str) -> Option<Self> {
        let mut offset = self.children_start;
        while let Some(rest) = self.bytes.get(offset..).filter(|rest| !rest.is_empty()) {
            let child = Self::parse(rest)?;
            debug_assert!(
                child.bytes.len() >= Self::HEADER_LEN,
                "a parsed child of {} bytes must cover its header so the walk advances",
                child.bytes.len()
            );
            if child.key_is(key) {
                return Some(child);
            }
            offset = offset.checked_add(Self::align(child.bytes.len())?)?;
        }
        None
    }

    fn key_is(&self, key: &str) -> bool {
        let fold = |unit: u16| {
            u8::try_from(unit).map_or(unit, |byte| u16::from(byte.to_ascii_lowercase()))
        };
        Self::utf16_units(self.key)
            .map(fold)
            .eq(key.encode_utf16().map(fold))
    }

    /// The value as `wValueLength` bytes, all inside the node.
    fn binary_value(&self) -> Option<&'a [u8]> {
        let end = self
            .value_start
            .checked_add(usize::from(self.value_length))?;
        self.bytes.get(self.value_start..end)
    }

    /// The value as at most `wValueLength` UTF-16 units, cut at the first
    /// NUL. Some resource compilers count string lengths in bytes, doubling
    /// the declared length, so the end of the node also ends the value.
    fn text_units(&self) -> Vec<u16> {
        let value = self.bytes.get(self.value_start..).unwrap_or_default();
        Self::utf16_units(value)
            .take(usize::from(self.value_length))
            .take_while(|unit| *unit != 0)
            .collect()
    }

    fn utf16_units(bytes: &[u8]) -> impl Iterator<Item = u16> + '_ {
        bytes
            .chunks_exact(size_of::<u16>())
            .filter_map(|unit| unit.first_chunk().copied())
            .map(u16::from_le_bytes)
    }
}

/// Little-endian fields of an untrusted buffer; a field the buffer does not
/// wholly hold reads as `None`.
#[derive(Clone, Copy)]
struct LeBytes<'a>(&'a [u8]);

impl LeBytes<'_> {
    fn u16(self, offset: usize) -> Option<u16> {
        self.0
            .get(offset..)?
            .first_chunk()
            .copied()
            .map(u16::from_le_bytes)
    }

    fn u32(self, offset: usize) -> Option<u32> {
        self.0
            .get(offset..)?
            .first_chunk()
            .copied()
            .map(u32::from_le_bytes)
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
