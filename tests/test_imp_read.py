"""The standard-library IMP decoder the HD overlay's setup runs on the player's machine.

The decisive tests are the corpus ones, checked against the Rust decoder it was ported from (the
asset viewer), never against itself:

- every IMP member of an installed imp.mpq has as many frames as `--describe-imp` lists, and a
  member one refuses the other refuses too;
- every frame of every member -- duplicates and shared-pixel frames through what they resolve to --
  has the indices, the palette and the colour key `--export-imp-frame` writes.

Both need the archive and a built viewer (`cargo build --release` in spikes/asset-viewer), and skip
without them; the skip names what was missing. A summary line on stderr says how much was compared.
The unit tests pin the pieces a corpus cannot isolate.
"""

from __future__ import annotations

import collections
import concurrent.futures
import hashlib
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import imp_read  # noqa: E402
import mpq_read  # noqa: E402
from png_index_patch import read_indexed_png  # noqa: E402

VIEWER = ROOT / "spikes/asset-viewer/target/release/lom-asset-viewer"
LISTFILE = ROOT / "reports/member-names/all-profiles-imp-recovered.txt"
GAME = "drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English"
ARCHIVES = {
    "vanilla (3.02)": pathlib.Path.home() / "Applications/Lords of Magic 3.02.app/Contents/SharedSupport/prefix" / GAME / "imp.mpq",
    "GS5R3": ROOT / "artifacts/experiment-backups/gs5r3-20260916/imp.mpq.orig",
}
WORKERS = 8


def distinct_archives() -> dict[str, pathlib.Path]:
    """The archives present, one per content: the two installs here hold the same imp.mpq, and a
    second sweep of the same bytes would double the run for nothing."""
    seen, out = set(), {}
    for label, path in ARCHIVES.items():
        if path.is_file():
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            if digest not in seen:
                seen.add(digest)
                out[label] = path
    return out


def imp_members() -> list[str]:
    return sorted({line.strip().lower() for line in LISTFILE.read_text().splitlines()
                   if line.strip().lower().endswith(".imp")})


def viewer_frame_count(archive: pathlib.Path, member: str) -> int | None:
    result = subprocess.run([str(VIEWER), "--describe-imp", str(archive), member, "--listfile", str(LISTFILE)],
                            capture_output=True, text=True)
    if result.returncode != 0:
        return None
    return sum(1 for line in result.stdout.splitlines() if line.startswith("frame\t"))


def viewer_export(archive: pathlib.Path, member: str, frame: int, scratch: pathlib.Path):
    out = scratch / f"{hashlib.sha1(member.encode()).hexdigest()[:12]}_{frame}.png"
    result = subprocess.run([str(VIEWER), "--export-imp-frame", str(archive), member, str(frame), str(out),
                             "--listfile", str(LISTFILE)], capture_output=True, text=True)
    if result.returncode != 0:
        return None
    try:
        return read_indexed_png(out.read_bytes())
    finally:
        out.unlink(missing_ok=True)


def trns_key(png) -> int | None:
    for kind, payload in png.chunks:
        if kind == b"tRNS":
            zero = [i for i, alpha in enumerate(payload) if alpha == 0]
            return zero[0] if len(zero) == 1 else None
    return None


@unittest.skipUnless(VIEWER.is_file(), f"no asset viewer at {VIEWER} (cargo build --release)")
@unittest.skipUnless(LISTFILE.is_file(), f"no listfile at {LISTFILE}")
class Corpus(unittest.TestCase):
    """Every member, every frame, against the Rust decoder."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.archives = distinct_archives()
        cls.members = imp_members()

    def archives_or_skip(self) -> dict[str, pathlib.Path]:
        if not self.archives:
            self.skipTest("no imp.mpq here: " + ", ".join(str(p) for p in ARCHIVES.values()))
        return self.archives

    def test_every_member_has_the_viewers_frame_count(self) -> None:
        for label, path in self.archives_or_skip().items():
            archive = mpq_read.Archive(path)
            present = [m for m in self.members if m in archive]
            with concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
                want = dict(zip(present, pool.map(lambda m: viewer_frame_count(path, m), present)))
            refused = 0
            for member in present:
                try:
                    got = len(imp_read.parse(archive.read(member)).frames)
                except imp_read.ImpError:
                    got = None
                    refused += 1
                with self.subTest(archive=label, member=member):
                    self.assertEqual(got, want[member])
            self.assertGreater(len(present), 1000, "the listfile names most of imp.mpq")
            print(f"\n[imp_read] {label}: {len(present)} members, frame counts compared, "
                  f"{refused} refused by both", file=sys.stderr)

    def test_every_frame_matches_the_viewers_export(self) -> None:
        for label, path in self.archives_or_skip().items():
            archive = mpq_read.Archive(path)
            sprites = {}
            for member in self.members:
                if member in archive:
                    try:
                        sprites[member] = imp_read.parse(archive.read(member))
                    except imp_read.ImpError:
                        pass                 # the frame-count test holds the viewer refuses it too
            jobs = [(m, i) for m, s in sprites.items() for i in range(len(s.frames))]
            coverage = collections.Counter()
            mismatches = []
            with tempfile.TemporaryDirectory() as scratch, \
                    concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
                exports = pool.map(lambda job: viewer_export(path, job[0], job[1], pathlib.Path(scratch)), jobs)
                for (member, index), png in zip(jobs, exports):
                    sprite = sprites[member]
                    frame = sprite.frames[index]
                    shown = sprite.resolved_frame(index)
                    if shown.width == 0:     # an empty frame: the viewer refuses to write a PNG of it
                        if png is not None:
                            mismatches.append(f"{member}#{index}: empty here, exported by the viewer")
                        coverage["empty"] += 1
                        continue
                    ours = ((shown.width, shown.height), shown.indices,
                            b"".join(bytes(c) for c in sprite.palette), sprite.color_key)
                    theirs = None if png is None else ((png.width, png.height), bytes(png.indices),
                                                       png.palette()[:768], trns_key(png))
                    if ours != theirs:
                        mismatches.append(f"{member}#{index}")
                    coverage[(sprite.file_flags & 0x31, sprite.record_variant)] += 1
                    coverage["duplicate" if frame.flags & imp_read.FRAME_FLAG_DUPLICATE
                             and not frame.flags & imp_read.FRAME_FLAG_SHARED_PIXELS else
                             "shared-pixels" if frame.flags & imp_read.FRAME_FLAG_SHARED_PIXELS else "direct"] += 1
            self.assertEqual(mismatches[:20], [], f"{len(mismatches)} of {len(jobs)} frames differ")
            # The sweep must actually have reached what it claims to cover.
            self.assertGreater(coverage["duplicate"], 0)
            self.assertGreater(coverage["shared-pixels"], 0)
            self.assertGreaterEqual(len({k for k in coverage if isinstance(k, tuple)}), 8)
            print(f"\n[imp_read] {label}: {len(jobs)} frames of {len(sprites)} members byte-identical "
                  f"to the viewer -- {dict(sorted(coverage.items(), key=str))}", file=sys.stderr)


class Pixels(unittest.TestCase):
    def test_rle_repeats_are_control_plus_three_and_literals_0x100_minus_control(self) -> None:
        out = bytearray()
        end = imp_read._decode_rle_packet(bytes([0x00, 7, 0xFE, 1, 2]), 0, out)
        self.assertEqual((bytes(out), end), (bytes([7, 7, 7]), 2))
        imp_read._decode_rle_packet(bytes([0x00, 7, 0xFE, 1, 2]), end, out)
        self.assertEqual(bytes(out), bytes([7, 7, 7, 1, 2]))

    def test_packed_sizes_admit_tight_floor_only_at_1bpp(self) -> None:
        self.assertEqual(imp_read.packed_sizes(7, 5, 1), [4, 5])
        self.assertEqual(imp_read.packed_sizes(3, 2, 4), [3, 4])
        self.assertEqual(imp_read.packed_sizes(3, 2, 8), [6])

    def test_row_padded_layout_restarts_each_row(self) -> None:
        # 3x2 at 4bpp: tight is 3 bytes, row-padded 4. 0x12 0x30 | 0x45 0x60
        self.assertEqual(imp_read.unpack_pixels(bytes([0x12, 0x30, 0x45, 0x60]), 3, 2, 4), bytes([1, 2, 3, 4, 5, 6]))
        self.assertEqual(imp_read.unpack_pixels(bytes([0x12, 0x34, 0x56]), 3, 2, 4), bytes([1, 2, 3, 4, 5, 6]))

    def test_variant_zero_stops_at_the_first_acceptable_size_on_a_packet_boundary(self) -> None:
        # 17x4 at 1bpp accepts [8, 9, 12]: one 12-byte literal lands on 12 without passing 8 or 9.
        stream = bytes([0x100 - 12]) + bytes(range(1, 13)) + b"\xff\xff"
        packed, used = imp_read._decode_rle_until_size(stream, 0, imp_read.packed_sizes(17, 4, 1))
        self.assertEqual((len(packed), used), (12, 13))

    def test_a_1bpp_frame_that_dropped_its_last_byte_reads_zeros(self) -> None:
        self.assertEqual(imp_read.unpack_pixels(bytes([0xFF]), 9, 1, 1), bytes([1] * 8 + [0]))


if __name__ == "__main__":
    unittest.main()
