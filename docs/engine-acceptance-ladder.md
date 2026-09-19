# Engine-acceptance ladder run sheet

**Rungs 6 and 7 were run attended on 2026-09-19, and two further rungs were built and run the same
evening in answer to what they showed. Rungs 0–5 have not been run.** Everything below this outcome
was written before the game was launched, so none of it is hindsight; rungs 8 and 9 were added
afterwards, and each states the prediction that was recorded before its own run.

## Outcome, 2026-09-19

**The engine accepts `STORED`-class archives from this pipeline.** That class — `0x80010000`,
EXISTS with no compression — had never been accepted from this project before. Rung 6's no-op was
indistinguishable from the shipped game, and rung 7's tones were plainly audible, on two different
load paths: `playsoundfx` off the sound dictionary, and `loadstaticsound` +
`setdefaultbuttonsound`.

**`sndfx.mpq` is the archive the engine opens, for both shared members.** 1,214 names are shared
between `sndfx.mpq` and `special.mpq` carrying byte-identical members, and nothing before this said
which copy was read.

| Member | Archive the engine read | How it was established |
| --- | --- | --- |
| `wav\welcome.wav` | `sndfx.mpq` | Rung 7 gave each archive a different tone and the listener heard `sndfx.mpq`'s; **rung 8 exchanged the two tones and the report flipped**, which is what makes this a measurement rather than a reading. |
| `wav\button.wav` | `sndfx.mpq` | Rung 9 replaced it with digital silence in `sndfx.mpq` and a 1760 Hz tone in `special.mpq`. The click was silent. |

### The reading that was withdrawn, and why the instrument was at fault

Rung 7 was first reported as a **split** — `welcome.wav` from `sndfx.mpq`, `button.wav` from
`special.mpq`. That conclusion was wrong and is withdrawn.

`wav\button.wav` is **41 ms**. At 220 Hz that is nine cycles, which the ear receives as a click
with no perceptible pitch, and in rung 7 it played moments after a 5.67-second low tone, so
"higher" could as easily have been relative to what had just stopped. Rung 8's report for the
button — *"still seems high but maybe not as high"* — is neither a flip nor a non-flip; it is the
listener correctly reporting that **the instrument cannot be read**.

Rung 9 replaced the pitch judgement with a presence judgement, which 41 ms can carry, and answered
on the first listen. The error inverts
[[feedback-state-the-expected-value-before-you-look]]: the expected values *were* stated, but for a
discrimination the human sensor cannot make. Asking for one produces a confident answer either way,
and that confidence is indistinguishable from a real result.

Rung 9 is also built knowing which of its two readings is weaker. A silent click asks the listener
to certify an **absence**, so the audible side was made as loud and as many cycles as 41 ms allows —
1760 Hz, about 75 cycles — rather than the nine that made rung 8 unreadable.

### A third thing, found by accident and worse than either

**`refuse_if_game_running` had never matched the game.** Its pattern,
`^[A-Za-z]:[\\]lomse[.]exe`, requires `lomse.exe` to follow the drive letter immediately; the
executable is six directories down. Observed against the live process, PID 77245, during rung 7:

```
c:\program files (x86)\steam\steamapps\common\lords of magic special edition\english\lomse.exe /* MVK_CONFIG_FULL_IMAGE_VIEW_SWIZZLE=1
```

`install-dev.sh` advertises that it refuses while the game runs, and would instead have swapped
archives under the live process — the one corruption no checksum afterwards can undo. The guard had
been "verified in both directions" the previous day against a decoy reading `d:\lomse.exe`, written
from the same assumption as the pattern: a fixture shaped like the belief under test can only agree
with it, which is [[feedback-fixture-shaped-like-the-corpus]] landing inside a safety check. Fixed
on `claude/lomse-guard-pattern`, with tests that spawn the observed command line and drive the
**shipped** shell function rather than a copy of the regex.

### Corrections to this sheet itself

- The listener was told to expect a tone lasting the member's full **5.67 s**. Only the first
  **750 ms** is tone; the remaining 4,921 ms is deliberate silence, which rung 7's own mod manifest
  states and that instruction contradicted. The listener reported the shorter duration and was
  right. An expected value stated wrongly is worse than none: had they deferred to it, they would
  have reported a defect that does not exist.
- `wav\welcome.wav` **does** play at menu open, which rung 6 alone could not establish — a no-op is
  by construction indistinguishable from silence if nothing plays there at all. Rung 7 settled it.


Seven builds, each installed into the development profile on its own, and one look or one listen
each.
macOS will not let this project drive the Wine window, so every keypress here is a request to a
person; the ladder is ordered and worded to cost as few as possible.

## The four questions, and the fifth that arrived late

1. **Does our IMP pixel encoder survive the engine?** Every IMP this repository has ever written
   landed in a loose `.imp`, and loose files were measured on 2026-09-16 not to override MPQ
   members. So **no IMP we ever wrote has been read by the engine.**
2. **Does the full ByteRun1 PBM *encoder* survive the engine?** The one accepted `pic.mpq` change
   used `tools/pbm_patch.py`, which rewrites the second byte of a repeat packet and therefore
   cannot change a file's length. The encoder in `spikes/asset-viewer/src/pbm.rs` is unproven.
3. **Does a size-changing member edit survive?**
4. **Does an *added* member survive?**
5. **Does a STORED member survive?** Added 2026-09-18 with the WAVE encoder. Every archive
   acceptance this project has proven — `gs.mpq` on 2026-09-16, `pic.mpq` on 2026-09-18 — was on
   members stored `0x80010100`, IMPLODE. Every member of `sndfx.mpq` and `special.mpq` is
   `0x80010000`, **STORED**. That is a storage class the engine has never been asked to accept from
   us, and it is a different question from another image member rather than a repeat of one.

## The blocker, and how it was cleared

`imp.mpq` has no `(listfile)` and no self-named member — and neither, it turns out, do `sndfx.mpq`
or `special.mpq`. Without names, every member of each lists under a `File%08u.xxx` pseudo-name, and
**Observed 2026-09-18** both ways round that fail: a real name is refused for not being in the
archive's catalogue, and the pseudo-name reaches `SFileAddFileEx` and comes back with StormLib
error 22, because it resolves by block position and is never hashed into the hash table. Until the
recovered names were threaded through, `pic.mpq` could not be packed at all, and neither could
these three.

`profile_listfile()` in `scripts/lib-mod-pipeline.sh` now supplies
`reports/member-names/all-profiles-imp-recovered.txt` (3,600 names),
`all-profiles-sndfx-recovered.txt` (1,880) and `all-profiles-special-recovered.txt` (1,218), and
`scripts/repack-archive.sh` hands the same file to the repack **and to both manifests** — a shape
check that named one side and not the other would be comparing two different addressings of one
archive. With that in place all three pack, deterministically, with their member sets intact.

**`gs.mpq` is deliberately left alone.** It carries its own `(listfile)`, and supplying recovered
names for its 372 unnamed entries would re-address them from an unnamed multiset to per-block named
members *inside the shape check* — a change to the exact path Phase 4 proved against the engine.

## The ladder

A failure has to name the step that failed, or the run is unreadable. Read the "if it fails" column
as strictly as the "if it works" one: two of these rungs expect the screen to look **exactly as
shipped**, and a rung whose expected picture is the shipped picture cannot, on its own, tell "the
engine read our file" from "the engine ignored it". Those are **corruption controls**, and each one
is read together with the rung after it, which changes something unmissable through the same
machinery.

| Rung | Mod | Archive(s) | New variable |
| ---: | --- | --- | --- |
| **0+1** | `imp-cursor-noop` | `imp.mpq` | Control. Repacking `imp.mpq` at all, and our IMP encoder reproducing a shipped frame exactly. **One artefact**, because the encoder's output is byte-identical to the member — asserted, not assumed. |
| **2** | `imp-cursor-repaint` | `imp.mpq` | An IMP pixel change, at **identical length** (`shift=0`, member still 90,000 bytes). |
| **3** | `pic-newgame-reencode` | `pic.mpq` | The PBM **encoder**, pixels untouched, member **302,432 → 302,714 bytes**. Corruption control for rung 4, and the first size-changing member. |
| **4** | `pic-newgame-stripe` | `pic.mpq` | The PBM encoder with a visible change, member **302,432 → 256,996 bytes**. |
| **5** | `imp-added-member` | `imp.mpq` | A member `imp.mpq` never had, carried alongside rung 2's visible change. |
| **6** | `audio-welcome-noop` | `sndfx.mpq`, `special.mpq` | Control for the **STORED** class, and our WAVE encoder reproducing two shipped members exactly. One artefact, same as rungs 0+1. |
| **7** | `audio-welcome-tone` | `sndfx.mpq`, `special.mpq` | An audible replacement of **identical length**, a *different* one in each archive. |

**Rungs 6 and 7 may be run first.** They depend on nothing in 0–5 — different archives, a different
storage class, a different decoder — and they are by some distance the cheapest observation on the
sheet: no navigation, no picture to compare, the sound either fires or it does not. If only one
sitting is available, do 6 then 7.

## Before the run

```sh
# 1. Build every rung. Installs nothing, launches nothing. Takes a few minutes.
scripts/build-acceptance-ladder.sh

# 2. Record a pristine copy of the three archives the development profile predates.
#    Adds only what is missing, refuses an archive that is already modified, and writes
#    nothing into the game directory. Without it, rollback for those archives does not exist.
scripts/install-dev.sh --record-pristine
```

Step 1 writes `artifacts/engine-acceptance-ladder/offline-checks.txt`. **Read the build ids out of
that file**, not out of this one: a build id is a digest of the mod tree, the base archives *and the
tool binaries*, so recompiling the tools changes it. The ids below are what the 2026-09-18 build
produced and are here to be compared against, not copied blindly.

| Rung | `scripts/install-dev.sh MOD_ID BUILD_ID` | Archive digest |
| ---: | --- | --- |
| 0+1 | `imp-cursor-noop 97cbdaa91ce8` | `imp.mpq` `fd84136cb54c53a8` |
| 2 | `imp-cursor-repaint 662dbe11a111` | `imp.mpq` `bc49262901870699` |
| 3 | `pic-newgame-reencode b630c5fdf97a` | `pic.mpq` `d8d59a106112ef98` |
| 4 | `pic-newgame-stripe 677597622f4e` | `pic.mpq` `e78486a2be7170e6` |
| 5 | `imp-added-member 04999ceaaa69` | `imp.mpq` `ef834c35483471a6` |
| 6 | `audio-welcome-noop 48ba953d15c7` | `sndfx.mpq` `c82323af22335959`, `special.mpq` `140cef430465ab06` |
| 7 | `audio-welcome-tone b7452899a1c3` | `sndfx.mpq` `bf7c93ecb9ad46f1`, `special.mpq` `61102efd225d9677` |

The archive digests are a function of the archive bytes alone and did not move when the tools were
rebuilt; the build ids did. That is the difference the warning above is about.

## The loop, once per rung

```sh
scripts/install-dev.sh MOD_ID BUILD_ID     # refuses while lomse.exe is running
```

1. Open **`Lords of Magic Development.app`** from `~/Applications/`. Nothing else: the three other
   profiles are the recovery baseline and `tools/install_guard.py` will not let this pipeline write
   to them.
2. Make the observation for the rung, below.
3. Quit the game.

```sh
scripts/restore-dev.sh                     # back to pristine, verified against MANIFEST.sha256
```

`restore-dev.sh` restores **every** archive in the pipeline, so one call undoes any rung. It checks
each source against the record written when the profile was created — never against the file it was
copied from, which would prove the copy succeeded and nothing else.

---

## Rung 0+1 — the repack control, and the IMP encoder as a no-op

**Install** `imp-cursor-noop`. **Where to look:** the mouse pointer, the moment the main menu
appears. No clicks.

**Why there.** `gs/cursor.gs` opens with
`"iface/cursors.imp" loadcursorimp CURSORS_POINTER setcursortype`, and `START.GS` runs
`"gs/cursor.gs"` and then `showcursor` before `newdlg opendialog`. The member is
`iface\cursors.imp`; the `POINTER` sequence is frame **111**, 31×28, a `direct` record with a
payload of its own.

**Expected value:** `artifacts/engine-acceptance-ladder/rung0-1-pointer-expected.png` — the shipped
green-gauntlet pointer, exported by our decoder from the archive that is being installed.

| | |
| --- | --- |
| **If it works** | The game starts and the pointer is the usual green gauntlet, matching the PNG. |
| **If it fails** | No cursor at all; a garbled or mis-coloured cursor; or the game fails to start. Any of those is the *repack* breaking `imp.mpq`, because not one member's bytes differ from the shipped archive. |

**What it proves.** That StormLib rewriting `imp.mpq` under our recovered names leaves an archive
the engine can still read. Rung 1 rides along: the member packed here came out of
`write_frame_pixels`, and the build asserts it is byte-identical to the shipped member — so the
engine is reading our encoder's output even though nothing looks different.

**What it does not prove.** That the engine read *this* archive rather than falling back to
something else. It cannot: the expected picture is the shipped picture. Rung 2 is what closes that.

---

## Rung 2 — an IMP pixel change the engine cannot hide

**Install** `imp-cursor-repaint`. **Where to look:** the same mouse pointer, same moment.

**Expected value:** `artifacts/engine-acceptance-ladder/rung2-pointer-expected.png` — the same
gauntlet with its body repainted **white/ivory**, its dark outline and its silhouette unchanged.

| | |
| --- | --- |
| **If it works** | The pointer is a white gauntlet of exactly the shipped shape. |
| **If it fails, one way** | The pointer is the shipped **green** gauntlet. The engine did not read the member we wrote — rung 0+1 having passed, that is the engine ignoring a changed `imp.mpq`, not a broken one. |
| **If it fails, the other way** | The pointer's *shape* is wrong — torn, shifted, half-missing. That is the IMP encoder or the frame writer, not archive acceptance. |

The three outcomes are different observations and cannot be confused, which is the point of leaving
the outline and silhouette out of the edit.

**Why the edit is shaped like that.** Eight **disjoint palette-index swaps**, whole-frame. A set of
disjoint swaps is a permutation of the index alphabet, and a permutation maps equal neighbours to
equal neighbours and unequal to unequal — so the IMP run-length encoder emits the same packets at
the same lengths, the payload keeps its byte count (`stored=530->530`, `shift=0`) and not one
absolute pointer inside the file moves. Size is rung 3's variable. `tests/test_png_index_patch.py`
asserts this against the real encoder on this exact member, with a fill as the negative control.

---

## Rung 3 — the PBM encoder, and a member that changed size

**Install** `pic-newgame-reencode`. **Where to look:** the main menu itself, as soon as it appears.

**Why there.** `gs/dlg/newdlg.gs` builds the main menu with
`/newgame_page "lbm/newgame.lbm" lbm def /newgame_backdrop newgame_page 0 0 640 480 doodad def`.
Despite its name, `lbm\newgame.lbm` is the **main menu backdrop** — the doors image — not a screen
shown during new-game setup. That mistake cost an observer a wrong instruction on 2026-09-18 and is
written down here so it is not made twice.

**Expected value:** `artifacts/engine-acceptance-ladder/rung3-menu-expected.png` — pixel for pixel
the shipped menu. The member's *bytes* changed (302,432 → 302,714, every ByteRun1 packet repacked by
`pbm.rs`); its *pixels* did not, and the build asserts that by exporting the image back out of the
packed archive and comparing it with the shipped export.

| | |
| --- | --- |
| **If it works** | The main menu is exactly as it has always been. |
| **If it fails** | Torn, smeared, colour-shifted or shifted-by-rows artwork; a black screen; or a failure to start. Any of those means the engine read a re-encoded, longer member and could not make sense of it. |

**What it proves and does not.** A clean result proves the engine is not *broken* by a re-encoded,
size-changed member. It cannot prove the engine read it, for the same reason as rung 0+1. Rung 4 is
the pair to this one and must be run second.

Only 8 of the 1,045 shipped PBMs re-encode byte-identically, so "re-encode without changing the
length" is not an option this member offers. That is why encoder and size change share a rung here
and are separated by comparing rung 3 against the already-accepted `pbm_patch` run instead.

---

## Rung 4 — the PBM encoder with something to see

**Install** `pic-newgame-stripe`. **Where to look:** the main menu.

**Expected value:** `artifacts/engine-acceptance-ladder/rung4-menu-expected.png` — the doors image
with a solid **red vertical stripe**, x 260–380, y 60–420, straight-edged, and every one of the four
screen edges untouched.

| | |
| --- | --- |
| **If it works** | A clean red stripe down the middle of the doors, with the surrounding artwork exactly as shipped. |
| **If it fails, one way** | The shipped menu, no stripe. The engine did not read the member — and rung 3 having passed rules out "the archive is broken". |
| **If it fails, the other way** | The stripe is there but the rest of the image is damaged. That is the encoder, not acceptance. |

**Two things make this more than a repeat of the 2026-09-18 run.** The member's length moves again
(302,432 → 256,996 — a fill merges runs, so the file *shrinks*). And the edges are **straight**:
`pbm_patch.py` could only repaint whole runs and left a ragged edge, so a clean-edged rectangle is
a thing the old mechanism could not have produced. The colour is not a new assumption — palette
index 1 is pure red in this member's own CMAP and the engine was seen rendering it as red from this
very member.

---

## Rung 5 — a member the archive never had

**Install** `imp-added-member`. **Where to look:** the mouse pointer again.

This build carries **two** changes: `iface\cursors.imp` repainted exactly as in rung 2, and a new
member `iface\ladder.imp` that no shipped archive holds and nothing in the game will ever ask for.

**Expected value:** `artifacts/engine-acceptance-ladder/rung5-pointer-expected.png` — identical to
rung 2's.

| | |
| --- | --- |
| **If it works** | The white gauntlet, exactly as in rung 2. The hash table, the block table and all 3,600 original members survived the archive growing by one. |
| **If it fails** | The game fails to start, or the pointer reverts to green, or another part of the interface is wrong. Rung 2 having passed with the same visible change, the added member is the only new variable. |

**The limit, stated plainly.** This tests whether the engine **tolerates** an archive that grew a
member. It does not test whether it can **read** one, and no observation on screen could — nothing
asks for `iface\ladder.imp`. What *is* established, offline: `lom-mpq probe-names` reopens the
packed archive and resolves that name **through the archive's own hash table**, which is the same
lookup the engine's Storm performs, and the member reads back as the bytes it was added from.

---

## Rung 6 — the STORED class, and the WAVE encoder as a no-op

**Install** `audio-welcome-noop`. **Two things to listen for**, both with the volume up:

1. **`wav\welcome.wav`** — the moment the main menu opens, after the intro videos and the loading
   bar. No clicks. `START.GS` ends with `soundfxdict begin Welcome_wav playsoundfx end` immediately
   before `newdlg opendialog`, and `gs/soundfx.gs` defines
   `/Welcome_wav "wav/welcome.wav" addsoundfx def`. Stereo 8-bit 22,050 Hz, 125,049 frames,
   **5.67 seconds**, 250,142 bytes.
2. **`wav\button.wav`** — click any button on the main menu, as many times as you like.
   `gs/soundfx.gs` does `/button_sound "wav/button.wav" addsoundfx def button_sound
   loadstaticsound button_sound setdefaultbuttonsound`, and **no script in the corpus calls
   `setbuttonsound`**, so every dialog button plays it. Mono 8-bit 22,050 Hz, 915 frames, 41 ms,
   1,036 bytes.

**Why two members.** `welcome.wav` is the observable — long, unattended, impossible to miss — but
its WAVE layout is the weakest in the archive: `fmt |data` with an **even** data chunk and no
ancillary chunk at all, so it exercises no pad byte and carries nothing verbatim. `button.wav`
covers exactly that gap: `fmt |data|LIST` with an **odd** 915-byte data chunk, so a pad byte is
written *between* two chunks rather than trailing where a reader could skip it, and a 68-byte
`LIST` that `--import-wave` carries through untouched. It is also repeatable on demand, which
`welcome.wav` is not.

**A bound, stated because it is a real one.** There is **no member of either archive with an
odd-sized *ancillary* chunk**. Measured 2026-09-18 across all 3,098 members: the only chunk that is
ever odd is `data` — 1,294 of 1,880 in `sndfx.mpq`, 754 of 1,218 in `special.mpq` — and every one
of those 2,048 pad bytes is `0x00`. So the corpus cannot exercise a non-zero pad on its own. That
matters because `wave.rs`'s `rebuild()` briefly wrote `0` for every pad instead of the value it had
parsed, and a no-op control that "passed" because the corpus happened to agree with a bug would be
a control that proved nothing. `scripts/build-acceptance-ladder.sh` therefore **gates both audio
rungs on a fixture** with an odd `LIST` body whose pad is `0x20`: if `--import-wave` does not hand
that byte back, rungs 6 and 7 are not built at all.

**Why both archives.** `sndfx.mpq` and `special.mpq` each hold both members, byte-identically, and
nothing known says which one the engine opens — 1,214 member names are shared between them.
Rewriting one and not the other would make a null result mean either "the engine rejected our
archive" or "the engine read the other copy": two answers wearing one observation.

**Expected values:** `artifacts/engine-acceptance-ladder/rung6-welcome-expected.wav` and
`rung6-button-expected.wav` — the shipped sounds, playable in any audio player.

| | |
| --- | --- |
| **If it works** | The game starts; the usual welcome sound plays as the menu opens; a button click makes the usual click. Both unchanged. |
| **If it fails** | Silence, noise, a truncated sound, or a failure to start. Not one member's bytes differ from the shipped archives, so that would be the repack breaking a STORED archive. |

**Confirm both sounds are audible on this rung**, because rung 7 reads a *null* result as meaning
something, and "the volume was down" must not be one of the things it could mean.

---

## Rung 7 — an audible replacement, and which archive the engine opened

**Install** `audio-welcome-tone`. **Listen at the same two moments.**

Both members keep their exact lengths in both archives, and each *archive* gets a different tone:
**220 Hz into `sndfx.mpq`**, **1760 Hz into `special.mpq`**, three octaves apart. The first 750 ms
is tone and the rest is silence — which, for `button.wav`, means the whole 41 ms is tone.

**Expected values:** `rung7-welcome-expected-sndfx.wav`, `rung7-welcome-expected-special.wav`,
`rung7-button-expected-sndfx.wav`, `rung7-button-expected-special.wav`, all in
`artifacts/engine-acceptance-ladder/`. Play all four before the run so they are in the ear.

| Heard | What it means |
| --- | --- |
| **A low beep** where the voice was, and a low blip on a button click | The engine read our rewritten **`sndfx.mpq`**. A STORED member rewritten by our WAVE encoder reached the engine, and `sndfx.mpq` is where sounds come from. |
| **A high beep**, and a high blip | The same result, for **`special.mpq`**. |
| **The shipped voice and the shipped click** | Neither rewritten archive was read. Rung 6 having passed, the archives are not broken, so either the engine rejects a *changed* STORED member or the sound comes from a third place. |
| **One member changed and the other did not** | A finding in itself, and the reason two members are in the build: whatever separates them — the odd data chunk, the `LIST`, the length, the sound's role — is the next thing to chase. |
| **Silence, or a click and then nothing** | The member was read and mangled. That is the encoder or the length arithmetic, not acceptance. |

**Its own control travels with it.** "A beep, then quiet, for as long as the voice used to last" is
a different observation from "the sound is broken", and the length being unchanged is what makes
that distinction available.

**Why same-length, and why this is the first audio rung.** `--import-wave` now reads loop points and
cue offsets and **refuses** a length change that would strand them — 147 corpus files carry such
metadata — with `--allow-dangling-loops` as the deliberate escape. A length-changing audio rung
belongs after this one, not instead of it, exactly as the length-preserving `pbm_patch` edit was the
right first `pic.mpq` test.

**Do not read the corpus sweep as making this a formality.** 3,140 of 3,140 members parse,
reserialise identically and pass `verify_import`. That is a strong statement about the **container
walk** and about the PCM conversion, and a much weaker one about everything else: `encode()` replays
declared sizes, pads, `Other` chunk bodies and trailing bytes verbatim, so only `fmt ` and `data`
are genuinely reconstructed. And none of it touches the engine. **This rung is testing more than the
sweep did, not confirming what it already showed.**

---

## Rung 8 — the same two tones, exchanged

**Install** `audio-tone-swapped`. **Same two listens as rung 7**, nothing else.

Rung 8 is rung 7 with the frequencies **exchanged**: 1760 Hz into `sndfx.mpq`, 220 Hz into
`special.mpq`, both members, identical lengths throughout. The build asserts that each rung 8 tone
is **byte-identical to rung 7's opposite-archive tone** — the same sound, the other archive — which
is what makes it a swap rather than a second arbitrary build.

**Prediction, recorded before the run.** If rung 7's archive attribution is real, the report must
**invert**: high at menu open, low on the click. If it does not invert, the attribution is an
artifact of the build or of the listening, and rung 7's conclusion is void. Both outcomes are
results; the rung exists because rung 7 alone could not tell them apart.

**Outcome.** Menu open inverted exactly as predicted — long low on rung 7, high whistle on rung 8,
both times `sndfx.mpq`'s tone. `wav\welcome.wav` is served from **`sndfx.mpq`**.

The button did not invert and did not fail to invert. The report was *"still seems high but maybe
not as high and fairly short still"* — an unreadable instrument, not a measurement. See rung 9.

---

## Rung 9 — presence instead of pitch

**Install** `audio-button-silence`. **Listen at menu open, then click a main-menu button several
times.**

| Member | `sndfx.mpq` | `special.mpq` |
| --- | --- | --- |
| `wav\welcome.wav` | 440 Hz | 440 Hz — **byte-identical to the other archive's** |
| `wav\button.wav` | **every data sample `0x80`**, the zero level | 1760 Hz |

`welcome.wav` carries one variable on purpose: it is the **liveness control**. It fires at menu
open whichever archive wins, so a silent click is attributable to the archive rather than to "the
engine rejected the build" — and 440 Hz is neither tone the listener had heard before, so it cannot
be confused with a previous sitting. The silence is `0x80` throughout, **not** zero bytes: the
member keeps its 1,036 bytes, its `LIST` chunk and its odd-`data` pad byte, all asserted against
the shipped member, because silence that shortened the member would be a different experiment.

**Prediction, recorded before the run.** Audible click → the engine reads `button.wav` from
`special.mpq`, and the two members come from **different** archives. Silent click → it reads
`sndfx.mpq`, and both come from the same one. No pitch judgement either way.

**Outcome.** The menu tone played; the click was **silent**. `wav\button.wav` is served from
**`sndfx.mpq`**. There is no split: both members come from the same archive, across two different
load paths.

---

## A candidate rung that is NOT built: `lom.cfg` audio volume

Proposed as the cheapest rung on the ladder. Measuring it first says it is not, and the measurement
is worth more than the rung would have been.

The proposal was that patching `lom.cfg[0x00]` — the last-music-volume word, config object `+0x18`,
global `0x5aa144` — reaches the helper `setmusicvolume` calls, via `setlastaudiosettings`, and that
the path runs because `setlastaudiosettings` is mentioned once in every script corpus.

**Where that mention actually is.** `gs/scenario.gs`, inside a scenario-start sequence:

```
... processgamemessages initusers 0 setuserforplayer loadconfig setlastaudiosettings
    togglescrollmusic playterrainambiance gamemode multiplayer_startscenario ...
```

`START.GS` ends with `loadconfig` and **never calls `setlastaudiosettings`**. So the file is read at
startup and applied at **scenario start**. The observation costs a full new-game navigation — faith,
champion type, difficulty, map — which is the most expensive rung on the sheet, not the cheapest.

**And there is a free observation sitting in front of it.** The Development profile's `lom.cfg` is
the **160-byte** form, and its first sixteen bytes are all zero: all four last-audio words are
already `0`. If `0` meant silence and that path applied it, in-game music would already be silent
every time a scenario starts. So whoever picks this up should **start by asking whether music plays
inside a game at all**, before editing anything — that single answer constrains the result more than
a patched byte would, and it is free on any session where somebody starts a game for another reason.

Nothing here has been edited. `lom.cfg` in all four profiles is untouched, and this section exists
so the next attempt starts from the call site rather than from the assumption.

## What was checked without the engine

`scripts/build-acceptance-ladder.sh` re-derives every edit from the installed baseline on each run
and writes `artifacts/engine-acceptance-ladder/offline-checks.txt`. As of 2026-09-18 every check
passes:

- **Reproducible.** Each archive is repacked three times and all three runs are byte-identical.
- **Shape preserved.** Every rung's shape check accounts for every member: 3,599 of 3,600 `imp.mpq`
  members proven unchanged with exactly one declared change; 1,879 of 1,880 and 1,217 of 1,218 for
  the audio archives. Rung 5 additionally reports `member_added_as_declared iface\ladder.imp`, which
  is the only way an added member is not a failure.
- **The no-op rungs are checked as no-ops, not waved through.** `--expect-unchanged` inverts the
  rule that catches a repack which silently did nothing: for rungs 0+1 and 6, content that *moved*
  is the failure.
- **Read back from the packed archive**, never from the loose file that went in: each changed
  member is exported out of the built MPQ by our own decoder and compared with what was drawn or
  written. That export *is* the expected-value PNG or WAV the observer uses.
- **`tools/mod_validate.py` and `tools/asset_validate.py` clean.** Both re-encoded images pass the
  IFF walk, BMHD, CMAP and palette-range checks.
- **The audio writer is gated on a fixture, not on the corpus.** Rungs 6 and 7 are not built unless
  `--import-wave` hands back a `0x20` pad byte on a template the corpus could never have supplied.
- **`--validate-imp` on every packed `imp.mpq`**: 1,798 stem pairs, 0 failures.
- **Only the intended member moved.** Rung 2 additionally exports frame 112 of the same member —
  which the edit never named — out of the packed archive and compares it with the shipped export.
- **Rollback verified by bytes.** Each pristine copy in the development profile is compared with the
  baseline archive using `cmp`, not against a digest this project recorded. A digest snapshot
  detects a change; it cannot undo one, and it cannot notice that the copy it describes was itself
  overwritten.
- **The other profiles are untouched.** Every archive of all three installed profiles is hashed
  before and after the whole run and compared.

## What could not be checked without the engine

Everything the rungs exist to ask. In particular:

- whether the engine reads a rewritten `imp.mpq`, `sndfx.mpq` or `special.mpq` at all;
- whether it tolerates an added member, and — untestable by any observation here — whether it could
  read one;
- **which** of `sndfx.mpq` and `special.mpq` it opens, which only rung 7 can answer;
- whether `lom.cfg`'s last-audio words reach the applied volume — see the candidate rung above,
  which is unanswerable offline and costs a full new-game navigation to answer at all;
- whether the IMP `layout=Tight` reading is the one the engine uses for `iface\cursors.imp` frame
  111. The frame is not in the 45 ambiguous or the 752 unobservable classes, and the importer would
  have warned if it were, so this is not a live worry — but nothing offline can close it.

## Cost and risk

Seven installs into `Lords of Magic Development.app` and nothing else. The three installed profiles
are opened read-only by this pipeline and written by none of it; `~/Applications` is refused as an
output path by `scripts/repack-archive.sh`, and every write to the development profile is routed
through `tools/install_guard.py`, an allowlist of exactly one directory. The loose `map/` directory
— which has no backup anywhere — is never touched. Rollback is one command and is verified against
a record written when the profile was created.
