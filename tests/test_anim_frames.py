"""anim_frames: which animated frames become pack records, in which groups, and what each costs to
render. The asset viewer, ImageMagick and the upscaler are fakes -- no archive is needed."""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "tools" / "hd-review"))
sys.path.insert(0, str(ROOT / "tests"))

import anim_frames  # noqa: E402
import hd_portrait_pack as pack  # noqa: E402
from png_index_patch import read_indexed_png  # noqa: E402
from test_sprite_pack import PLTE, indexed_png  # noqa: E402

KEY = 5


def frame_png(w: int, h: int, fill: int, *, shadow_at: int | None = None, flat: bool = False) -> bytes:
    """A frame with a 2-pixel key border and, inside, colours counting up from `fill` (one colour
    if `flat`): eligible when w - 4 >= 8. Different `fill`s are different frames."""
    inside = [fill] * (w - 4) if flat else [10 + (fill * 16 + x) % 200 for x in range(w - 4)]
    row = bytes([KEY] * 2 + inside + [KEY] * 2)
    indices = bytearray(row * h)
    if shadow_at is not None:
        indices[shadow_at] = pack.SHADOW_INDEX
    trns = bytearray(b"\xff" * 256)
    trns[KEY] = 0
    return indexed_png(w, h, bytes(indices), PLTE, bytes(trns))


class FakeViewer:
    def __init__(self, frames: dict[str, list[bytes]]):
        self.frames = frames
        self.calls = 0

    def run(self, cmd, **kwargs):
        assert cmd[1] == "--export-imp-frame", cmd
        self.calls += 1
        member, index, out = cmd[3], int(cmd[4]), pathlib.Path(cmd[5])
        if member not in self.frames or index >= len(self.frames[member]):
            return subprocess.CompletedProcess(cmd, 1, stdout="", stderr="no such frame")
        out.write_bytes(self.frames[member][index])
        return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")


class AnimFramesTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.work = pathlib.Path(self.tmp.name) / "work"
        self.addCleanup(setattr, anim_frames, "prepare", anim_frames.prepare)
        anim_frames.prepare = lambda raw, prepped: shutil.copy(raw, prepped)
        self.addCleanup(setattr, anim_frames.subprocess, "run", anim_frames.subprocess.run)
        self.renders: list[tuple[str, list[str]]] = []

    def viewer(self, frames: dict[str, list[bytes]]) -> FakeViewer:
        fake = FakeViewer(frames)
        anim_frames.subprocess.run = fake.run
        return fake

    def plan(self, frames: dict[str, list[bytes]], choices: dict, workers: int = 1):
        self.viewer(frames)
        resolved = {m.split("\\")[-1][:-4]: (m, len(f)) for m, f in frames.items()}
        return anim_frames.plan(pathlib.Path("imp.mpq"), pathlib.Path("viewer"), pathlib.Path("list"),
                                resolved, choices, self.work, "fp", workers=workers)

    def fake_render(self, option: str, inputs: dict, dest: pathlib.Path) -> None:
        """Writes each output as a file naming the size it claims to be (2x the input)."""
        self.renders.append((option, sorted(inputs)))
        dest.mkdir(parents=True, exist_ok=True)
        for key, src in inputs.items():
            png = read_indexed_png(src.read_bytes())
            (dest / f"{key}.png").write_text(f"{png.width * 2}x{png.height * 2}")

    @staticmethod
    def fake_load_hd_rgba(render: pathlib.Path, w: int, h: int):
        rw, rh = (int(v) for v in render.read_text().split("x"))
        if (rw, rh) != (w * 2, h * 2):
            raise SystemExit("wrong size")
        return rw, rh, bytes(rw * rh * 4)

    # --- plan ---------------------------------------------------------------------------------

    def test_repeats_ineligible_frames_and_missing_picks(self) -> None:
        cav = [frame_png(20, 6, 3), frame_png(20, 6, 3), frame_png(20, 6, 4)]      # frame 1 repeats 0
        glow = [frame_png(20, 6, 9), frame_png(10, 6, 9)]                           # frame 1 too narrow
        sprites, skipped, counts = self.plan(
            {"units\\cav.imp": cav, "aura\\glow.imp": glow, "units\\nopick.imp": cav},
            {"sprite__cav": "anime2x", "sprite__glow": "ultrasharp"})
        by_name = {s.name: s for s in sprites}
        self.assertEqual(sorted(by_name), ["cav", "glow"])
        self.assertEqual([f.index for f in by_name["cav"].frames], [0, 2])
        self.assertEqual([f.index for f in by_name["glow"].frames], [0])
        self.assertTrue(by_name["cav"].mirror, "units are drawn mirrored on the map")
        self.assertFalse(by_name["glow"].mirror, "only units were measured mirrored")
        self.assertEqual((counts["repeats"], counts["ineligible"]), (1, 1))
        self.assertEqual(skipped, ["nopick: no usable upscale pick (None)"])

    def test_a_frame_repeated_in_another_sprite_is_packed_once(self) -> None:
        shared = frame_png(20, 6, 3)
        sprites, _, counts = self.plan(
            {"units\\aaa.imp": [shared, frame_png(20, 6, 4)], "units\\bbb.imp": [shared, frame_png(20, 6, 7)]},
            {"sprite__aaa": "anime2x", "sprite__bbb": "anime2x"})
        self.assertEqual([(s.name, [f.index for f in s.frames]) for s in sprites],
                         [("aaa", [0, 1]), ("bbb", [1])])
        self.assertEqual(counts["repeats"], 1)

    def test_key_and_shadow_are_different_pixels(self) -> None:
        """A frame differing from another only where one has the shadow and the other the key is
        not a repeat: the game draws them differently."""
        a = read_indexed_png(frame_png(20, 6, 3, shadow_at=0))
        b = read_indexed_png(frame_png(20, 6, 3))
        self.assertNotEqual(anim_frames.frame_identity(a, KEY), anim_frames.frame_identity(b, KEY))
        self.assertEqual(anim_frames.frame_identity(b, KEY),
                         anim_frames.frame_identity(read_indexed_png(frame_png(20, 6, 3)), KEY))

    def test_static_sprites_and_the_approved_pipeline_are_not_animated(self) -> None:
        sprites, skipped, _ = self.plan(
            {"units\\one.imp": [frame_png(20, 6, 3)], "units\\por.imp": [frame_png(20, 6, 3), frame_png(20, 6, 4)]},
            {"sprite__one": "anime2x", "sprite__por": "approved"})
        self.assertEqual(sprites, [])
        self.assertEqual(skipped, ["por: no usable upscale pick ('approved')"])

    def test_a_frame_that_will_not_export_is_reported(self) -> None:
        self.viewer({})
        sprites, skipped, _ = anim_frames.plan(
            pathlib.Path("imp.mpq"), pathlib.Path("viewer"), pathlib.Path("list"),
            {"cav": ("units\\cav.imp", 2)}, {"sprite__cav": "anime2x"}, self.work, "fp", workers=1)
        self.assertEqual(sprites, [])
        self.assertEqual(len(skipped), 2)
        self.assertTrue(all("could not export (no such frame)" in line for line in skipped))

    def test_exports_are_reused(self) -> None:
        frames = {"units\\cav.imp": [frame_png(20, 6, 3), frame_png(20, 6, 4)]}
        self.plan(frames, {"sprite__cav": "anime2x"})
        again = self.viewer(frames)
        resolved = {"cav": ("units\\cav.imp", 2)}
        anim_frames.plan(pathlib.Path("imp.mpq"), pathlib.Path("viewer"), pathlib.Path("list"),
                         resolved, {"sprite__cav": "anime2x"}, self.work, "fp", workers=1)
        self.assertEqual(again.calls, 0)

    def test_a_units_repeat_of_an_earlier_unmirrored_frame_makes_it_mirror(self) -> None:
        """building\\aaa sorts first and keeps the record; units\\bbb draws the same frame, and
        may draw it flipped -- so the kept record must be MIRROR (Claude review, 2026-09-23)."""
        shared = frame_png(20, 6, 3)
        sprites, _, _ = self.plan(
            {"building\\aaa.imp": [shared, frame_png(20, 6, 4)], "units\\bbb.imp": [shared, frame_png(20, 6, 7)]},
            {"sprite__aaa": "anime2x", "sprite__bbb": "anime2x"})
        aaa = sprites[0]
        self.assertFalse(aaa.mirror)
        self.assertEqual([f.mirror for f in aaa.frames], [True, False])

    def test_a_frame_with_no_probe_of_enough_colours_is_left_out(self) -> None:
        sprites, _, counts = self.plan(
            {"units\\cav.imp": [frame_png(20, 6, 3, flat=True), frame_png(20, 6, 4)]},
            {"sprite__cav": "anime2x"})
        self.assertEqual([f.index for f in sprites[0].frames], [1])
        self.assertEqual(counts["no_probe"], 1)

    def test_caches_are_keyed_by_member_not_only_name(self) -> None:
        """A name that resolves to another member on another run must not reuse the first
        member's prepared frames or renders (Claude review, 2026-09-23)."""
        frames = [frame_png(20, 6, 3), frame_png(20, 6, 4)]
        first, _, _ = self.plan({"aura\\glow.imp": frames}, {"sprite__glow": "anime2x"})
        second, _, _ = self.plan({"imp\\glow.imp": frames}, {"sprite__glow": "anime2x"})
        self.assertNotEqual(first[0].frames[0].prepped, second[0].frames[0].prepped)

    # --- render -------------------------------------------------------------------------------

    def test_rendering_goes_in_batches_by_pick_and_resumes(self) -> None:
        frames = {"units\\cav.imp": [frame_png(20, 6, v) for v in (3, 4, 6, 7, 8)],
                  "aura\\glow.imp": [frame_png(20, 6, v) for v in (9, 10)]}
        sprites, _, _ = self.plan(frames, {"sprite__cav": "anime2x", "sprite__glow": "ultrasharp"})
        anim_frames.render_all(sprites, self.work, "fp", self.fake_render, batch=2, log=lambda _: None)
        self.assertEqual([(o, len(k)) for o, k in self.renders],
                         [("anime2x", 2), ("anime2x", 2), ("anime2x", 1), ("ultrasharp", 2)])
        self.renders.clear()
        anim_frames.render_all(sprites, self.work, "fp", self.fake_render, batch=2, log=lambda _: None)
        self.assertEqual(self.renders, [], "every frame already rendered")

    # --- records ------------------------------------------------------------------------------

    def test_one_consecutive_group_per_sprite_with_mirror_on_units(self) -> None:
        frames = {"units\\cav.imp": [frame_png(20, 6, 3), frame_png(20, 6, 4)],
                  "aura\\glow.imp": [frame_png(20, 6, 9), frame_png(22, 6, 9)]}
        sprites, _, _ = self.plan(frames, {"sprite__cav": "anime2x", "sprite__glow": "ultrasharp"})
        anim_frames.render_all(sprites, self.work, "fp", self.fake_render, log=lambda _: None)
        skipped: list[str] = []
        out = self.work / "anim.pack"
        pack.write_records(out, anim_frames.records(sprites, self.work, "fp", self.fake_load_hd_rgba,
                                                    skipped, first_group=5))
        got = [(name, flags, group) for name, _, _, flags, _, group in pack.read(out.read_bytes())]
        mirror = pack.FLAG_MASKED | pack.FLAG_MIRROR
        self.assertEqual(got, [("anim__cav#000", mirror, 5), ("anim__cav#001", mirror, 5),
                               ("anim__glow#000", pack.FLAG_MASKED, 6),
                               ("anim__glow#001", pack.FLAG_MASKED, 6)])
        self.assertEqual(skipped, [])

    def test_a_units_repeat_makes_the_kept_buildings_record_mirror_in_the_pack(self) -> None:
        """The flag must reach the encoded record, not only the plan (Codex review, 2026-09-23)."""
        shared = frame_png(20, 6, 3)
        sprites, _, _ = self.plan(
            {"building\\aaa.imp": [shared, frame_png(20, 6, 4)], "units\\bbb.imp": [shared, frame_png(20, 6, 7)]},
            {"sprite__aaa": "anime2x", "sprite__bbb": "anime2x"})
        anim_frames.render_all(sprites, self.work, "fp", self.fake_render, log=lambda _: None)
        skipped: list[str] = []
        out = self.work / "anim.pack"
        pack.write_records(out, anim_frames.records(sprites, self.work, "fp", self.fake_load_hd_rgba, skipped))
        got = {name: flags for name, _, _, flags, _, _ in pack.read(out.read_bytes())}
        mirror = pack.FLAG_MASKED | pack.FLAG_MIRROR
        self.assertEqual(got, {"anim__aaa#000": mirror, "anim__aaa#001": pack.FLAG_MASKED,
                               "anim__bbb#001": mirror})
        self.assertEqual(skipped, [])

    def test_a_missing_render_leaves_out_that_frame_and_a_sprite_with_none_takes_no_group(self) -> None:
        frames = {"units\\aaa.imp": [frame_png(20, 6, 3), frame_png(20, 6, 4)],
                  "units\\bbb.imp": [frame_png(20, 6, 6), frame_png(20, 6, 7)],
                  "units\\ccc.imp": [frame_png(20, 6, 8), frame_png(20, 6, 9)]}
        sprites, _, _ = self.plan(frames, {f"sprite__{n}": "anime2x" for n in ("aaa", "bbb", "ccc")})
        anim_frames.render_all(sprites, self.work, "fp", self.fake_render, log=lambda _: None)
        render = self.work / "fp" / "render" / "anime2x"
        stem = {s.name: [f.prepped.stem for f in s.frames] for s in sprites}
        (render / f"{stem['aaa'][1]}.png").unlink()
        for key in stem["bbb"]:
            (render / f"{key}.png").unlink()
        skipped: list[str] = []
        records = list(anim_frames.records(sprites, self.work, "fp", self.fake_load_hd_rgba, skipped))
        groups = [pack.entry_fields(entry)[4] for entry, _, _ in records]
        self.assertEqual(groups, [1, 2, 2], "aaa is group 1, bbb has nothing, ccc is group 2")
        self.assertEqual(len(skipped), 3)

    def test_a_render_of_the_wrong_size_is_refused(self) -> None:
        frames = {"units\\cav.imp": [frame_png(20, 6, 3), frame_png(20, 6, 4)]}
        sprites, _, _ = self.plan(frames, {"sprite__cav": "anime2x"})
        anim_frames.render_all(sprites, self.work, "fp", self.fake_render, log=lambda _: None)
        (self.work / "fp" / "render" / "anime2x" / f"{sprites[0].frames[0].prepped.stem}.png").write_text("38x12")
        skipped: list[str] = []
        records = list(anim_frames.records(sprites, self.work, "fp", self.fake_load_hd_rgba, skipped))
        self.assertEqual(len(records), 1)
        self.assertEqual(skipped, ["anim__cav#000: wrong size"])


if __name__ == "__main__":
    unittest.main()
