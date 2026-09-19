//! Throwaway: hexdump the bytes at a virtual address, as bytes and as dwords.
//!
//! The companion to `disasm.rs`. `disasm` reads code; this reads the tables code jumps through.
//! It is what read the `LS_SPR_` dispatch table at `0x004F73B8` and each class's vtable, and it is
//! here so that reading is reproducible rather than something a past pass asserts.
//!
//! ```text
//! cargo run --example dumpva -- path/to/lomse.exe 0x004F73B8 48
//! ```
use lom_asset_viewer::native_table::PeImage;
fn main() {
    let mut args = std::env::args().skip(1);
    let exe = args.next().expect("exe path");
    let addr =
        u32::from_str_radix(args.next().expect("va").trim_start_matches("0x"), 16).expect("hex");
    let count: usize = args.next().map_or(64, |v| v.parse().unwrap());
    let bytes = std::fs::read(&exe).expect("read exe");
    let image = PeImage::parse(&bytes).expect("parse pe");
    let off = image.file_offset(addr).expect("unmapped");
    let b = &image.bytes()[off..off + count];
    for (i, chunk) in b.chunks(16).enumerate() {
        print!("{:08x} ", addr as usize + i * 16);
        for c in chunk {
            print!("{:02x} ", c);
        }
        print!("  ");
        for c in chunk {
            print!(
                "{}",
                if c.is_ascii_graphic() {
                    *c as char
                } else {
                    '.'
                }
            );
        }
        println!();
    }
    println!("--- as dwords ---");
    for (i, c) in b.chunks(4).enumerate() {
        if c.len() == 4 {
            println!(
                "{:08x}  {:08x}",
                addr as usize + i * 4,
                u32::from_le_bytes([c[0], c[1], c[2], c[3]])
            );
        }
    }
}
