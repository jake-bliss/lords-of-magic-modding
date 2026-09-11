"""Small lexical helpers for Lords of Magic's PostScript-like .gs files."""

from __future__ import annotations


DELIMITERS = frozenset("{}[]()")


def tokens(source: str) -> list[str]:
    """Return tokens while ignoring layout and semicolon-to-EOL comments."""
    result: list[str] = []
    current: list[str] = []
    index = 0

    def flush() -> None:
        if current:
            result.append("".join(current))
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
            result.append(character)
            index += 1
            continue
        if character == '"':
            flush()
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
            result.append("".join(string_token))
            continue
        current.append(character)
        index += 1

    flush()
    return result


def normalized_bytes(source: bytes) -> bytes:
    # Latin-1 is a lossless byte-to-code-point mapping. Some legacy scripts
    # contain bytes that are undefined in Windows-1252.
    decoded = source.decode("latin1")
    return "\0".join(tokens(decoded)).encode("utf-8")
