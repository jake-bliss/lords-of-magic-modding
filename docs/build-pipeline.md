# The build and validation pipeline

## What this is

Four commands that take a mod source tree from a directory to a running development profile and
back again: `mod-validate`, `mod-build`, `install-dev`, `restore-dev`. They sit on top of
[the repack command](repack.md), which already turns a source archive plus replacement members into
a new archive and refuses to hand it on unless the output kept the source's shape. That document
ends by saying `--install` refuses because installing is Phase 3. This is Phase 3.

Nothing here is a second archive writer. `scripts/repack-archive.sh` is still the only thing in this
repository that writes an MPQ, and `tools/mpq_shape.py` is still the only thing that decides whether
an output is allowed to be installed.

| Piece | Job |
|---|---|
| `mods/<mod-id>/` | the source tree: `mod.toml` plus files under `archives/<archive>/` |
| `tools/mod_tree.py` | maps a source file to an archive member name; refuses shapes it cannot map |
| `lom-asset-viewer --gs-facts` | one JSON Lines record per `.gs` member, from the Rust lexer |
| `tools/mod_validate.py` | the findings engine; pure logic over manifests, facts and the corpus |
| `tools/mod_report.py` | the change report, including what changed *inside* a `.gs` member |
| `tools/mod_build.py` | the build id, the plan, and `build.json` |
| `tools/install_guard.py` | the one-entry path allowlist |
| `scripts/mod-seed.sh` | copy a member out of the base profile into a mod tree |
| `scripts/mod-validate.sh` | validate |
| `scripts/mod-build.sh` | validate, repack, report |
| `scripts/install-dev.sh` | create the development profile; install a build into it |
| `scripts/restore-dev.sh` | roll back, to pristine or to an earlier build |

## The mod source tree

```
mods/orinf-rebalance/
  mod.toml
  archives/gs.mpq/units/orinf.gs        ->  member  units\orinf.gs
  archives/gs.mpq/START.GS              ->  member  START.GS
  archives/pic.mpq/LBM/ACTIONS.lbm      ->  member  LBM\ACTIONS.lbm
```

The path **below** `archives/<archive-name>/` is the member name with `/` turned into `\`. Case is
preserved exactly.

The `archives/<archive-name>/` level is not optional and is not inferred. `START.GS` and
`gs\hotkey.gs` are both real members of `gs.mpq` — one at the archive root, one in a directory — and
a single `gs/` source directory could not express the first without inventing a rule about which
files live at the root.

`mod.toml` declares:

```toml
id = "orinf-rebalance"           # must equal the directory name; [a-z0-9][a-z0-9-]*
name = "Order's Footmen rebalance"
version = "0.1.0"
base_profile = "vanilla"         # vanilla | patch302 | gs5r3
description = "..."              # optional
new_members = []                 # members being ADDED rather than replaced
allow_new_members = false        # and the second act required to permit that
```

A key this pipeline does not understand is a **refusal**, not a warning. A key an author believes is
doing something must not silently do nothing.

**Mod source files are not committed.** `mods/.gitignore` excludes `*/archives/`, because those
files are extracted from an installed game and this repository stores no game content.
`scripts/mod-seed.sh` reconstructs the tree from a local install, byte-for-byte, so that the first
`mod-validate` after a seed reports an unchanged member and no findings. That run is the control: it
establishes that the pipeline agrees with the archive before anything is edited.

### Adding a member is refused by default

Every member name inferred from the tree must already exist in the base archive's manifest,
case-exactly. A file that names no existing member is an error.

Adding a member is a different and riskier operation. **No archive this project has produced with an
added member has ever been put in front of the engine** — every engine-verified write has *replaced*
an existing member. So adding one takes two deliberate acts: naming it in `new_members` and setting
`allow_new_members = true`. Even then it is reported as a warning that says it is Inferred and
untested.

## `validate`

`scripts/mod-validate.sh mods/<mod-id>` opens no installed game file for writing and runs no
StormLib writer. It reads the base profile's archives read-only for their manifests and their
GameScript facts. `mod-build` runs it as a gate: a tree that fails validation is never packed.

Every finding carries a severity, a location (`file:line:column` where one exists), and one line
saying what specifically is wrong. Validate exits nonzero on any error-severity finding.

| Check | Severity | What it catches |
|---|---|---|
| `tree-shape` | error | a file that maps to no member: loose under `archives/`, an unsupported archive, a symlink, Finder litter, a case collision between two source files |
| `missing-member` | error | a member name the base archive does not have |
| `case-mismatch` | error | the tree and the manifest spelling one member differently |
| `ambiguous-member` | error | a name the archive holds more than once; see below |
| `new-member` | error / warning | an added member, refused unless declared **and** allowed |
| `lex` | error | a `.gs` file the lexer rejects, or an unclosed `{`, at `file:line:column` |
| `duplicate-definition` | error | a definition this mod introduces that another member already defines |
| `dropped-definition` | error / note | a definition the edit removed; error if any other member calls it, note otherwise |
| `unresolved-reference` | error | an executable name that neither the post-change corpus defines nor the profile's vocabulary contains |
| `missing-run-target` | error | a `"…" run` target that names no member |
| `missing-asset` | warning | a path-shaped literal that names no member of any base archive |
| `encoding` | error / warning | control bytes no shipped member contains; introduced high bytes; a UTF-8 re-save |
| `line-endings` | warning | the line-ending census differing from the base member's |
| `engine-acceptance` | warning | any member targeting `pic.mpq` |

### The ambiguous-member refusal

PIC5R3 holds **two distinct members under one byte-identical name**, `portrait\AIpotM.lbm`, at
blocks 1108 and 1144, with different content. A source tree has one file per name and therefore
cannot say which block it means. Validate refuses rather than picking; a repack that picked one
would silently pick.

An earlier reading of this as a filesystem case-insensitivity problem is **Refuted**: the two names
are the same bytes, and an MPQ cannot hold two names differing only in case at all. The
case-mismatch check above is a different thing — it is the mod tree versus the manifest.

### Encoding: what the corpus actually says

The rules below are **Observed in a local binary, 2026-09-18**, over **all 4,692 `.gs` members of
the three installed profiles' `gs.mpq`** (vanilla 1,315, 3.02 1,681, GS5R3 1,696; 0 unreadable).

**Line endings.** There is no single style to assert:

| Exclusive style | Members |
|---|---:|
| none at all — one single line, no terminator | **3,050** |
| CRLF only | 1,337 |
| CRLF mixed with bare CR | 193 |
| bare CR only | 49 |
| CRLF mixed with bare LF | 40 |
| bare LF only | 23 |

The summary "GameScript uses bare CR line endings" is **Refuted** as a general rule: bare CR is the
exclusive style of 49 members out of 4,692, and the majority style is *no line ending at all*. The
Phase 4 target `units\orinf.gs` is in that majority — 1,798 bytes, one line, zero CR, zero LF, no
trailing newline.

This is why the check compares the replacement's census against the **base member's** census rather
than against any fixed style. An editor pretty-printing `orinf.gs` into 35 lines produces a file that
a CR-versus-LF rule waves through and that this check reports as `0 line endings -> 34 bare LF`.

**What is refuted is the short summary, not this repository's documentation.**
[`docs/gamescript-format.md`](gamescript-format.md#line-endings-bare-cr-is-a-line-ending-here)
already says these are "overlapping counts and not a partition", already records that 501 GS5R3
members have no terminator, and already separates the 49 bare-CR-*only* members from the 193 that
also contain CRLF. The survey above was run independently, across all three profiles rather than
GS5R3 alone, and **agrees with every one of its six GS5R3 figures**: ≥1 CRLF 890+193+40 = 1,123;
≥1 bare CR 193+49 = 242; ≥1 bare LF 40+23 = 63; no terminator 501; bare-CR-only 49; bare CR also
CRLF 193. It also confirms "3.02 has no bare CR at all" — 302 CRLF members, 1,379 with no
terminator, zero bare CR. Two independently written measurements agreeing on six counts is
confirmation; the table here extends the scope from 1,696 members to 4,692, and the all-profile
exclusive partition is the part that makes "3,050 have no line ending at all" visible.

**Alphabet.** The only control bytes present anywhere in the corpus are TAB, CR and LF — **zero**
occurrences of any other byte below 0x20, and zero of 0x7f, across 4,692 members. 971 members
contain a TAB. Bytes above 0x7e occur in **17 members**, and **not one of those 17 is valid UTF-8**.

So GameScript is a byte format with a single-byte high half. A validator demanding UTF-8 would
reject shipped members. This pipeline treats `.gs` as bytes everywhere; the lexer is used for
*checking* and never for rewriting, and no step of the pipeline re-encodes or reflows a source file.
The checks are: a control byte outside TAB/CR/LF is an **error** (the corpus says it cannot happen);
a high byte the base member does not contain is a **warning**; and a replacement that is valid UTF-8
where the base was not, with the base's high bytes gone, is a **warning** naming the likely cause —
an editor re-saving the file as UTF-8, which turns every high byte into a two-byte sequence the
engine has never been shown to read.

### Why the lexer is the Rust one

Validation lexes through `lom-asset-viewer --gs-facts`, which uses
`spikes/asset-viewer/src/gamescript.rs`: it reports `line` and `column`, which the Python tokenizer
does not. There is no third lexer. The change report calls `tools/gs_syntax.py` as well, on purpose,
and **reports when the two disagree** rather than choosing one silently.

**Five divergences from that lexer were closed in `gs_syntax.py` on 2026-09-18**, all five found
the same way — by reading the two implementations side by side rather than by a failure — and all
five measured against the corpus rather than bounded from above. The instrument is a Python port of
`gamescript.rs`'s rules, validated first: its token digest matches `--gs-facts`'s `token_sha256` on
**4,692 of 4,692** members, so a disagreement it reports is a disagreement in `gs_syntax.py` and
not in the port. That port is a throwaway instrument and is **not committed**, so this number is
not reproducible from the repository; what is committed is `AuthorityParityTest` in
`tests/test_gs_syntax.py`, which measures one fixture per divergence against the real
`lom-asset-viewer` and rebuilds it when `gamescript.rs` is newer. Both sides decode latin1 for the
comparison, which removes the one difference that
is about decoding rather than about token boundaries (the Rust lexer decodes with
`from_utf8_lossy`, so a high byte becomes U+FFFD in its token text). Evidence class: Observed,
2026-09-18, over the three installed profiles; the archives are not in this repository, so this
measurement is the only evidence for these numbers.

| Divergence in `gs_syntax.py` | Members it changed | Where |
|---|---:|---|
| A `;` comment ended at `\n` only, but bare CR is a line ending | **34** | all GS5R3; 25 of them have no LF at all |
| `\` was an escape inside a string, which the authority has no rule for | **5** | vanilla 1, 3.02 2, GS5R3 2 |
| `/` did not end a name, and `is_separator` says it does | **5** | vanilla 1, 3.02 1, GS5R3 3 |
| `str.isspace()` is wider than `is_ascii_whitespace` | **0** | 1 member holds such a byte, inside a string |
| `(` and `)` were delimiters here; the authority has no parenthesis in `is_separator` | **0** | 530 members hold one, always inside a string or a comment |
| **After all five: members where the two tokenizers disagree** | **0 of 4,692** | |

The rows are not disjoint: `Dlg\lib_dlg.gs` trips both the string rule and the `/` rule, in
all three profiles, which is why the totals cannot simply be added.

The worst single case was `gs\dungeons\water\wacave.gs` — 5,347 bytes, 712 tokens, of which
`gs_syntax.py` saw **6**. The most alarming was not in GS5R3 at all: vanilla's `gs\Dlg\lib_dlg.gs`
holds the punctuation table `"@#${}()[]\"`, whose closing quote the escape rule ate, and that
member lexed to 3,247 tokens against its real 2,324. `vanilla` is the profile
`mods/orinf-rebalance` is built against, so the claim that this class of defect could not reach a
first mod was wrong before it was written here.

The last two rows are the ones to read carefully, and they were fixed despite measuring zero
because a disagreement about the grammar is a defect whether or not the shipped corpus exercises
it. `foo(1)` was five tokens here and one to the engine's lexer, so an edit to `foo (1)` would read
as a real change to the authority and as layout-only here — in a mod that does not exist yet, which
is the case this pipeline is for.

The whitespace row is the other one. It is a real divergence in the code —
`str.isspace()` accepts ASCII `\x0b` and `\x1c`-`\x1f` as well as `\x85` and `\xa0`, and the
authority accepts none of them — but its corpus reach is **zero**: exactly one member,
GS5R3's `gs\artifact\_custom\shield_balkoth.gs`, contains such a byte (one `\x85`) and it sits
**inside a string literal**, where no layout rule looks. It was fixed for parity, not because it
was costing anything. An earlier draft of this page said that member "trips it", which confused
containing the byte with lexing differently because of it.

`reports/gs/summary.md` regenerates **byte-identical** after all five fixes. The token hash moved
for the affected members in each comparison and **no member changed status**, so no tracked report
needed regenerating.

**What is still open, and why the disagreement check stays.** One divergence: `<` and `>` end a
name for the authority and do not here, so `x<<y` is three tokens to the engine's lexer and one to
`gs_syntax.py`. **Zero** corpus members reach it — every `<<` and `>>` in the corpus is already
whitespace-separated or inside a string, which is also why
[gamescript-format.md](gamescript-format.md) claimed until 2026-09-18 that both lexers handled
dictionary delimiters and was wrong. Unlike the parentheses it cannot be closed by editing a set: a
single `<` not followed by `<` is a *parse error* to the authority, and `gs_syntax.py` has no error
channel. The same is true of an unterminated string and of an empty literal name. Those
are named here rather than fixed, and they are what the change report's two-lexer check is left
watching for. A run of it that reports no disagreement now proves more than it did — the two agree
on the whole corpus — but it is still a check on two implementations, not on the engine.

## `build`

```sh
scripts/mod-build.sh mods/<mod-id> [--determinism-runs N] [--force]
```

Validates, repacks each archive through `scripts/repack-archive.sh`, and writes

```
artifacts/build/<mod-id>/<build-id>/
  gs.mpq
  gs.mpq.source-manifest.tsv
  gs.mpq.manifest.tsv
  build.json
  change-report.tsv
```

`build.json` records the mod's identity, the source-tree digest, the base archive digests, the tool
digests, the git commit, every output archive's SHA-256, the full change report, and the engine
acceptance caveat for each archive — so a build carries its own caveat rather than relying on a
reader having found this document.

**The build id is a digest of the inputs, not a timestamp.** It is derived from the source-tree
digest, the base archive digests and the tool digests. Building the same tree against the same
archives with the same tools twice lands in the same directory with the same bytes, and re-running
is refused unless `--force` is passed. A build id that moved with the wall clock would make two
identical builds look like two different things, and the install log would record a change that
never happened.

### No step runs against a stale tool

A stale `.build/lom-mpq` once produced 17 confusing Python test failures. `prepare_tools` in
`scripts/lib-mod-pipeline.sh` recompiles `lom-mpq` (which `scripts/build-tools.sh` does
unconditionally) and runs `cargo build` before any run, and puts both binaries' SHA-256 into
`build.json`. The cached base-archive facts are keyed on the **archive's own digest**, so a
different archive is a different cache key: there is no cache to invalidate and no window in which
the pipeline reads facts about bytes it is not packing.

### The change report

Every changed member gets its old and new size and digest. For a `.gs` member the report also says
what changed:

```
  [modified] gs.mpq  units\orinf.gs
      size 1798 -> 1798
      sha  4eadbc3cf9e11c46 -> 0e151fa020dc252e
      tokens 200 -> 200 (+0)
      values changed: hit_points: 13 -> 18
      gameplay symbols defined here: orinf (unit)
```

That is built from three existing pieces rather than a fourth:

- `compare_trees.py`'s unchanged / **reformatted** / modified split — the repo's existing way of
  saying "layout and comments only" — recomputed from the CR-aware token digest, with
  `gs_syntax.py`'s answer printed alongside whenever the two disagree;
- the lexer's `scalar_definitions`: definitions of the exact shape `/name VALUE def` where VALUE is
  one **number or string** token, which is why a unit rebalance reads as `hit_points: 13 -> 18`
  instead of "1,798 bytes differ". `units\orinf.gs` has 35 definitions and **29** are that shape.
  The six that are not are three procedures (`impfile_proc`, `level_procedure`, `orinf`) and three
  whose value is a bare constant — `/code INF def`, `/race HUMAN def`, `/faith ORDER def`. A
  constant is an executable name, not a number, so it is deliberately not summarised here: the same
  rule that would admit `HUMAN` would admit `exch` from `/a exch def` and report a computed
  definition as if it were a value. Those changes are still fully described, by the call-site lines:
  editing `/code INF def` to `/code CAV def` reports `now calls: CAV` and `no longer calls: INF`.

  One of the three used to be summarised, by accident. Before PR #64 the lexer classified a token as
  a number with `parse::<f64>()`, which accepts `INF`, so `/code INF def` was read as a numeric
  definition and the report printed `code: INF`. That was the defect, not the feature; `race` and
  `faith` were never summarised because `HUMAN` and `ORDER` were never parseable. The fix makes the
  three consistent. Its reach across the corpus is **84 of 4,692 `.gs` members** containing a token
  the old rule misread — 24 vanilla, 28 3.02, 32 GS5R3, 158 occurrences, all of them `INF` (150) or
  `NAN` (8). The token *digest* is unaffected either way, because it hashes token text rather than
  token class, so the reformatted-versus-modified split did not move;
- `reports/gameplay/symbols.tsv`, the 1,535-symbol gameplay index, to name the symbol a member
  defines.

A row that finds nothing to say says *that*, rather than being silent.

## `install-dev`

```sh
scripts/install-dev.sh --create-profile [--recreate]   # attended, once
scripts/install-dev.sh MOD_ID BUILD_ID
```

Installs only into `~/Applications/Lords of Magic Development.app`.

**The three installed profiles are never written.** `Steambuild 32 64bit DXVK.app` is the preserved
baseline and has no second copy; the loose `map/` directory inside every profile has no backup at
all.

### The allowlist

A check that the target "is not the baseline" is the weak form. It fails open on every path nobody
thought to name: a sibling profile, a typo, a `..` escape, a symlink resolving into the baseline.

`tools/install_guard.py` is the strong form — an **allowlist of exactly one directory**, through
which every write is routed. An unlisted path is not merely disapproved; there is no code path that
produces a handle to it. The profile name is a module constant, not a parameter, so no caller can
widen it. `assert_writable` refuses, in order: a path containing `..` checked against the literal
path before any resolution; a development-profile root that is itself a symlink (the hole that makes
"compare the resolved paths" insufficient on its own, because resolving both sides would make every
write to a linked-to baseline compare equal); and anything that is not the root or a descendant of
it after resolving symlinks through the part of the path that exists.

`tests/test_install_guard.py` asserts 19 cases against a fabricated `Applications` directory, and
`tests/test_mod_pipeline.py` re-hashes every fabricated profile after every operation.

### Creating the profile

Creating it is an explicit, attended step, separate from installing, because it is the one moment
the pipeline reads the preserved baseline. It refuses if the profile already exists unless
`--recreate` is passed, and refuses while `lomse.exe` is running.

The clone is `cp -c -R`, which asks for APFS `clonefile`. **Measured 2026-09-18: 3.7 GB in 2.7 s
with a zero `df` delta.** `du` reports the full 3.5 GB afterwards and is lying — it counts cloned
blocks in full. The script checks that the baseline and `~/Applications` are on the same volume
*before* copying, because `cp -c` degrades to a real copy across volumes and says nothing about it,
and it measures the `df` delta across the copy and **reports a failure to clone rather than
absorbing it**.

On creation it records, inside the profile:

```
.lom-pipeline/MANIFEST.sha256    the PRISTINE archive hashes
.lom-pipeline/PROFILE.json       what it was cloned from, and when
.lom-pipeline/pristine/*.mpq     a second clone of the pristine archives
.lom-pipeline/INSTALLS.tsv       the install log
```

`MANIFEST.sha256` records the hashes **the baseline held**, verified against the clone before being
written. That distinction is the one `scripts/lib-game-archives.sh` sets out: checking a restored
archive against the file it was just copied from is a tautology that would happily certify a
pristine copy which had itself been overwritten.

### Installing

Every archive named in `build.json` is preflighted — its bytes re-hashed against the digest
`build.json` records, its target approved by the allowlist — **before any of them is written**, so a
corrupt second archive cannot leave the profile half-installed. After the copy, each installed
archive's hash is compared against the build's recorded hash and appended to `INSTALLS.tsv`.

## `restore-dev`

```sh
scripts/restore-dev.sh                       # back to pristine
scripts/restore-dev.sh --to MOD_ID BUILD_ID  # back to a specific earlier build
```

Restores from `.lom-pipeline/pristine/` or from the build directory, verifying the source against an
independent record first — `MANIFEST.sha256` or that build's `build.json` — and verifying the result
against the same record afterwards, never against the file it copied from. It refuses while
`lomse.exe` is running, preflights every source before writing any of them, and ends by printing the
hashes it produced, as every script in this repository does.

Because the pristine archives live inside the development profile, **rollback never opens the
baseline at all**. The baseline is read exactly once, when the profile is created.

## What this does not guarantee

This is the part of the document worth reading twice.

- **It does not prove the game runs anything it builds.** The only engine acceptance evidence for a
  rewritten archive remains the attended 2026-09-16 round trip of an `MPQ_FILE_IMPLODE` member of
  `gs.mpq`. The Phase 4 target `units\orinf.gs` has flags `0x80010100` — EXISTS | ENCRYPTED |
  IMPLODE — so it is in **that same class**, and that is the strongest thing that can be said. It is
  not a statement about `gs.mpq` in general or about other flag combinations. **Corrected
  2026-09-18:** this used to add "and about `pic.mpq`, for which a rewritten archive has never faced
  the engine and the compression choice is Inferred". Both halves are now stale. A rewritten
  `pic.mpq` **was** read by the engine and the change read off the screen on 2026-09-18
  ([roadmap](roadmap.md#the-picmpq-slice-observed-in-gameplay-2026-09-18)), and the compression
  choice was never Inferred: every one of the 1,071 baseline `pic.mpq` members carries flags
  `0x80010100`, the same storage class Phase 4 proved. Validate still warns on `pic.mpq` members,
  which is now a caution about one attended member rather than about an untried archive.
- **A token classified as a number is never reference-checked.** Reference resolution walks
  `executable_names`, so anything the lexer files as a number, a string or a literal name is outside
  it by construction. This is not hypothetical: before PR #64 the lexer read `INF` as a float, so
  `units\orinf.gs`'s `/code INF def` was invisible to this check, and the fix enlarged the checked
  surface by 158 occurrences across 84 members without anything in this pipeline changing. The
  surface is defined by the lexer's classification, and a future correction to it moves the surface
  again.
- **Validation is a vocabulary check, not a link check.** A reference "resolves" if the post-change
  corpus defines it or if the base profile's published executable vocabulary contains it. That is
  *presence in the corpus*, not *reachability from the calling member*: it cannot tell a name the
  interpreter would find from one defined in a dictionary that is never open. Every run prints the
  count of names accepted on that basis alone.
- **Every bounded negative here is printed with its blind spot.** A `validate` run ends with a "what
  this run could not check" block: base members that do not lex and are therefore missing from the
  definition map, references resolved by vocabulary presence only, string literals never examined as
  paths (a path assembled at run time from fragments is in that count and is invisible), path-shaped
  literals actually checked, and members with no content validation at all. "No unresolved
  references" means nothing without those numbers next to it.
- **Non-`.gs` members get no content validation whatsoever.** Their bytes are packed as given. There
  is no image, sprite, palette, dimension or map validation in this pipeline. A corrupt `.lbm` passes
  every check here.
- **The shape check cannot identify an unnamed member.** **Observed 2026-09-18**: StormLib
  *renumbers* unnamed blocks when it rewrites vanilla `gs.mpq`, and an unnamed member's only name is
  the `File%08u.xxx` pseudo-name synthesised from its block index. Addressing them that way refused
  a repack that was correct: source and output each held **1,688 entries**, each held **372 unnamed**
  entries, all **1,316 named** members were byte-identical, and exactly **one** member differed —
  `units\orinf.gs`, the one declared. The shape check's verdict on that archive was **4 members
  added, 4 missing and 26 undeclared content changes**, every one of them a block that had moved by
  two positions and kept its content. `tools/mpq_shape.py` now compares unnamed members as
  a **multiset** of content identities. That is strictly weaker: it can still see one lost, added or
  altered in content, size, flags or locale, but it cannot say *which*, and it cannot see two with
  identical content swap places — though nothing could, since they are indistinguishable by every
  property the manifest records. Named members keep the per-block treatment the PIC5R3 case needs.
  This affects vanilla `gs.mpq` (372 unnamed), 3.02 `gs.mpq`, and PIC5R3 `pic.mpq` (409).
  **The weakening's scope is a function of how many members are unnamed, and nothing else.** A
  member whose name is known is compared by name, per block, at full strength. So if the unnamed
  population shrinks — by recovering names and supplying them through a listfile, for instance —
  this check gets stronger for free, with no change to the code and no decision to revisit. It is
  written this way on purpose: do not treat the multiset comparison as the permanent design, and do
  not remove it either. It is the correct comparison for exactly those members that have no name.
- **Determinism is measured, not proven.** `--determinism-runs N` repacks N more times and compares.
  Three runs of the `orinf` build were byte-identical (**Observed 2026-09-18**,
  `a69732513e54d3c8…`), which is a measurement of this StormLib build on this machine for these
  inputs and not a promise from StormLib's contract.
- **Adding a member has never been tested against the engine.** The pipeline refuses it by default
  and warns when permitted. That warning is the whole of the evidence.
- **Compaction is off and stays off.** `SFileCompactArchive` fails with `ERROR_UNKNOWN_FILE_NAMES`
  on any archive holding unnamed members, which includes vanilla `gs.mpq` and PIC5R3 `pic.mpq`.
- **The development profile has never been created.** As of this writing the command exists and its
  refusals are tested against fabricated directories; no `Lords of Magic Development.app` exists on
  any machine. The first real creation is an attended step.
- **The case-collision test in `tests/test_mod_tree.py` skips on a case-insensitive filesystem**,
  which is the macOS default, so on this machine that one refusal is asserted by code inspection
  rather than by a passing test. The skip is reported rather than silently passing.
- **`tests/test_mod_pipeline.py` uses fake archives.** The archives are a few bytes of nonsense,
  because nothing it tests opens one — creating a profile, approving a path, hashing a file and
  verifying a restore are archive-agnostic. It proves the safety machinery, not that a real install
  produces a playable profile.

## Running it

```sh
scripts/mod-seed.sh    mods/orinf-rebalance 'gs.mpq:units\orinf.gs'
scripts/mod-validate.sh mods/orinf-rebalance          # control: expect no findings
# ... edit mods/orinf-rebalance/archives/gs.mpq/units/orinf.gs ...
scripts/mod-validate.sh mods/orinf-rebalance
scripts/mod-build.sh    mods/orinf-rebalance --determinism-runs 2
scripts/install-dev.sh  --create-profile               # once, attended
scripts/install-dev.sh  orinf-rebalance <build-id>
# ... play ...
scripts/restore-dev.sh
```

## Tests

```sh
cargo clippy --all-targets      # cargo test does not build examples
cargo test
python3 -m unittest discover -s tests
```

`tests/test_mod_tree.py`, `tests/test_mod_validate.py` and `tests/test_mod_report.py` run entirely
against fixtures, with no game installed and no StormLib. `tests/test_install_guard.py` and
`tests/test_mod_pipeline.py` build an `Applications` directory in a temporary folder and run the
real scripts against it; the latter re-hashes every fabricated profile after every operation.
`tests/test_mpq_shape.py` covers the unnamed-member multiset comparison.

Every constant introduced here was mutation-tested in **both** directions — disabled and
always-firing — and the sweep is recorded in the commit. A test that cannot fail is worse than no
test; seven have been deleted from this repository for that.
