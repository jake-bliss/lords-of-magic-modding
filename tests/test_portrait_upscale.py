"""tools/portrait-upscale/upscale.py, the `approved` portrait pipeline, with a stand-in for the model.

The model runs once over a folder of every portrait, not once per portrait (2026-09-30: checked on
the GPU to give pixel-identical output for all 396 approved portraits, 245 s -> 15 s). What is
checked here is the plumbing around that one run: each portrait gets its OWN upscale back, the
file each writes is the one the single-portrait path (`upscale_one`) writes, and a portrait the
model wrote nothing for stops the run rather than being skipped."""

from __future__ import annotations

import os
import pathlib
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "tools" / "portrait-upscale" / "upscale.py"
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(SCRIPT.parent))

import hd_upscale  # noqa: E402
import lbm_png  # noqa: E402
import upscale  # noqa: E402

# A stand-in for realesrgan-ncnn-vulkan: nearest-neighbour 4x, file to file or folder to folder,
# one line in $FAKE_ESRGAN_LOG per run. $FAKE_ESRGAN_SKIP names an input it writes nothing for.
FAKE_ESRGAN = """#!/bin/sh
while [ $# -gt 0 ]; do case $1 in -i) i=$2; shift;; -o) o=$2; shift;; esac; shift; done
echo run >> "$FAKE_ESRGAN_LOG"
if [ -d "$i" ]; then
  for f in "$i"/*.png; do
    [ "$(basename "$f")" = "$FAKE_ESRGAN_SKIP" ] && continue
    magick "$f" -filter point -resize 400% "$o/$(basename "$f")"
  done
else
  magick "$i" -filter point -resize 400% "$o"
fi
"""
NAMES = ("aicavp00", "lildwp01", "orwizp12")


@unittest.skipUnless(shutil.which("magick") and os.name != "nt", "needs ImageMagick and a POSIX shell")
class Batched(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = pathlib.Path(tmp.name)
        self.src, self.models = self.dir / "src", self.dir / "models"
        (self.src / "portrait").mkdir(parents=True)
        self.models.mkdir()
        self.esrgan = self.dir / "fake-esrgan"
        self.esrgan.write_text(FAKE_ESRGAN)
        self.esrgan.chmod(self.esrgan.stat().st_mode | stat.S_IEXEC)
        self.log = self.dir / "esrgan.log"
        palette = [(i, (i * 5) % 256, 255 - i) for i in range(256)]
        header = struct.pack(">HHhhBBBBHBBhh", 70, 67, 0, 0, 8, 0, 1, 0, 0, 1, 1, 70, 67)
        for n, name in enumerate(NAMES):           # three different pictures: a mix-up would show
            pixels = bytes((x * (n + 2) + y * (3 * n + 1) + 40 * n) % 256 for y in range(67) for x in range(70))
            lbm_png.encode(self.src / "portrait" / f"{name}.lbm", 70, 67, pixels, palette,
                           [(b"BMHD", header), (b"CMAP", b""), (b"BODY", b"")])
        self.names = self.dir / "names.txt"
        self.names.write_text("".join(f"portrait\\{name}.lbm\n" for name in NAMES))

    def runs(self) -> int:
        return len(self.log.read_text().splitlines()) if self.log.exists() else 0

    def batch(self, out: pathlib.Path, jobs: str = "3", skip: str = "") -> subprocess.CompletedProcess:
        env = {**os.environ, "FAKE_ESRGAN_LOG": str(self.log), "FAKE_ESRGAN_SKIP": skip,
               hd_upscale.JOBS_ENV: jobs, "PYTHONDONTWRITEBYTECODE": "1"}
        return subprocess.run([sys.executable, str(SCRIPT), str(self.src), str(out), "--names", str(self.names),
                               "--esrgan", str(self.esrgan), "--models", str(self.models)],
                              capture_output=True, text=True, env=env)

    def test_one_run_of_the_model_writes_what_a_run_per_portrait_writes(self) -> None:
        out = self.dir / "out"
        result = self.batch(out)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.runs(), 1, "one run of the model for every portrait")
        os.environ["FAKE_ESRGAN_LOG"] = str(self.dir / "single.log")
        self.addCleanup(os.environ.pop, "FAKE_ESRGAN_LOG")
        for name in NAMES:
            work = self.dir / "single" / name
            work.mkdir(parents=True)
            ref = self.dir / "ref" / f"{name}.lbm"
            upscale.upscale_one(self.src / "portrait" / f"{name}.lbm", ref, work, self.esrgan, self.models,
                                "ultrasharp-4x", 140, 134)
            got = out / "portrait" / f"{name}.lbm"
            self.assertEqual(got.read_bytes(), ref.read_bytes(), name)
            self.assertEqual(lbm_png.decode(got)[:2], (140, 134))
        self.assertEqual(len((self.dir / "single.log").read_text().splitlines()), 3, "the control: one run each")
        self.assertNotEqual(*((out / "portrait" / f"{n}.lbm").read_bytes() for n in NAMES[:2]))
        self.assertEqual(sorted(p.name for p in out.iterdir()), ["portrait"], "no scratch left behind")

    def test_one_at_a_time_writes_the_same_files(self) -> None:
        together, alone = self.dir / "together", self.dir / "alone"
        self.assertEqual(self.batch(together).returncode, 0)
        self.assertEqual(self.batch(alone, jobs="1").returncode, 0)
        for name in NAMES:
            self.assertEqual((together / "portrait" / f"{name}.lbm").read_bytes(),
                             (alone / "portrait" / f"{name}.lbm").read_bytes(), name)

    def test_a_portrait_the_model_wrote_nothing_for_stops_the_run(self) -> None:
        out = self.dir / "out"
        result = self.batch(out, skip="00002.png")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("the upscaler wrote nothing for portrait\\lildwp01.lbm", result.stderr)
        self.assertFalse((out / "portrait").exists(), "nothing written: the run stopped before any")

    def test_a_stopped_runs_scratch_never_passes_for_this_runs(self) -> None:
        """A run stopped after the model left its output behind. If that folder were reused, a
        portrait the model skips this time would silently get the old run's upscale."""
        out = self.dir / "out"
        self.assertEqual(self.batch(out).returncode, 0)
        stale = out / ".work" / "out" / "00001.png"
        stale.parent.mkdir(parents=True)
        subprocess.run(["magick", "-size", "280x268", "xc:gray", str(stale)], check=True)
        result = self.batch(out, skip="00001.png")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("the upscaler wrote nothing for portrait\\aicavp00.lbm", result.stderr)


if __name__ == "__main__":
    unittest.main()
