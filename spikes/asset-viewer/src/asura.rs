//! `Asura` containers: the launcher's localised string tables (`.asr`).
//!
//! One file in the corpus, `English/Text/Menu/Menu_En.asr`, 375 bytes. It does not belong to
//! `lomse.exe` at all.
//!
//! **Observed in a local binary, 2026-09-19: the owner is `English/Launcher/LOMLauncher.exe`.**
//! This was previously recorded as *Inferred from the file's contents*; reading the binary settles
//! it. The launcher contains the format strings `Text\Menu\Menu_%s.asr` and `Text\%s_%s.asr` --
//! it builds this exact path -- together with the literals `Asura   ` and `HTXT`, the RTTI names
//! `Asura_HashedLocalisedText_Page`, `Asura_ResourceSet`, `Asura_Handle_List` and
//! `Launcher_ResourceProtocol`, an embedded HTML page whose buttons are the placeholders
//! `[[LAUNCHER_PLAY]]` and `[[LAUNCHER_SUPPORT]]`, and the build path
//! `C:\Source Code\ASURA_ROOT\FileTools\Launcher\LordsOfMagic\Workspace\Release\LOMLauncher.pdb`.
//! `lomse.exe` contains none of these.
//!
//! # What is decoded, and what is preserved instead
//!
//! Every field the layout below names is decoded. The four words whose meaning is **not**
//! determined -- and the trailing NUL run -- are carried verbatim so that re-encoding is exact and
//! so that no value is invented for them. A writer that could only reproduce the parts it
//! understood would silently drop the rest.

use std::fmt;

/// The eight-byte magic. Three trailing spaces, not one. **Observed in the corpus.**
pub const MAGIC: [u8; 8] = *b"Asura   ";

/// The chunk this project decodes: hashed localised text. **Observed in the corpus.**
pub const CHUNK_HTXT: [u8; 4] = *b"HTXT";

/// Bytes before the chunk payload: the magic, the chunk id, its size, and two more words.
const CHUNK_PAYLOAD_OFFSET: usize = 24;

/// The page-name field that follows the string records. Eight bytes, `Menu` and four NULs.
///
/// **Not determined:** whether this is one eight-byte NUL-padded name field or a four-byte name
/// followed by a separate zero word. The single corpus file has a four-character name, so the two
/// readings produce identical bytes and nothing here can separate them. It is modelled as one
/// field and carried verbatim, which is correct under either reading.
const PAGE_NAME_FIELD: usize = 8;

/// The launcher's string hash: `h = h * 31 + c`, case-folded, over the key.
///
/// **Observed in a local binary, 2026-09-19.** `LOMLauncher.exe` `0x004018b0`, in full:
///
/// ```text
/// 004018b0  push esi / mov esi,eax / xor eax,eax      ; h = 0
/// 004018b5  test esi,esi / je  end                    ; a null pointer hashes to 0
/// 004018c0  lea edx,[ecx-0x41] / cmp dl,0x19 / ja +   ; 'A'..'Z' ...
/// 004018c8  add cl,0x20                               ;   ... folded to lower case
/// 004018cd  cmp cl,0x5c / jne + / mov cl,0x2f         ; '\' is folded to '/'
/// 004018d4  mov edx,eax / shl edx,5 / sub edx,eax     ; edx = h * 31
/// 004018db  movsx eax,cl                              ; the byte is SIGN-extended
/// 004018e2  add eax,edx                               ; h = h * 31 + c
/// ```
///
/// Three details the corpus cannot show, all taken from the instructions rather than from the
/// bytes: the terminator is **not** hashed (the loop tests for it first), a backslash is folded to
/// a forward slash, and the character is **sign-extended**, so a byte at or above `0x80`
/// contributes a negative value. Every key in the one installed file is plain upper-case ASCII, so
/// a wrong rule for any of the three would round-trip perfectly.
///
/// **Observed in the corpus, 2026-09-19.** It reproduces all five of the file's record hashes from
/// their keys, and the page-name word `0x0033155f` from `Menu`. Six for six.
pub fn asura_hash(text: &str) -> u32 {
    let mut hash = 0_u32;
    for byte in text.as_bytes() {
        let folded = match byte {
            b'A'..=b'Z' => byte + 0x20,
            b'\\' => b'/',
            other => *other,
        };
        hash = hash
            .wrapping_mul(31)
            .wrapping_add(i32::from(folded as i8) as u32);
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsuraError(String);

impl AsuraError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for AsuraError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AsuraError {}

/// One localised string: the hash of the key that finds it, and the text itself.
///
/// **Observed in the corpus.** `u32` hash, `u32` unit count, then that many UTF-16LE code units.
/// The count **includes** the NUL terminator: the five counts are 5, 8, 31, 6 and 40 against texts
/// of 4, 7, 30, 5 and 39 characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsuraString {
    pub hash: u32,
    pub text: String,
}

impl AsuraString {
    /// How many UTF-16 code units this record stores, terminator included.
    pub fn unit_count(&self) -> usize {
        self.text.encode_utf16().count() + 1
    }
}

/// An `Asura` `HTXT` page: the string records and the key table that names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsuraText {
    /// The chunk id. `HTXT` in the one corpus file; kept so a different chunk cannot be silently
    /// re-labelled by the writer.
    pub chunk_id: [u8; 4],
    /// The word at `0x10`. `3` in the corpus file. **Not determined**; carried verbatim.
    pub chunk_version: u32,
    /// The word at `0x14`. `0` in the corpus file. **Not determined**; carried verbatim.
    pub chunk_reserved: u32,
    /// The word at `0x24`. `0` in the corpus file. **Not determined**; carried verbatim.
    pub payload_reserved: u32,
    pub strings: Vec<AsuraString>,
    /// The eight bytes after the last record: `Menu` and four NULs. See [`PAGE_NAME_FIELD`].
    pub page_name_field: [u8; PAGE_NAME_FIELD],
    /// The key table, in record order: `LAUNCHER_PLAY`, `LAUNCHER_SUPPORT`, ...
    pub keys: Vec<String>,
    /// Whatever follows the key table. Sixteen NULs in the corpus file. **Not determined**:
    /// whether this is alignment padding or a field. It is not alignment to any power of two --
    /// the file is 375 bytes -- so it is carried verbatim rather than regenerated.
    pub trailer: Vec<u8>,
}

impl AsuraText {
    /// The page name, with its NUL padding stripped.
    pub fn page_name(&self) -> String {
        String::from_utf8_lossy(
            &self.page_name_field[..self
                .page_name_field
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(PAGE_NAME_FIELD)],
        )
        .into_owned()
    }

    /// The page-name hash this page's name implies.
    pub fn page_name_hash(&self) -> u32 {
        asura_hash(&self.page_name())
    }

    /// The total UTF-16 byte count the records imply: the word at `0x20`.
    pub fn text_bytes(&self) -> usize {
        self.strings
            .iter()
            .map(|record| record.unit_count() * 2)
            .sum()
    }

    /// The key table's byte count: each key plus its terminator.
    pub fn key_bytes(&self) -> usize {
        self.keys.iter().map(|key| key.len() + 1).sum()
    }

    /// The text stored under `key`, found the way the launcher finds it -- by hashing the key.
    pub fn get(&self, key: &str) -> Option<&str> {
        let hash = asura_hash(key);
        self.strings
            .iter()
            .find(|record| record.hash == hash)
            .map(|record| record.text.as_str())
    }

    /// Replace the text stored under `key`. `false` when no record carries that key's hash.
    ///
    /// Lengths are free to change: the record's unit count and every length word derived from it
    /// are recomputed by [`to_bytes`](Self::to_bytes) rather than stored.
    pub fn set(&mut self, key: &str, text: impl Into<String>) -> bool {
        let hash = asura_hash(key);
        match self
            .strings
            .iter_mut()
            .find(|record| record.hash == hash)
        {
            Some(record) => {
                record.text = text.into();
                true
            }
            None => false,
        }
    }

    /// Append a new key and its text.
    ///
    /// **This is possible because the hash function is recovered rather than guessed**; before it
    /// was, only replacing an existing record was, since a new record's hash word could not be
    /// filled in. Refuses a key already present, because two records sharing a hash would make
    /// [`get`](Self::get) answer with whichever came first.
    pub fn push(&mut self, key: &str, text: impl Into<String>) -> Result<(), AsuraError> {
        let hash = asura_hash(key);
        if self.strings.iter().any(|record| record.hash == hash) {
            return Err(AsuraError::new(format!(
                "{key} already has a record (hash {hash:#010x})"
            )));
        }
        self.strings.push(AsuraString {
            hash,
            text: text.into(),
        });
        self.keys.push(key.to_owned());
        Ok(())
    }

    /// Whether every key hashes to the record in the same position.
    ///
    /// The key table and the record table are two independent lists; nothing in the file states
    /// that they are parallel. This is the equation that says they are, and it is what a test
    /// should assert rather than a transcribed table of hashes.
    pub fn keys_match_records(&self) -> bool {
        self.keys.len() == self.strings.len()
            && self
                .keys
                .iter()
                .zip(&self.strings)
                .all(|(key, record)| asura_hash(key) == record.hash)
    }

    pub fn parse(source: &[u8]) -> Result<Self, AsuraError> {
        if source.len() < CHUNK_PAYLOAD_OFFSET {
            return Err(AsuraError::new("Asura container is shorter than its header"));
        }
        if source[..8] != MAGIC {
            return Err(AsuraError::new(
                "not an Asura container: the magic is not \"Asura   \"",
            ));
        }
        let chunk_id: [u8; 4] = source[8..12].try_into().expect("four bytes");
        let chunk_size = read_u32(source, 12)?;
        let chunk_version = read_u32(source, 16)?;
        let chunk_reserved = read_u32(source, 20)?;
        let expected = source.len() - CHUNK_PAYLOAD_OFFSET;
        if chunk_size as usize != expected {
            return Err(AsuraError::new(format!(
                "chunk size {chunk_size} does not account for the file: {expected} bytes follow \
                 the header"
            )));
        }
        if chunk_id != CHUNK_HTXT {
            return Err(AsuraError::new(format!(
                "unsupported Asura chunk {:?}; this parser decodes HTXT only",
                String::from_utf8_lossy(&chunk_id)
            )));
        }

        let string_count = read_u32(source, 24)?;
        let page_name_hash = read_u32(source, 28)?;
        let text_bytes = read_u32(source, 32)?;
        let payload_reserved = read_u32(source, 36)?;

        let mut offset = 40;
        let mut strings = Vec::new();
        for index in 0..string_count {
            let hash = read_u32(source, offset)?;
            let units = read_u32(source, offset + 4)? as usize;
            if units == 0 {
                return Err(AsuraError::new(format!(
                    "string record {index} has no terminator: its unit count is zero"
                )));
            }
            let start = offset + 8;
            let end = start
                .checked_add(units * 2)
                .ok_or_else(|| AsuraError::new("string record overflows"))?;
            if end > source.len() {
                return Err(AsuraError::new(format!(
                    "string record {index} wants {units} units but the file ends"
                )));
            }
            let mut units_le = Vec::with_capacity(units);
            for unit in 0..units {
                units_le.push(u16::from_le_bytes([
                    source[start + unit * 2],
                    source[start + unit * 2 + 1],
                ]));
            }
            if units_le.last() != Some(&0) {
                return Err(AsuraError::new(format!(
                    "string record {index} is not NUL-terminated"
                )));
            }
            units_le.pop();
            let text = String::from_utf16(&units_le)
                .map_err(|_| AsuraError::new(format!("string record {index} is not valid UTF-16")))?;
            strings.push(AsuraString { hash, text });
            offset = end;
        }

        let page_name_end = offset
            .checked_add(PAGE_NAME_FIELD)
            .ok_or_else(|| AsuraError::new("page name overflows"))?;
        if page_name_end + 4 > source.len() {
            return Err(AsuraError::new(
                "the file ends before the page name and key table",
            ));
        }
        let page_name_field: [u8; PAGE_NAME_FIELD] = source[offset..page_name_end]
            .try_into()
            .expect("eight bytes");
        let key_bytes = read_u32(source, page_name_end)? as usize;
        let keys_start = page_name_end + 4;
        let keys_end = keys_start
            .checked_add(key_bytes)
            .ok_or_else(|| AsuraError::new("key table overflows"))?;
        if keys_end > source.len() {
            return Err(AsuraError::new(format!(
                "the key table wants {key_bytes} bytes but the file ends"
            )));
        }
        let key_block = &source[keys_start..keys_end];
        if key_block.last() != Some(&0) {
            return Err(AsuraError::new("the key table's last key is unterminated"));
        }
        let mut keys = Vec::new();
        for key in key_block[..key_block.len() - 1].split(|byte| *byte == 0) {
            keys.push(
                std::str::from_utf8(key)
                    .map_err(|_| AsuraError::new("a key is not valid UTF-8"))?
                    .to_owned(),
            );
        }
        let trailer = source[keys_end..].to_vec();

        let page = Self {
            chunk_id,
            chunk_version,
            chunk_reserved,
            payload_reserved,
            strings,
            page_name_field,
            keys,
            trailer,
        };
        // The two derived words are checked rather than stored, so a file whose head disagrees
        // with its own body is an error instead of a silently re-derived "round trip".
        if page.strings.len() as u32 != string_count {
            return Err(AsuraError::new("string count does not match the records"));
        }
        if page.text_bytes() != text_bytes as usize {
            return Err(AsuraError::new(format!(
                "the header says {text_bytes} bytes of text; the records hold {}",
                page.text_bytes()
            )));
        }
        if page.page_name_hash() != page_name_hash {
            return Err(AsuraError::new(format!(
                "the page-name word is {page_name_hash:#010x}; {:?} hashes to {:#010x}",
                page.page_name(),
                page.page_name_hash()
            )));
        }
        Ok(page)
    }

    /// This page as a complete file.
    ///
    /// Every length and every hash the format derives is **recomputed** here -- the chunk size,
    /// the record count, the total text length, each record's unit count, the page-name hash and
    /// the key-table length. Only the four words whose meaning is undetermined and the trailing
    /// NUL run are copied. That is what makes this an emitter rather than a byte-preserver: a
    /// caller can change a string's length, or add one, and the file stays self-consistent.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        payload.extend_from_slice(&self.page_name_hash().to_le_bytes());
        payload.extend_from_slice(&(self.text_bytes() as u32).to_le_bytes());
        payload.extend_from_slice(&self.payload_reserved.to_le_bytes());
        for record in &self.strings {
            payload.extend_from_slice(&record.hash.to_le_bytes());
            payload.extend_from_slice(&(record.unit_count() as u32).to_le_bytes());
            for unit in record.text.encode_utf16() {
                payload.extend_from_slice(&unit.to_le_bytes());
            }
            payload.extend_from_slice(&0_u16.to_le_bytes());
        }
        payload.extend_from_slice(&self.page_name_field);
        payload.extend_from_slice(&(self.key_bytes() as u32).to_le_bytes());
        for key in &self.keys {
            payload.extend_from_slice(key.as_bytes());
            payload.push(0);
        }
        payload.extend_from_slice(&self.trailer);

        let mut bytes = Vec::with_capacity(CHUNK_PAYLOAD_OFFSET + payload.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&self.chunk_id);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.chunk_version.to_le_bytes());
        bytes.extend_from_slice(&self.chunk_reserved.to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }
}

fn read_u32(source: &[u8], offset: usize) -> Result<u32, AsuraError> {
    source
        .get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")))
        .ok_or_else(|| AsuraError::new(format!("Asura container ends before offset {offset}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hash is pinned by the values it must reproduce, stated as key-to-word pairs.
    ///
    /// These five words are **not** transcribed from this implementation: they are the words in
    /// `Menu_En.asr`, and the corpus-gated test in `tests/loose.rs` re-reads them off disk. This
    /// unit test keeps the rule falsifiable on a machine with no install.
    #[test]
    fn the_hash_reproduces_the_words_in_the_installed_file() {
        for (key, word) in [
            ("LAUNCHER_PLAY", 0xbe68_f873_u32),
            ("LAUNCHER_SUPPORT", 0xe867_9630),
            ("LAUNCHER_GAME_TITLE", 0x8cf7_d5ca),
            ("LAUNCHER_ERROR_TITLE", 0xdf12_af42),
            ("LAUNCHER_ERROR", 0x0e1e_0ca9),
            ("Menu", 0x0033_155f),
        ] {
            assert_eq!(asura_hash(key), word, "{key}");
        }
    }

    /// Case folding, backslash folding and the empty string, which the corpus cannot exercise.
    #[test]
    fn the_hash_folds_case_and_backslashes() {
        assert_eq!(asura_hash("LAUNCHER_PLAY"), asura_hash("launcher_play"));
        assert_eq!(asura_hash("a\\b"), asura_hash("a/b"));
        assert_eq!(asura_hash(""), 0);
        // The byte is sign-extended, so a high byte is not the same as its unsigned value.
        assert_ne!(asura_hash("\u{80}"), 0x80);
    }

    fn sample() -> AsuraText {
        let mut page = AsuraText {
            chunk_id: CHUNK_HTXT,
            chunk_version: 3,
            chunk_reserved: 0,
            payload_reserved: 0,
            strings: Vec::new(),
            page_name_field: *b"Menu\0\0\0\0",
            keys: Vec::new(),
            trailer: vec![0; 16],
        };
        page.push("LAUNCHER_PLAY", "Play").expect("a fresh key");
        page
    }

    #[test]
    fn a_page_round_trips_through_its_own_encoder() {
        let page = sample();
        let bytes = page.to_bytes();
        assert_eq!(AsuraText::parse(&bytes).expect("it parses"), page);
    }

    #[test]
    fn a_longer_replacement_stays_self_consistent() {
        let mut page = sample();
        assert!(page.set("LAUNCHER_PLAY", "Play the game right now"));
        let bytes = page.to_bytes();
        let reparsed = AsuraText::parse(&bytes).expect("it parses");
        assert_eq!(reparsed.get("LAUNCHER_PLAY"), Some("Play the game right now"));
        assert!(reparsed.keys_match_records());
    }

    #[test]
    fn a_new_key_can_be_added_and_found_again() {
        let mut page = sample();
        page.push("LAUNCHER_QUIT", "Quit").expect("a fresh key");
        let reparsed = AsuraText::parse(&page.to_bytes()).expect("it parses");
        assert_eq!(reparsed.get("LAUNCHER_QUIT"), Some("Quit"));
        assert_eq!(reparsed.strings.len(), 2);
        assert!(reparsed.keys_match_records());
    }

    #[test]
    fn a_duplicate_key_is_refused() {
        let mut page = sample();
        assert!(page.push("LAUNCHER_PLAY", "Again").is_err());
        assert!(!page.set("LAUNCHER_MISSING", "nothing"));
    }

    #[test]
    fn a_header_that_disagrees_with_its_body_is_an_error() {
        let mut bytes = sample().to_bytes();
        // Claim one more byte of text than the records hold.
        let claimed = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) + 2;
        bytes[32..36].copy_from_slice(&claimed.to_le_bytes());
        let error = AsuraText::parse(&bytes).expect_err("the head no longer matches the body");
        assert!(error.to_string().contains("bytes of text"), "{error}");
    }

    #[test]
    fn a_wrong_magic_is_refused() {
        let mut bytes = sample().to_bytes();
        bytes[0] = b'B';
        assert!(AsuraText::parse(&bytes).is_err());
    }
}
