"""hd_sprites: which IMP frames become pack records, prepared how, in which groups, and what each
costs to render. Sprites are built in memory (imp_read's own types) or as tiny IMP files; the
upscaler is a fake. ImageMagick is only needed by the tests that compare against it, which skip
without it."""

from __future__ import annotations

import os
import pathlib
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "tools" / "hd-review"))

import hd_portrait_pack as pack  # noqa: E402
import hd_sprites  # noqa: E402
import hd_upscale  # noqa: E402
import imp_read  # noqa: E402
from imp_members import candidate_members  # noqa: E402

KEY = 5
PALETTE = [(i, 255 - i, (i * 7) % 256) for i in range(256)]
HAVE_MAGICK = shutil.which("magick") is not None


def frame(w: int, h: int, fill: int, *, shadow_at: int | None = None, flat: bool = False,
          key: int = KEY) -> tuple[int, int, bytes]:
    """A frame with a 2-pixel key border and, inside, colours counting up from `fill` (one colour
    if `flat`): eligible when w - 4 >= 8. Different `fill`s are different frames."""
    inside = [fill] * (w - 4) if flat else [10 + (fill * 16 + x) % 200 for x in range(w - 4)]
    indices = bytearray(bytes([key] * 2 + inside + [key] * 2) * h)
    if shadow_at is not None:
        indices[shadow_at] = pack.SHADOW_INDEX
    return w, h, bytes(indices)


def sprite(*frames, key: int = KEY, duplicates: dict[int, int] | None = None) -> imp_read.Sprite:
    """An imp_read.Sprite as `parse` returns one; `duplicates` maps a frame slot to the earlier
    frame it repeats (a 0x08 record)."""
    out = []
    for i, (w, h, idx) in enumerate(frames):
        source = (duplicates or {}).get(i)
        if source is not None:
            out.append(imp_read.Frame(0x08, 0, 0, b"", source, i * 16, 0, None, None))
        else:
            out.append(imp_read.Frame(0, w, h, idx, None, i * 16, 0, i * 1000, len(idx)))
    return imp_read.Sprite(0, 1, False, 8, 1000, 1000, key, list(PALETTE), [], [], out,
                           len(duplicates or {}))


def imp_file(frames: list[tuple[int, int, bytes]], key: int = KEY, duplicate_of: dict[int, int] | None = None) -> bytes:
    """A real 8bpp, uncompressed, variant-1 IMP file: one sequence, one facing."""
    n = len(frames)
    palette_at, sequence_at = 32, 32 + 1024
    facing_at = sequence_at + 16
    table_at = facing_at + 8
    pixels_at = table_at + 16 * n
    header = bytearray(32)
    header[0], header[2], header[3] = 0x00, 1, key
    struct.pack_into("<HHI", header, 4, 1000, 1000, palette_at)
    struct.pack_into("<HI", header, 26, 1, sequence_at)
    palette = b"".join(bytes((b, g, r, 0)) for r, g, b in PALETTE)
    sequence = bytes(11) + bytes([1]) + struct.pack("<I", facing_at)
    facing = struct.pack("<HHI", 0, n, table_at)
    table, pixels = bytearray(), bytearray()
    for i, (w, h, idx) in enumerate(frames):
        if duplicate_of and i in duplicate_of:
            table += struct.pack("<BBHHHII", 0x08, 0, 0, 0, 0, 0, duplicate_of[i])
            continue
        table += struct.pack("<BBHHHII", 0, 0, w, h, w * h, 0, pixels_at + len(pixels))
        pixels += idx
    return bytes(header) + palette + sequence + facing + bytes(table) + bytes(pixels)


def indexed_png(w: int, h: int, indices: bytes, plte: bytes, trns: bytes | None = None) -> bytes:
    """A colour-type-3 PNG shaped like `--export-imp-frame`'s: PLTE, tRNS up to the key."""
    chunk = hd_sprites._png_chunk
    raw = b"".join(b"\x00" + indices[y * w:(y + 1) * w] for y in range(h))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 3, 0, 0, 0))
            + chunk(b"PLTE", plte) + (chunk(b"tRNS", trns) if trns is not None else b"")
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def fake_read(root: pathlib.Path, wanted):
    """read_renders' contract without ImageMagick: missing and wrong-size renders are refused."""
    out = {}
    for rel, w, h in wanted:
        path = root / rel
        if not path.is_file():
            out[rel] = "render is missing"
        elif hd_upscale.png_size(path) != (w, h):
            rw, rh = hd_upscale.png_size(path)
            out[rel] = f"render is {rw}x{rh}, expected {w}x{h}: made from a different original"
        else:
            out[rel] = bytes(w * h * 4)
    return out


class Base(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name) / "work"
        self.renders: list[tuple[str, list[str]]] = []
        self.members: dict[str, imp_read.Sprite] = {}
        self.reads: list[str] = []

    def read_sprite(self, member: str) -> imp_read.Sprite:
        self.reads.append(member)
        if member not in self.members:
            raise imp_read.ImpError("not in this archive")
        return self.members[member]

    def plan(self, members: dict[str, imp_read.Sprite], choices: dict, *, animated: bool = True):
        self.members.update(members)
        resolved = {m.split("\\")[-1][:-4]: (m, len(s.frames)) for m, s in members.items()}
        return hd_sprites.plan(resolved, self.read_sprite, choices, self.root, animated=animated)

    def fake_render(self, option: str, inputs: dict, dest: pathlib.Path) -> None:
        """Writes each output as a real PNG exactly 2x its input, as hd_upscale.render does."""
        self.renders.append((option, sorted(inputs)))
        dest.mkdir(parents=True, exist_ok=True)
        for key, src in inputs.items():
            w, h = hd_upscale.png_size(src)
            hd_sprites.write_png_rgba(dest / f"{key}.png", w * 2, h * 2, bytes(w * h * 16))

    def pack_of(self, plan, skipped=None, **kwargs):
        hd_sprites.render_all(plan.static + plan.animated, self.root, self.fake_render, log=lambda _: None)
        skipped = [] if skipped is None else skipped
        out = self.root / "out.pack"
        pack.write_records(out, hd_sprites.records(plan.static + plan.animated, self.root, self.read_sprite,
                                                   skipped, read=fake_read, **kwargs))
        return [(name, flags, group, small, large, key)
                for name, small, large, flags, key, group in pack.read(out.read_bytes())]


class Names(unittest.TestCase):
    def test_record_names(self) -> None:
        self.assertEqual(hd_sprites.static_record_name("tree"), "sprite__tree")
        self.assertEqual(hd_sprites.record_name("cav", 7), "anim__cav#007")

    def test_only_units_are_mirrored(self) -> None:
        self.assertTrue(hd_sprites.mirrored("UNITS\\imp\\cav.imp"))
        self.assertFalse(hd_sprites.mirrored("building\\units.imp"))

    def test_key_and_shadow_are_different_pixels(self) -> None:
        """A frame differing from another only where one has the shadow and the other the key is
        not a repeat: the game draws them differently."""
        a = frame(20, 6, 3, shadow_at=0)
        b = frame(20, 6, 3)
        self.assertNotEqual(hd_sprites.frame_identity(*a, PALETTE, KEY), hd_sprites.frame_identity(*b, PALETTE, KEY))
        self.assertEqual(hd_sprites.frame_identity(*b, PALETTE, KEY),
                         hd_sprites.frame_identity(*frame(20, 6, 3), PALETTE, KEY))


class CandidateMembersTest(unittest.TestCase):
    def test_case_variant_spellings_of_one_path_collapse_to_one_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            listfile = pathlib.Path(tmp) / "list.txt"
            listfile.write_text("unit\\Tree.imp\nunit\\TREE.imp\nunit\\Goblin.imp\nunit\\not-a-sprite.pbm\n")
            grouped = candidate_members(listfile)
            self.assertEqual(set(grouped), {"tree", "goblin"})
            self.assertEqual(len(grouped["tree"]), 1, "one spelling of unit\\tree.imp, not both")

    def test_different_folders_sharing_a_basename_are_kept_as_separate_candidates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            listfile = pathlib.Path(tmp) / "list.txt"
            listfile.write_text("aura\\agx06b.imp\nimp\\agx06b.imp\n")
            self.assertEqual(candidate_members(listfile), {"agx06b": ["aura\\agx06b.imp", "imp\\agx06b.imp"]})


class FakeArchive:
    def __init__(self, members: dict[str, bytes]):
        self.members = {k.lower(): v for k, v in members.items()}

    def __contains__(self, name: str) -> bool:
        return name.lower() in self.members

    def read(self, name: str) -> bytes:
        return self.members[name.lower()]


class DevResolve(unittest.TestCase):
    """sprite_pack.py resolves names the way setup does: through imp_read, not the viewer."""

    def resolve(self, listed: str, present: dict[str, bytes]):
        import sprite_pack
        with tempfile.TemporaryDirectory() as tmp:
            listfile = pathlib.Path(tmp) / "list.txt"
            listfile.write_text(listed)
            return sprite_pack.resolve_sprites(FakeArchive(present), listfile)

    def test_a_present_spelling_is_found_when_another_spelling_is_not(self) -> None:
        resolved, considered, skipped = self.resolve("aura\\agx06b.imp\nimp\\agx06b.imp\n",
                                                     {"imp\\agx06b.imp": imp_file([frame(20, 6, 3)])})
        self.assertEqual((resolved, considered, skipped), ({"agx06b": ("imp\\agx06b.imp", 1)}, 1, []))

    def test_two_present_members_sharing_a_name_are_a_collision(self) -> None:
        one = imp_file([frame(20, 6, 3)])
        resolved, _, skipped = self.resolve("aura\\agx06b.imp\nimp\\agx06b.imp\n",
                                            {"aura\\agx06b.imp": one, "imp\\agx06b.imp": one})
        self.assertEqual(resolved, {})
        self.assertIn("ambiguous", skipped[0])

    def test_absent_and_undecodable_members_are_not_in_this_archive(self) -> None:
        resolved, _, skipped = self.resolve("imp\\ghost.imp\nimp\\junk.imp\n", {"imp\\junk.imp": b"junk"})
        self.assertEqual(resolved, {})
        self.assertEqual(skipped, ["ghost: not in this archive", "junk: not in this archive"])


class ImpFile(unittest.TestCase):
    def test_the_synthetic_imp_parses_with_duplicates_resolved(self) -> None:
        a, b = frame(20, 6, 3), frame(20, 6, 4)
        parsed = imp_read.parse(imp_file([a, b, a], duplicate_of={2: 0}))
        self.assertEqual(len(parsed.frames), 3)
        self.assertEqual(parsed.resolved_frame(2).indices, a[2])
        self.assertEqual(parsed.palette, PALETTE)
        self.assertEqual(parsed.color_key, KEY)


class Static(Base):
    def test_a_static_eligible_sprite_with_a_pick_is_packed_as_group_0_never_mirror(self) -> None:
        plan = self.plan({"units\\tree.imp": sprite(frame(20, 6, 3))}, {"sprite__tree": "anime2x"})
        self.assertEqual(plan.skipped, [])
        [(name, flags, group, small, large, key)] = self.pack_of(plan)
        self.assertEqual((name, flags, group, key), ("sprite__tree", pack.FLAG_MASKED, 0, KEY))
        self.assertEqual((small[0], small[1], bytes(small[3])), (20, 6, frame(20, 6, 3)[2]))
        self.assertEqual(large[:2], (40, 12))

    def test_the_low_res_half_is_the_frame_as_stored_shadow_and_all(self) -> None:
        f = frame(20, 6, 3, shadow_at=25)
        plan = self.plan({"imp\\tree.imp": sprite(f)}, {"sprite__tree": "anime2x"})
        [(_, _, _, small, _, _)] = self.pack_of(plan)
        self.assertEqual(bytes(small[3]), f[2])
        self.assertEqual(small[2], PALETTE)

    def test_skips_name_their_reason(self) -> None:
        cases = {
            "dot": (sprite(frame(10, 6, 3)), "anime2x", "matcher"),                  # under MASKED_MIN_WIDTH
            "flat": (sprite(frame(20, 6, 9, flat=True)), "anime2x", "no 8-pixel run"),
            "nopick": (sprite(frame(20, 6, 3)), None, "no usable upscale pick (None)"),
            "appr": (sprite(frame(20, 6, 3)), "approved", "no usable upscale pick ('approved')"),
            "orig": (sprite(frame(20, 6, 3)), "original", "picked 'original'"),
            "toowide": (sprite(frame(641, 16, 3)), "anime2x", "exceeds"),
        }
        plan = self.plan({f"imp\\{n}.imp": s for n, (s, _, _) in cases.items()},
                         {f"sprite__{n}": c for n, (_, c, _) in cases.items() if c})
        self.assertEqual(plan.static, [])
        by_name = {line.split(":")[0]: line for line in plan.skipped}
        for name, (_, _, reason) in cases.items():
            with self.subTest(name):
                self.assertIn(reason, by_name[name])

    def test_an_upscale_at_exactly_the_max_side_is_packed(self) -> None:
        plan = self.plan({"imp\\wide.imp": sprite(frame(640, 16, 3))}, {"sprite__wide": "anime2x"})
        self.assertEqual(plan.skipped, [])
        self.assertEqual(len(self.pack_of(plan)), 1)

    def test_a_record_name_over_the_dll_limit_is_skipped_before_any_read(self) -> None:
        name = "a" * 32                                     # "sprite__" + 32 = 40, one over 39
        plan = self.plan({f"imp\\{name}.imp": sprite(frame(20, 6, 3))}, {f"sprite__{name}": "anime2x"})
        self.assertIn("longer than the DLL's 39-character limit", plan.skipped[0])
        self.assertEqual(self.reads, [], "an oversized name must not even be read")

    def test_a_member_that_will_not_decode_is_reported(self) -> None:
        plan = hd_sprites.plan({"broken": ("imp\\broken.imp", 1)}, self.read_sprite,
                               {"sprite__broken": "anime2x"}, self.root)
        self.assertEqual(plan.skipped, ["broken: could not read imp\\broken.imp (not in this archive)"])

    def test_static_only_leaves_animated_sprites_unread(self) -> None:
        plan = self.plan({"imp\\one.imp": sprite(frame(20, 6, 3)),
                          "units\\cav.imp": sprite(frame(20, 6, 3), frame(20, 6, 4))},
                         {"sprite__one": "anime2x", "sprite__cav": "anime2x"}, animated=False)
        self.assertEqual([s.name for s in plan.static], ["one"])
        self.assertEqual(plan.animated, [])
        self.assertEqual(self.reads, ["imp\\one.imp"])

    def test_a_render_of_the_wrong_size_is_refused_not_resized(self) -> None:
        plan = self.plan({"imp\\tree.imp": sprite(frame(20, 6, 3))}, {"sprite__tree": "anime2x"})
        hd_sprites.render_all(plan.static, self.root, self.fake_render, log=lambda _: None)
        stem = plan.static[0].frames[0].stem
        hd_sprites.write_png_rgba(self.root / "render" / "anime2x" / f"{stem}.png", 41, 13, bytes(41 * 13 * 4))
        skipped: list[str] = []
        self.assertEqual(list(hd_sprites.records(plan.static, self.root, self.read_sprite, skipped,
                                                 read=fake_read)), [])
        self.assertEqual(skipped, ["sprite__tree: anime2x render is 41x13, expected 40x12: made from a "
                                   "different original"])


class Animated(Base):
    def test_repeats_ineligible_frames_and_missing_picks(self) -> None:
        cav = sprite(frame(20, 6, 3), frame(20, 6, 3), frame(20, 6, 4))           # frame 1 repeats 0
        glow = sprite(frame(20, 6, 9), frame(10, 6, 9))                           # frame 1 too narrow
        plan = self.plan({"units\\cav.imp": cav, "aura\\glow.imp": glow, "units\\nopick.imp": cav},
                         {"sprite__cav": "anime2x", "sprite__glow": "ultrasharp"})
        by_name = {s.name: s for s in plan.animated}
        self.assertEqual(sorted(by_name), ["cav", "glow"])
        self.assertEqual([f.index for f in by_name["cav"].frames], [0, 2])
        self.assertEqual([f.index for f in by_name["glow"].frames], [0])
        self.assertTrue(by_name["cav"].mirror, "units are drawn mirrored on the map")
        self.assertFalse(by_name["glow"].mirror, "only units were measured mirrored")
        self.assertEqual((plan.counts["repeats"], plan.counts["ineligible"]), (1, 1))
        self.assertEqual(plan.skipped, ["nopick: no usable upscale pick (None)"])

    def test_a_duplicate_record_resolves_and_is_a_repeat(self) -> None:
        a = frame(20, 6, 3)
        plan = self.plan({"units\\cav.imp": imp_read.parse(imp_file([a, frame(20, 6, 4), a], duplicate_of={2: 0}))},
                         {"sprite__cav": "anime2x"})
        self.assertEqual([f.index for f in plan.animated[0].frames], [0, 1])
        self.assertEqual(plan.counts["repeats"], 1)

    def test_a_frame_repeated_in_another_sprite_is_packed_once(self) -> None:
        shared = frame(20, 6, 3)
        plan = self.plan({"units\\aaa.imp": sprite(shared, frame(20, 6, 4)),
                          "units\\bbb.imp": sprite(shared, frame(20, 6, 7))},
                         {"sprite__aaa": "anime2x", "sprite__bbb": "anime2x"})
        self.assertEqual([(s.name, [f.index for f in s.frames]) for s in plan.animated],
                         [("aaa", [0, 1]), ("bbb", [1])])
        self.assertEqual(plan.counts["repeats"], 1)

    def test_a_units_repeat_of_an_earlier_unmirrored_frame_makes_it_mirror(self) -> None:
        """building\\aaa sorts first and keeps the record; units\\bbb draws the same frame, and
        may draw it flipped -- so the kept record must be MIRROR (Claude review, 2026-09-23)."""
        shared = frame(20, 6, 3)
        plan = self.plan({"building\\aaa.imp": sprite(shared, frame(20, 6, 4)),
                          "units\\bbb.imp": sprite(shared, frame(20, 6, 7))},
                         {"sprite__aaa": "anime2x", "sprite__bbb": "anime2x"})
        got = {name: flags for name, flags, *_ in self.pack_of(plan)}
        mirror = pack.FLAG_MASKED | pack.FLAG_MIRROR
        self.assertEqual(got, {"anim__aaa#000": mirror, "anim__aaa#001": pack.FLAG_MASKED,
                               "anim__bbb#001": mirror})

    def test_a_frame_with_no_probe_of_enough_colours_is_left_out(self) -> None:
        plan = self.plan({"units\\cav.imp": sprite(frame(20, 6, 3, flat=True), frame(20, 6, 4))},
                         {"sprite__cav": "anime2x"})
        self.assertEqual([f.index for f in plan.animated[0].frames], [1])
        self.assertEqual(plan.counts["no_probe"], 1)

    def test_work_files_are_keyed_by_member_not_only_name(self) -> None:
        """A name that resolves to another member on another run must not reuse the first
        member's prepared frames or renders (Claude review, 2026-09-23)."""
        frames = [frame(20, 6, 3), frame(20, 6, 4)]
        first = self.plan({"aura\\glow.imp": sprite(*frames)}, {"sprite__glow": "anime2x"})
        second = self.plan({"imp\\glow.imp": sprite(*frames)}, {"sprite__glow": "anime2x"})
        self.assertNotEqual(first.animated[0].frames[0].stem, second.animated[0].frames[0].stem)

    def test_prepared_inputs_are_reused(self) -> None:
        members = {"units\\cav.imp": sprite(frame(20, 6, 3), frame(20, 6, 4))}
        plan = self.plan(members, {"sprite__cav": "anime2x"})
        prepped = self.root / "prep" / f"{plan.animated[0].frames[0].stem}.png"
        prepped.write_bytes(b"left alone")
        self.plan(members, {"sprite__cav": "anime2x"})
        self.assertEqual(prepped.read_bytes(), b"left alone")

    def test_rendering_goes_in_batches_by_pick_and_resumes(self) -> None:
        plan = self.plan({"units\\cav.imp": sprite(*(frame(20, 6, v) for v in (3, 4, 6, 7, 8))),
                          "aura\\glow.imp": sprite(frame(20, 6, 9), frame(20, 6, 10))},
                         {"sprite__cav": "anime2x", "sprite__glow": "ultrasharp"})
        hd_sprites.render_all(plan.animated, self.root, self.fake_render, batch=2, log=lambda _: None)
        self.assertEqual([(o, len(k)) for o, k in self.renders],
                         [("anime2x", 2), ("anime2x", 2), ("anime2x", 1), ("ultrasharp", 2)])
        self.renders.clear()
        hd_sprites.render_all(plan.animated, self.root, self.fake_render, batch=2, log=lambda _: None)
        self.assertEqual(self.renders, [], "every frame already rendered")

    def test_one_consecutive_group_per_sprite_after_the_static_ones(self) -> None:
        plan = self.plan({"units\\cav.imp": sprite(frame(20, 6, 3), frame(20, 6, 4)),
                          "aura\\glow.imp": sprite(frame(20, 6, 9), frame(22, 6, 9)),
                          "imp\\one.imp": sprite(frame(20, 6, 11))},
                         {"sprite__cav": "anime2x", "sprite__glow": "ultrasharp", "sprite__one": "anime2x"})
        skipped: list[str] = []
        got = [(name, flags, group) for name, flags, group, *_ in self.pack_of(plan, skipped, first_group=5)]
        mirror = pack.FLAG_MASKED | pack.FLAG_MIRROR
        self.assertEqual(got, [("sprite__one", pack.FLAG_MASKED, 0),
                               ("anim__cav#000", mirror, 5), ("anim__cav#001", mirror, 5),
                               ("anim__glow#000", pack.FLAG_MASKED, 6), ("anim__glow#001", pack.FLAG_MASKED, 6)])
        self.assertEqual(skipped, [])

    def test_groups_stay_consecutive_across_read_batches(self) -> None:
        plan = self.plan({f"units\\s{i}.imp": sprite(frame(20, 6, 3 * i + 1), frame(20, 6, 3 * i + 2))
                          for i in range(5)}, {f"sprite__s{i}": "anime2x" for i in range(5)})
        got = [group for _, _, group, *_ in self.pack_of(plan, batch=3)]
        self.assertEqual(got, [1, 1, 2, 2, 3, 3, 4, 4, 5, 5])

    def test_a_missing_render_leaves_out_that_frame_and_a_sprite_with_none_takes_no_group(self) -> None:
        plan = self.plan({"units\\aaa.imp": sprite(frame(20, 6, 3), frame(20, 6, 4)),
                          "units\\bbb.imp": sprite(frame(20, 6, 6), frame(20, 6, 7)),
                          "units\\ccc.imp": sprite(frame(20, 6, 8), frame(20, 6, 9))},
                         {f"sprite__{n}": "anime2x" for n in ("aaa", "bbb", "ccc")})
        hd_sprites.render_all(plan.animated, self.root, self.fake_render, log=lambda _: None)
        render = self.root / "render" / "anime2x"
        stem = {s.name: [f.stem for f in s.frames] for s in plan.animated}
        (render / f"{stem['aaa'][1]}.png").unlink()
        for key in stem["bbb"]:
            (render / f"{key}.png").unlink()
        skipped: list[str] = []
        records = list(hd_sprites.records(plan.animated, self.root, self.read_sprite, skipped, read=fake_read))
        self.assertEqual([pack.entry_fields(entry)[4] for entry, _, _ in records], [1, 2, 2],
                         "aaa is group 1, bbb has nothing, ccc is group 2")
        self.assertEqual(skipped, ["anim__aaa#001: anime2x render is missing",
                                   "anim__bbb#000: anime2x render is missing",
                                   "anim__bbb#001: anime2x render is missing"])


class Preparation(unittest.TestCase):
    def test_key_and_shadow_are_clear_grey_and_the_rest_opaque(self) -> None:
        rgba = hd_sprites.prepared_rgba(bytes([KEY, 1, 7]), PALETTE, KEY)
        self.assertEqual(rgba, bytes([0x20, 0x22, 0x28, 0, 0x20, 0x22, 0x28, 0, *PALETTE[7], 255]))

    @unittest.skipUnless(HAVE_MAGICK, "no ImageMagick (magick) on PATH")
    def test_identical_to_the_reviewed_magick_preparation(self) -> None:
        """The review prepared each sprite with `clear_shadow` + `magick -background #202228 -alpha
        background PNG32:` from the viewer's indexed export; setup writes the same pixels itself.
        Compared as decoded RGBA over keys below, at and above the shadow index."""
        sys.path.insert(0, str(ROOT / "tools" / "hd-review"))
        from sprite_originals import clear_shadow  # noqa: E402
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            for key in (0, 1, 5, 200, 255):
                with self.subTest(key=key):
                    w, h, idx = 23, 7, bytes((x * 37 + key) % 256 for x in range(23 * 7))
                    idx = bytes([key, 1]) + idx[2:]
                    trns = bytearray(b"\xff" * (key + 1))
                    trns[key] = 0
                    review = tmp / f"review{key}.png"
                    review.write_bytes(indexed_png(w, h, idx, b"".join(bytes(c) for c in PALETTE), bytes(trns)))
                    clear_shadow(review)
                    subprocess.run(["magick", str(review), "-background", "#202228", "-alpha", "background",
                                    f"PNG32:{review}"], check=True)
                    ours = tmp / f"ours{key}.png"
                    hd_sprites.write_png_rgba(ours, w, h, hd_sprites.prepared_rgba(idx, PALETTE, key))
                    decode = lambda p: subprocess.run(["magick", str(p), "-depth", "8", "RGBA:-"],  # noqa: E731
                                                      check=True, capture_output=True).stdout
                    self.assertEqual(decode(ours), decode(review))


@unittest.skipUnless(HAVE_MAGICK, "no ImageMagick (magick) on PATH")
class ReadRenders(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name) / "a folder with spaces"
        (self.root / "render" / "opt").mkdir(parents=True)

    def png(self, name: str, w: int, h: int, fill: int) -> str:
        hd_sprites.write_png_rgba(self.root / "render" / "opt" / name, w, h, bytes([fill, 2, 3, 4]) * (w * h))
        return f"render/opt/{name}"

    def test_one_batch_reads_each_render_and_refuses_the_wrong_size(self) -> None:
        a, b, c = self.png("a.png", 4, 2, 9), self.png("b.png", 6, 6, 8), self.png("c.png", 5, 3, 7)
        got = hd_sprites.read_renders(self.root, [(a, 4, 2), (b, 6, 4), (c, 5, 3), ("render/opt/none.png", 2, 2)])
        self.assertEqual(got[a], bytes([9, 2, 3, 4]) * 8)
        self.assertEqual(got[c], bytes([7, 2, 3, 4]) * 15)
        self.assertEqual(got[b], "render is 6x6, expected 6x4: made from a different original")
        self.assertEqual(got["render/opt/none.png"], "render is missing")

    def test_a_damaged_render_costs_only_itself(self) -> None:
        a = self.png("a.png", 4, 2, 9)
        damaged = self.root / "render" / "opt" / "d.png"
        damaged.write_bytes((self.root / a).read_bytes()[:40])            # a header, then nothing
        got = hd_sprites.read_renders(self.root, [(a, 4, 2), ("render/opt/d.png", 4, 2)])
        self.assertEqual(got[a], bytes([9, 2, 3, 4]) * 8)
        self.assertIsInstance(got["render/opt/d.png"], str)
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ["render"], "no scratch left behind")


if __name__ == "__main__":
    unittest.main()
