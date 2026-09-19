"""Small lexical helpers for Lords of Magic's PostScript-like .gs files.

`spikes/asset-viewer/src/gamescript.rs` is the authority on this grammar -- it is the lexer the
build pipeline validates with -- and this module is the second implementation of it. Rules here are
taken from that file rather than from PostScript or from Python habit; four divergences closed on
2026-09-18 were all of the second kind. `docs/gamescript-format.md` lists what is still open
between the two, and `tests/test_gs_syntax.py` asserts agreement with the real thing.
"""

from __future__ import annotations


DELIMITERS = frozenset("{}[]()")

# Every line ending GameScript uses. `gs.mpq` in a GS5R3 install holds CRLF, bare CR and bare LF
# members, so a rule written around `\n` alone does not see the end of a line in a bare-CR member.
LINE_ENDINGS = frozenset("\r\n")

# The authority's layout rule is `u8::is_ascii_whitespace`, which is exactly these five bytes.
# `str.isspace()` is a wider set in two directions and neither is only about non-ASCII: it also
# accepts ASCII `\x0b` and `\x1c`-`\x1f`, and, on a latin1-decoded member, `\x85` and `\xa0`. A
# name containing one of those is one token to the engine's lexer and two to a `str.isspace()`
# rule. `character.isascii() and character.isspace()` does NOT close this -- it still admits
# `\x0b` and `\x1c`-`\x1f` -- so the set is written out.
ASCII_WHITESPACE = frozenset(" \t\n\r\x0c")


def tokens(source: str) -> list[str]:
    """Return tokens while ignoring layout and semicolon-to-EOL comments."""
    return [token for token, _ in tokens_with_offsets(source)]


def tokens_with_offsets(source: str) -> list[tuple[str, int]]:
    """Every token with the source offset it starts at.

    Call sites in this corpus live on lines thousands of characters long, so citing one by line
    alone is not enough to find it again; and re-deriving a position by searching for the token
    text lands on the wrong occurrence whenever a name repeats. The offset is carried through
    instead.
    """
    result: list[tuple[str, int]] = []
    current: list[str] = []
    current_start = 0
    index = 0

    def flush() -> None:
        nonlocal current_start
        if current:
            result.append(("".join(current), current_start))
            current.clear()

    while index < len(source):
        character = source[index]
        if character in ASCII_WHITESPACE:
            flush()
            index += 1
            continue
        if character == ";":
            flush()
            # A `;` comment ends at the first line ending, and in GameScript a bare CR is one:
            # GS5R3 ships members with no LF anywhere. Ending at `\n` alone swallowed the rest of
            # such a member as comment text -- `gs\dungeons\water\wacave.gs` normalised to 6 tokens
            # against its real 712. The terminator is left unconsumed and handled by the whitespace
            # branch above, which is exactly what the authoritative CR-aware Rust lexer does
            # (`skip_layout` in `spikes/asset-viewer/src/gamescript.rs`, which stops the comment at
            # `\r | \n` and lets its whitespace loop advance past it, so CRLF costs no special case).
            index += 1
            while index < len(source) and source[index] not in LINE_ENDINGS:
                index += 1
            continue
        if character in DELIMITERS:
            flush()
            result.append((character, index))
            index += 1
            continue
        if character == '"':
            flush()
            # A string ends at the first `"`, with no escape rule at all. This used to treat `\`
            # as an escape, which is the same swallow-the-file defect the `;` comment rule had:
            # GameScript paths are backslash-separated, so a string ending in one -- vanilla's
            # `gs\Dlg\lib_dlg.gs` holds the punctuation table `"@#${}()[]\"` -- consumed its own
            # closing quote and ran on to the next quote in the file. It cost that member 923
            # tokens. The authority is `read_string` in `spikes/asset-viewer/src/gamescript.rs`,
            # which matches on `"` and takes every other byte verbatim; it has no escape branch,
            # and its own fixture `"path\to\file"` keeps both backslashes.
            string_start = index
            string_token = ['"']
            index += 1
            while index < len(source):
                character = source[index]
                string_token.append(character)
                index += 1
                if character == '"':
                    break
            result.append(("".join(string_token), string_start))
            continue
        if character == "/":
            # `/` both ends a name and starts one. `is_separator` in the authority lists it, so
            # `w/name` is two tokens to the engine's lexer -- `w` and the literal name `/name` --
            # and was one to this tokenizer. GS5R3's `gs\spells\AIR\chain_lightning2.gs` writes
            # exactly that, and the punctuation tables in `lib_dlg.gs` do too. A bare trailing `/`
            # is an error to the authority ("empty or unsupported name"); this tokenizer has no
            # error channel, so it keeps the `/` as a token rather than inventing one.
            flush()
            current_start = index
            current.append(character)
            index += 1
            continue
        if not current:
            current_start = index
        current.append(character)
        index += 1

    flush()
    return result


def normalized_bytes(source: bytes) -> bytes:
    # Latin-1 is a lossless byte-to-code-point mapping. Some legacy scripts
    # contain bytes that are undefined in Windows-1252.
    decoded = source.decode("latin1")
    return "\0".join(tokens(decoded)).encode("utf-8")
