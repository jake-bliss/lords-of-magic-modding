"""The tone writer for the audio rung.

One property carries the whole rung: the output must have the template's exact frame count, format
and channel layout, because `--import-wave` takes only the audio from it and a different frame
count would silently turn a same-length experiment into a length-changing one.
"""

import struct
import sys
import tempfile
import unittest
import wave
from pathlib import Path

PROJECT_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PROJECT_DIR / "tools"))

from wav_tone import ToneError, main, tone_frames  # noqa: E402


def write_template(
    path: Path, *, channels: int, width: int, rate: int, frames: int
) -> None:
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(channels)
        handle.setsampwidth(width)
        handle.setframerate(rate)
        silence = (b"\x80" if width == 1 else b"\x00\x00") * channels
        handle.writeframes(silence * frames)


class ToneFramesTest(unittest.TestCase):
    def test_eight_bit_silence_is_128_not_zero(self) -> None:
        # Writing 0 into an unsigned 8-bit file is full-scale negative DC, not silence: a loud
        # click and then nothing. Audible, and audible for the wrong reason.
        data = tone_frames(
            frames=4, channels=1, sample_width=1, sample_rate=1000,
            hertz=100, tone_ms=0, amplitude=0.5,
        )
        self.assertEqual(data, b"\x80\x80\x80\x80")

    def test_sixteen_bit_silence_is_zero(self) -> None:
        data = tone_frames(
            frames=2, channels=1, sample_width=2, sample_rate=1000,
            hertz=100, tone_ms=0, amplitude=0.5,
        )
        self.assertEqual(data, struct.pack("<hh", 0, 0))

    def test_the_tone_stops_and_the_rest_is_silence(self) -> None:
        data = tone_frames(
            frames=200, channels=1, sample_width=1, sample_rate=1000,
            hertz=250, tone_ms=100, amplitude=1.0,
        )
        self.assertEqual(len(data), 200)
        self.assertTrue(any(byte != 128 for byte in data[:100]), "no tone was written")
        self.assertEqual(set(data[100:]), {128}, "the tail is not silent")

    def test_every_channel_carries_the_same_sample(self) -> None:
        data = tone_frames(
            frames=3, channels=2, sample_width=1, sample_rate=1000,
            hertz=250, tone_ms=1000, amplitude=1.0,
        )
        self.assertEqual(len(data), 6)
        for frame in range(3):
            self.assertEqual(data[frame * 2], data[frame * 2 + 1])

    def test_a_tone_longer_than_the_template_is_clipped_not_extended(self) -> None:
        data = tone_frames(
            frames=10, channels=1, sample_width=1, sample_rate=1000,
            hertz=250, tone_ms=10_000, amplitude=1.0,
        )
        self.assertEqual(len(data), 10)

    def test_an_unsupported_sample_width_is_refused_by_name(self) -> None:
        with self.assertRaises(ToneError) as raised:
            tone_frames(
                frames=1, channels=1, sample_width=3, sample_rate=1000,
                hertz=1, tone_ms=1, amplitude=0.5,
            )
        self.assertIn("24-bit", str(raised.exception))

    def test_an_amplitude_outside_the_range_is_refused(self) -> None:
        with self.assertRaises(ToneError):
            tone_frames(
                frames=1, channels=1, sample_width=1, sample_rate=1000,
                hertz=1, tone_ms=1, amplitude=1.5,
            )


class CommandLineTest(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.root = Path(self._temporary.name)
        self.addCleanup(self._temporary.cleanup)

    def test_the_output_matches_the_templates_format_and_frame_count(self) -> None:
        template = self.root / "template.wav"
        output = self.root / "tone.wav"
        write_template(template, channels=2, width=1, rate=22050, frames=1234)

        self.assertEqual(main([str(template), str(output), "--hertz", "440"]), 0)

        with wave.open(str(output), "rb") as handle:
            self.assertEqual(handle.getnchannels(), 2)
            self.assertEqual(handle.getsampwidth(), 1)
            self.assertEqual(handle.getframerate(), 22050)
            self.assertEqual(handle.getnframes(), 1234)

    def test_two_frequencies_produce_different_audio(self) -> None:
        # The audio rung reads WHICH archive the engine opened off the pitch it hears, so two
        # frequencies producing the same samples would silently collapse the experiment.
        template = self.root / "template.wav"
        write_template(template, channels=1, width=1, rate=22050, frames=22050)
        low, high = self.root / "low.wav", self.root / "high.wav"

        main([str(template), str(low), "--hertz", "220"])
        main([str(template), str(high), "--hertz", "1760"])

        self.assertNotEqual(low.read_bytes(), high.read_bytes())

    def test_refuses_to_overwrite_an_existing_output(self) -> None:
        template = self.root / "template.wav"
        write_template(template, channels=1, width=1, rate=11025, frames=8)
        output = self.root / "tone.wav"
        output.write_bytes(b"do not lose me")

        self.assertEqual(main([str(template), str(output), "--hertz", "440"]), 1)
        self.assertEqual(output.read_bytes(), b"do not lose me")


if __name__ == "__main__":
    unittest.main()
