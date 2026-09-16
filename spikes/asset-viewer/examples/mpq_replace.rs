//! Throwaway feasibility probe: replace one member of an MPQ in place with StormLib.
//!
//! Usage: mpq_replace ARCHIVE 'member\name' LOCAL_FILE
//! The archive is modified in place, so always run it against a copy.
use std::ffi::CString;
use std::os::raw::c_char;

type Handle = *mut std::ffi::c_void;

const MPQ_FILE_IMPLODE: u32 = 0x0000_0100;
const MPQ_FILE_REPLACEEXISTING: u32 = 0x8000_0000;

#[link(name = "storm")]
unsafe extern "C" {
    fn SFileOpenArchive(name: *const c_char, priority: u32, flags: u32, archive: *mut Handle)
        -> bool;
    fn SFileCloseArchive(archive: Handle) -> bool;
    fn SFileAddFileEx(
        archive: Handle,
        local_name: *const c_char,
        archived_name: *const c_char,
        flags: u32,
        compression: u32,
        compression_next: u32,
    ) -> bool;
    fn SFileCompactArchive(archive: Handle, listfile: *const c_char, reserved: bool) -> bool;
    fn SErrGetLastError() -> u32;
}

fn main() {
    let mut args = std::env::args().skip(1);
    let archive_path = CString::new(args.next().expect("archive path")).unwrap();
    let member = CString::new(args.next().expect("member name")).unwrap();
    let local = CString::new(args.next().expect("local file")).unwrap();

    let mut archive: Handle = std::ptr::null_mut();
    // SAFETY: all pointers outlive the call; the handle is closed exactly once below.
    if !unsafe { SFileOpenArchive(archive_path.as_ptr(), 0, 0, &mut archive) } {
        panic!("open failed, error {}", unsafe { SErrGetLastError() });
    }
    let added = unsafe {
        SFileAddFileEx(
            archive,
            local.as_ptr(),
            member.as_ptr(),
            MPQ_FILE_IMPLODE | MPQ_FILE_REPLACEEXISTING,
            0,
            0,
        )
    };
    if !added {
        let code = unsafe { SErrGetLastError() };
        unsafe { SFileCloseArchive(archive) };
        panic!("add failed, error {code}");
    }
    unsafe { SFileCompactArchive(archive, std::ptr::null(), false) };
    unsafe { SFileCloseArchive(archive) };
    println!("replaced");
}
