//! Throwaway: dump a linear disassembly of one native entry point.
use iced_x86::{Decoder, DecoderOptions, Formatter, NasmFormatter};
use lom_asset_viewer::native_table::PeImage;

fn main() {
    let mut args = std::env::args().skip(1);
    let exe = args.next().expect("exe path");
    let addr = u32::from_str_radix(
        args.next().expect("entry point").trim_start_matches("0x"),
        16,
    )
    .expect("hex entry point");
    let count: usize = args.next().map_or(80, |v| v.parse().unwrap());

    let bytes = std::fs::read(&exe).expect("read exe");
    let image = PeImage::parse(&bytes).expect("parse pe");
    let offset = image.file_offset(addr).expect("address not mapped");
    let mut decoder = Decoder::with_ip(32, &image.bytes()[offset..], u64::from(addr), DecoderOptions::NONE);
    let mut formatter = NasmFormatter::new();
    let mut text = String::new();
    for instruction in decoder.iter().take(count) {
        text.clear();
        formatter.format(&instruction, &mut text);
        println!("{:08x}  {}", instruction.ip(), text);
    }
}
