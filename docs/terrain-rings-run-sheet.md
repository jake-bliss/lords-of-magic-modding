# `terrainrings` run sheet

> **RUN 2026-09-17. All three sections delivered.**
>
> - **Section A:** the ring is **one offset table** — `N−13 S−14 W−11 E−12 NW+3 NE+4 SW+2 SE+1` —
>   relative to a per-background anchor, and the anchors are `15 + 48k` for k=0..7. Eight of eight
>   blending backgrounds reproduce exactly. `tt_dirt` and `tt_impassible` blend nothing; `tt_road`
>   depends on the painted terrain as a background and is ragged along every edge as a foreground.
> - **Section B:** **`resetvisibility`** clears tag bit `0x00800000` — not `rebuild3dmap`, which
>   this sheet's own hypothesis named. The bit is visibility state.
> - **Section C:** `forall` enumerated `terrainsprites`. **197 entries, 178 name-to-id pairs**, plus
>   nine per-faith arrays. `--map-place-sprite` now takes a name.
>
> Results in [the map format doc](map-format.md); reasoning in [the research log](research-log.md).
> This sheet is kept as written, because it states what each outcome *would* mean and was written
> before the run. Note that its Section B table predicted `rebuild3dmap` as the likely culprit and
> was wrong — which is the point of writing predictions down.
>
> No crash this run.

One attended keypress. It finishes the terrain half of the CLI editor and carries two riders.

## The gap it closes

The [`mapload` run](map-format.md#setterrain-transition-tiles) measured `setterrain`'s transition
ring against **one** background — tile 15, `tt_land` — and found it identical for nine of the eleven
painted terrains. That made a terrain painter tractable but not possible. The structure generalises;
the numbers do not. A painter that applied the land ring to a water or desert background would write
plausible-looking wrong tiles, and nothing in this project would catch it.

This paints **every terrain onto every background**: 11 maps, 11 blobs each, **121 measurements** in
one keypress. That is what turns `--map-set-terrain` from forcing one cell into painting.

## Section A — the matrix

For each background terrain `0..10`:

1. `newmap`, then `clearmap` with that terrain's base tile. **Forced, never painted** — `setterrain`
   is the operator under test and it blends, so sweeping it across the map would lay transitions
   against the default terrain and then partly overwrite them, leaving the ring around each blob
   unattributable.
2. `getterrain` at `(0, 0)` **before any blob**, logged against the expected type. A forced tile
   that does not answer the intended type invalidates that whole row, and it is better to know from
   the log than to discover it while fitting a table.
3. Eleven isolated 3x3 blobs, one per terrain, on an 8-cell stride — the geometry the mapload run
   already proved keeps a one-cell halo clear of its neighbour.
4. A capture, then one save: `zr0.scn` … `zr10.scn`.

**The diagonal is each row's control.** Painting a terrain onto its own background produced a ring
of pure background on the one row already measured — no transition — which is what shows the other
ten rings are measuring a boundary rather than reporting noise.

## Section B — which renderer call clears `0x00800000`

The mapload run bracketed the bit between "no renderer calls" (**set** on all 4,096 cells) and
`rebuild3dmap resetvisibility rendermap refreshdirty` (**clear** on all 4,096) without isolating
which of the four does it. Five fresh maps, one call each:

| file | sequence |
| --- | --- |
| `zf0.scn` | `clearmap`, save — the control, expected **set** |
| `zf1.scn` | `clearmap`, `rebuild3dmap`, save |
| `zf2.scn` | `clearmap`, `resetvisibility`, save |
| `zf3.scn` | `clearmap`, `rendermap`, save |
| `zf4.scn` | `clearmap`, `refreshdirty`, save |

A **fresh map each time**, because once something clears the bit it stays cleared and a second call
on the same map would measure the previous call's result — the same confounding that made the
original reading wrong.

## Section C — the sprite-type table

This is the one thing blocking a *content* builder rather than a terrain editor. `sprite_type` is a
**script-assigned index**: there are 536 `addterrainspritetype` call sites in `gs\tree.gs` and
`gs\tree2.gs`, allocated in execution order, many computed at runtime from faith and direction. A
map's object ids mean nothing without the table, and the table is built by executing GameScript.

`terrainsprites` is a dict keyed by name — shipped script reads `terrainsprites /barrow get` — and
`forall` enumerates dicts in this dialect with the body receiving **key then value**
(`spell_help_text_dict{kill pop}forall`, `memory_dict{exch pop exec ...}forall`). So:

```
terrainsprites{/zv exch def /zk exch def ... }forall
```

logs `name -> type id` for every registered terrain sprite.

**It runs last on purpose.** `forall` over a dict is read out of the shipped scripts rather than
documented anywhere, and `cvs` on a name key is the part most likely to misbehave. Everything above
it is already saved with its log flushed, so this section risks only itself.

## Before the run

```sh
cd spikes/asset-viewer && cargo build --release && cd ../..
LOM_PROBE=terrainrings scripts/install-engine-probe.sh
```

No prerequisites — unlike `mapload`, this probe builds everything it needs in-engine.

## The run

1. Launch **Lords of Magic GS5R3**
2. Open the **Map Editor**
3. Press **`z`**, once
4. Wait — it writes 16 maps and 11 captures
5. Quit

`zdone` guards it, so a second `z` does nothing.

## After the run

```sh
scripts/restore-game-archives.sh
python3 tools/terrain_rings.py <directory holding the zr*.scn files>
```

The analyser prints the eight ring tiles per background/terrain pair, flags any edge that is **not**
uniform, and groups each row by which terrains share a ring. It was validated against the mapload
run's saved map before this probe existed: it reproduces `N=2 S=1 W=4 E=3 NW=18 NE=19 SW=17 SE=16`
for the nine, all-15 for terrain 6, and flags terrain 9 as ragged on all four edges.

## What each outcome would mean

- **A row's background reads back as the wrong terrain type** → that row is void; the base tile for
  that type does not do what the terrain table implies. Everything else still stands.
- **Nine-of-eleven holds on every background** → the painter needs 11 direction tables and one road
  special case. That is the good case and it is what the single measured row predicts.
- **The grouping differs per background** → transition choice depends on the pair after all, and the
  painter needs the full matrix as data rather than a rule. Still shippable; just bigger.
- **Edges come back ragged for terrains other than road** → the ring is not a per-direction constant
  and a painter must consider run length, not just adjacency. This is the case that would most
  change the design, and the analyser calls it out rather than averaging it away.
- **Section B's control is not `set`** → the earlier bracket does not reproduce, and the bit finding
  goes back to open.
- **Section C logs nothing, or a count of 0** → `forall` does not enumerate this dict and the table
  needs the GameScript VM instead. Costs nothing but the lines.

## Cost and risk

One keypress. Writes 16 new maps and 11 captures into the game's loose `map/` directory, all in the
probe's `z` namespace, none overwriting anything — the directory has **no backup**, which is why
every name is one `generated_map_names()` lists and `restore-game-archives.sh` removes. `gs.mpq` is
modified and rolled back by the restore script, verified against `MANIFEST.sha256`.
