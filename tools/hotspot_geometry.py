"""Test whether a sprite's cursor hotspot can be derived from its frame geometry.

Every IMP frame carries hotspot records, and type 0 — `CURSOR_HOTSPOT` in the engine's own
vocabulary — is the one present on essentially every unit frame and described on the modding board
as the anchor the others hang from.

Whether that anchor is *derivable* is the question that matters for anyone rebuilding sprites. The
community's long-standing workaround for the "512x512 hotspot" problem was to crop each frame to its
minimum extent and then re-centre the frames against each other, which silently assumes the anchor
is a function of frame size. This module tests that assumption on each axis separately, by fitting
the hotspot against width and height and reporting how much of the variance survives the fit.

Input is the tab-separated output of `--describe-imp`, which already reports per-frame dimensions
and hotspot records.
"""

from __future__ import annotations

import argparse
import re
import statistics
import subprocess
from pathlib import Path

FRAME_DIMENSIONS = re.compile(r"^(\d+)x(\d+)$")

# The engine's hotspot type numbers, from the `*_HOTSPOT` constants in lomse.exe.
CURSOR_HOTSPOT = 0
MISSILE_TARGET_HOTSPOT = 7


class Frame:
    """One decoded frame: its size and the hotspots attached to it."""

    def __init__(self, width: int, height: int, hotspots: dict[int, tuple[int, int]]) -> None:
        self.width = width
        self.height = height
        self.hotspots = hotspots


def parse_frames(describe_output: str) -> list[Frame]:
    """Pull frames with hotspot records out of `--describe-imp` output.

    Frames whose placement column holds an origin pair rather than hotspot records are skipped:
    those carry no hotspot array at all, so they say nothing about the anchor.
    """
    frames = []
    for line in describe_output.splitlines():
        fields = line.split("\t")
        if fields[0] != "frame" or len(fields) < 8 or fields[7] == "-":
            continue
        dimensions = FRAME_DIMENSIONS.match(fields[4])
        if not dimensions:
            continue
        hotspots = {}
        for record in fields[7].split("|"):
            parts = record.split(":")
            if len(parts) != 3 or not parts[0].isdigit():
                hotspots = {}
                break
            hotspots[int(parts[0])] = (int(parts[1]), int(parts[2]))
        if hotspots:
            frames.append(Frame(int(dimensions.group(1)), int(dimensions.group(2)), hotspots))
    return frames


def _fit(inputs: list[float], outputs: list[float]) -> tuple[float, float]:
    """Least-squares slope and intercept of `outputs` against `inputs`."""
    mean_input = statistics.mean(inputs)
    mean_output = statistics.mean(outputs)
    variance = sum((value - mean_input) ** 2 for value in inputs)
    if variance == 0:
        return 0.0, mean_output
    slope = (
        sum((a - mean_input) * (b - mean_output) for a, b in zip(inputs, outputs)) / variance
    )
    return slope, mean_output - slope * mean_input


def axis_fit(frames: list[Frame], axis: int) -> dict[str, float]:
    """Fit the cursor hotspot on one axis against the matching frame dimension.

    `residual_stdev` against `raw_stdev` is the result to read: a hotspot derivable from frame size
    would leave almost nothing behind.
    """
    sizes = [float(frame.width if axis == 0 else frame.height) for frame in frames]
    values = [float(frame.hotspots[CURSOR_HOTSPOT][axis]) for frame in frames]
    slope, intercept = _fit(sizes, values)
    residuals = [value - (slope * size + intercept) for size, value in zip(sizes, values)]
    return {
        "frames": len(frames),
        "slope": slope,
        "intercept": intercept,
        "raw_stdev": statistics.pstdev(values),
        "residual_stdev": statistics.pstdev(residuals),
        "mean": statistics.mean(values),
        "median": statistics.median(values),
    }


def missile_target_offset(frames: list[Frame]) -> dict[str, float]:
    """Where the missile-target hotspot sits relative to the cursor hotspot."""
    offsets = [
        (
            frame.hotspots[MISSILE_TARGET_HOTSPOT][0] - frame.hotspots[CURSOR_HOTSPOT][0],
            frame.hotspots[MISSILE_TARGET_HOTSPOT][1] - frame.hotspots[CURSOR_HOTSPOT][1],
        )
        for frame in frames
        if MISSILE_TARGET_HOTSPOT in frame.hotspots
    ]
    if not offsets:
        return {"frames": 0}
    return {
        "frames": len(offsets),
        "mean_x": statistics.mean(x for x, _ in offsets),
        "mean_y": statistics.mean(y for _, y in offsets),
        "stdev_x": statistics.pstdev([x for x, _ in offsets]),
        "stdev_y": statistics.pstdev([y for _, y in offsets]),
    }


def collect(viewer: Path, archive: Path, listfile: Path, members: list[str]) -> list[Frame]:
    """Run the viewer over each member and gather its frames."""
    frames: list[Frame] = []
    for member in members:
        result = subprocess.run(
            [str(viewer), "--describe-imp", str(archive), member, "--listfile", str(listfile)],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode == 0:
            frames.extend(parse_frames(result.stdout))
    return frames


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("viewer", type=Path, help="path to lom-asset-viewer")
    parser.add_argument("archive", type=Path, help="imp.mpq")
    parser.add_argument("listfile", type=Path)
    parser.add_argument("members", type=Path, help="file of IMP member names, one per line")
    parser.add_argument("--prefix", default="units\\imp\\", help="only members with this prefix")
    arguments = parser.parse_args()

    members = [
        line.strip()
        for line in arguments.members.read_text().splitlines()
        if line.strip().startswith(arguments.prefix)
    ]
    frames = [
        frame
        for frame in collect(arguments.viewer, arguments.archive, arguments.listfile, members)
        if CURSOR_HOTSPOT in frame.hotspots
    ]

    print(f"frames-with-cursor-hotspot\t{len(frames)}")
    for axis, label in ((0, "x"), (1, "y")):
        fit = axis_fit(frames, axis)
        dimension = "width" if axis == 0 else "height"
        print(
            f"cursor-hotspot-{label}\tslope-vs-{dimension}={fit['slope']:+.4f}\t"
            f"intercept={fit['intercept']:+.3f}\tmean={fit['mean']:+.2f}\t"
            f"median={fit['median']:+.1f}\traw-stdev={fit['raw_stdev']:.2f}\t"
            f"residual-stdev={fit['residual_stdev']:.2f}"
        )
    offset = missile_target_offset(frames)
    if offset["frames"]:
        print(
            f"missile-target-minus-cursor\tframes={offset['frames']}\t"
            f"mean=({offset['mean_x']:+.2f},{offset['mean_y']:+.2f})\t"
            f"stdev=({offset['stdev_x']:.2f},{offset['stdev_y']:.2f})"
        )


if __name__ == "__main__":
    main()
