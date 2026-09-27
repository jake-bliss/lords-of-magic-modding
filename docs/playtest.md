# Playtesting and bug triage

How bugs found while playing Lords of Magic with the HD mod get reported, fixed, and shipped. The
same rules apply to bugs from other players, such as the ones that come in on Discord or through
issues.

## Where a bug lives decides what happens to it

| Where the bug lives | Fix it? | Ship the fix? | Label | Tell |
| --- | --- | --- | --- | --- |
| **The HD mod**: `ddraw.dll` (the cnc-ddraw fork), `lomhd_setup.py`, the art pipeline | Yes, as found | Yes, in the next release | `hd-mod` | Release notes |
| **The original `lomse.exe`**: engine bugs that no script mod can reach, e.g. the Death/Frozen Shade freeze | Yes | **Yes, in our installer.** Each fix is small, lives in its own `tools/exe_patches/*.toml`, is reversed by `--uninstall`, and is credited in the notes | `base-game` | Release notes; Mantera as information |
| **GS5R3's scripts or data** (`gs.mpq`, `pic.mpq` content from Mantera's mod) | Yes, prepared in this repo | **No.** Only if Mantera asks | `gs5r3` (+ `reported-to-mantera` once sent) | Mantera directly, with the prepared fix |

Add `crash` for crashes and hangs, and `playtest` for anything found while playtesting.

A GS5R3 fix is kept as a mod tree under `mods/` (the pipeline in
[`build-pipeline.md`](build-pipeline.md)), so it can be tested locally and handed to Mantera as a
ready diff. It is never added to the HD mod's release zip.

## The loop

1. **Play** in `~/Applications/Lords of Magic HD.app`: GS5R3 plus the HD mod, with HD terrain and
   animated sprites. It is a separate copy of the GS5R3 app, so the pipeline's test builds (which
   use `Lords of Magic Development.app`) never touch it. **Save often, under new names**, so there
   is a save from just before any bug.
2. **When something breaks**, say what you were doing. Paste the Wine crash dialog, or a
   screenshot, and name the nearest save. (Once the DLL crash reporter ships, crashes write
   `lomhd_crash_*.txt` / `.dmp` into the game folder and nothing needs pasting.)
3. **Triage**: save the evidence outside every worktree, under
   `~/personal-projects/lom-artifacts-keep/playtest/<date>/`. Open an issue with the labels
   above, including the crash address as module+offset and the repro save's name.
4. **Fix** on a branch. Get a Claude and a Codex review. Anything that ships goes into the next HD
   mod release. Game launches are asked for one at a time.
5. **Close the issue** with where the fix went: the release, the prepared GS5R3 mod tree, or
   "reported to Mantera".

## Players outside this repo

`python lomhd_setup.py --report` (from 0.5.2) zips `lomhd.log`, any crash reports, `ddraw.ini`,
the install record and a system summary into one file to attach to an issue. Saves are only
included on request (`--with-save`), and the report strips the user's home path.
