#!/usr/bin/env python3
"""Write a sine tone into a WAVE that keeps a template's format and its exact frame count.

`--import-wave` takes the audio from an edited file and everything else -- the `fmt ` details, the
Sound Forge `LIST INFO` block, any `cue ` or `smpl` chunk -- from the template. That makes the
edited file's job narrow: same encoding, same channel count, same sample rate, same sample width,
and the SAME NUMBER OF FRAMES. Get the frame count wrong and the member changes length, which is a
different experiment from the one the audio rung is running.

The frame count is not a parameter here. It is read off the template and held, and the tone is
padded with silence to reach it. `--tone-ms` only decides how much of that length is tone: 5.7
seconds of unbroken sine is a worse instrument than a short beep, because a listener stops being
able to say when it started.

Why a tone at all, rather than another shipped sound: a shipped sound of the same length would have
to be found first, and any two the engine might substitute for each other are exactly the pair a
listener cannot tell apart under the wrong hypothesis. A sine at a stated frequency is nothing the
game contains, so "I heard a beep" and "I heard the usual voice" are not confusable, and two
frequencies an octave or more apart are not confusable with each other either -- which is what lets
one listen name WHICH archive the engine read.

Usage:
  tools/wav_tone.py TEMPLATE.wav OUTPUT.wav --hertz 220 [--tone-ms 750] [--amplitude 0.5]
"""

from __future__ import annotations

import argparse
import math
import struct
import sys
import wave
from pathlib import Path


class ToneError(Exception):
    """A template this tool cannot match, as opposed to an argument it will not accept."""


def tone_frames(
    *,
    frames: int,
    channels: int,
    sample_width: int,
    sample_rate: int,
    hertz: float,
    tone_ms: int,
    amplitude: float,
) -> bytes:
    """`frames` frames of a sine at `hertz`, then silence, interleaved across `channels`.

    8-bit WAVE samples are UNSIGNED with 128 as silence and 16-bit samples are SIGNED with 0 as
    silence. Writing 0 into an 8-bit file would produce a full-scale negative DC offset, which is a
    loud click followed by nothing -- audible, and audible for the wrong reason. The two cases are
    therefore separate rather than shifted into each other.
    """
    if sample_width not in (1, 2):
        raise ToneError(
            f"template is {sample_width * 8}-bit; this tool writes 8-bit or 16-bit PCM only"
        )
    if not 0.0 < amplitude <= 1.0:
        raise ToneError(f"amplitude {amplitude} is outside (0, 1]")

    tone_frame_count = min(frames, round(sample_rate * tone_ms / 1000))
    out = bytearray()
    for frame in range(frames):
        if frame < tone_frame_count:
            value = math.sin(2.0 * math.pi * hertz * frame / sample_rate) * amplitude
        else:
            value = 0.0
        if sample_width == 1:
            sample = max(0, min(255, int(round(128 + value * 127))))
            encoded = bytes((sample,)) * channels
        else:
            sample = max(-32768, min(32767, int(round(value * 32767))))
            encoded = struct.pack("<h", sample) * channels
        out.extend(encoded)
    return bytes(out)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("template", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--hertz", type=float, required=True)
    parser.add_argument(
        "--tone-ms",
        type=int,
        default=750,
        help="how much of the template's length is tone; the rest is silence",
    )
    parser.add_argument("--amplitude", type=float, default=0.5)
    arguments = parser.parse_args(argv)

    if arguments.output.exists():
        print(f"output already exists; choose a fresh path: {arguments.output}", file=sys.stderr)
        return 1

    try:
        with wave.open(str(arguments.template), "rb") as source:
            channels = source.getnchannels()
            sample_width = source.getsampwidth()
            sample_rate = source.getframerate()
            frames = source.getnframes()
        data = tone_frames(
            frames=frames,
            channels=channels,
            sample_width=sample_width,
            sample_rate=sample_rate,
            hertz=arguments.hertz,
            tone_ms=arguments.tone_ms,
            amplitude=arguments.amplitude,
        )
        with wave.open(str(arguments.output), "wb") as target:
            target.setnchannels(channels)
            target.setsampwidth(sample_width)
            target.setframerate(sample_rate)
            target.writeframes(data)
    except (OSError, wave.Error, ToneError) as error:
        print(f"{arguments.template}: {error}", file=sys.stderr)
        return 1

    print(
        f"wrote\t{arguments.output}\tchannels={channels}\tsample-rate={sample_rate}\t"
        f"bits-per-sample={sample_width * 8}\tframes={frames}\thertz={arguments.hertz}\t"
        f"tone-ms={arguments.tone_ms}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
