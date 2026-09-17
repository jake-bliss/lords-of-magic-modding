"""Small lexical helpers for Lords of Magic's PostScript-like .gs files."""

from __future__ import annotations


DELIMITERS = frozenset("{}[]()")


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
        if character.isspace():
            flush()
            index += 1
            continue
        if character == ";":
            flush()
            newline = source.find("\n", index)
            index = len(source) if newline == -1 else newline + 1
            continue
        if character in DELIMITERS:
            flush()
            result.append((character, index))
            index += 1
            continue
        if character == '"':
            flush()
            string_start = index
            string_token = ['"']
            index += 1
            while index < len(source):
                character = source[index]
                string_token.append(character)
                index += 1
                if character == "\\" and index < len(source):
                    string_token.append(source[index])
                    index += 1
                elif character == '"':
                    break
            result.append(("".join(string_token), string_start))
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
