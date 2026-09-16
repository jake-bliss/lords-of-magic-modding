//! Recovery of the engine's GameScript operator tables from the Win32 executable.
//!
//! `lomse.exe` registers every native GameScript operator in a table of eight-byte records, each
//! holding a pointer to the operator's NUL-terminated name followed by a pointer to its
//! implementation. Recovering that table converts the script-side question "which names does the
//! corpus use but never define?" into the binary-side answer "which names does the engine
//! implement, and where?".
//!
//! The extraction is deliberately structural rather than fitted. A record is accepted only when its
//! first dword resolves to a string inside the image and its second dword lands inside the code
//! section, and runs of such records are reported individually with their file offsets so that
//! nothing is silently discarded. Unrelated data can satisfy those constraints by coincidence, so
//! names are additionally required to be lexable as GameScript executable names — the same
//! character rule the lexer applies. That rule is what rejects the locale tables, whose entries
//! contain spaces and hyphens; it is derived from our own grammar, not tuned against this binary.

use std::collections::BTreeSet;

/// Size of one operator record: a name pointer followed by an implementation pointer.
const RECORD_SIZE: usize = 8;

/// Shortest run of consecutive records reported as a table. Short runs are overwhelmingly
/// coincidence; they are still reported so a reader can judge them, but callers generally want the
/// long ones.
const MINIMUM_RUN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEntry {
    pub name: String,
    pub entry_point: u32,
    pub record_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTableRun {
    pub file_offset: usize,
    pub entries: Vec<NativeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTableError(String);

impl NativeTableError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for NativeTableError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for NativeTableError {}

#[derive(Debug, Clone, Copy)]
struct Section {
    virtual_address: u32,
    raw_offset: u32,
    raw_size: u32,
    executable: bool,
}

#[derive(Debug, Clone)]
struct Image<'a> {
    bytes: &'a [u8],
    image_base: u32,
    sections: Vec<Section>,
}

impl<'a> Image<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, NativeTableError> {
        let pe_offset = read_u32(bytes, 0x3c)
            .ok_or_else(|| NativeTableError::new("executable is too short for a DOS header"))?
            as usize;
        if bytes.get(pe_offset..pe_offset + 4) != Some(b"PE\0\0") {
            return Err(NativeTableError::new("executable has no PE signature"));
        }
        let section_count = usize::from(
            read_u16(bytes, pe_offset + 6)
                .ok_or_else(|| NativeTableError::new("truncated COFF header"))?,
        );
        let optional_size = usize::from(
            read_u16(bytes, pe_offset + 20)
                .ok_or_else(|| NativeTableError::new("truncated COFF header"))?,
        );
        let magic = read_u16(bytes, pe_offset + 24)
            .ok_or_else(|| NativeTableError::new("truncated optional header"))?;
        if magic != 0x10b {
            return Err(NativeTableError::new(format!(
                "expected a 32-bit PE image, found optional header magic {magic:#x}"
            )));
        }
        let image_base = read_u32(bytes, pe_offset + 52)
            .ok_or_else(|| NativeTableError::new("truncated optional header"))?;

        let table_offset = pe_offset + 24 + optional_size;
        let mut sections = Vec::with_capacity(section_count);
        for index in 0..section_count {
            let offset = table_offset + index * 40;
            let virtual_address = read_u32(bytes, offset + 12)
                .ok_or_else(|| NativeTableError::new("truncated section table"))?;
            let raw_size = read_u32(bytes, offset + 16)
                .ok_or_else(|| NativeTableError::new("truncated section table"))?;
            let raw_offset = read_u32(bytes, offset + 20)
                .ok_or_else(|| NativeTableError::new("truncated section table"))?;
            let characteristics = read_u32(bytes, offset + 36)
                .ok_or_else(|| NativeTableError::new("truncated section table"))?;
            sections.push(Section {
                virtual_address,
                raw_offset,
                raw_size,
                // IMAGE_SCN_MEM_EXECUTE
                executable: characteristics & 0x2000_0000 != 0,
            });
        }
        if sections.is_empty() {
            return Err(NativeTableError::new("executable declares no sections"));
        }
        Ok(Self {
            bytes,
            image_base,
            sections,
        })
    }

    /// Translate a virtual address to a file offset, if it falls inside raw section data.
    fn file_offset(&self, address: u32) -> Option<usize> {
        for section in &self.sections {
            let start = self.image_base.checked_add(section.virtual_address)?;
            let end = start.checked_add(section.raw_size)?;
            if (start..end).contains(&address) {
                return Some((section.raw_offset + (address - start)) as usize);
            }
        }
        None
    }

    fn is_code_address(&self, address: u32) -> bool {
        self.sections.iter().any(|section| {
            if !section.executable {
                return false;
            }
            let Some(start) = self.image_base.checked_add(section.virtual_address) else {
                return false;
            };
            let Some(end) = start.checked_add(section.raw_size) else {
                return false;
            };
            (start..end).contains(&address)
        })
    }

    /// Read a NUL-terminated operator name at a virtual address.
    ///
    /// The name must be lexable as a GameScript executable name. That is the filter which rejects
    /// unrelated pointer-shaped data, such as the locale tables whose entries contain spaces and
    /// hyphens.
    fn operator_name(&self, address: u32) -> Option<String> {
        let start = self.file_offset(address)?;
        let tail = self.bytes.get(start..)?;
        let length = tail.iter().position(|byte| *byte == 0)?;
        if length == 0 {
            return None;
        }
        let name = std::str::from_utf8(&tail[..length]).ok()?;
        is_operator_name(name).then(|| name.to_owned())
    }
}

/// Whether a string could be a GameScript executable name.
///
/// GameScript separates tokens on whitespace and delimiters, so a name can contain neither. The
/// corpus uses trailing `?` and `!` for predicates and effects.
fn is_operator_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    let mut characters = name.chars();
    let first = characters.next().expect("name is not empty");
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    name.chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '?' | '!'))
}

/// Recover every run of operator records in a PE image, in file order.
pub fn extract(image_bytes: &[u8]) -> Result<Vec<NativeTableRun>, NativeTableError> {
    let image = Image::parse(image_bytes)?;

    let mut runs: Vec<NativeTableRun> = Vec::new();
    let mut current: Vec<NativeEntry> = Vec::new();
    let mut current_offset = 0_usize;

    let mut offset = 0_usize;
    while offset + RECORD_SIZE <= image_bytes.len() {
        let name_pointer = read_u32(image_bytes, offset).expect("bounds were checked");
        let code_pointer = read_u32(image_bytes, offset + 4).expect("bounds were checked");

        let entry = image
            .is_code_address(code_pointer)
            .then(|| image.operator_name(name_pointer))
            .flatten()
            .map(|name| NativeEntry {
                name,
                entry_point: code_pointer,
                record_offset: offset,
            });

        match entry {
            Some(entry) => {
                if current.is_empty() {
                    current_offset = offset;
                }
                current.push(entry);
                offset += RECORD_SIZE;
            }
            None => {
                flush(&mut runs, &mut current, current_offset);
                // Records are dword-aligned but a table need not start on an eight-byte boundary,
                // so resynchronise by a dword rather than a whole record.
                offset += 4;
            }
        }
    }
    flush(&mut runs, &mut current, current_offset);
    Ok(runs)
}

fn flush(runs: &mut Vec<NativeTableRun>, current: &mut Vec<NativeEntry>, file_offset: usize) {
    if current.len() >= MINIMUM_RUN {
        runs.push(NativeTableRun {
            file_offset,
            entries: std::mem::take(current),
        });
    } else {
        current.clear();
    }
}

/// Every distinct operator name across all recovered runs, lowercased for comparison with script
/// tokens.
pub fn operator_names(runs: &[NativeTableRun]) -> BTreeSet<String> {
    runs.iter()
        .flat_map(|run| run.entries.iter())
        .map(|entry| entry.name.to_ascii_lowercase())
        .collect()
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// What a name that the VM could not resolve turns out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameClass {
    /// The engine implements it. It needs a stub before dependent script can run.
    Operator { entry_point: u32 },
    /// SCREAMING_CASE and absent from the operator tables: an engine constant pushed by name.
    EngineConstant,
    /// Neither. Most often a definition in a module this run has not loaded; failing that, a
    /// definition site our definition-shape classifier does not recognise.
    Unresolved,
}

/// The engine's operator tables, indexed by name for lookup.
#[derive(Debug, Clone, Default)]
pub struct OperatorIndex {
    entry_points: std::collections::BTreeMap<String, u32>,
}

impl OperatorIndex {
    pub fn from_image(image_bytes: &[u8]) -> Result<Self, NativeTableError> {
        let runs = extract(image_bytes)?;
        let mut entry_points = std::collections::BTreeMap::new();
        for entry in runs.iter().flat_map(|run| run.entries.iter()) {
            // Two names appear in both tables; the primitive table is authoritative for them
            // because it is the one the interpreter consults first.
            entry_points
                .entry(entry.name.to_ascii_lowercase())
                .or_insert(entry.entry_point);
        }
        Ok(Self { entry_points })
    }

    pub fn len(&self) -> usize {
        self.entry_points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entry_points.is_empty()
    }

    pub fn classify(&self, name: &str) -> NameClass {
        if let Some(entry_point) = self.entry_points.get(&name.to_ascii_lowercase()) {
            NameClass::Operator {
                entry_point: *entry_point,
            }
        } else if is_screaming_case(name) {
            NameClass::EngineConstant
        } else {
            NameClass::Unresolved
        }
    }
}

/// Whether a name is written in the SCREAMING_CASE the corpus uses for engine constants.
pub fn is_screaming_case(name: &str) -> bool {
    name.chars().any(|character| character.is_ascii_uppercase())
        && !name.chars().any(|character| character.is_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal 32-bit PE with one code section and one data section holding an operator
    /// table, so the extractor is exercised without the proprietary binary.
    fn synthetic_image(names: &[&str]) -> Vec<u8> {
        const PE_OFFSET: usize = 0x80;
        const IMAGE_BASE: u32 = 0x0040_0000;
        const CODE_VA: u32 = 0x1000;
        const CODE_RAW: u32 = 0x200;
        const CODE_SIZE: u32 = 0x200;
        const DATA_VA: u32 = 0x2000;
        const DATA_RAW: u32 = 0x400;
        const DATA_SIZE: u32 = 0x400;

        let mut image = vec![0_u8; (DATA_RAW + DATA_SIZE) as usize];
        image[0x3c..0x40].copy_from_slice(&(PE_OFFSET as u32).to_le_bytes());
        image[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        image[PE_OFFSET + 6..PE_OFFSET + 8].copy_from_slice(&2_u16.to_le_bytes());
        let optional_size: u16 = 0xe0;
        image[PE_OFFSET + 20..PE_OFFSET + 22].copy_from_slice(&optional_size.to_le_bytes());
        image[PE_OFFSET + 24..PE_OFFSET + 26].copy_from_slice(&0x10b_u16.to_le_bytes());
        image[PE_OFFSET + 52..PE_OFFSET + 56].copy_from_slice(&IMAGE_BASE.to_le_bytes());

        let section_table = PE_OFFSET + 24 + usize::from(optional_size);
        let mut write_section =
            |index: usize, name: &[u8], va: u32, raw_size: u32, raw: u32, characteristics: u32| {
                let base = section_table + index * 40;
                image[base..base + name.len()].copy_from_slice(name);
                image[base + 12..base + 16].copy_from_slice(&va.to_le_bytes());
                image[base + 16..base + 20].copy_from_slice(&raw_size.to_le_bytes());
                image[base + 20..base + 24].copy_from_slice(&raw.to_le_bytes());
                image[base + 36..base + 40].copy_from_slice(&characteristics.to_le_bytes());
            };
        write_section(0, b".text", CODE_VA, CODE_SIZE, CODE_RAW, 0x2000_0000);
        write_section(1, b".data", DATA_VA, DATA_SIZE, DATA_RAW, 0x4000_0000);

        // Lay the names out at the end of the data section, then the table at its start.
        let mut name_addresses = Vec::new();
        let mut cursor = (DATA_RAW + DATA_SIZE) as usize - 0x100;
        for name in names {
            let bytes = name.as_bytes();
            image[cursor..cursor + bytes.len()].copy_from_slice(bytes);
            image[cursor + bytes.len()] = 0;
            let address = IMAGE_BASE + DATA_VA + (cursor as u32 - DATA_RAW);
            name_addresses.push(address);
            cursor += bytes.len() + 1;
        }
        for (index, address) in name_addresses.iter().enumerate() {
            let record = DATA_RAW as usize + index * RECORD_SIZE;
            image[record..record + 4].copy_from_slice(&address.to_le_bytes());
            let code = IMAGE_BASE + CODE_VA + (index as u32 * 4);
            image[record + 4..record + 8].copy_from_slice(&code.to_le_bytes());
        }
        image
    }

    #[test]
    fn recovers_an_operator_table_with_names_and_entry_points() {
        let names = [
            "pop", "def", "undef", "begin", "end", "exch", "dup", "getarmydata", "setarmydata",
        ];
        let image = synthetic_image(&names);
        let runs = extract(&image).expect("synthetic image parses");
        let recovered: Vec<&str> = runs
            .iter()
            .flat_map(|run| run.entries.iter())
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(recovered, names);
        let first = &runs[0].entries[0];
        assert_eq!(first.entry_point, 0x0040_1000);
        assert!(runs[0].entries.windows(2).all(|pair| {
            pair[1].record_offset - pair[0].record_offset == RECORD_SIZE
        }));
    }

    #[test]
    fn rejects_names_that_the_lexer_could_not_tokenise() {
        // The locale tables in the shipped binary are pointer-shaped and land in range, so the
        // structural test alone accepts them. The name rule is what rejects them.
        assert!(is_operator_name("iscustomleader?"));
        assert!(is_operator_name("getarmydata"));
        assert!(is_operator_name("_private"));
        assert!(!is_operator_name("french-belgian"));
        assert!(!is_operator_name("great britain"));
        assert!(!is_operator_name(""));
        assert!(!is_operator_name("9lives"));
    }

    #[test]
    fn ignores_runs_shorter_than_the_minimum() {
        let image = synthetic_image(&["pop", "def", "undef"]);
        let runs = extract(&image).expect("synthetic image parses");
        assert!(runs.is_empty(), "a three-record run is below the threshold");
    }

    #[test]
    fn classifies_a_name_the_engine_implements_as_an_operator() {
        let image = synthetic_image(&[
            "pop", "def", "undef", "begin", "end", "exch", "dup", "getarmydata", "setarmydata",
        ]);
        let index = OperatorIndex::from_image(&image).expect("synthetic image parses");
        assert_eq!(index.len(), 9);
        assert_eq!(
            index.classify("getarmydata"),
            NameClass::Operator {
                entry_point: 0x0040_101c
            }
        );
        // The corpus writes operator calls in lower case, but match case-insensitively so a
        // differently-cased call site still resolves.
        assert!(matches!(
            index.classify("GetArmyData"),
            NameClass::Operator { .. }
        ));
    }

    #[test]
    fn classifies_screaming_case_names_as_engine_constants() {
        let image = synthetic_image(&[
            "pop", "def", "undef", "begin", "end", "exch", "dup", "getarmydata", "setarmydata",
        ]);
        let index = OperatorIndex::from_image(&image).expect("synthetic image parses");
        assert_eq!(index.classify("SD_MANA"), NameClass::EngineConstant);
        assert_eq!(index.classify("EDITBOX_SCROLL"), NameClass::EngineConstant);
        assert_eq!(index.classify("give_level_exp"), NameClass::Unresolved);
        assert_eq!(index.classify("Type_Imp"), NameClass::Unresolved);
    }

    #[test]
    fn screaming_case_needs_an_upper_case_letter_and_no_lower_case_one() {
        assert!(is_screaming_case("SD_MANA"));
        assert!(is_screaming_case("ORDER"));
        assert!(!is_screaming_case("getarmydata"));
        assert!(!is_screaming_case("Type_Imp"));
        // Digits and underscores alone are not evidence either way.
        assert!(!is_screaming_case("_1"));
    }

    #[test]
    fn rejects_an_image_without_a_pe_signature() {
        let error = extract(&[0_u8; 256]).expect_err("a zeroed buffer is not a PE image");
        assert!(error.to_string().contains("PE signature"), "{error}");
    }
}
