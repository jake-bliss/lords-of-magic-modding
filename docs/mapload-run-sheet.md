# `mapload` run sheet

> **RUN 2026-09-17. All seven rungs passed.** The engine loads maps this project wrote, including
> edited, sprite-placed, created-from-nothing and **non-square** maps, and the two created-from-
> nothing maps re-saved byte-identically. The run also settled the header word at `0x00`, showed tag
> bit `0x00800000` is not durable map data, and recovered `setterrain`'s transition ring. Results are
> in [the map format doc](map-format.md#engine-acceptance-measured); the log and its reasoning are in
> [the research log](research-log.md). This sheet is kept as written, because it states what each
> outcome *would* mean — and it was written before the run, so none of it is hindsight.
>
> One note for a rerun: the game **crashed on exit**, after the probe had logged `map load probe
> done`. Nothing was lost; every artifact was already on disk. Whether that is the probe or Wine on
> shutdown is untested.

One attended keypress. It answers the cheapest unknown left in the map work and two others at the
same time.

## The question

Every claim this project makes about writing maps rests on **round-trip identity** — 365 of 365
installed maps re-encode to the bytes they were read from. That shows this writer matches the
engine's **writer**. It says nothing about the engine's **reader**.

**No map this project produced has ever been loaded by the game.** That is a different claim, and it
is the one that decides whether the map editor is real.

## The instrument

`gs\hotkey.gs` hands it over. `loadscenariomap` takes a filename and **returns a boolean**, which
the shipped editor tests:

```
... mapfilename loadscenariomap not{T_PHRASE_failed_to_load_map ...}if
    set_sprite_mode_for_current_zoom rebuild3dmap resetvisibility rendermap
```

So acceptance is a value the engine hands back, not something to be read off a screenshot.

Each rung then **saves the loaded map straight back out**. The offline diff of input against echo is
the strongest readback available: it shows not only that a file was accepted but whether the engine
**normalised** anything on the way through. Any field the engine rewrites — the header word at
`0x00`, the border bit, the `+24` attribute — appears as a byte difference. That is how one keypress
attacks three unknowns.

## The ladder, and why it is in this order

A failure has to name the step that failed, or the run is unreadable.

| Rung | File | What it establishes |
| ---: | --- | --- |
| **0** | `zm0.scn` | **Control.** The engine generates a map, saves it, loads its own save. If this fails, the *instrument* is broken and nothing after it means anything. |
| **1** | `zm1.scn` | **Control.** Our re-encode of shipped `URAK.scn`, byte-identical to it. The bytes are *equal* to a file the game certainly loads, so anything but success is the harness, not the writer. |
| **2** | `zm2.scn` | **The question.** `URAK.scn` with three terrain cells changed by our tool. |
| **3** | `zm3.scn` | A placed sprite — exercises the **minted `+24`** field, whose value contradicts the corpus reading. |
| **4** | `zm4.scn` | The border bit `0x00800000` on an **interior** 4×4 rectangle. The corpus flags only the perimeter, in 146 of 146 files, so this is a shape the engine has never been given. |
| **5** | `zm5.scn` | A 64×64 map **created from nothing**, composing only byte patterns the engine itself wrote. |
| **6** | `zm6.scn` | A **96×64** created map. No shipped or engine-generated map has ever been non-square. |
| **blend** | `zb0.scn` | `clearmap` background, then eleven isolated 3×3 `setterrain` blobs, one per terrain type. Recovers **which transition tiles** `setterrain` blends — the last thing blocking real terrain painting. |

Every rung logs the engine's verdict, the map dimensions, three read-back cells, and captures a
frame. The renderer is only touched **inside** the success guard: rebuilding the 3D map from a state
the engine has just rejected is the likeliest way to lose the whole run, and it would take every
later rung with it. The log is closed and reopened after each rung for the same reason.

## Before the run

```sh
cd spikes/asset-viewer && cargo build --release && cd ../..

# 1. Build the input maps. Uses only this project's own writer; refuses if leftovers exist;
#    verifies each file through our own reader before it goes anywhere near the game.
scripts/build-mapload-inputs.sh

# 2. Install the probe. Refuses if any input map is missing or a stale zm0.scn is present.
LOM_PROBE=mapload scripts/install-engine-probe.sh
```

## The run

1. Launch **Lords of Magic GS5R3**.
2. Open the **Map Editor** (this is the part that cannot be automated — macOS blocks synthetic input
   to Wine, so the menu navigation is yours).
3. Press **`z`**, once.
4. Wait for the disk to settle — the probe writes 15 map files and 8 captures.
5. Quit the game.

The probe guards itself with `zdone`, so a second `z` does nothing.

## After the run

```sh
scripts/restore-game-archives.sh      # removes the probe and every map it wrote
```

Then hand back `zprobe.log`. The offline analysis compares each `zm*.scn` against its `zn*.scn`
echo, and reads the blend tiles out of `zb0.scn`.

## What each outcome would mean

- **Rung 0 fails** → instrument broken. Nothing else in the run is evidence. Do not read rung 2 as a
  verdict on the writer.
- **Rung 0 passes, rung 1 fails** → the harness is wrong, not the writer. Rung 1's bytes are equal to
  a shipped map's.
- **Rungs 0–1 pass, rung 2 fails** → the engine's reader rejects something our writer emits, and the
  echo diff says what. This is the outcome that would stop the editor.
- **Rungs 0–4 pass, 5–6 fail** → editing is sound, creating from nothing is not. The editor ships;
  the builder does not.
- **Rung 6 fails alone** → non-square maps are not supported by the engine, which no observation
  could have told us.
- **Any echo differs from its input** → the engine normalises that field. That is a *finding*, not a
  failure, and it is the most likely way the `0x00800000` and `+24` questions get answered.

## Cost and risk

One keypress. Writes 15 new files into the game's loose `map/` directory, all in the probe's `z`
namespace, none overwriting anything — the directory has **no backup**, which is why every name is
one `generated_map_names()` lists and `restore-game-archives.sh` removes. `gs.mpq` is modified and
rolled back by the restore script, verified against `MANIFEST.sha256`. The shipped donor `URAK.scn`
is only ever read.
