"""exe_patch: every site is verified before any byte is written, and a scan proves no site was missed.

The unit tests run on a synthetic two-section PE whose sections have DIFFERENT raw-to-VA deltas,
because the one real mistake with this binary was converting an .rdata VA with another section's
formula. The corpus tests run on the real `lomse.exe` when a pristine copy is present; it is not in
the repository, so they skip elsewhere."""

from __future__ import annotations

import hashlib
import os
import pathlib
import struct
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import exe_patch as ep  # noqa: E402

BASE = 0x400000
TEXT_VA, TEXT_RAW, TEXT_SIZE = 0x1000, 0x200, 0x200
RDATA_VA, RDATA_RAW, RDATA_SIZE = 0x3000, 0x400, 0x100

STRIDE = bytes.fromhex("c1fb07 81e300feffff")  # sar ebx,7 ; and ebx,0xfffffe00
STRIDE_EAX = bytes.fromhex("c1f807 2500feffff")  # sar eax,7 ; and eax,0xfffffe00


def make_pe() -> bytes:
    """MZ + PE header + two sections: .text at VA 0x401000 raw 0x200, .rdata at VA 0x403000 raw 0x400."""
    img = bytearray(RDATA_RAW + RDATA_SIZE)
    img[0:2] = b"MZ"
    pe = 0x40
    struct.pack_into("<I", img, 0x3C, pe)
    img[pe:pe + 4] = b"PE\0\0"
    struct.pack_into("<H", img, pe + 6, 2)       # NumberOfSections
    struct.pack_into("<H", img, pe + 20, 0x60)   # SizeOfOptionalHeader
    struct.pack_into("<I", img, pe + 24 + 28, BASE)
    table = pe + 24 + 0x60
    for i, (name, va, raw, size) in enumerate(
        [(b".text", TEXT_VA, TEXT_RAW, TEXT_SIZE), (b".rdata", RDATA_VA, RDATA_RAW, RDATA_SIZE)]
    ):
        e = table + 40 * i
        img[e:e + 8] = name.ljust(8, b"\0")
        struct.pack_into("<IIII", img, e + 8, size, va, size, raw)
    # two stride sites in .text, one float in .rdata
    img[TEXT_RAW + 0x10:TEXT_RAW + 0x10 + len(STRIDE)] = STRIDE
    img[TEXT_RAW + 0x40:TEXT_RAW + 0x40 + len(STRIDE_EAX)] = STRIDE_EAX
    img[RDATA_RAW + 0x20:RDATA_RAW + 0x24] = struct.pack("<f", 320.0)
    return bytes(img)


IMAGE = make_pe()
SHA = hashlib.sha256(IMAGE).hexdigest()
T = BASE + TEXT_VA
R = BASE + RDATA_VA
SCAN = r"'\xc1[\xf8-\xff]\x07(?:\x81[\xe0-\xe7]|\x25)\x00\xfe\xff\xff'"


def patch(va: int, old: str, new: str, what: str = "x") -> str:
    return f'[[patch]]\nva = {va:#x}\nold = "{old}"\nnew = "{new}"\nwhat = "{what}"\n'


FULL_STRIDE = "".join([
    patch(T + 0x10, "c1 fb 07", "c1 fb 06"),
    patch(T + 0x13, "81 e3 00 fe ff ff", "81 e3 00 fc ff ff"),
    patch(T + 0x40, "c1 f8 07", "c1 f8 06"),
    patch(T + 0x43, "25 00 fe ff ff", "25 00 fc ff ff"),
])


AFTER = r"'\xc1[\xf8-\xff]\x06(?:\x81[\xe0-\xe7]|\x25)\x00\xfc\xff\xff'"


def scan(count: int = 2) -> str:
    return (f"[[scan]]\nstart = {T:#x}\nend = {T + TEXT_SIZE:#x}\nregex = {SCAN}\nafter = {AFTER}\n"
            f"count = {count}\n")


class Case(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = pathlib.Path(tempfile.mkdtemp())

    def set_file(self, body: str, sha: str = SHA, name: str = "s.toml") -> pathlib.Path:
        p = self.tmp / name
        p.write_text(f'[target]\nsha256 = "{sha}"\n\n{body}')
        return p

    def sets(self, *bodies: str) -> list[ep.PatchSet]:
        return [ep.load_set(self.set_file(b, name=f"s{i}.toml")) for i, b in enumerate(bodies)]


class SectionMapping(Case):
    def test_each_section_uses_its_own_delta(self) -> None:
        secs = ep.sections(IMAGE)
        self.assertEqual([s.name for s in secs], [".text", ".rdata"])
        self.assertEqual(ep.va_to_offset(secs, T + 0x10), TEXT_RAW + 0x10)
        self.assertEqual(ep.va_to_offset(secs, R + 0x20), RDATA_RAW + 0x20)

    def test_va_outside_every_section_is_refused(self) -> None:
        secs = ep.sections(IMAGE)
        with self.assertRaises(ep.PatchError):
            ep.va_to_offset(secs, BASE + 0x2000)  # the gap between the sections

    def test_edit_straddling_a_section_end_is_refused(self) -> None:
        secs = ep.sections(IMAGE)
        with self.assertRaises(ep.PatchError):
            ep.va_to_offset(secs, T + TEXT_SIZE - 2, 4)

    def test_rdata_float_patch_lands_in_rdata(self) -> None:
        out = ep.apply(IMAGE, self.sets(patch(R + 0x20, "00 00 a0 43", "00 00 20 44")))
        self.assertEqual(struct.unpack_from("<f", out, RDATA_RAW + 0x20)[0], 640.0)
        self.assertEqual(sum(a != b for a, b in zip(IMAGE, out)), 2)


class Verification(Case):
    def test_full_stride_set_applies(self) -> None:
        out = ep.apply(IMAGE, self.sets(scan() + FULL_STRIDE))
        self.assertEqual(out[TEXT_RAW + 0x10:TEXT_RAW + 0x19], bytes.fromhex("c1fb06 81e300fcffff"))
        self.assertEqual(out[TEXT_RAW + 0x40:TEXT_RAW + 0x48], bytes.fromhex("c1f806 2500fcffff"))

    def test_every_mismatched_site_is_named(self) -> None:
        body = patch(T + 0x10, "c1 fb 08", "c1 fb 06") + patch(T + 0x40, "c1 f8 09", "c1 f8 06")
        with self.assertRaises(ep.PatchError) as ctx:
            ep.plan(IMAGE, self.sets(body))
        self.assertIn(hex(T + 0x10), str(ctx.exception))
        self.assertIn(hex(T + 0x40), str(ctx.exception))

    def test_one_bad_site_writes_nothing(self) -> None:
        good = patch(T + 0x10, "c1 fb 07", "c1 fb 06")
        bad = patch(T + 0x40, "c1 f8 09", "c1 f8 06")
        src, dst = self.tmp / "in.exe", self.tmp / "out.exe"
        src.write_bytes(IMAGE)
        rc = ep.main(["build", str(src), str(dst), "--set", str(self.set_file(good + bad))])
        self.assertEqual(rc, 1)
        self.assertFalse(dst.exists())
        self.assertEqual(src.read_bytes(), IMAGE)

    def test_patched_input_is_refused_by_hash(self) -> None:
        once = ep.apply(IMAGE, self.sets(FULL_STRIDE))
        with self.assertRaises(ep.PatchError) as ctx:
            ep.plan(once, self.sets(FULL_STRIDE))
        self.assertIn("pristine", str(ctx.exception))

    def test_overlap_across_sets_is_refused(self) -> None:
        a = patch(T + 0x13, "81 e3 00 fe ff ff", "81 e3 00 fc ff ff")
        b = patch(T + 0x15, "00 fe", "00 fd")
        with self.assertRaises(ep.PatchError) as ctx:
            ep.plan(IMAGE, self.sets(a, b))
        self.assertIn("overlapping", str(ctx.exception))

    def test_length_change_is_refused(self) -> None:
        with self.assertRaises(ep.PatchError):
            ep.load_set(self.set_file(patch(T + 0x10, "c1 fb 07", "c1 fb")))

    def test_edit_without_a_reason_is_refused(self) -> None:
        with self.assertRaises(ep.PatchError):
            ep.load_set(self.set_file(patch(T + 0x10, "c1 fb 07", "c1 fb 06", what="")))

    def test_build_refuses_to_overwrite_its_input(self) -> None:
        src = self.tmp / "in.exe"
        src.write_bytes(IMAGE)
        rc = ep.main(["build", str(src), str(src), "--set", str(self.set_file(FULL_STRIDE))])
        self.assertEqual(rc, 1)
        self.assertEqual(src.read_bytes(), IMAGE)


    def test_build_refuses_a_hard_link_to_its_input(self) -> None:
        import os
        src, link = self.tmp / "in.exe", self.tmp / "link.exe"
        src.write_bytes(IMAGE)
        os.link(src, link)
        rc = ep.main(["build", str(src), str(link), "--set", str(self.set_file(FULL_STRIDE))])
        self.assertEqual(rc, 1)
        self.assertEqual(src.read_bytes(), IMAGE)


class Scans(Case):
    def test_undercounted_scan_is_refused(self) -> None:
        with self.assertRaises(ep.PatchError) as ctx:
            ep.plan(IMAGE, self.sets(scan(count=1) + FULL_STRIDE))
        self.assertIn("found 2", str(ctx.exception))

    def test_site_left_out_of_the_list_is_named(self) -> None:
        only_first = patch(T + 0x10, "c1 fb 07", "c1 fb 06") + patch(T + 0x13, "81 e3 00 fe ff ff", "81 e3 00 fc ff ff")
        with self.assertRaises(ep.PatchError) as ctx:
            ep.plan(IMAGE, self.sets(scan() + only_first))
        self.assertIn(hex(T + 0x40), str(ctx.exception))

    def test_half_patched_site_is_refused(self) -> None:
        # Site 2's sar is patched and its and is not. The old pattern no longer matches there and
        # the site's start is covered, so only the finished-form count can see the stride is 512.
        half = FULL_STRIDE.replace(patch(T + 0x43, "25 00 fe ff ff", "25 00 fc ff ff"), "")
        with self.assertRaises(ep.PatchError) as ctx:
            ep.plan(IMAGE, self.sets(scan() + half))
        self.assertIn("1 of 2", str(ctx.exception))

    def test_half_patched_site_passes_without_after(self) -> None:
        # The control for the test above: the hole `after` closes is real.
        half = FULL_STRIDE.replace(patch(T + 0x43, "25 00 fe ff ff", "25 00 fc ff ff"), "")
        no_after = "\n".join(l for l in scan().splitlines() if not l.startswith("after")) + "\n"
        self.assertEqual(len(ep.plan(IMAGE, self.sets(no_after + half))), 3)


@unittest.skipUnless(
    os.environ.get("LOM_PRISTINE_EXE") and pathlib.Path(os.environ["LOM_PRISTINE_EXE"]).exists(),
    "set LOM_PRISTINE_EXE to a pristine GS5R3 lomse.exe",
)
class RealBinary(unittest.TestCase):
    """The shipped sets against the binary they were derived from."""

    SETS = ROOT / "tools" / "exe_patches"

    def setUp(self) -> None:
        self.image = pathlib.Path(os.environ["LOM_PRISTINE_EXE"]).read_bytes()

    def test_every_shipped_set_applies_alone(self) -> None:
        for path in sorted(self.SETS.glob("*.toml")):
            with self.subTest(path.name):
                ep.plan(self.image, [ep.load_set(path)])

    # The builds that are installed together (scripts/terrain-hd-ladder.sh). The two terrain builds
    # are alternatives -- they give the same sites different values -- so they never combine. The
    # vanilla fix goes on every install (lomhd_setup.py), alone or with the hybrid.
    BUILDS = {
        "magnify": ["viewport-2x", "terrain-render-2x", "sprites-2x", "terrain-stride-1024"],
        "hybrid": ["terrain-hybrid-2x", "terrain-stride-1024", "fix-mirror-narrow"],
        "magnify+fix": ["viewport-2x", "terrain-render-2x", "sprites-2x", "terrain-stride-1024",
                        "fix-mirror-narrow"],
        "fix": ["fix-mirror-narrow"],
    }

    def test_every_build_applies(self) -> None:
        for name, sets in self.BUILDS.items():
            with self.subTest(name):
                ep.plan(self.image, [ep.load_set(self.SETS / f"{s}.toml") for s in sets])

    def test_every_shipped_set_is_in_a_build(self) -> None:
        used = {s for sets in self.BUILDS.values() for s in sets}
        self.assertEqual({p.stem for p in self.SETS.glob("*.toml")}, used)

    def test_the_hybrid_leaves_the_texel_shifts_alone(self) -> None:
        # The same two vertex-setup functions shift the texel u/v with shl 16 as well; TILESIZE in
        # the .til already doubles those, so doubling them here too would sample at 4x.
        uv = {0x512574, 0x512583, 0x512599, 0x5125BA, 0x518F87, 0x518F95, 0x518FAB, 0x518FC7}
        vas = {p.va for p in ep.load_set(self.SETS / "terrain-hybrid-2x.toml").patches}
        self.assertFalse(uv & vas)
        for va in uv:
            off = ep.va_to_offset(ep.sections(self.image), va, 3)
            self.assertEqual(self.image[off], 0xC1, hex(va))
            self.assertEqual(self.image[off + 2], 0x10, hex(va))

    def test_the_stride_pattern_exists_only_in_the_rasterizer(self) -> None:
        # Whole file, not the declared range: a 19th site outside it would never be scanned.
        import re
        s = ep.load_set(self.SETS / "terrain-stride-1024.toml").scans[0]
        secs = ep.sections(self.image)
        text = next(x for x in secs if x.name == ".text")
        hits = [text.va + m.start() - text.raw for m in re.finditer(s.regex, self.image, re.S)]
        self.assertEqual(len(hits), 18)
        self.assertTrue(all(s.start <= h < s.end for h in hits))

    def test_the_mirror_guard_is_the_only_one_of_its_kind(self) -> None:
        # Whole file: `sar r,1; dec r; je` over the same register occurs only at the mirror guard,
        # and the fix changes exactly that one byte.
        import re
        guards = [m.start() for r in range(8)
                  for m in re.finditer(bytes([0xD1, 0xF8 + r, 0x48 + r, 0x74]), self.image)]
        secs = ep.sections(self.image)
        self.assertEqual(guards, [ep.va_to_offset(secs, 0x49D45C, 4)])
        fixed = ep.apply(self.image, [ep.load_set(self.SETS / "fix-mirror-narrow.toml")])
        diff = [i for i, (a, b) in enumerate(zip(self.image, fixed)) if a != b]
        self.assertEqual(diff, [ep.va_to_offset(secs, 0x49D45F)])
        self.assertEqual((self.image[diff[0]], fixed[diff[0]]), (0x74, 0x7E))    # je -> jle


if __name__ == "__main__":
    unittest.main()
