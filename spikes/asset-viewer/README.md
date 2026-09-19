# Rust Asset/MPQ Tool

## Outcome

The initial spike succeeded and is now evolving into the Stage 1 native asset layer. A native 64-bit Rust program opens the installed game's MPQs through a narrow read-only StormLib wrapper, loads an external filename catalog, classifies and inspects members, decodes IFF `FORM PBM` images, IMP sprite frames, tile-set definitions, map records, RIFF WAVE audio and Smacker containers, probes BMP metadata, and displays images, animations, and terrain through SDL3.

The repository contains no game assets. Commands below require a local, legally obtained installation.

## Prerequisites

The current build targets Apple Silicon Homebrew:

```sh
brew install rust stormlib sdl3 node
```

`node` is needed only by `tests/test_map_editor_client.py`, which drives the map editor's browser
client; that file fails rather than skipping when it is absent, because a check that quietly does
not run is not a check. Rust can instead be installed with `rustup`. The current `build.rs` expects StormLib and SDL3 under `/opt/homebrew/opt`; portable dependency discovery is tracked as remaining Stage 1 work.

## Build and test

```sh
cd spikes/asset-viewer
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## Use

Set paths to local archives without copying them into the repository:

```sh
PIC_MPQ='/path/to/Lords of Magic Special Edition/English/pic.mpq'
IMP_MPQ='/path/to/Lords of Magic Special Edition/English/imp.mpq'
GS_MPQ='/path/to/Lords of Magic Special Edition/English/gs.mpq'
SNDFX_MPQ='/path/to/Lords of Magic Special Edition/English/sndfx.mpq'
LOMSE_EXE='/path/to/Lords of Magic Special Edition/English/lomse.exe'

cd ../..
scripts/fetch-lom-listfile.sh
LISTFILE="$PWD/artifacts/reference-listfiles/lords-of-magic.txt"
cd spikes/asset-viewer

target/release/lom-asset-viewer --list "$PIC_MPQ"
target/release/lom-asset-viewer --catalog "$PIC_MPQ"
target/release/lom-asset-viewer --scan "$PIC_MPQ"
target/release/lom-asset-viewer --scan-gamescript "$GS_MPQ" --listfile "$LISTFILE" --exe "$LOMSE_EXE"
target/release/lom-asset-viewer --probe-gamescript "$GS_MPQ" 'gs\standard.gs' --listfile "$LISTFILE" --eval '3 5 min 3 5 max'
target/release/lom-asset-viewer --inspect "$PIC_MPQ" 'LBM\ACTIONS5.lbm'
target/release/lom-asset-viewer --extract "$PIC_MPQ" 'LBM\ACTIONS5.lbm' /tmp/actions5.lbm
target/release/lom-asset-viewer --validate-imp "$IMP_MPQ" --listfile "$LISTFILE"
target/release/lom-asset-viewer --describe-imp "$IMP_MPQ" 'units\imp\chcr5a.imp' --listfile "$LISTFILE"
target/release/lom-asset-viewer --view-imp "$IMP_MPQ" 'units\imp\chcr5a.imp' --listfile "$LISTFILE"
target/release/lom-asset-viewer --export-imp-frame "$IMP_MPQ" 'units\imp\chcr5a.imp' 155 /tmp/chcr5a-frame-155.png --listfile "$LISTFILE"
target/release/lom-asset-viewer "$PIC_MPQ" 'LBM\ACTIONS5.lbm'
target/release/lom-asset-viewer --wave-roundtrip "$SNDFX_MPQ"
target/release/lom-asset-viewer --wave-roundtrip-dir '/path/to/Lords of Magic Special Edition/English/Wav'
target/release/lom-asset-viewer --export-wave "$SNDFX_MPQ" File00000001.wav /tmp/sound.wav
target/release/lom-asset-viewer --import-wave /tmp/edited.wav /tmp/template.wav /tmp/new-member.wav
target/release/lom-asset-viewer --scan-smk-dir '/path/to/Lords of Magic Special Edition/English/smk'
target/release/lom-asset-viewer --describe-smk '/path/to/Lords of Magic Special Edition/English/smk/Intro.smk'
target/release/lom-asset-viewer --scan-map-dir '/path/to/Lords of Magic Special Edition/English/map'
target/release/lom-asset-viewer --inspect-file '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --describe-map '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --dump-map-cells MAP.scn
target/release/lom-asset-viewer --dump-map-cells MAP.scn 8 8 20 8
target/release/lom-asset-viewer --diff-maps BEFORE.scn AFTER.scn
target/release/lom-asset-viewer --view-map '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --view-map MAP.scn tilesb01.til tilesb01.lbm
target/release/lom-asset-viewer --export-map-preview MAP.scn tilesb01.til tilesb01.lbm /tmp/map-preview.png
target/release/lom-asset-viewer --serve --pic "$PIC_MPQ"
```

`--dump-map-cells` prints one line per cell — `x`, `y`, packed index, raw tag, masked tile index,
whether the unexplained `0x00800000` flag is set, and the elevation word — for the whole grid, or for
an inclusive `X0 Y0 X1 Y1` rectangle. `--diff-maps` prints both headers, every differing cell, and
the first differing byte of the trailing section with a hex window either side, comparing the tails
as raw bytes rather than as assumed records. They are the readback half of an engine probe: writing
chosen values from the running game only proves something if they can be read out again.

## Writing maps

```sh
target/release/lom-asset-viewer --map-roundtrip '/path/to/Lords of Magic Special Edition/English/map'
target/release/lom-asset-viewer --map-set-tile      IN.scn X Y TILE_SLOT   OUT.scn
target/release/lom-asset-viewer --map-set-terrain   IN.scn X Y TERRAIN     OUT.scn
target/release/lom-asset-viewer --map-set-elevation IN.scn X Y VALUE       OUT.scn
target/release/lom-asset-viewer --map-fill-terrain  IN.scn TERRAIN         OUT.scn
target/release/lom-asset-viewer --map-paint-terrain IN.scn X0 Y0 X1 Y1 TERRAIN OUT.scn TILESET.til [--seed N]
target/release/lom-asset-viewer --map-place-sprite  IN.scn X Y SPRITE_TYPE OUT.scn
target/release/lom-asset-viewer --map-sprite-types
target/release/lom-asset-viewer --map-transition-rings
target/release/lom-asset-viewer --map-remove-sprite IN.scn INSTANCE_ID     OUT.scn
```

Everything here rests on one property: **an unedited map re-encodes to the exact bytes it was read
from.** `--map-roundtrip` asserts it over the installed corpus — 365 checked, 365 byte-identical,
21,117 placed-sprite records rebuilt from their typed fields, 0 failures. Run it first.

When **editing existing data**, fields whose meaning is still Unknown — the header word at `0x00`,
the trailing footer, the record attribute field at `+24`, tag bit `0x00800000`, every constant word
in a record tail, and the entire trailing section of any tail that cannot be pinned to one layout —
are **copied, never minted**.
Placing a *new* sprite is the exception: a record that did not exist has to get its bytes from
somewhere, and **it is minted in the map's own layout** — 12 fields for a 48-byte record, 13 for a
52-byte one, 14 for a 53-byte one. In the 47- and 49-byte layouts the minted `+24` is `0x00000001`,
which *contradicts* the corpus reading of that field; in the other three it is `0`, which **agrees**
with all 4,003 of their corpus records but has no engine measurement behind it at all. Only the
49-byte layout's mint was watched being written, so `--map-place-sprite` prints a note on the other
five. See
[map format](../../docs/map-format.md#writing-maps). That is what lets the writer be correct
while the format is only partly solved, and it is also why there is no create-a-map-from-nothing
mode: three of those fields would have to be invented. Generate in the shipped GS5R3 editor, which
makes maps from 32 to 1024 in steps of 32, then edit here.

`TERRAIN` is a number `0..10` or a `gs\maplib.gs` name with or without its `tt_` prefix, so `1`,
`tt_water` and `water` are the same thing.

`SPRITE_TYPE` is likewise **a name or a raw id** — `castle1` works, and a near miss suggests
alternatives. The names come from the engine's own `terrainsprites` dict, dumped on 2026-09-17, and
are **profile-specific**: ids are assigned in script execution order, so a different script set
shifts them. `--map-sprite-types` lists the table and both it and named placement say so.

`--map-set-terrain` reproduces the editor's `forcetexture`: **one cell**, hard edge. The engine's
`setterrain` also blends transition tiles into the 8-neighbourhood, and **that ring is now
measured** for all eleven backgrounds — one offset table plus a per-background anchor. See
[the transition rings](../../docs/map-format.md#setterrain-transition-tiles-one-offset-table-one-anchor-per-background)
and `--map-transition-rings`.

A painted region's **interior** is a random draw from its terrain's tile family and cannot be
reproduced by any writer; the ring can.

A removed `instance_id` **is** reissued by the next invocation — the high-water mark that holds it
back cannot be persisted, because the format has nowhere to put one. If anything outside the map
references a sprite by id, do not remove-then-place.

The loose `map/` directory has no backup, so there is no in-place mode: every command takes an
explicit output path, refuses to write over its input by canonical path, opens the output
`create_new`, and re-parses the encoded bytes to read the edit back before anything reaches disk.

## The map editor UI

```sh
target/release/lom-asset-viewer --serve --pic "$PIC_MPQ"
target/release/lom-asset-viewer --serve tilesb01.til tilesb01.lbm --port 9000
```

A local web UI for the paint verb: point it at a maps directory, choose a file from the listing,
see it drawn through its own tileset, pick a terrain, drag a rectangle, paint, undo, Save As. With
`--pic` the maps directory is filled in for you — in a standard install the maps sit beside
`pic.mpq` — so the common case is zero typing.

**Loopback is not a security boundary, and this does not pretend it is.** The server binds
`127.0.0.1` and never `0.0.0.0`, but any web page in the world can issue requests to `127.0.0.1`,
and a request that only *writes* never needs to read the response, so neither the same-origin policy
nor CORS stops it. Two checks do, and every request passes both:

- **`Origin`** must be this editor's own page when it is present at all, and
- **`Host`** must be a loopback literal carrying this editor's port. That is the one that closes DNS
  rebinding: an attacker who points a name at `127.0.0.1` gets a browser that treats the responses
  as same-origin, but it sends that name in `Host`.

Opening a map is a `POST` for the same reason — it replaces the server's whole session, and a
state-mutating `GET` is reachable from a bare `<img src>`. There is still no user authentication and
none is planned; what there is, is an origin check, and the difference matters.

The page, its script and its stylesheet are compiled into the binary, so the tool is still one file.
No build step, no npm, no framework: the client is vanilla JS drawing 32x32 atlas tiles onto a
`<canvas>`. The atlas is sent once as a PNG and a paint redraws only the cells the server says
changed, so there is no image round trip per edit.

`--pic` is the easy path: the tileset a map is read through is resolved from the gamescript bindings
and both the `.til` and its `.lbm` are read straight out of `pic.mpq` by member name. Nothing is
guessed. A combat map with **no** binding — 168 of the 337 installed `.smp` files — is refused with
that explanation rather than defaulted, and so is one five encounters read through five different
tilesets. For those, name the `.til` and atlas yourself in the second form; that path runs the same
`tileset_mismatch` check `--map-paint-terrain` does, so a modded tileset is accepted and a shipped
one the engine would not use here is refused.

Combat maps work: the terrain palette is built from the resolved tileset's own tiles, so it shows
`aibldg01.til`'s nineteen terrain ids for a battle map and `tilesb01.til`'s eleven for a world one.
Terrain ids are tileset-local and reach 42, so there is no built-in list of terrain names anywhere in
the UI.

**Nothing is ever written in place.** Save As is a filename inside the chosen directory, always a
new file: the target is checked against the open map by device and inode, refused if the extension
changes the map's class, refused if the name is anything but one plain path component, encoded and
re-parsed before anything reaches disk, and opened `create_new`. An existing file is never
clobbered, and the picker is not a way round that.

### Browse, and why the server opens the dialog

Next to both path fields is a **Browse…** button. A web page cannot hand a server a real filesystem
path: `<input type="file" webkitdirectory>` gives file *contents* under fake relative names, and the
File System Access API gives an opaque handle and is Chrome-only. Neither yields `/Users/…`, which
is what the server has to read and write. The server is on the user's own machine, so **the server
opens the dialog** — `POST /api/pick-directory` and `POST /api/pick-save` shell out to the OS
chooser and get the genuine path back.

**All three platforms, through programs rather than a linked crate, so there is still no new
dependency.** macOS uses `osascript`, Windows uses PowerShell driving `System.Windows.Forms`, and
Linux uses `zenity` or `kdialog`, whichever is on `PATH`. Where none is available — a Linux box with
neither installed — the endpoint reports itself unavailable and names what to install, and the typed
field keeps working.

**A dialog program on `PATH` is not a dialog that can be shown.** (One known false refusal: a
Qt or GTK kiosk target — `QT_QPA_PLATFORM=eglfs`, `GDK_BACKEND=broadway` — can draw without either
variable set, and this refuses it. The typed field still works, so the cost is a refusal rather than
a failure, and the trade is worth it.) Running the editor over SSH on a
headless box is supported, and there `zenity` is very often installed with no display at all — it
then fails `gtk_init` and exits 1 with empty stdout, which is indistinguishable from a cancel. So
`DISPLAY` or `WAYLAND_DISPLAY` is required before a Unix flavour is claimed at all. Without that
check Browse became a button that did nothing, logged nothing, and no amount of retrying fixed.

**What is verified, and what is not.** Only the macOS dialog has been watched by a human. The
Windows and Linux builders are asserted at the level of the argument list, the environment, and the
cancel/failure reading; nobody has clicked through either. That is a real limit, so it is worth
saying what those assertions are actually worth: the one bug this feature has already shipped was an
argument-order mistake that no test then covered, and argument lists are exactly what is covered
now. The flavour is carried as a value rather than decided by `cfg!`, specifically so that **a Mac
builds and asserts the Windows and Linux commands too** — `cfg`-gating them would leave two dialogs
that no test anywhere could look at.

Per-platform details that are easy to get wrong, each pinned by a test:

- **`-NoProfile` is the load-bearing Windows flag**: a user profile that writes anything to stdout
  corrupts the path read back off it. `-STA` is passed too, but as explicit insurance rather than a
  fix — an earlier version of this note claimed PowerShell 5 runs `-Command` as MTA and that is
  **wrong**: STA has been the default since PowerShell 3.0 and `-MTA` is the opt-out. Codex caught
  that; the Claude reviewer did not.
- **A cancel exits with a reserved code, not 1.** The scripts are ours, so they can say
  "dismissed" unambiguously — and they must, because the interesting Windows failures exit 1 with
  stderr only, which is the exact shape of a cancel. `Add-Type` failing on a box with no .NET
  Desktop runtime is the real case; a lost `-STA` would be another.
- **The scripts set their output encoding to UTF-8.** `[Console]::Out` otherwise uses the console
  code page, and Rust decodes as UTF-8 unconditionally, so a user whose path contains non-ASCII
  characters would get replacement characters and a path that does not exist.
- **The Windows strings travel as environment variables, never interpolated into the script.** A
  path like `C:\Program Files (x86)\…` is full of PowerShell metacharacters, and needing to type
  such a path is the problem this feature exists to remove — re-introducing it as a quoting bug
  would be a poor trade. With no starting directory the variable is *removed* rather than inherited,
  so a stale value cannot send the dialog somewhere nobody asked for.
- **zenity's `--filename` needs its trailing separator** to mean "start inside this directory"
  rather than "select this directory"; without it the chooser opens one level up.
- **kdialog needs a positional start directory**; given the flag alone it prints usage and exits
  non-zero, which would surface to the user as a broken dialog. Its positionals are also parsed as
  options, so anything beginning with `-` is prefixed `./` — otherwise a file named `--help` makes
  kdialog print help and exit *successfully*, and the help text becomes the chosen path.

**On a Wine-wrapper install the folder dialog may not be able to reach your maps at all, and that
is macOS's rule rather than ours.** This community mostly runs the game inside a wrapper, so the
maps end up somewhere like
`…/Lords of Magic GS5R3.app/Contents/SharedSupport/prefix/drive_c/Program Files (x86)/…/English/map`
— *inside a `.app` bundle*. The folder chooser greys bundles out and will not descend into one by
clicking. Three things follow, and the first is the one to remember:

1. **The route that works needs no dialog.** With `--pic`, the maps directory is already in the
   field when the page loads; press **List** and pick a map. That is zero typing and it does not
   touch the chooser.
2. `choose folder` is now asked `with showing package contents`, which is the documented way to let
   a chooser enter a bundle. **Observed 2026-09-17:** the script compiles and the dialog launches
   with it, and with a default location inside a bundle. Whether a person can then click all the
   way through has not been watched — that needs a human at a desktop — so it is an improvement
   offered, not a fix claimed.
3. Inside any macOS dialog, **`Cmd+Shift+G`** accepts a typed path and ignores the bundle rule.
   That is said on the page next to the Browse button, because a note in this file is worth nothing
   to somebody standing in front of the dialog right now.

Browse is not going anywhere: it is right for maps kept somewhere ordinary, and for a save target,
which is the case where the user genuinely has to name a new place. `choose file name` has no
package-contents parameter and is not given one — saving *into* the game's own bundle is the one
thing this tool should make awkward, because the loose `map/` directory has no backup.

Four things this gets right on purpose:

- **The typed fields are unchanged and are not second-class.** They survive SSH and a headless box,
  they are what most of the tests drive, they are the only route that works on a wrapper install,
  and Browse is an accelerator for them — the chosen path is written into the field, where it can
  still be edited.
- **The dialog starts at the maps directory, including the first time.** `start_in` used to come
  only from the page, and on a freshly loaded page there is no directory yet — so the *first*
  Browse, the one that matters most, opened wherever macOS happened to be. It now falls back to the
  same `--pic`-derived suggestion the field is seeded from, so the two cannot disagree about where
  this install keeps its maps.
- **Cancelling is not an error**, and each platform says so differently. Dismissing the dialog
  answers `"ok": true, "cancelled": true`, changes nothing and logs nothing. `osascript` exits
  non-zero for a cancel as well as a failure, so the two are told apart by AppleScript's error
  **number** `-128` rather than by the text "User canceled", which is localised. zenity, kdialog and
  our PowerShell scripts instead exit non-zero with **nothing on stdout**, and their stderr is not a
  signal at all — GTK and Qt both emit warnings on a perfectly ordinary run, so reading a non-empty
  stderr as failure would report a cancel as an error to anyone on a noisy desktop. That rule errs
  toward reading an ambiguous failure as a cancel, deliberately: a cancel misreported as an error
  puts an alarming refusal in front of someone who did nothing but change their mind, and teaches
  them to ignore the log, which is the one place this editor says things that matter. The reverse
  mistake costs a silent no-op they can simply retry.
- **A dialog that never appears is killed, not waited on.** The request loop is single-threaded, so
  a child blocked on a window that will never be drawn freezes the whole editor; there is a 120
  second bound and the child is killed at it. A missing dialog program is reported the same way.
- **A picked path is trusted exactly as far as a typed one.** It goes through the same guards. In
  particular the native save dialog asks its own "replace?" question and hands back an existing path
  when the user says yes — **and we refuse it anyway**, with a refusal that says why: the map
  directory has no backup, this tool never writes in place, and the OS dialog does not get to
  override that.

The arguments go to `osascript` as `argv`, never interpolated into the script text, and there is no
`sh -c`. The first version put a `-` between the script and its arguments; `osascript` does not
consume it after `-e`, so it arrived as `item 1 of argv` and shifted every string by one. A test
pins the argument list.

The directory listing is **not a filesystem browser**. It lists the map files of the directory it is
given — no subdirectories, no parent, no recursion — and it does not canonicalise the path, because
the obvious workaround for a 180-character install path is a symlink and resolving it would make the
listed names belong to somewhere the user did not type.

**The log panel is the point.** The refusals and notes the CLI prints go there as readable text that
stays on screen — "no tile of terrain 9 accepts the neighbourhood at (4, 2)", "1 of the 25 written
cells were newly painted with several equally valid tiles … a legal choice, not the engine's", "10
written cells have a neighbour off the map". A refusal is information, not an error to hide, so it
comes back as a normal `200` answer with `"ok": false` and the library's own message. Expect refusals
on a shipped world map — **30.6% of every position a 3x3 rectangle fits on `URAK.scn`** is refused,
measured at every one of the 142,884 of them and not sampled, and
[map format](../../docs/map-format.md#painting-a-shipped-world-map-is-refused-about-30-of-the-time)
records the spread across four maps.

Painting is deterministic by default — the lowest matching atlas slot — and the seed box reaches the
same `--seed` the CLI has. Neither is the engine's draw, and the UI says so every time it happens.
The count a save reports is **how many cells of the file still hold a drawn tile**, not how many
draws the session ever made: painting over one puts it back under the tileset's control, and an
honesty mechanism that over-reports is one people learn to ignore.

Undo walks back the last 32 paints, with the draw account moving with the map at every step. Redo is
not offered. Ctrl-scroll or a trackpad pinch over the map zooms **to the cursor**; a plain scroll
pans the pane.

Not in this version: creating a map, sprite placement or removal, elevation, flag editing, redo,
navigating between directories, and opening more than one map at a time. **One process holds one
map**, so a second browser tab does not get a second session — it gets a handle the server then
refuses, which is the loud version of a tab silently painting into a map it is not showing.

The server is tested without a browser. Most tests call the request handler directly —
`Editor::handle` is a pure function of the request *including its `Host` and `Origin`*, with the
socket confined to `run` — and several drive the whole loop over a real loopback socket, which is
what catches a listener bound to the wrong interface, a POST body never read, or a handler panic
taking the session with it. The fixtures are synthetic and the map is **11x5**, because every
shipped world map is square and a square fixture cannot fail on a transposed cell index.

The **client** is tested too, which needs `node`: `tools/map_editor_client_harness.js` loads
`src/ui/app.js` verbatim against a stub DOM, fires the real handlers, and reports what it computed
for `tests/test_map_editor_client.py` to assert — where a click lands at two zooms and on a scrolled
page, that ctrl-scroll zooms about the cursor, that a drag released outside the window does not stay
live, that a refused open cannot leave the page saying "No map open." over a live session, and what
the page does with each of the file dialog's three answers. A missing `node` fails that file rather
than skipping it.

**What no test covers, because it needs a human and a desktop:** that a dialog actually appears,
that it is usable, that `with showing package contents` really lets a person click into a `.app`
bundle, and that a real cancel from a real click produces the `-128` this code reads. **Only the
four AppleScript scripts** were confirmed to compile and reach the dialog, by running each under a
short kill timer, including with a default location inside the user's own bundle. The PowerShell
scripts have never been executed at all — they are asserted as text, against the variable names and
exit code the Rust side actually uses, and nothing more.

Two limits are known and **not** closed, both raised in review:

- `Child::kill` is not a process-tree kill. None of `osascript`, `zenity`, `kdialog` or
  `powershell.exe` launches its dialog as a separate child, but a distribution that shims one into
  a wrapper script would leave the window up after a timeout — and a descendant still holding the
  pipes could make the editor block, which is the one outcome there is no recovering from.
- `kdialog` writes its path with Qt's `toLocal8Bit`, so on a machine whose locale is not UTF-8 a
  non-ASCII path arrives mis-encoded. The PowerShell scripts set their output encoding to sidestep
  exactly this; `kdialog` has no equivalent switch and converting would need a dependency.

**Filed, not built: remembering the last-used maps directory between runs.** It would sidestep the
dialog entirely after one successful List, and it is the obvious next thing. It is not in v1 because
the `--pic` suggestion already covers the standard install with zero typing, and because persistent
state that silently overrides a derived default is a new way for the tool to be confidently wrong
about where the maps are — after a reinstall or a second copy of the game, a stale entry and a fresh
derivation look identical to the user. If it is added it must be a default only, fall back silently
when missing or gone, and never widen where a save may write.

## Writing sprite placement

The engine draws a frame as `top_left = anchor + placement - (width >> 1, height >> 1)`, measured in
the running engine on 2026-09-16. The placement pair is the vector from the anchor to the **centre**
of the frame, in screen pixels with `+y` down, and it is added.

Solve for the value a re-cropped frame needs — here `palm1b.imp` frame 0, 53x53 at `(9, -20)`, padded
by 4 pixels on every side:

```sh
target/release/lom-asset-viewer --imp-placement-for 61 61 320 180 303 134
# placement	13	-16
```

Each axis shifts by half the added pixels, because the convention is centre-relative.

Write it back into a loose IMP:

```sh
target/release/lom-asset-viewer --set-imp-placement in.imp 0 13 -16 out.imp
target/release/lom-asset-viewer --set-imp-placement in.imp 0 5 -40 out.imp --hotspot 0
```

Frame record bytes `+8..+12` are overloaded: a frame carries **either** an origin pair **or** a
pointer to hotspot records, never both. Use `--hotspot TYPE` for the second form; `--describe-imp`
shows which a frame has. The writer keeps the file length identical, re-parses before writing,
refuses a placement it cannot read back, refuses to overwrite an existing output, refuses a duplicate
frame's origin, and warns when several frames share the record or the hotspot array being written.

**Both placement forms obey the same rule.** A frame with a zero hotspot count carries its placement
as the origin pair; a frame with hotspot records carries it in **record 0**, which is engine-reserved
and unreadable from script. Record 0 was confirmed by measurement in the running engine on
2026-09-16. Use `--hotspot 0` for that form. `examples/imp_placement_survey.rs` reports the corpus
split, and `examples/shift_record0.rs` shifts record 0 across every frame of a sprite.

One caveat: the record-0 measurement went through the terrain-sprite draw path, so a unit-specific
constant in the *anchor* is not ruled out. The sign and the centre-relative form are settled.

`--extract`, `--export-imp-frame`, and `--export-map-preview` use create-new semantics and refuse to overwrite an existing output. IMP frame export writes an 8-bit indexed PNG with the source palette indices and RGB palette intact. Map preview export writes an RGBA overview using the original terrain atlas at 8×8 output pixels per map cell. Neither path reimports PNGs into the game format. Add `--listfile "$LISTFILE"` to any command when public names are needed. To inventory all five archives in one pass, use [`scripts/inventory-native-assets.sh`](../../scripts/inventory-native-assets.sh).

In the PBM archive viewer:

- Right, Down, or Space selects the next decodable PBM member.
- Left or Up selects the previous one.
- Escape or closing the window exits.

In the IMP frame viewer:

- Right or Left selects the next or previous visible logical frame within the current facing.
- Down or Up selects the next or previous facing within the current action.
- Page Down or Page Up selects the next or previous action sequence.
- Space toggles facing-scoped animation at the current provisional 100 ms frame interval.
- C facings between clean preview, visible mask, and raw-palette modes.
- Escape or closing the window exits.

When a paired generated `.h` member is available, the window title shows its recovered action name. Facing direction and the provisional frame interval still require confirmation against the original executable.

The IMP title and `--describe-imp` report raw sequence/facing metadata plus candidate origin or hotspot values. In the map viewer, `C` switches among original terrain artwork (when `.til` and atlas paths are supplied), candidate elevation, and stable diagnostic tag colors. The terrain mode proves atlas selection and orientation, but its overview is not yet a recreation of the original renderer's full-size terrain composition.

Member matching is case-insensitive because the archive catalog and Windows game paths do not have reliable case consistency.

## Measured result

Tested against the installed GS5R3 profile on 2026-09-11:

| Check | Result |
| --- | ---: |
| Archive entries | 1,406 |
| Readable entries | 1,406 |
| IFF `FORM PBM` images | 1,377 |
| Successfully decoded PBMs | 1,377 |
| Core archive members classified | 9,804 / 9,804 |
| IMP binaries structurally parsed | 1,800 / 1,800 |
| IMP binaries pixel-expanded without decoder errors | 1,800 / 1,800 |
| IMP pairs matching their generated header exactly | 1,795 / 1,800 |
| Remaining pairs, each a value-pinned exception | 5 |
| Loose `.scn`/`.smp`/`.lgd` grids parsed | 365 / 365 |
| Tile-set definitions parsed | 26 / 26 |
| Placed-object records decoded | 21,117 in 365 files, across six record layouts |
| Maps `--map-place-sprite` / `--map-remove-sprite` accept | 365 / 365 |
| Release-mode archive enumeration | ~0.20 s |
| Release-mode full PBM scan/decode | ~1.0 s |

The viewer displayed `lbm\ACTIONS5.lbm` as a 612×120 image with 256 palette entries and ByteRun1 compression. The full scan found one image whose final compressed packet crosses a scanline boundary; matching the format's scanline semantics resolved it and is covered by a regression test.

The IMP decoder handles both observed frame-record variants, the custom packet RLE, 8/4/2/1-bit indexed pixels, row padding, direct duplicates, `0x04` shared-pixel records inside an ordinary frame-record array, typed placement candidates, and explicit sequence/facing/frame ranges. `--validate-imp` reports **zero failures** over the 1,800 paired members: 1,795 match on every statistic, and the remaining five sit in the value-pinned `IMP_VALIDATION_EXCEPTIONS` table alongside two value-pinned orphan catalog notes. Anything not covered by those exact recorded numbers is still reported as a failure. See the [Stage 1 record](../../docs/native-asset-stage.md) for the exact scope and remaining gates.

## What the tool proves

- StormLib can be isolated behind a small safe-facing Rust API and can read the real game archive without extraction.
- The primary observed picture format is straightforward enough to implement and validate natively.
- SDL3 is adequate for immediate 2D inspection and nearest-neighbor presentation.
- All observed IMP payloads can be bounded and expanded into recognizable indexed sprite frames.
- Asset tooling can deliver value independently of a complete engine rewrite.

## What it does not prove

- The rendering is not yet behaviorally equivalent to the game. A visible bright-green color in the atlas suggests an engine-level chroma-key rule that is not represented by the PBM header's masking field.
- IMP rendering is not yet behaviorally equivalent to the game. The clean preview hides palette indices 0 and 1 as the inferred background and secondary-mask channels; the other viewer modes expose them. Placement is settled — see [hotspots](../../docs/hotspots.md). What still needs reference comparison is the shadow-index blend, the chroma-key rule, and sequence timing and facing direction ([issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2)).
- BMP/WAVE currently have metadata probes. Map/scenario/component grids, standard terrain lookup, and all six placed-object record layouts are decoded — but what the layouts' constant tail words *mean* is Unknown, and fonts and video are not decoded at all.
- The viewer recreates its streaming texture while drawing; caching is a production optimization, not a spike requirement.
- There is no thumbnail grid, search UI, batch/GUI export, or general asset reimport. IMP **placement** write-back exists (`--set-imp-placement`); pixel and frame reimport do not. MPQ member replacement is not a command of *this* tool: it lives in `lom-mpq repack`, with a shape check, in [deterministic MPQ repack](../../docs/repack.md). `examples/mpq_replace.rs` remains only because `scripts/install-engine-probe.sh` uses it to edit an installed archive in place.
- A successful asset decoder does not reduce the much larger uncertainty in the GameScript host, simulation, AI, saves, or multiplayer.

## Code map

- `src/mpq.rs` — manual StormLib FFI and read-only archive/member ownership.
- `src/pbm.rs` — bounded IFF chunk parsing, palette conversion, PBM row handling, and ByteRun1 decoding.
- `src/imp.rs` — bounds-checked IMP tables, palette, RLE and packed-pixel decoding, hotspot and duplicate/repeated-frame structures, and generated-header validation.
- `src/map.rs` — bounded common header/cell-grid parsing, packed `y * width + x` coordinates, terrain tags, the measured terrain-type-to-tile table, and the six placed-object record layouts for SCN/SMP/LGD files.
- `src/tile.rs` — parser for `.til` atlas geometry, terrain types, and the full eight-column neighbour constraints, plus the constraint matcher `--map-paint-terrain` re-tiles from.
- `examples/paint_refusal_survey.rs` — plan a 3x3 paint of every terrain the tileset draws at **every position it legally fits** on one map, and report how often the declared constraints refuse and how many cells per accepted paint were drawn at random. A coarser stride is an optional argument; it used to be the only behaviour, and the disjoint sample it produced was published as a rate over all paints. Plans only: nothing is applied and nothing is written. Takes a map and a `.til`, because neither is committed.
- `examples/operator_bodies.rs` — walk all 1,906 native operator bodies and write the classification table, the global-address clusters and the summary to a directory. Every absolute data reference, every direct call, every PE import, and a path-sensitive operand count that disagrees with the recorded site count for 71 operators. `--function ADDR` dumps one body instead, which is how a callee the table only names gets measured rather than guessed at. Takes `lomse.exe`, because it is not committed; the output is, in `reports/natives/`. See [inside the native operator bodies](../../docs/native-operator-bodies.md).
- `examples/parse_all_tilesets.rs` — parse every `.til` in a directory and report atlas size, terrain-id range and any row that fails to declare all eight constraints. Reading columns the parser used to discard can only *add* failure modes for `--view-map`, so this is the check that it has not: 26 parsed, 0 failed, 0 incomplete on the GS5R3 set. Takes a path, because no tileset is committed.
- `src/gamescript.rs` — bounded GameScript lexer, procedure diagnostics, name inventory, and static `run` references.
- `src/gamescript_vm.rs` — experimental bounded value stack, dictionaries, procedures, core operators, and structured execution failures.
- `src/png_export.rs` — lossless indexed IMP-frame PNG and RGBA map-preview output.
- `src/server.rs` — the `--serve` map editor: routing, one open map with one level of undo, the terrain palette built from the resolved tileset, and the Save As guards. The page, script and stylesheet it embeds are in `src/ui/`.
- `src/paths.rs` — the device-and-inode same-file check both writers use.
- `tools/map_editor_client_harness.js` — load `src/ui/app.js` verbatim against a stub DOM, fire its real handlers, and print what it computed. Asserts nothing itself; `tests/test_map_editor_client.py` holds the expected values.
- `src/asset.rs` — content-first classification and typed format metadata.
- `src/main.rs` — CLI inventory, extraction, validation, and SDL3 viewer.
- `build.rs` — local native-library search and runtime paths for the Apple Silicon spike.

## Sensible next slice

Expand the experimental GameScript VM only far enough to classify and run additional engine-light utilities, then add read-only module loading with structured unknown-name traces. The controlled Map Editor save diff remains parked because macOS accessibility controls prevented reliable automation of Wine's editor window; remaining map variants and original-engine comparisons stay in explicit [GitHub issues](https://github.com/jake-bliss/lords-of-magic-modding/issues) rather than being encoded as assumptions.
