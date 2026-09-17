#!/usr/bin/env python3
"""Extract every (combat map, tileset) binding the shipped gamescript declares.

Each encounter script defines ``mapfile`` and ``tileset`` as adjacent keys of its own dictionary,
so a combat map's tileset belongs to the *encounter* that loads it rather than to the map::

    /mapfile"map/aicave.smp"def   /tileset"til/aibldg01.til"def

This is the tool that produces ``COMBAT_TILESET_BINDINGS`` in
``spikes/asset-viewer/src/tile.rs``. It is committed because the table is derived data and a
derived table nobody can regenerate is a table nobody can check -- an earlier version of this
research rested on an uncommitted script and reached a conclusion that was wrong for the majority
of the corpus.

**No gamescript is committed**; the extracted ``gs.mpq`` directory is an argument. Extract it with::

    lom-asset-viewer --list gs.mpq --listfile LISTFILE
    lom-asset-viewer --extract gs.mpq 'MEMBER' OUT --listfile LISTFILE

Then::

    python3 tools/extract_map_tilesets.py /path/to/extracted/gs --rust
    python3 tools/extract_map_tilesets.py /path/to/extracted/gs --map-dir /path/to/English/map

Two things this deliberately does **not** do. It does not fall back to the ``combattileset``
default for a map it cannot bind -- that operator is the default for an encounter naming *no* map,
per a corpus comment in ``wilderness_land.gs``, and using it as a per-map answer is the error this
tool exists to correct. And it does not guess from filenames: ``{faith}bldg01.til`` matches the
script pairing for only 41 of 263 faith-prefixed maps.
"""

from __future__ import annotations

import argparse
import collections
import os
import re
import sys

MAPFILE_KEY = "mapfile"
TILESET_KEY = "tileset"
# The plural selector form: a `/mapfiles`/`/tilesets` procedure that indexes a separately-named
# array. See `array_bindings_in_member`.
MAPS_ARRAY_KEY = "maps"
TILES_ARRAY_KEY = "tiles"


def matching_brace(text: str, start: int) -> int:
    """Index just past the ``}`` matching the ``{`` at ``start``.

    Quoted strings are skipped, because a brace inside a string literal does not nest. Returns
    ``len(text)`` for an unterminated procedure rather than raising, so one malformed member cannot
    abort an extraction over 1,700 files.
    """
    if start >= len(text) or text[start] != "{":
        raise ValueError("matching_brace must start on an opening brace")
    depth = 0
    index = start
    while index < len(text):
        character = text[index]
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth == 0:
                return index + 1
        elif character == '"':
            index += 1
            while index < len(text) and text[index] != '"':
                index += 1
        index += 1
    return len(text)


def strip_comments(text: str) -> str:
    """Blank out every ``;`` comment, preserving line structure.

    **Gamescript members use bare ``CR`` line endings**, so ``;`` really does comment to end of
    line even though a whole procedure looks like one line to tools that only split on ``LF``. A
    ``;`` inside a quoted string is not a comment.

    This is load-bearing and was missing. Without it the extractor reads bindings out of
    **commented-out code**: `wilderness_land.gs` and `wilderness_sea.gs` have their
    `chcave.smp`/`cavewatr.til` pair commented out -- which is the whole point of those members,
    since they are the *outside combat encounter* files -- and `earthmult.gs` yields a different
    map and a different tileset depending on whether comments are honoured. Seven members change.
    """
    pieces = re.split(r"(\r\n|\r|\n)", text)
    out: list[str] = []
    for piece in pieces:
        if piece in ("\r", "\n", "\r\n"):
            out.append(piece)
            continue
        quoted = False
        cut = len(piece)
        for index, character in enumerate(piece):
            if character == '"':
                quoted = not quoted
            elif character == ";" and not quoted:
                cut = index
                break
        out.append(piece[:cut])
    return "".join(out)


def basename(path: str) -> str:
    """The lowercase basename of a gamescript path, which uses backslashes or slashes."""
    return os.path.basename(path.replace("\\", "/")).lower()


def definitions(text: str, key: str) -> list[tuple[int, list[str]]]:
    """Every ``/key`` definition as ``(position, [quoted values])``.

    A definition's value may be a **string literal** or a **procedure**. Both forms occur and both
    matter: ``aimult.gs`` writes ``/tileset{dungeon_id getdungeonstrength 3 le{...}...}`` whose
    branches all yield the same tileset, and ``genchaos.gs`` writes a ``terrainsprites``-keyed
    selector yielding two. Reading only string literals loses those.
    """
    found: list[tuple[int, list[str]]] = []
    for match in re.finditer(r"/" + re.escape(key) + r"\s*", text, re.I):
        cursor = match.end()
        if cursor >= len(text):
            continue
        if text[cursor] == '"':
            closing = text.find('"', cursor + 1)
            if closing == -1:
                continue
            found.append((match.start(), [basename(text[cursor + 1 : closing])]))
        elif text[cursor] == "{":
            body = text[cursor : matching_brace(text, cursor)]
            found.append(
                (match.start(), [basename(value) for value in re.findall(r'"([^"]+)"', body)])
            )
    return found


def array_definitions(text: str, key: str) -> list[list[str]]:
    """Every ``/key[ "a" "b" ]`` array as a list of lowercase basenames."""
    found: list[list[str]] = []
    for match in re.finditer(r"/" + re.escape(key) + r"\s*\[", text, re.I):
        opening = text.index("[", match.start())
        closing = text.find("]", opening)
        if closing == -1:
            continue
        found.append(
            [basename(value) for value in re.findall(r'"([^"]+)"', text[opening:closing])]
        )
    return found


def array_bindings_in_member(text: str) -> list[tuple[str, str]]:
    """Bindings from the **plural selector** form, read coarsely.

    A member may define a `/tilesets` procedure that picks among a separately-named `/tiles[...]`
    array at runtime, alongside a `/mapfiles` procedure over `/maps[...]`::

        /tilesets{ ... tiles 0 get ... currentterrainsprite getterrainspritelocation
                   8 mod 2 eq{pop tiles 2 get}if ... }/dummy currentdict replace
        /tiles["til/cavewatr.til" "til/cavecrys.til" "til/cavelava.til" "til/aibldg01.til"]replace bind def

    So **one** encounter selects among up to four tilesets from a sprite's runtime map location.
    That is a second, independent reason a combat map has no single tileset.

    **This reading is deliberately coarse: every tileset in `/tiles[...]` becomes a candidate for
    every map in `/maps[...]`.** The two procedures evaluate *different* predicates -- in
    `waming.gs` the `/mapfiles` body branches on `4 mod 0`, `4 mod 2` and `8 mod 7` while
    `/tilesets` branches on `8 mod 2`, `8 mod 4`, `8 mod 6` and `8 mod 7`, with different branches
    commented out in each -- so the arrays **cannot be zipped positionally**. A positional zip
    would produce a table that looks authoritative and is wrong.

    Evaluating the predicates properly would need `getterrainspritelocation`, a runtime value. An
    over-wide candidate set that says it is over-wide is the honest option; a narrow wrong one is
    not. It also errs in the safe direction for the paint gate, where refusing a *legitimate*
    tileset is the failure that shipped once already.
    """
    maps = [entry for array in array_definitions(text, MAPS_ARRAY_KEY) for entry in array]
    tiles = [entry for array in array_definitions(text, TILES_ARRAY_KEY) for entry in array]
    maps = [entry for entry in maps if entry.endswith(".smp")]
    tiles = [entry for entry in tiles if entry.endswith(".til")]
    return [(map_name, tileset) for map_name in maps for tileset in tiles]


def bindings_in_member(text: str) -> list[tuple[str, str]]:
    """Every ``(map, tileset)`` pair one gamescript member declares.

    Each ``/mapfile`` is paired with the ``/tileset`` definition that **follows** it in the same
    member, falling back to the nearest preceding one when nothing follows -- which is how these
    dictionaries are written, in either key order. A member with no tileset contributes nothing:
    that is the *outside combat encounter* case and it has no map tileset to record.

    On the shipped corpus this agrees exactly with a nearest-by-absolute-distance reading -- the
    table is byte-identical either way -- but that reading is wrong in principle and a test with
    two encounters in one member catches it.
    """
    map_definitions = [
        (position, [value for value in values if value.endswith(".smp")])
        for position, values in definitions(text, MAPFILE_KEY)
    ]
    tileset_definitions = [
        (position, [value for value in values if value.endswith(".til")])
        for position, values in definitions(text, TILESET_KEY)
    ]
    map_definitions = [(p, v) for p, v in map_definitions if v]
    tileset_definitions = [(p, v) for p, v in tileset_definitions if v]
    if not map_definitions or not tileset_definitions:
        return []

    pairs: list[tuple[str, str]] = []
    for position, maps in map_definitions:
        # The corpus convention is `/mapfile"..."def /tileset"..."def`, so the **following**
        # tileset is the one that belongs to this map. Nearest-by-absolute-distance is wrong: with
        # two encounters in one member, the preceding encounter's tileset can be a character
        # closer than this encounter's own, and the result looks entirely plausible because both
        # are real shipped names. Only if nothing follows does the nearest preceding one apply,
        # which covers the members that write the keys in the other order.
        following = [entry for entry in tileset_definitions if entry[0] > position]
        if following:
            _, tilesets = min(following, key=lambda entry: entry[0] - position)
        else:
            _, tilesets = min(tileset_definitions, key=lambda entry: position - entry[0])
        for map_name in maps:
            for tileset in tilesets:
                pairs.append((map_name, tileset))
    return pairs


def extract(gs_dir: str) -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    """Every binding in a directory of extracted gamescript members.

    Returns two tables, kept **separate on purpose**:

    1. ``declared`` -- from ``/mapfile"..."`` beside ``/tileset"..."``. One map, one tileset, named
       together. This is what the project reports as *the* tileset of a map.
    2. ``coarse`` -- the extra tilesets the plural selector form puts in reach at runtime, minus
       anything already declared. Over-wide by construction; see
       :func:`array_bindings_in_member`.

    They are not merged because merging costs precision and buys nothing measurable: every one of
    the 43 maps named in a ``/maps[...]`` array is **already** declared by a literal pair, so the
    coarse reading adds **no** coverage, while widening 42 of those 43 across more than one tileset
    *rule class* -- it would put `ruins01.til` (81.3% on `demina.smp`) beside the declared
    `debldg01.til` (98.3%) as if they were equals.

    **Comments are stripped first**, so nothing is read out of disabled code.
    """
    declared: dict[str, set[str]] = collections.defaultdict(set)
    coarse: dict[str, set[str]] = collections.defaultdict(set)
    for name in sorted(os.listdir(gs_dir)):
        path = os.path.join(gs_dir, name)
        if not os.path.isfile(path):
            continue
        with open(path, encoding="latin-1") as handle:
            text = strip_comments(handle.read())
        for map_name, tileset in bindings_in_member(text):
            declared[map_name].add(tileset)
        for map_name, tileset in array_bindings_in_member(text):
            coarse[map_name].add(tileset)
    # An array candidate that is already declared is not additional information.
    for map_name in list(coarse):
        coarse[map_name] -= declared.get(map_name, set())
        if not coarse[map_name]:
            del coarse[map_name]
    return (
        {name: sorted(tilesets) for name, tilesets in sorted(declared.items())},
        {name: sorted(tilesets) for name, tilesets in sorted(coarse.items())},
    )


def emit_rust(
    table: dict[str, list[str]], coarse: dict[str, list[str]] | None = None
) -> str:
    def emit(name: str, entries: dict[str, list[str]]) -> list[str]:
        lines = ["pub static %s: &[(&str, &[&str])] = &[" % name]
        for map_name, tilesets in sorted(entries.items()):
            joined = ", ".join('"%s"' % tileset for tileset in tilesets)
            lines.append('    ("%s", &[%s]),' % (map_name, joined))
        lines.append("];")
        return lines

    lines = emit("COMBAT_TILESET_BINDINGS", table)
    lines.append("")
    lines.extend(emit("COMBAT_TILESET_ARRAY_CANDIDATES", coarse or {}))
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("gs_dir", help="directory of extracted gs.mpq members")
    parser.add_argument("--rust", action="store_true", help="emit the Rust table")
    parser.add_argument("--map-dir", help="installed map/ directory, to report coverage")
    arguments = parser.parse_args(argv)

    if not os.path.isdir(arguments.gs_dir):
        print("not a directory: %s" % arguments.gs_dir, file=sys.stderr)
        return 2
    table, coarse = extract(arguments.gs_dir)
    if not table:
        # An empty extraction is a failed extraction, not an empty corpus. Exiting 0 here is how a
        # regeneration could silently replace the table with nothing.
        print(
            "no bindings found in %s; an empty result is a failed extraction"
            % arguments.gs_dir,
            file=sys.stderr,
        )
        return 1

    if arguments.rust:
        sys.stdout.write(emit_rust(table, coarse))
        return 0

    single = [name for name, tilesets in table.items() if len(tilesets) == 1]
    multiple = [name for name, tilesets in table.items() if len(tilesets) > 1]
    print(
        "declared-maps\t%d\tsingle-valued\t%d\tmulti-valued\t%d"
        % (len(table), len(single), len(multiple))
    )
    print("maps-with-extra-array-candidates\t%d" % len(coarse))
    for name in multiple:
        print("  ambiguous\t%s\t%s" % (name, " ".join(table[name])))
    for name in sorted(coarse):
        print("  array-only\t%s\t%s" % (name, " ".join(coarse[name])))
    if arguments.map_dir:
        installed = {
            entry.lower()
            for entry in os.listdir(arguments.map_dir)
            if entry.lower().endswith(".smp")
        }
        bound = installed & set(table)
        print("installed\t%d\tbound\t%d\tunresolved\t%d" % (len(installed), len(bound), len(installed - set(table))))
        for name in sorted(set(table) - installed):
            print("  bound-but-not-installed\t%s" % name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
