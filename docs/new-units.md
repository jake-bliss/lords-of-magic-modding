# Creating new units

**Verdict: yes, and the cap everyone assumes does not exist.** The engine's unit-type table is
heap-allocated at a size the *script* chooses. The real constraint is on this project's side — we
have an IMP frame *repainter*, not an IMP *author* — so a new unit's sprite must be a repainted
clone of an existing unit's art.

Written 2026-09-21. Every assertion carries an evidence class; an unlabelled assertion is a defect.

## There is no unit-type cap

**Observed in the corpus.** `gs.mpq` member `gs\unittype.gs` (3,777 bytes, minified) opens:

```
/unittypedict 200 dict def 200 maxunittypes 30 dict begin ...
```

and then runs **153** `"units/<name>.gs"run` statements — 152 unique, `units/lildf.gs` appearing
twice — which between them hold **155** `begin_unit_definition` blocks (`units/gate.gs` alone holds
three: `gate`, `rgate`, `lgate`). It is the only member in the entire corpus that calls
`maxunittypes`.

**So 155 of 200 declared slots are used in vanilla, and ~45 are free.**

🔴 **That count is per profile, and the difference is not small. Observed in the corpus and in
gameplay, 2026-09-21:**

| profile | `units/*.gs` runs | unique | unit types at runtime |
| --- | ---: | ---: | ---: |
| **GS5R3** | **159** | **159** | **160** (measured by `numunittypes` in a running game) |
| 3.02 | 153 | 152 | not measured |
| vanilla | 153 | 152 | not measured |

**GS5R3 therefore already ships unit types at indices 155-159**, and has 40 free slots rather than
45. The runtime figure is the authoritative one; the run-list count is a proxy, because one member
can hold several definitions (`units/gate.gs` holds three).

⚠️ **Always name the profile when quoting this number.** The [`unitindex` run](unit-index-run-sheet.md)
was designed against the vanilla count and executed on GS5R3, and its baseline assertion failed for
exactly that reason. It failed *loudly*, because the probe logs the baseline rather than assuming
it — which is the only reason the discrepancy was noticed rather than silently absorbed.

**Observed in a local binary.** The `200` is a number the *script* picks, and nothing in the engine
bounds it. `maxunittypes` (`0x00524720`) passes its operand to the allocator at `0x00524100`:

```
524129  8b 74 24 1c   mov  esi,[esp+0x1c]    ; the requested count
52412d  83 fe 01      cmp  esi,1
524130  7d 16         jge  0x524148          ; count < 1 fails -- THE ONLY BOUND
52414f  8d 04 b6      lea  eax,[esi+esi*4]   ; x5
52415e  8d 04 80      lea  eax,[eax+eax*4]   ; x25
524161  8d 04 80      lea  eax,[eax+eax*4]   ; x125
524164  8d 0c c5 04.. lea  ecx,[eax*8+4]     ; = count*1000 + 4
52416c  e8 ..         call operator new
5241a4  89 47 08      mov  [edi+8],eax       ; 0x5cd2ec+8 = heap pointer
5241c1  89 37         mov  [edi],esi         ; 0x5cd2ec+0 = capacity
5241c3  c7 47 04 0..  mov  dword [edi+4],0   ; 0x5cd2ec+4 = used
```

`0x005cd2ec` is a **12-byte header** — capacity, used-count, heap pointer — not an inline array.
There is no `cmp` or `test` against any constant on this path. The only ceiling that exists is in
the append helper at `0x005241e0`, and it compares against the capacity the script asked for:

```
5241e4  8b 42 04      mov eax,[edx+4]   ; used
5241e7  8b 0a         mov ecx,[edx]     ; capacity  <- from maxunittypes
5241e9  3b c1         cmp eax,ecx
5241ed  83 c8 ff      or  eax,-1        ; fail -> "unittype failed" @ 0x57417c
```

**Record stride is 1000 bytes (`0x3E8`)**, confirmed independently by the allocator arithmetic
above, by `mov ecx,0xfa / rep movsd` (250 dwords) in the append helper, by `sub esp,0x3e8` in the
`unittype` operator (`0x00524b60`), by the addressing in `getunittypedata` (`0x00524a20`) and
`setunittypedata` (`0x00524820`), and by `add ecx,0x3e8` in the walkers at `0x00524300` and
`0x00524390`.

**Raising the ceiling costs one edit** to `gs\unittype.gs` — bump `200 dict` and `200 maxunittypes`
together — and 1,000 bytes of heap per added slot. `gs.mpq` has accepted a size-changing member edit
in the running engine (**Observed in gameplay, 2026-09-19**).

### What the "no cap" search could not see

A negative is only as strong as its instrument. An exhaustive `.text` sweep of every reference to
the used-count (`[0x5cd2f0]`) and the data pointer (`[0x5cd2f4]`) found all of them comparing
against the runtime-loaded value and none against a literal; a whole-binary search for `cmp …,0xc8`
(200) returned six sites, none touching the unit-type globals.

That sweep **cannot** see: a bound expressed through a different global; a bound inside a function
reached only by an indirect or virtual call that a linear sweep did not attribute; or a **narrower
field in a downstream consumer** — a savegame serializer writing the type index as a byte would cap
at 256 with no `cmp` anywhere. That last one is the live risk and is tracked as an open question
below.

### A real hard cap nearby, which is NOT this one

**Observed in a local binary.** `getleaderunittype` (`0x004bd630`) bound-checks against `0x10` —
`cmp ecx,0x10 / jge fail` — over a **fixed static array** at `0x005af194`, stride 7,556. That is
**16 leader classes**, compile-time, orthogonal to `maxunittypes` and not a bound on it.

## The real constraint: no IMP author

**Observed in local source.** `spikes/asset-viewer/src/imp.rs` exposes exactly three writers, and
all three are in-place edits of an already-parsed file: `write_frame_origin` (:1066),
`write_frame_hotspot` (:1103), `write_frame_pixels` (:1300). Nothing builds an IMP header, sequence
table, facing table or frame table, and there is **no frame-dimension writer** —
`write_frame_pixels` packs against `frame.width`/`frame.height` read from the donor.

**Consequence: a new unit's sprite is a byte-clone of an existing unit's `.imp` with its frames
repainted**, inheriting the donor's cycle set, facing counts, frame counts, per-frame pixel
dimensions and hotspots. A new unit gets its own name, stats, flags, faith, art, portrait, sounds
and description. It does **not** get a novel animation layout.

⚠️ **Donor choice is constrained.** `write_frame_pixels` refuses shared-pixel (`0x04`) and duplicate
(`0x08`) frame records. **Observed in the corpus**, across all 278 unit A/B files: only **26 of 139
units have both zoom files 100% `direct`**. `units\imp\orinfb.imp` is 55-of-91 shared and is
unusable as a donor even though `orinfa.imp` is 97/97 direct.

🟢 **Usable donors, both files fully direct, smallest first:** `aicr4` (38 frames each, 4 cycles),
`ficr4` (41/4), `decr4` (75/4), `orcr4` (92/7), `deinf` (95/7), `licr1` (110/7), `eacr5` (110/7),
`deldt` (155/11), `dethf` (155/11).

## `code` is a closed 37-value engine enum

**Observed in the corpus.** `gs.mpq` member `gs\champion.gs`:

```
/unit_code_strings["INF""MIS""CAV""WIZ""FIT""THF""CR1""CR2""CR3""CR4""LDW""LDF""LDT"
"LDR""GAT""SHP""CHI""GOA""COW""ELE""FT2""TF1""CR5""SC1""LD1""LD2""LD3""WM1""WM2""WM3"
"WMC""WMF""WMI""WMM""WMT""WMW""WZ2"]def
```

Exactly 37, each an engine constant (`reports/gs/vocabulary-vanilla.tsv`: `INF 32`, `ELE 4`, …).
**You cannot invent a 38th.**

**Observed in the corpus** (vanilla; see the profile caveat above): 36 of the 37 are in use across the 155 shipped definitions, and **`WMT`
is used by no unit in any faith** — 8 immediately free `(faith, code)` pairs. Duplicates are legal:
`orcav`/`orcav2` are both ORDER+CAV, `deldw`/`deldr` both DEATH+LDW, and the three gates share
NONE+GAT.

## The checklist

### 1. The `.gs` declaration

**Observed in the corpus.** `gs.mpq` member `units\easyunit.gs` defines the machinery.
`/end_unit_definition` calls the native `unittype` operator to allocate the next slot, then writes
each key of `unitdict` with `setunittypedata` at the index `unitdictxref` gives it — which is
`unitdict`'s **declaration order**.

🔴 **You may not add or reorder fields.** `unitdict`'s key order *is* the engine's field enum.
Reordering it silently writes every unit's data into the wrong fields.

Required in all 157 vanilla definitions (`reports/gameplay/field-ranges.tsv`): `name`, `flags`,
`attack`, `hit_points`, `mps`, `impfile_proc`. Near-universal: `code`, `faith`, `race`, `armor`,
`dexterity`, `strength`, `wisdom`, `sight_radius`, `frames_per_grid`, `attack_recovery_ticks`,
`get_hit_recovery_ticks`, `stealth_noise_factor`, `health_bar_x/y`, `morale_bar_x/y`,
`level_procedure`.

A complete minimal example, `gs.mpq` member `units\pyele.gs`:

```
begin_unit_definition /name"Elephant"def /code ELE def
/flags UNITTYPELAND CAN_ATTACK or def /race LESSER_STONE_GIANT def /faith EARTH def
/attack 10 def /armor 4 def /strength 12 def /dexterity 6 def /wisdom 3 def
/hit_points 20 def /mps 7 def /sight_radius 3 def /stealth_noise_factor 12 def
/attack_recovery_ticks 18 def /get_hit_recovery_ticks 8 def /frames_per_grid 7 def
/health_bar_x -16 def /health_bar_y -80 def /morale_bar_x -12 def /morale_bar_y -80 def
/impfile_proc{"pyele"unittype_imp_filename}def /level_procedure{pop pop 3}bind def
end_unit_definition /pyele exch def
```

### 2. Registration

Append `"units/<name>.gs"run` to `gs\unittype.gs` **after every existing run**. Slots are
sequential, so inserting would renumber every later unit.

### 3. Art — two members

**Observed in the corpus.** `gs\imps.gs`'s `unit_zoom_letter` maps COMBAT_SCREEN and
LOCATION_SCREEN to `A`, and SCROLLINGMAP_SCREEN, REGION_SCREEN and WORLD_SCREEN to `B`:

- **`units/imp/<base>A.imp`** — combat and location screens
- **`units/imp/<base>B.imp`** — scrolling map, region, world

🔴 **Each `.imp` needs its `.H` companion too.** `units/imp/<base>A.H` and `<base>B.H` are **a live
engine input**, not build residue: `Imp::BuildActionRemap` (`0x0049AE00`) parses their `#define`
lines at load time to map an animation ACTION onto a sequence slot. Clone the donor's `.H` alongside
its `.imp` and rename the `#define` prefix to match. A mod that ships the binary and drops the
header silently loses every animation. See
[imp format](imp-format.md#-the-remap-is-parsed-from-the-h-companion-member-at-load-time).

The `F` variant is **not** needed for an ordinary unit: sweeping the listfile for
`units\imp\*f.imp` returns only the 24 `ld*` leader sprites, which are what the faith-select screen
shows. Optional companions, each with its own proc in `gs\imps.gs`: `s` shadow, `a` alpha, `di`/`da`
death, `m` mount, `bw`/`fw` wings.

**Minimum cycle set — Observed in the corpus.** Across the 139 declared units' 278 A/B files,
cycle-count histogram for the A-files: 4→10, 5→6, 6→2, 7→46, 8→21, 9→36, 10→5, 11→13. Presence:
DIE 137, CORPSE 137, MOVE 133, MELEE_ATTACK 131, GET_HIT 124, DEFEND 120, STAND 119.

**The minimum any shipped unit has is 4.** `aicr4`, `decr4`, `ficr4` carry
MOVE/MELEE_ATTACK/DIE/CORPSE; the seven `*shp` units carry MOVE/RANGED_ATTACK/DIE/CORPSE. **STAND,
DEFEND and GET_HIT are all optional**, on the engine's own evidence. The modal set (46 A-files) is
MOVE/STAND/MELEE_ATTACK/DEFEND/GET_HIT/DIE/CORPSE.

Facings: 5 stored, mirrored to 8 directions (`Imp::DirectionCount` `0x0049D920`). Mirroring negates
`placement.x` — see [hotspots](hotspots.md#the-rule).

🔴 **Refuted.** The `NO_DEFEND_ANIM` flag does **not** mean "ships no DEFEND cycle": of the 27 units
carrying it, 26 *have* a DEFEND cycle, and 18 units without the flag have none. The flag means
something else, currently unknown.

### 4. Portrait — one member

**Observed in the corpus.** `gs\Dlg\newbuild.gs`'s `get_unit_portrait_name` derives the filename
**entirely from `(faith, code)`** — there is no per-unit portrait field:

```
portrait/<FF><CODE>P00.LBM
```

except codes CHI/GOA/COW/ELE, which take the literal prefix `"Py"`. The listfile holds 149 such
members; `portrait\ORINFP00.LBM`, `portrait\PyELEP00.LBM` and `portrait\licr2p00.lbm` all inspect
as **70x67, 256-entry palette, ByteRun1**, matching the `0 0 70 67 doodad` calls in
`gs\Dlg\infopan.gs`.

*The array-index-to-constant mapping is Inferred, but it self-checks: the `"Py"` branch fires for
`code > SHP && code <= ELE`, and SHP/CHI/GOA/COW/ELE occupy exactly positions 15-19 of
`unit_code_strings`.*

Portraits are authorable: `pbm.rs::encode_with_indices` re-encodes a parsed PBM with arbitrary
palette indices at the same dimensions and palette, and a re-encoded member rendered in the engine
at a length that *shrank* (**Observed in gameplay, 2026-09-20**).

### 5. Sounds — optional

**Observed in the corpus.** `gs\soundfx.gs` declares symbols
(`/oimov"wav/move/oimov.wav"addsoundfx def`); units bind them (`orinf MOVE oimov setunittypesound`).
`units\gate.gs` ships with none, so they can be omitted entirely. Table size `2500 setmaxsoundfx`.

### 6. Description text — only if recruitable

**Observed in the corpus.** `gs\Dlg\newbuild.gs`'s `load_unit_text` builds
`gs/text/<ff><ff><BLDG><SLOT>.gs`, keyed by building **slot**, not by unit. 153 such members exist.

**Not needed anywhere:** a separate string table, an ID registry, or an engine-side manifest. The
display name is the `/name` field in the unit's own `.gs`.

## Getting it into play

All four routes are **script-reachable**. **Observed in the corpus.**

| Route | Mechanism |
| --- | --- |
| **Recruitment** | `gs\Dlg\newbuild.gs` hard-codes the candidate list *in script*: barracks -> FIT/INF/CAV/SHP, thieves' guild -> THF/MIS/CR4, wizard's tower -> WIZ/CR1/CR2, great temple -> CR3 plus WZ2/FT2/CR5. `cantrain?` (`0x00442320`) and `trainunitat` (`0x00443780`) are native but read the unit record's own cost fields. |
| **Summoning** | `gs\spells\raisskel.gs` takes `/ut unittypedict /decr1 get def` by symbol name and calls `addunitnow`. Wholly script. |
| **Encounter placement** | `add_unit_to_location` (`gs\enc_tool.gs`, `gs\ENC_TOOL5.gs` in GS5R3). `TYPE STR ARTLIST NAME LOC OWNER add_unit_to_location`. |
| **Scenario start** | `gs\generate.gs`: `unittypedict begin 20 16 liinf 1 -1 addunit ... end`. |
| **AI production / mercenaries** | `gs\turnai\produce.gs`, `mercs.gs`, `defend.gs` — all script, keyed by `faith CODE unitcodegettype`. |

**Observed in gameplay, 2026-09-20.** The `unitanchor` run placed `/pyele` at three chosen cells
using `add_unit_to_location` copied verbatim from `gs\PLAYER5.gs:430`, and the engine drew it with
**zero placement residual on all three cells**. Script-driven unit creation is not theoretical here.

**Engine-internal, not reachable:** pathfinding, combat AI target selection, the animation-selection
state machine, direction mirroring, morale and rout logic. **Partly reachable:** `setautocalcproc`
(`0x0042fbe0`) and `setcombatprocid` (`0x004df860`) let script supply the auto-resolve procedure.

## Adding members to `pic.mpq` works

**Observed in a local binary, 2026-09-21.** This was previously recorded as the weakest link in the
chain, on the grounds that `pic.mpq` carries no `(listfile)` and no self-named member. Tested
directly, against the pristine archive, writing only to a scratch copy:

```
lom-mpq repack pic.mpq OUT.mpq --listfile NAMES.txt --add 'portrait\LIWMTP00.LBM=donor.lbm'
```

- The add succeeded, inheriting storage flags `0x00010100` from every member of the source.
- `probe-names` resolves the new name **through the archive's own hash table** — the lookup Storm
  performs — at block index 1071, hash index 653.
- The member reads back **byte-identical** to the file it was added from and inspects as a valid
  `70x67` IFF-PBM.
- A full manifest comparison shows **all 1,071 original members unchanged** — block index, hash
  index, size, compressed size, flags, locale *and* sha256 — with exactly one member added.

The earlier refusals recorded in [repack](repack.md#naming-members-of-an-archive-that-names-none)
are about **replacing** a member under a name the storage-flags map has no key for, and about the
`File%08u.xxx` pseudo-name. Neither applies to adding a genuinely new name.

⚠️ **This is the repack layer, not the engine.** No `pic.mpq` with an added member has been put in
front of the engine. The nearest precedent is `imp.mpq`, the same class of archive — no listfile, no
self-named member — where an added member was both **tolerated** (rung 5, 2026-09-20) and **read by
name** by a script (2026-09-17, `imp\zzpal.imp`).

## Open questions

| # | Question | State |
| ---: | --- | --- |
| 1 | ~~Does a unit type at index 155+ register and draw?~~ | ✅ **answered 2026-09-21 — YES.** A type defined at runtime at index **160** drew a sprite indistinguishable from the shipped control's. See [the run sheet](unit-index-run-sheet.md). The question's premise was also wrong: GS5R3 already ships types at 155-159. |
| 2 | Does the engine load a `pic.mpq` with an added member? | **open** — attended run (the repack layer is [settled](#adding-members-to-picmpq-works)) |
| 3 | ~~Does raising `maxunittypes` past 200 break a downstream consumer?~~ | ✅ **answered 2026-09-21 — see below** |
| 4 | ~~Where does the action-to-sequence remap come from?~~ | ✅ **answered 2026-09-21** — parsed from the `.H` companion member; see [imp format](imp-format.md#-the-remap-is-parsed-from-the-h-companion-member-at-load-time) |
| 5 | With a duplicate `(faith, code)`, which unit does `unitcodegettype` return? | **open** — attended run. `unitcodegettype` (`0x004447C0`) linearly scans all `used` records and returns the **first** match, so "the lower index" is the likely answer, but that is *Inferred*. |
| 6 | What does `NO_DEFEND_ANIM` actually mean? | **open** — of 27 units carrying it, 26 *have* a DEFEND cycle |
| 7 | Does a build with `maxauratypes` above 70 boot, and does a save survive it? | **open** — attended run. The allocator has no upper bound ([below](#raising-a-full-cap-is-a-one-token-script-edit)), but nothing has raised a cap in front of the engine, and aura ids were never looked for in a savegame. |

### Nothing downstream caps the unit-type count

**Observed in a local binary, 2026-09-21.** Every unit-type index in the engine is a full signed
32-bit value, bounds-checked against the table's live `used` count rather than against any literal.

- **The savegame stores a unit type as a `u32`.** `LS_SPR_` class 0 is the army/unit container; its
  slot array is at `army+0xc4`, stride `0x4c` (76 bytes), count at `army+0x4c`
  (`0x005242B2`-`0x005242D6`). The unit type is the **first `u32`** of that 76-byte blob, read as a
  full dword at four independent sites (`0x00420205`, `0x004675D4`, `0x004F9617`, `0x00525EAA`).
  `LS_PLR_`'s per-unit 20-byte records are five separate `fwrite(ptr, 4, 1)` calls — all dwords.
- **The map format carries no unit type at all.** Its only type field is `sprite_type` at `+28`,
  which is a *terrain-sprite* id from a different table. Units arrive by symbolic name through
  `addunit` / `add_unit_to_location`, never as a numeric id on disk.
- **The accessors do not narrow.** `getunittypedata` (`0x00524A20`) and `setunittypedata`
  (`0x00524820`) share one idiom: `movl` the index (never `movzbl`/`movswl`), reject negative,
  `cmp` against `[0x5cd2f0]` (**used**, not capacity, not a literal), then multiply by 1000.

**Independently corroborated by byte sweep**, 2026-09-21: `cmp r32,[0x5cd2f0]` occurs **57** times
in `.text`; **zero** of those 57 have any narrowing instruction (`0f b6`/`b7`/`be`/`bf`) in the
preceding 16 bytes; and `cmp r32, imm8 200` occurs **zero** times anywhere in the image.

**Observed in the corpus.** `maxunittypes` appears exactly once across all 1,687 `gs.mpq` members.
The only script structure that must grow in lockstep is its sibling `/unittypedict 200 dict` on the
same line. Every other `200 dict` in the archive is a namespace dict. Two members already do it
right, sizing from the live count: `/unit_array numunittypes array def`.

**Three adjacent declarations are exactly or nearly full** (Observed in the corpus):

| Declaration | Declared | Used | Slack |
| --- | ---: | ---: | ---: |
| `maxauratypes` (`gs\aura.gs`) | 70 | 70 | **none** |
| `maxmissiletypes` | 51 | 47 | 4 |
| `maxmounttypes` | 50 | 28 | 22 |
| `maxgraphics` (`START.GS`) | 3,500 | — | `imp.mpq` alone holds 3,600 members |

**None of these is an engine cap.** Each is a literal the boot script hands to an allocator, and the
allocator accepts any count. See [raising a full cap](#raising-a-full-cap-is-a-one-token-script-edit)
below.

**What this search could not see.** No empirical corpus check was made that `LS_SPR_` slot+0 values
actually fall in `0..154` across the savegame corpus — the claim rests on the disassembly alone,
four sites but one instrument. **Multiplayer packet serialisers were not examined**; a type packed
as a byte on the wire would never touch the table header and so would be invisible here (mitigating:
every peer must already run a byte-identical `gs.mpq`). A second array keyed by unit type that never
touches the header would also be invisible.

⚠️ **A boot-path caveat worth keeping.** Several members contain unit definitions that are *not* in
the run list, and `File00000001.xxx` / `File00000006.xxx` are alternate boot scripts with
**different** `maxpalettes`/`maxdialogs` literals. Which boot script the shipping executable
actually runs was not traced, so the active cap set could differ from the retail path assumed here.

## Raising a full cap is a one-token script edit

**Observed in a local binary, 2026-09-21.** Six `max*` declarations were followed into the engine.
Each is a literal the boot script hands to an allocator method, and **none of the six allocators
carries an upper bound**. The only guard any of them applies is `count >= 1`:

| Operator | Body | Allocator | Table header | Stride |
| --- | --- | --- | --- | ---: |
| `maxauratypes` | `0x0042e1e0` | `0x0042e080` | `0x005cd334` | 72 |
| `maxmissiletypes` | `0x004af560` | `0x004af3e0` | `0x005cd2f8` | 52 |
| `maxmounttypes` | `0x004b0f40` | `0x004b0e30` | `0x005cd328` | 48 |
| `maxterrainspritetypes` | `0x0050d1f0` | `0x0050d030` | `0x005cd304` | 48 |
| `maxreferencepalettes` | `0x004b9eb0` | `0x004b9d60` | `0x005cd34c` | 4 |
| `maxunittypes` | `0x00524720` | `0x00524100` | `0x005cd2ec` | 1000 |

Each allocator opens with `cmp <count>, 1` / `jge`, computes `count * stride` by `lea`/`shl`, calls
the allocator at `0x00533864` (`C:\lomse\source\storm.h`), runs the per-element constructor where
there is one, then writes a three-field header. **Each of the six function bodies contains exactly
one compare against a literal, and it is that `cmp <count>, 1`** — checked across each whole body,
not just its entry.

Five of the six share one header layout — **`+0` capacity, `+4` used, `+8` base**.
`maxreferencepalettes` does **not**: `0x004b9d94` writes the base to `+0` and `0x004b9d9d` writes the
capacity to `+8`, with `used` still at `+4`. It also has no per-element constructor loop, its
elements being 4 bytes. Do not carry the aura layout across to it.

The aura path in full, since it is the one that is exactly full today:

```
0042e1e0  maxauratypes           ; pops n, ecx = 0x5cd334, call 0x42e080
0042e256  push 0x555ea0          ; "maxauratypes failed" on a false return

0042e0a0  cmp  ebx,1
0042e0a7  jge  0x42e0ad          ; count < 1 fails -- THE ONLY BOUND
0042e0b4  lea  eax,[ebx+ebx*8]   ; x9
0042e0c3  shl  eax,3             ;  => count * 72 bytes
0042e0ea  call 0x42dcc0          ; per-element ctor, esi += 0x48 each pass
0042e103  mov  [edi+8],eax       ; base
0042e108  mov  [edi],ebx         ; capacity
0042e10a  mov  [edi+4],0         ; used
```

**Overflow is loud, not fatal.** `addauratype` (`0x0042e290`) pops five operands and appends through
`0x0042e160`, which is the same idiom the unit-type table uses:

```
0042e164  mov  eax,[edx+4]       ; used
0042e167  mov  ecx,[edx]         ; capacity
0042e169  cmp  eax,ecx
0042e16b  jl   0x42e175          ; else return -1
0042e193  rep  movsl             ; ecx = 0x12 -> 72 bytes copied in
```

A 71st `addauratype` against a capacity of 70 therefore returns **-1**, the operator prints
`"addauratype failed"` (`0x00555eb4`), and the script's `def` binds the name to -1. Nothing faults at
registration time. That exact byte sequence for the append guard occurs at **5** sites in `.text` —
`0x0042e164` (aura), `0x0043c434`, `0x004af4d4` (missile), `0x0050d144` (terrain sprite),
`0x005241e4` (unit type) — so the tables share one implementation, not five similar ones.

Lookup (`0x0042e130`) takes a **full 32-bit** index, rejects negatives, and compares against
`[ecx+4]` — the live **used** count, never the capacity and never a literal — then scales by 72.
Same shape as `getunittypedata`. Nothing reaches the aura table except through it: the constant
`0x005cd334` appears at **9** sites in `.text`, all of them `mov ecx` for a method call, and the
header words `0x005cd338` / `0x005cd33c` appear at **none**, so no code reads `used` or the base by
absolute displacement. No aura index is narrowed at any site this search reached.

**Observed in the corpus (GS5R3).** `gs\aura.gs` is the only member of the 1,700 that mentions
auras at all. Line 1 is `70 maxauratypes`; below it are exactly **70** non-comment `addauratype`
calls — **62** of the form `/name ... addauratype def`, and **8** more (one per faith) whose results
go into the `/aura_dict << ... >>` dictionary instead of a name. So the table is exactly full, and
raising it means editing one literal on line 1.

### What this does not settle

- **No attended run has raised any cap.** The verdict rests on the disassembly alone.
- **Six allocators, not every cap.** `setmaxartifacttypes`, `setmaxquesttypes`, `maxpalettes`,
  `maxdialogs` and `maxgraphics` were **not** examined; nothing here says they share the shape.
- **Savegames were never examined for aura ids.** The savegame work covered `LS_SPR_` / `LS_PLR_`
  and never touched the aura table, so a save written by a build with more than 70 aura types is
  untested in both directions.
- 🔴 **One unchecked dereference.** `0x0042d6b0` calls the lookup and immediately executes
  `mov eax,[eax+0x24]` with **no null test**, while its sibling setter `0x0042d750` does null-check
  (`test edi,edi` at `0x0042d775`). An object still carrying an out-of-range aura id would fault
  there. Whether that path is reachable after a failed registration was **not traced** — the setter
  writes the id into `+8` before validating it, which is what makes the question live.
- **Memory is not the constraint.** 72 bytes per entry: 70 -> 200 costs 9,360 bytes.

## The build, concretely — one new unit (LIFE + `WMT`)

1. Donor **`aicr4`** — 38 frames per zoom, 4 cycles, both files 100% direct.
2. `archives/imp.mpq/units/imp/liwmtA.imp` + `liwmtB.imp` — donor bytes, frames repainted through
   `write_frame_pixels` at the donor's exact per-frame dimensions. `allow_new_members = true`.
   **Plus `liwmtA.H` and `liwmtB.H`** — the donor's headers with the `#define` prefix renamed to
   `LIWMTA_`/`LIWMTB_`. Without them the unit loads and animates nothing.
3. `archives/pic.mpq/portrait/LIWMTP00.LBM` — a 70x67 donor portrait through `encode_with_indices`.
4. `archives/gs.mpq/units/liwmt.gs` — shaped like `units\pyele.gs`, with `/code WMT /faith LIFE`
   and `/impfile_proc{"liwmt"unittype_imp_filename}`.
5. `archives/gs.mpq/gs/unittype.gs` — append `"units/liwmt.gs"run` after every existing run.
6. Optional sounds in `gs/soundfx.gs`.
7. Reachability, cheapest first: a summon spell modelled on `gs\spells\raisskel.gs`, or an encounter
   spawn via `add_unit_to_location`. Full recruitment additionally needs a
   `building_faith WMT unitcodegettype` branch in `gs\Dlg\newbuild.gs` and a new
   `gs/text/ll<BLDG><SLOT>.gs`.
8. `scripts/mod-validate.sh` -> `scripts/mod-build.sh` -> `scripts/install-dev.sh`.
