# New-game screen slice

The first `pic.mpq` member ever put in front of the engine. **Observed in gameplay 2026-09-18: the
engine read it.**

The top third of the main menu rendered spattered red and the bottom two thirds rendered exactly as
shipped, matching the reference PNG exported beside the build. `lbm\start01.lbm` -- a different
member of the same rewritten archive -- rendered correctly, which is an independent control that the
archive was not broadly damaged. Rollback afterwards was verified by extracting the member and
comparing its bytes to the pristine seed, not by trusting a digest.

One thing the run corrected: **`lbm\newgame.lbm` is the main menu itself**, not a screen shown during
new-game setup. The name means "the new-game menu". The observation cost no navigation as a result,
but the instruction given to the human observer was wrong about where to look, because it was based
on reading the image rather than on knowing where the engine draws it.

## Why this mod exists

`docs/roadmap.md` closed Phase 4 with an explicit limit: *"This is one member, one archive, one
compression class. A `pic.mpq` replacement has still **never** faced the engine and its compression
choice remains **Inferred**."* This mod is the experiment that addresses the first half of that
sentence. The second half turned out to be the wrong worry -- see below.

## What it changes

`pic.mpq:lbm\newgame.lbm`, the 640x480 doors image the game shows when a new game is started.
Every flat run of pixels in the top 160 rows is repainted to palette index 1, which in this image's
own CMAP is pure red `(255, 0, 0)`.

Reproduce it from a clean checkout:

```sh
scripts/mod-seed.sh mods/newgame-picslice 'pic.mpq:lbm\newgame.lbm'
cp mods/newgame-picslice/archives/pic.mpq/lbm/newgame.lbm /tmp/pristine.lbm
python3 tools/pbm_patch.py /tmp/pristine.lbm \
  mods/newgame-picslice/archives/pic.mpq/lbm/newgame.lbm --index 1 --rect 0 0 640 160
scripts/mod-build.sh mods/newgame-picslice --determinism-runs 2
```

## Why the edit is shaped like that

There is no PBM or LBM **encoder** in this repository -- `spikes/asset-viewer` decodes images and
exports PNG, and nothing goes the other way. An edit that has to re-compress therefore could not be
made at all.

`BMHD.compression` is 1, ByteRun1, and that leaves exactly one opening: a *repeat* packet is two
bytes standing for up to 128 pixels, so rewriting the second of those two bytes repaints every pixel
it covers and the file keeps its byte count. `tools/pbm_patch.py` does only that, refuses to split a
run that straddles the rectangle edge, and never touches a literal packet. The resulting region has
a ragged edge, which is visible in the render and is the honest shape of the mechanism.

## The observable, and why it is not a repeat of the Phase 4 mistake

Phase 4's first attempt changed `hit_points` and read the unit panel, which applies modifiers -- an
uncalibrated instrument, so the reading confirmed nothing and cost a human a game session.

Here the expected value is not predicted, it is **rendered**. The build's `pic.mpq` is
`b649ee7c335b0aa9226fd2b6354ab2a4d5b91c71628ada417e1d86ce118683e9`, and the reference PNG beside the
build was exported from a byte-identical archive by our own decoder. The screen is compared against
that image, not against a description of it.

The edit also carries its own control: rows 160 and below are untouched. "The top third is red and
the bottom two thirds is exactly as shipped" is a different observation from "the image is broken",
and the two cannot be confused.

## What the flag histogram already settled

Every one of the 1,071 members of the baseline `pic.mpq` carries flags `0x80010100`
(EXISTS | ENCRYPTED | IMPLODE) -- measured 2026-09-18, the same storage class as the `gs.mpq` member
the engine accepted on 2026-09-16. So the roadmap's "the compression choice is Inferred" was not the
risk it looked like.

The real blocker was addressing. `pic.mpq` has no `(listfile)` and no self-named member at all, so
every member lists under a `File%08u.xxx` pseudo-name. Naming a real member was refused for not
being in the archive's catalogue, and naming the pseudo-name reached `SFileAddFileEx`, which
rejected it with StormLib error 22 -- the pseudo-name resolves by block position and nothing hashes
it into the hash table. Until the recovered names from PR #66 were threaded through the pipeline,
**`pic.mpq` could not be packed at all**.
