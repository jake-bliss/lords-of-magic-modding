use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt;
use std::path::Path;
use std::ptr;

const MAX_PATH: usize = 260;
const MPQ_OPEN_READ_ONLY: u32 = 0x0000_0100;
const SFILE_OPEN_FROM_MPQ: u32 = 0;
const SFILE_INVALID_SIZE: u32 = u32::MAX;

type Handle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct SFileFindData {
    file_name: [c_char; MAX_PATH],
    plain_name: *mut c_char,
    hash_index: u32,
    block_index: u32,
    file_size: u32,
    file_flags: u32,
    compressed_size: u32,
    file_time_low: u32,
    file_time_high: u32,
    locale: u32,
}

unsafe extern "C" {
    fn SFileOpenArchive(
        archive_name: *const c_char,
        priority: u32,
        flags: u32,
        archive: *mut Handle,
    ) -> bool;
    fn SFileCloseArchive(archive: Handle) -> bool;
    fn SFileOpenFileEx(
        archive: Handle,
        file_name: *const c_char,
        search_scope: u32,
        file: *mut Handle,
    ) -> bool;
    fn SFileGetFileSize(file: Handle, size_high: *mut u32) -> u32;
    fn SFileReadFile(
        file: Handle,
        buffer: *mut c_void,
        bytes_to_read: u32,
        bytes_read: *mut u32,
        overlapped: *mut c_void,
    ) -> bool;
    fn SFileCloseFile(file: Handle) -> bool;
    fn SFileAddListFileEntries(
        archive: Handle,
        entries: *const *const c_char,
        entry_count: u32,
    ) -> u32;
    fn SFileFindFirstFile(
        archive: Handle,
        mask: *const c_char,
        find_data: *mut SFileFindData,
        list_file: *const c_char,
    ) -> Handle;
    fn SFileFindNextFile(search: Handle, find_data: *mut SFileFindData) -> bool;
    fn SFileFindClose(search: Handle) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub size: u32,
    pub compressed_size: u32,
    pub flags: u32,
    pub locale: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpqError(String);

impl MpqError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for MpqError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MpqError {}

pub struct Archive {
    handle: Handle,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self, MpqError> {
        let path = path
            .to_str()
            .ok_or_else(|| MpqError::new("archive path is not valid UTF-8"))?;
        let path =
            CString::new(path).map_err(|_| MpqError::new("archive path contains a null byte"))?;
        let mut handle = ptr::null_mut();

        // SAFETY: StormLib receives a valid NUL-terminated path and an out pointer.
        if !unsafe { SFileOpenArchive(path.as_ptr(), 0, MPQ_OPEN_READ_ONLY, &mut handle) }
            || handle.is_null()
        {
            return Err(MpqError::new("StormLib could not open the MPQ archive"));
        }

        let archive = Self { handle };
        archive.load_internal_listfile()?;
        Ok(archive)
    }

    pub fn entries(&self) -> Result<Vec<Entry>, MpqError> {
        let mask = CString::new("*").expect("static mask contains no null byte");
        let mut data = empty_find_data();
        // SAFETY: The archive is open, mask and output storage remain valid for the call.
        let search =
            unsafe { SFileFindFirstFile(self.handle, mask.as_ptr(), &mut data, ptr::null()) };
        if search.is_null() {
            return Err(MpqError::new("StormLib could not enumerate the archive"));
        }

        let mut entries = Vec::new();
        loop {
            entries.push(entry_from_find_data(&data)?);
            // SAFETY: The search handle and output storage are valid until closed below.
            if !unsafe { SFileFindNextFile(search, &mut data) } {
                break;
            }
        }
        // SAFETY: search came from SFileFindFirstFile and is closed exactly once.
        unsafe { SFileFindClose(search) };
        entries.sort_by_key(|entry| entry.name.to_ascii_lowercase());
        Ok(entries)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, MpqError> {
        let name = CString::new(name)
            .map_err(|_| MpqError::new("archive filename contains a null byte"))?;
        let mut file = ptr::null_mut();
        // SAFETY: The archive is open, filename is NUL-terminated, and file is an out pointer.
        if !unsafe { SFileOpenFileEx(self.handle, name.as_ptr(), SFILE_OPEN_FROM_MPQ, &mut file) }
            || file.is_null()
        {
            return Err(MpqError::new(format!(
                "StormLib could not open archive member {}",
                name.to_string_lossy()
            )));
        }

        // SAFETY: file is a live StormLib file handle.
        let size = unsafe { SFileGetFileSize(file, ptr::null_mut()) };
        if size == SFILE_INVALID_SIZE {
            // SAFETY: file is live and must be closed on this error path.
            unsafe { SFileCloseFile(file) };
            return Err(MpqError::new("StormLib could not determine member size"));
        }

        let mut bytes = vec![0_u8; size as usize];
        let mut bytes_read = 0;
        // SAFETY: bytes has capacity for size bytes; all pointers are valid for this call.
        let read_ok = unsafe {
            SFileReadFile(
                file,
                bytes.as_mut_ptr().cast(),
                size,
                &mut bytes_read,
                ptr::null_mut(),
            )
        };
        // SAFETY: file is live and is closed exactly once.
        unsafe { SFileCloseFile(file) };

        if !read_ok || bytes_read != size {
            return Err(MpqError::new("StormLib returned a short read"));
        }
        Ok(bytes)
    }

    fn load_internal_listfile(&self) -> Result<(), MpqError> {
        let Ok(contents) = self.read("(listfile)") else {
            return Ok(());
        };

        let names: Vec<CString> = contents
            .split(|byte| *byte == b'\n')
            .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
            .filter(|line| !line.is_empty())
            .filter_map(|line| CString::new(line).ok())
            .collect();
        let pointers: Vec<*const c_char> = names.iter().map(|name| name.as_ptr()).collect();
        if pointers.is_empty() {
            return Ok(());
        }

        // SAFETY: Every pointer references a live CString for the duration of the call.
        let result = unsafe {
            SFileAddListFileEntries(self.handle, pointers.as_ptr(), pointers.len() as u32)
        };
        if result != 0 {
            return Err(MpqError::new("StormLib rejected the internal listfile"));
        }
        Ok(())
    }
}

impl Drop for Archive {
    fn drop(&mut self) {
        // SAFETY: handle was returned by SFileOpenArchive and is closed exactly once.
        unsafe { SFileCloseArchive(self.handle) };
    }
}

fn empty_find_data() -> SFileFindData {
    SFileFindData {
        file_name: [0; MAX_PATH],
        plain_name: ptr::null_mut(),
        hash_index: 0,
        block_index: 0,
        file_size: 0,
        file_flags: 0,
        compressed_size: 0,
        file_time_low: 0,
        file_time_high: 0,
        locale: 0,
    }
}

fn entry_from_find_data(data: &SFileFindData) -> Result<Entry, MpqError> {
    // SAFETY: StormLib guarantees that file_name is NUL-terminated on successful enumeration.
    let name = unsafe { CStr::from_ptr(data.file_name.as_ptr()) }
        .to_str()
        .map_err(|_| MpqError::new("archive member name is not valid UTF-8"))?
        .to_owned();
    Ok(Entry {
        name,
        size: data.file_size,
        compressed_size: data.compressed_size,
        flags: data.file_flags,
        locale: data.locale,
    })
}
