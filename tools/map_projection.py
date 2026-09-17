"""The engine's cell-to-screen projection, and the one thing it does not tell you.

`map2screen` was decoded from the binary and then measured in the running game. This module is the
executable form of that result, so it can be checked rather than remembered.

    map2screen(x, y, z) -> ( 14.4 * (x + y) + K ,
                             output1 - scroll - 20.3625 * z ,
                             33.941 * (x - y) + L )

Output 3 is screen x. Output 2 becomes screen y by subtracting a fixed offset:

    drawn_top = output2 + SCREEN_Y_OFFSET

measured at **exactly slope 1** across a camera move of 40 pixels, to better than a pixel. An
earlier fit reported `top = 0.9652 * output2 - 1822.09` with residuals up to 11 pixels. Both the
slope and the residual were artefacts: the slope came from fitting across cells whose *neighbours*
differed, and the "-80" that used to sit in the output-2 formula was that run's camera scroll, not
a constant. `map2screen` already contains the scroll, which is why the offset here survives moving
the camera.

**The third input is not the cell's elevation.** It is the height the renderer interpolates from the
surrounding mesh, which equals `getelevation` only where the neighbourhood is uniform. Measured
2026-09-17 on a mesh built for the purpose: with a uniform plateau the effective height was 2.004
against a cell elevation of 2.0, and on a spike whose ring the engine had clamped down to 1.0-1.5 it
was 1.395 for the same cell elevation of 2.0. Feeding `getelevation` straight into `map2screen` is
therefore wrong wherever the ground is not flat, and `effective_elevation` below is a hypothesis
fitted to a single neighbourhood shape -- read its docstring before trusting it.
"""

from __future__ import annotations

# Isometric constants, recovered from the binary and confirmed in gameplay.
# `33.941 = 24 * sqrt(2)` and `20.3625 ~ 14.4 * sqrt(2)`.
X_PER_ISO_STEP = 14.4
SCREEN_X_PER_STEP = 33.941
PIXELS_PER_ELEVATION = 20.3625

# `drawn_top - output2`, measured across three phases and a 40-pixel camera move on 2026-09-17:
# every one of 18 observations fell between -974.1 and -973.2 once the effective elevation was used.
SCREEN_Y_OFFSET = -973.4


def map2screen(x: float, y: float, z: float, k: float, scroll: float, l: float
               ) -> tuple[float, float, float]:
    """The operator's three outputs, given a camera.

    `k`, `scroll` and `l` describe where the view is. They are not derivable here: read them back
    from a live call, which is what the probes log. `scroll` is what a previous write-up mistook for
    a constant 80.
    """
    output1 = X_PER_ISO_STEP * (x + y) + k
    output2 = output1 - scroll - PIXELS_PER_ELEVATION * z
    output3 = SCREEN_X_PER_STEP * (x - y) + l
    return output1, output2, output3


def screen_y(output2: float) -> float:
    """Drawn top edge from output 2. Slope 1, and independent of where the camera is."""
    return output2 + SCREEN_Y_OFFSET


def corner_heights(neighbourhood: dict[tuple[int, int], float]) -> tuple[float, float, float, float]:
    """The four mesh corners of the centre cell, each the mean of the four cells meeting there.

    `neighbourhood` maps `(dx, dy)` in `-1..1` to elevation, as the probe's survey logs it.
    """
    def corner(ax: int, ay: int) -> float:
        cells = [(0, 0), (ax, 0), (0, ay), (ax, ay)]
        return sum(neighbourhood[cell] for cell in cells) / 4.0

    return corner(-1, -1), corner(1, -1), corner(-1, 1), corner(1, 1)


def effective_elevation(neighbourhood: dict[tuple[int, int], float]) -> float:
    """The height the renderer appears to use: the **maximum** of the four corner means.

    **This is the weakest claim in this module.** It is fitted to one non-uniform neighbourhood
    shape, measured once:

        1.0 1.0 1.0
        1.0 2.0 1.5     -> corners 1.25, 1.375, 1.25, 1.375; max 1.375
        1.0 1.0 1.0

    Three of the five sprites on that shape measured exactly 1.375; the other two measured 1.424,
    one pixel away. The *mean* of the corners, 1.3125, predicts 26.7 pixels where the smallest
    measurement was 28, so the mean is excluded -- but "excluded the only rival I tried" is not the
    same as "established". A second experiment with a different neighbourhood shape would settle it,
    and until then this function is a hypothesis with a number attached.

    What is NOT in doubt, because it is what the experiment actually compared: a uniform
    neighbourhood gives back the cell's own elevation, and a non-uniform one does not.
    """
    return max(corner_heights(neighbourhood))


def is_uniform(neighbourhood: dict[tuple[int, int], float]) -> bool:
    """True when `getelevation` on the centre cell is safe to feed straight into `map2screen`."""
    return len(set(neighbourhood.values())) == 1
