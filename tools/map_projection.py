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


# --- is a cell going to be in the picture? -------------------------------------------------------
#
# The 2026-09-17 map-tag run placed three sprites and photographed the map before and after. The two
# captures differed by ZERO bytes. Nothing was wrong with the placement -- the log counted the
# sprites appearing one by one and the saved records were correct -- but with the camera on (16,16)
# and the sprites on (20,30), the projection puts them about 339 pixels left of centre and 259 below
# it, which is off the left edge and underneath the editor's panel.
#
# The shared probe test already required a capture before and after the first placement. That is
# necessary and not sufficient: it cannot tell whether the thing being photographed is in the
# picture, so it passed a probe whose capture pair could not carry information. The projection is
# decoded, so this is checkable rather than hopeable.
#
# Measured from `zg0.bmp`: the capture is 640x480 and the editor's panel begins at y=375, so the map
# occupies 640x375. The donor frame the probes place (`imp/tree4e.imp`) is 72x104.
#
# The projected point is NOT the frame's top-left corner. The decoded rule is
# `top_left = anchor + placement - (width >> 1, height >> 1)` -- see docs/hotspots.md, which is the
# single source of truth -- so the frame's CENTRE sits at `anchor + placement`, and `placement` is
# per-frame art data this function does not have. The bound below therefore demands a full sprite
# of clearance on every side, which is visibility under either convention and for any placement
# within half a frame of the anchor. It is deliberately conservative: rejecting a cell that would
# in fact have been visible costs nothing, and accepting one that is not costs an attended run.
CAPTURE_WIDTH = 640
EDITOR_PANEL_TOP = 375
PROBE_SPRITE_WIDTH = 72
PROBE_SPRITE_HEIGHT = 104


def screen_offset_from_camera(cell: tuple[int, int], camera: tuple[int, int]) -> tuple[float, float]:
    """Pixels from the camera cell to `cell`, on flat ground.

    Only the difference is returned, because `map2screen`'s K and L are not known independently --
    they absorb the camera. A difference needs neither.
    """
    (x, y), (cx, cy) = cell, camera
    return (
        SCREEN_X_PER_STEP * ((x - y) - (cx - cy)),
        X_PER_ISO_STEP * ((x + y) - (cx + cy)),
    )


def is_in_frame(cell: tuple[int, int], camera: tuple[int, int]) -> bool:
    """True when a probe sprite on `cell` would be drawn wholly inside the map viewport.

    The assumption -- stated rather than hidden -- is that `centeron` puts the camera cell at the
    centre of the map area. That is what the operator's name claims and what the captures look
    like, but this repository has not measured the projection's origin, so treat this as a bound on
    "cannot be off screen the way the 2026-09-17 run was", not as a pixel-accurate prediction.
    """
    offset_x, offset_y = screen_offset_from_camera(cell, camera)
    return (
        abs(offset_x) + PROBE_SPRITE_WIDTH <= CAPTURE_WIDTH / 2
        and abs(offset_y) + PROBE_SPRITE_HEIGHT <= EDITOR_PANEL_TOP / 2
    )


# --- The eight stored directions, in screen terms -------------------------------------------
#
# `docs/map-format.md` recovers a static 8-entry direction table from `.data` and pairs direction 0
# with the `.til` column named `s`. What it could not say was where `s` points on the screen: the
# column names are the tileset authors' vocabulary, and a full string scan of `lomse.exe` finds no
# compass word anywhere in the image.
#
# The projection answers it without another engine run, because the 2026-09-17 capture varied the
# two operands INDEPENDENTLY and the drawn top was read off the screen both times:
#
#     +1 first operand   -> screen x +33.94, drawn top +14.4   (cells (26,32)..(41,32))
#     +1 second operand  -> screen x -33.94, drawn top +14.4   (cells (35,32) -> (35,41))
#
# The vertical half is immune to this document's x/y labelling ambiguity: `drawn top` grows by the
# same +14.4 for EITHER operand, so relabelling which one is "x" cannot change whether a direction
# moves up or down the screen. The horizontal half is not -- it rides on `(first - second)`, whose
# sign a global swap flips -- so it is marked below as the weaker of the two.
DIRECTION_DELTAS: tuple[tuple[int, int], ...] = (
    (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1), (1, 0), (1, 1),
)
TIL_COLUMN_NAMES: tuple[str, ...] = ("s", "sw", "w", "nw", "n", "ne", "e", "se")


def direction_screen_step(direction: int) -> tuple[float, float]:
    """Pixels the drawn cell moves per step in `direction`, on flat ground.

    Returns `(screen x, drawn top)`; drawn top grows DOWNWARD, as screen coordinates do. Flat
    ground matters: `map2screen` subtracts `PIXELS_PER_ELEVATION * z`, so a step that climbs is
    drawn higher than this says. That term is vertical only and cannot change the screen x.
    """
    first, second = DIRECTION_DELTAS[direction & 7]
    return (SCREEN_X_PER_STEP * (first - second), X_PER_ISO_STEP * (first + second))
