//! steamapps-scan - Synthetic PE images
//! =====================================
//!
//! Minimal PE32+ images holding one `.rsrc` section, built byte by byte so
//! version reading runs on every platform without a binary fixture.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "fixture offsets are small constants that cannot overflow"
)]

/// The `VS_VERSIONINFO` records one synthetic image carries.
pub struct VersionResource<'a> {
    /// First field of `VS_FIXEDFILEINFO`; anything but
    /// [`Self::SIGNATURE`] is malformed.
    pub fixed_signature: u32,
    pub fixed_product_version: [u16; 4],
    /// Language and code page naming the string table, also listed as the
    /// image's one translation.
    pub translation: [u16; 2],
    pub strings: &'a [(&'a str, &'a str)],
}

impl VersionResource<'_> {
    pub const SIGNATURE: u32 = 0xFEEF_04BD;

    /// This resource inside a complete PE image.
    pub fn image(&self) -> Vec<u8> {
        PeImage::build(Some(&self.encode()))
    }

    fn encode(&self) -> Vec<u8> {
        let [language, code_page] = self.translation;
        let strings: Vec<Vec<u8>> = self
            .strings
            .iter()
            .map(|(key, value)| NodeValue::Text(value).node(key, &[]))
            .collect();
        let table = NodeValue::Empty.node(&format!("{language:04x}{code_page:04x}"), &strings);
        let string_info = NodeValue::Empty.node("StringFileInfo", &[table]);
        let translation = [language.to_le_bytes(), code_page.to_le_bytes()].concat();
        let var = NodeValue::Binary(&translation).node("Translation", &[]);
        let var_info = NodeValue::Empty.node("VarFileInfo", &[var]);
        NodeValue::Binary(&self.fixed_info()).node("VS_VERSION_INFO", &[string_info, var_info])
    }

    /// `VS_FIXEDFILEINFO`: thirteen `u32` fields, the product version in the
    /// fifth and sixth as most- then least-significant halves.
    fn fixed_info(&self) -> Vec<u8> {
        let [major, minor, patch, build] = self.fixed_product_version;
        let mut fixed = Vec::new();
        fixed.extend(self.fixed_signature.to_le_bytes());
        fixed.extend(0x0001_0000_u32.to_le_bytes());
        for [high, low] in [
            [major, minor],
            [patch, build],
            [major, minor],
            [patch, build],
        ] {
            fixed.extend(low.to_le_bytes());
            fixed.extend(high.to_le_bytes());
        }
        fixed.resize(13 * size_of::<u32>(), 0);
        fixed
    }
}

/// A `VS_VERSIONINFO` node's value.
enum NodeValue<'a> {
    Empty,
    Text(&'a str),
    Binary(&'a [u8]),
}

impl NodeValue<'_> {
    /// Encode a node: a `wLength`, `wValueLength`, `wType` header, the
    /// NUL-terminated UTF-16 key, the value, and the children, each part
    /// aligned to 4 bytes from the node's start.
    fn node(&self, key: &str, children: &[Vec<u8>]) -> Vec<u8> {
        let mut node = vec![0; 6];
        node.extend(Self::utf16(key));
        Self::pad(&mut node);
        let (value_length, is_text) = match self {
            Self::Empty => (0, true),
            Self::Text(text) => {
                let units = Self::utf16(text);
                node.extend(&units);
                (units.len() / 2, true)
            }
            Self::Binary(bytes) => {
                node.extend(*bytes);
                (bytes.len(), false)
            }
        };
        for child in children {
            Self::pad(&mut node);
            node.extend(child);
        }
        let length = u16::try_from(node.len()).expect("fixture node fits in wLength");
        let value_length = u16::try_from(value_length).expect("fixture value fits in wValueLength");
        PeImage::put(&mut node, 0, &length.to_le_bytes());
        PeImage::put(&mut node, 2, &value_length.to_le_bytes());
        PeImage::put(&mut node, 4, &u16::from(is_text).to_le_bytes());
        node
    }

    /// UTF-16LE bytes of `text` and its terminating NUL.
    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn pad(node: &mut Vec<u8>) {
        node.resize(node.len().next_multiple_of(4), 0);
    }
}

/// Headers in the first `SECTION_OFFSET` bytes, then the resource section,
/// mapped at `SECTION_RVA`.
pub struct PeImage;

impl PeImage {
    const NT_HEADERS: usize = 0x40;
    const OPTIONAL_HEADER: usize = Self::NT_HEADERS + 24;
    const OPTIONAL_HEADER_LEN: u16 = 240;
    const SECTION_HEADER: usize = Self::OPTIONAL_HEADER + 240;
    const SECTION_OFFSET: usize = 0x200;
    const SECTION_RVA: u32 = 0x1000;
    /// Where the version data starts in the section: after the type, name,
    /// and language directories of one entry each and the data entry.
    const DATA_OFFSET: usize = 0x58;

    /// A valid image that declares no resources at all.
    pub fn without_resources() -> Vec<u8> {
        Self::build(None)
    }

    fn build(version: Option<&[u8]>) -> Vec<u8> {
        let section = version.map(Self::resource_section).unwrap_or_default();
        let section_len = u32::try_from(section.len()).expect("fixture section fits in u32");
        let mut image = vec![0; Self::SECTION_OFFSET];
        Self::put(&mut image, 0, b"MZ");
        Self::put(
            &mut image,
            0x3C,
            &u32::try_from(Self::NT_HEADERS).unwrap().to_le_bytes(),
        );
        Self::put(&mut image, Self::NT_HEADERS, b"PE\0\0");
        // COFF header: AMD64, one section, and the optional header's length.
        Self::put(&mut image, Self::NT_HEADERS + 4, &0x8664_u16.to_le_bytes());
        Self::put(&mut image, Self::NT_HEADERS + 6, &1_u16.to_le_bytes());
        Self::put(
            &mut image,
            Self::NT_HEADERS + 20,
            &Self::OPTIONAL_HEADER_LEN.to_le_bytes(),
        );
        // PE32+ optional header with all sixteen data directories, the third
        // of which is the resource table.
        Self::put(&mut image, Self::OPTIONAL_HEADER, &0x20B_u16.to_le_bytes());
        Self::put(
            &mut image,
            Self::OPTIONAL_HEADER + 108,
            &16_u32.to_le_bytes(),
        );
        if version.is_some() {
            Self::put(
                &mut image,
                Self::OPTIONAL_HEADER + 128,
                &Self::SECTION_RVA.to_le_bytes(),
            );
            Self::put(
                &mut image,
                Self::OPTIONAL_HEADER + 132,
                &section_len.to_le_bytes(),
            );
        }
        let raw_offset = u32::try_from(Self::SECTION_OFFSET).unwrap();
        Self::put(&mut image, Self::SECTION_HEADER, b".rsrc\0\0\0");
        Self::put(
            &mut image,
            Self::SECTION_HEADER + 8,
            &section_len.to_le_bytes(),
        );
        Self::put(
            &mut image,
            Self::SECTION_HEADER + 12,
            &Self::SECTION_RVA.to_le_bytes(),
        );
        Self::put(
            &mut image,
            Self::SECTION_HEADER + 16,
            &section_len.to_le_bytes(),
        );
        Self::put(
            &mut image,
            Self::SECTION_HEADER + 20,
            &raw_offset.to_le_bytes(),
        );
        image.extend(section);
        image
    }

    /// Resource directories leading `RT_VERSION` / `VS_VERSION_INFO` / US
    /// English to one data entry, followed by the version data itself.
    fn resource_section(version: &[u8]) -> Vec<u8> {
        const SUBDIRECTORY: u32 = 0x8000_0000;
        let mut section = vec![0; Self::DATA_OFFSET];
        Self::directory(&mut section, 0x00, 16, SUBDIRECTORY | 0x18);
        Self::directory(&mut section, 0x18, 1, SUBDIRECTORY | 0x30);
        Self::directory(&mut section, 0x30, 0x0409, 0x48);
        let data_rva = Self::SECTION_RVA + u32::try_from(Self::DATA_OFFSET).unwrap();
        let data_len = u32::try_from(version.len()).expect("fixture resource fits in u32");
        Self::put(&mut section, 0x48, &data_rva.to_le_bytes());
        Self::put(&mut section, 0x4C, &data_len.to_le_bytes());
        section.extend(version);
        section
    }

    /// A resource directory at `offset` holding one integer-ID entry.
    fn directory(section: &mut [u8], offset: usize, id: u32, target: u32) {
        Self::put(section, offset + 14, &1_u16.to_le_bytes());
        Self::put(section, offset + 16, &id.to_le_bytes());
        Self::put(section, offset + 20, &target.to_le_bytes());
    }

    fn put(buffer: &mut [u8], offset: usize, bytes: &[u8]) {
        buffer[offset..offset + bytes.len()].copy_from_slice(bytes);
    }
}
