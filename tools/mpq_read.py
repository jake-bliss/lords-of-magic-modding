#!/usr/bin/env python3
"""Read members out of a Lords of Magic MPQ archive. Python standard library only.

    python3 tools/mpq_read.py ARCHIVE --names NAMES.txt --out DIR

This exists for the player's machine. Every other MPQ reader in this repo goes through StormLib, and
the only build of it is a macOS one; the HD overlay's recipe has to extract portraits on Windows
from the player's own `pic.mpq` with nothing installed but Python.

It reads what Lords of Magic archives use and refuses everything else, loudly:

  - format 0 archives (the 32-byte header), hash and block tables encrypted as usual
  - members stored whole, PKWARE DCL "imploded" (flag 0x100), or "compressed" (0x200) with a
    compression mask of PKWARE (0x08) or zlib (0x02). Observed in pic.mpq: only 0x08.
  - members encrypted with the name-derived key (0x10000), with or without FIX_KEY (0x20000)

Correctness is checked against StormLib, not against itself: tests/test_mpq_read.py extracts every
named member of the installed archives with both readers and requires identical bytes.

The decompressor is a port of Mark Adler's blast.c (zlib contrib/blast, zlib licence), the reference
decoder for the PKWARE Data Compression Library format.
"""
from __future__ import annotations

import argparse
import pathlib
import struct
import zlib

HEADER = struct.Struct("<4sIIHHIIII")
MAGIC = b"MPQ\x1a"

FLAG_IMPLODE = 0x00000100
FLAG_COMPRESS = 0x00000200
FLAG_ENCRYPTED = 0x00010000
FLAG_FIX_KEY = 0x00020000
FLAG_SINGLE_UNIT = 0x01000000
FLAG_SECTOR_CRC = 0x04000000
FLAG_EXISTS = 0x80000000
# SECTOR_CRC is refused, not parsed: no Lords of Magic archive uses it, and reading past the
# checksums without verifying them would return corrupt sectors as good ones. (Codex review.)
KNOWN_FLAGS = FLAG_IMPLODE | FLAG_COMPRESS | FLAG_ENCRYPTED | FLAG_FIX_KEY | FLAG_EXISTS

HASH_EMPTY = 0xFFFFFFFF
HASH_DELETED = 0xFFFFFFFE

M32 = 0xFFFFFFFF


class MpqError(Exception):
    pass


def _crypt_table() -> list[int]:
    table = [0] * 0x500
    seed = 0x00100001
    for index1 in range(0x100):
        index2 = index1
        for _ in range(5):
            seed = (seed * 125 + 3) % 0x2AAAAB
            high = (seed & 0xFFFF) << 16
            seed = (seed * 125 + 3) % 0x2AAAAB
            table[index2] = high | (seed & 0xFFFF)
            index2 += 0x100
    return table


CRYPT = _crypt_table()


def hash_string(name: str, kind: int) -> int:
    seed1, seed2 = 0x7FED7FED, 0xEEEEEEEE
    for ch in name.upper().encode("latin-1"):
        seed1 = (CRYPT[kind * 0x100 + ch] ^ (seed1 + seed2)) & M32
        seed2 = (ch + seed1 + seed2 + (seed2 << 5) + 3) & M32
    return seed1


def decrypt(data: bytes, key: int) -> bytes:
    """Whole dwords only; a trailing partial dword is stored in the clear."""
    whole = len(data) // 4
    words = struct.unpack_from(f"<{whole}I", data)
    out = []
    seed = 0xEEEEEEEE
    for word in words:
        seed = (seed + CRYPT[0x400 + (key & 0xFF)]) & M32
        plain = word ^ ((key + seed) & M32)
        key = ((((~key) << 0x15) + 0x11111111) & M32) | (key >> 0x0B)
        seed = (plain + seed + (seed << 5) + 3) & M32
        out.append(plain)
    return struct.pack(f"<{whole}I", *out) + data[whole * 4:]


# --- PKWARE DCL explode, ported from blast.c ---------------------------------------------------

_MAXBITS = 13
_LITLEN = bytes([
    11, 124, 8, 7, 28, 7, 188, 13, 76, 4, 10, 8, 12, 10, 12, 10, 8, 23, 8,
    9, 7, 6, 7, 8, 7, 6, 55, 8, 23, 24, 12, 11, 7, 9, 11, 12, 6, 7, 22, 5,
    7, 24, 6, 11, 9, 6, 7, 22, 7, 11, 38, 7, 9, 8, 25, 11, 8, 11, 9, 12,
    8, 12, 5, 38, 5, 38, 5, 11, 7, 5, 6, 21, 6, 10, 53, 8, 7, 24, 10, 27,
    44, 253, 253, 253, 252, 252, 252, 13, 12, 45, 12, 45, 12, 61, 12, 45,
    44, 173])
_LENLEN = bytes([2, 35, 36, 53, 38, 23])
_DISTLEN = bytes([2, 20, 53, 230, 247, 151, 248])
_BASE = (3, 2, 4, 5, 6, 7, 8, 9, 10, 12, 16, 24, 40, 72, 136, 264)
_EXTRA = (0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8)


def _construct(rep: bytes) -> tuple[list[int], list[int]]:
    """Compact code-length representation -> (count per length, symbols in canonical order)."""
    lengths = []
    for byte in rep:
        lengths += [byte & 15] * ((byte >> 4) + 1)
    count = [0] * (_MAXBITS + 1)
    for length in lengths:
        count[length] += 1
    offs = [0] * (_MAXBITS + 2)
    for length in range(1, _MAXBITS):
        offs[length + 1] = offs[length] + count[length]
    symbol = [0] * len(lengths)
    for sym, length in enumerate(lengths):
        if length:
            symbol[offs[length]] = sym
            offs[length] += 1
    return count, symbol


_LITCODE = _construct(_LITLEN)
_LENCODE = _construct(_LENLEN)
_DISTCODE = _construct(_DISTLEN)


class _Bits:
    def __init__(self, data: bytes) -> None:
        self.data, self.pos, self.buf, self.cnt = data, 0, 0, 0

    def bits(self, need: int) -> int:
        while self.cnt < need:
            if self.pos >= len(self.data):
                raise MpqError("imploded data ends mid-code")
            self.buf |= self.data[self.pos] << self.cnt
            self.pos += 1
            self.cnt += 8
        value = self.buf & ((1 << need) - 1)
        self.buf >>= need
        self.cnt -= need
        return value

    def decode(self, table: tuple[list[int], list[int]]) -> int:
        """Codes are stored bit-inverted, first bit first -- blast.c's decode()."""
        count, symbol = table
        code = first = index = 0
        for length in range(1, _MAXBITS + 1):
            code |= self.bits(1) ^ 1
            n = count[length]
            if code < first + n:
                return symbol[index + code - first]
            index += n
            first = (first + n) << 1
            code <<= 1
        raise MpqError("imploded data holds an invalid code")


def explode(data: bytes, expected: int) -> bytes:
    """PKWARE DCL -> bytes. Decodes to the end code, as blast.c does, and requires exactly
    `expected` bytes: a stream that ends early or runs long means the block's size is wrong, and
    stopping at `expected` would hand back truncated data as if it were whole. (Codex review.)"""
    s = _Bits(data)
    lit = s.bits(8)
    if lit > 1:
        raise MpqError(f"imploded data: literal mode {lit}")
    dict_bits = s.bits(8)
    if not 4 <= dict_bits <= 6:
        raise MpqError(f"imploded data: dictionary size {dict_bits}")
    out = bytearray()
    while True:
        if len(out) > expected:
            raise MpqError(f"imploded data runs past its {expected} bytes")
        if s.bits(1):
            symbol = s.decode(_LENCODE)
            length = _BASE[symbol] + s.bits(_EXTRA[symbol])
            if length == 519:
                break
            shift = 2 if length == 2 else dict_bits
            dist = (s.decode(_DISTCODE) << shift) + s.bits(shift) + 1
            if dist > len(out):
                raise MpqError("imploded data copies from before its start")
            for _ in range(length):
                out.append(out[-dist])
        else:
            out.append(s.decode(_LITCODE) if lit else s.bits(8))
    if len(out) != expected:
        raise MpqError(f"imploded data holds {len(out)} bytes, {expected} expected")
    return bytes(out)


# --- archive ------------------------------------------------------------------------------------

class Archive:
    def __init__(self, path: pathlib.Path) -> None:
        self.path = pathlib.Path(path)
        self.data = self.path.read_bytes()
        (magic, header_size, _archive_size, version, shift, hash_pos, block_pos, hash_count,
         block_count) = self._header()
        if version != 0:
            raise MpqError(f"{path}: MPQ format {version}; only format 0 is supported")
        if hash_count == 0 or hash_count & (hash_count - 1):
            raise MpqError(f"{path}: hash table size {hash_count} is not a power of two")
        self.sector_size = 512 << shift
        self.hashes = [struct.unpack_from("<IIHHI", t, i * 16) for t in
                       [self._table(hash_pos, hash_count, "(hash table)")] for i in range(hash_count)]
        self.blocks = [struct.unpack_from("<IIII", t, i * 16) for t in
                       [self._table(block_pos, block_count, "(block table)")] for i in range(block_count)]

    def _header(self):
        if len(self.data) < HEADER.size:
            raise MpqError(f"{self.path}: too short to be an MPQ archive")
        fields = HEADER.unpack_from(self.data, 0)
        if fields[0] != MAGIC:
            raise MpqError(f"{self.path}: not an MPQ archive (no header at offset 0)")
        return fields

    def _table(self, pos: int, count: int, name: str) -> bytes:
        raw = self.data[pos:pos + count * 16]
        if len(raw) != count * 16:
            raise MpqError(f"{self.path}: {name} runs past the end of the file")
        return decrypt(raw, hash_string(name, 3))

    def _block_index(self, name: str) -> int | None:
        mask = len(self.hashes) - 1
        start = hash_string(name, 0) & mask
        name1, name2 = hash_string(name, 1), hash_string(name, 2)
        for step in range(len(self.hashes)):
            n1, n2, _locale, _platform, block = self.hashes[(start + step) & mask]
            if block == HASH_EMPTY:
                return None
            if block != HASH_DELETED and n1 == name1 and n2 == name2:
                return block
        return None

    def __contains__(self, name: str) -> bool:
        return self._block_index(name) is not None

    def read(self, name: str) -> bytes:
        block = self._block_index(name)
        if block is None or block >= len(self.blocks):
            raise KeyError(name)
        offset, packed_size, size, flags = self.blocks[block]
        if not flags & FLAG_EXISTS:
            raise KeyError(name)
        if flags & ~KNOWN_FLAGS:
            raise MpqError(f"{name}: unsupported flags {flags:#010x}")
        raw = self.data[offset:offset + packed_size]
        if len(raw) != packed_size:
            raise MpqError(f"{name}: runs past the end of the archive")

        key = None
        if flags & FLAG_ENCRYPTED:
            key = hash_string(name.replace("/", "\\").rsplit("\\", 1)[-1], 3)
            if flags & FLAG_FIX_KEY:
                key = ((key + offset) ^ size) & M32

        sectors = (size + self.sector_size - 1) // self.sector_size
        packed = flags & (FLAG_IMPLODE | FLAG_COMPRESS)
        if packed:
            entries = sectors + 1
            table = raw[:entries * 4]
            if len(table) != entries * 4:
                raise MpqError(f"{name}: sector table runs past the member")
            if key is not None:
                table = decrypt(table, (key - 1) & M32)
            bounds = struct.unpack(f"<{entries}I", table)
        else:
            bounds = [min(i * self.sector_size, packed_size) for i in range(sectors + 1)]

        out = bytearray()
        for i in range(sectors):
            chunk = raw[bounds[i]:bounds[i + 1]]
            if bounds[i + 1] < bounds[i] or bounds[i + 1] > packed_size:
                raise MpqError(f"{name}: sector {i} out of bounds")
            if key is not None:
                chunk = decrypt(chunk, (key + i) & M32)
            want = min(self.sector_size, size - i * self.sector_size)
            out += self._unpack(name, chunk, want, flags) if packed else chunk
        if len(out) != size:
            raise MpqError(f"{name}: {len(out)} bytes read, {size} expected")
        return bytes(out)

    @staticmethod
    def _unpack(name: str, chunk: bytes, want: int, flags: int) -> bytes:
        if len(chunk) >= want:          # stored: compressing this sector did not help
            return chunk[:want]
        if flags & FLAG_IMPLODE:
            return explode(chunk, want)
        mask, body = chunk[0], chunk[1:]
        if mask == 0x08:
            return explode(body, want)
        if mask == 0x02:
            return zlib.decompress(body)
        raise MpqError(f"{name}: unsupported compression mask {mask:#04x}")

    def listfile(self) -> list[str]:
        """The archive's own (listfile), when it has one. Vanilla pic.mpq does not."""
        if "(listfile)" not in self:
            return []
        text = self.read("(listfile)").decode("latin-1")
        return [line.strip() for line in text.replace("\r", "\n").split("\n") if line.strip()]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("archive", type=pathlib.Path)
    parser.add_argument("--names", type=pathlib.Path, required=True,
                        help="member names to extract, one per line; absent ones are skipped")
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()

    archive = Archive(args.archive)
    names = [n.strip() for n in args.names.read_text().splitlines() if n.strip()]
    args.out.mkdir(parents=True, exist_ok=True)
    found = 0
    for name in names:
        if name in archive:
            (args.out / name.replace("/", "\\").rsplit("\\", 1)[-1]).write_bytes(archive.read(name))
            found += 1
    print(f"{args.out}: {found} of {len(names)} named members extracted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
