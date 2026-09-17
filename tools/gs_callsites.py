"""Recover an operator's operand order from shipped call sites, rather than from the arity table.

The recovered arity table gives stack effect and nothing else. It cannot say what the operands
*are*, it undercounts operators that pop through the shared helper at `0x0040ADB0`, and designing
an operand order from it has already cost one attended engine run and one destroyed map object.
The shipped corpus is the authority. This module makes consulting it a command rather than a habit.

It also handles the trap that made `addterrainsprite` look variadic. These two call sites:

    s_x s_y terrainsprites /tower3 get addterrainsprite
    cx cy f terrainsprites begin keep_ttype end addterrainsprite

appear to pass three operands and four. They do not. `keep_ttype` is a *procedure* in the
`terrainsprites` dictionary that consumes the faith and returns one type id, so both sites pass
three. Counting tokens gets this wrong; resolving the names gets it right. Every name in a call
site's operand window is therefore classified against the corpus's own definitions, and any name
that resolves to a procedure is flagged as consuming or producing an unknown number of operands.

The corpus is proprietary and is never stored in this repository. Point this at a directory of
extracted `.gs` members:

    .build/lom-mpq extract gs.mpq /tmp/gsx
    python3 tools/gs_callsites.py /tmp/gsx addterrainsprite
"""

from __future__ import annotations

import argparse
import collections
from pathlib import Path

from gs_syntax import tokens_with_offsets

# How many tokens before the operator to show. Six covers every operand order observed so far and
# still fits a terminal line.
DEFAULT_WINDOW = 6

# Tokens that end an operand window early: nothing before them can be an operand of this call.
BOUNDARIES = frozenset({"{", "}", "def", "if", "ifelse", "for", "forall", "repeat"})


class Definitions:
    """Every `/name` definition in the corpus, split by whether it names a procedure or a value.

    A name bound to a procedure may consume operands; a name bound to a literal is exactly one
    operand. That distinction is the whole reason this module exists, so it is derived from the
    corpus rather than assumed.
    """

    def __init__(self) -> None:
        self.procedures: set[str] = set()
        self.values: set[str] = set()

    def add_file(self, file_tokens: list[str]) -> None:
        for index, token in enumerate(file_tokens):
            if len(token) < 2 or not token.startswith("/"):
                continue
            name = token[1:]
            following = file_tokens[index + 1] if index + 1 < len(file_tokens) else ""
            if following == "{":
                self.procedures.add(name)
            else:
                self.values.add(name)

    def classify(self, token: str) -> str:
        """One of `number`, `string`, `procedure`, `value`, `literal-name`, or `unknown`."""
        if token.startswith('"'):
            return "string"
        if token.startswith("/"):
            return "literal-name"
        try:
            float(token)
        except ValueError:
            pass
        else:
            return "number"
        if token in self.procedures:
            return "procedure"
        if token in self.values:
            return "value"
        return "unknown"


class CallSite:
    def __init__(self, path: Path, line: int, column: int, window: list[str]) -> None:
        self.path = path
        self.line = line
        self.column = column
        self.window = window

    @property
    def pattern(self) -> str:
        return " ".join(self.window)

    @property
    def where(self) -> str:
        """Line and column. Lines here run to thousands of characters; the column is not optional."""
        return f"{self.path.name}:{self.line}:{self.column}"


def scan(corpus: Path, operator: str, window: int = DEFAULT_WINDOW) -> tuple[list[CallSite], Definitions]:
    """Return every call site of `operator` in the corpus, plus the corpus's definitions."""
    definitions = Definitions()
    parsed: list[tuple[Path, str, list[tuple[str, int]]]] = []

    for path in sorted(corpus.rglob("*")):
        if not path.is_file() or path.suffix.lower() not in {".gs", ".txt"}:
            continue
        try:
            source = path.read_text(errors="replace")
        except OSError:
            continue
        located = tokens_with_offsets(source)
        definitions.add_file([token for token, _ in located])
        parsed.append((path, source, located))

    sites: list[CallSite] = []
    for path, source, located in parsed:
        names = [token for token, _ in located]
        for index, token in enumerate(names):
            if token != operator:
                continue
            # A `/name` immediately before is a definition of the operator's own name, not a call.
            if index and names[index - 1] == f"/{operator}":
                continue
            start = max(0, index - window)
            chunk = names[start:index]
            for offset in range(len(chunk) - 1, -1, -1):
                if chunk[offset] in BOUNDARIES:
                    chunk = chunk[offset + 1:]
                    break
            byte_offset = located[index][1]
            line = source.count("\n", 0, byte_offset) + 1
            column = byte_offset - (source.rfind("\n", 0, byte_offset) + 1) + 1
            sites.append(CallSite(path, line, column, chunk))
    return sites, definitions


def report(sites: list[CallSite], definitions: Definitions, operator: str) -> str:
    if not sites:
        return f"{operator}: no call sites found\n"

    lines = [f"{operator}: {len(sites)} call sites\n"]
    grouped: dict[str, list[CallSite]] = collections.defaultdict(list)
    for site in sites:
        grouped[site.pattern].append(site)

    lines.append("pattern (operands nearest the operator are last)")
    for pattern, members in sorted(grouped.items(), key=lambda kv: -len(kv[1])):
        example = members[0]
        lines.append(f"  {len(members):4d}  {pattern}")
        lines.append(f"        e.g. {example.where}")

    suspicious = {
        token
        for site in sites
        for token in site.window
        if definitions.classify(token) == "procedure"
    }
    if suspicious:
        lines.append("")
        lines.append(
            "These names in the operand windows are PROCEDURES, so the token count is not the"
        )
        lines.append(
            "operand count -- each may consume or produce operands. Read their definitions before"
        )
        lines.append("concluding anything about arity:")
        for name in sorted(suspicious):
            lines.append(f"  {name}")
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("corpus", type=Path, help="directory of extracted .gs members")
    parser.add_argument("operator", help="operator name to find call sites for")
    parser.add_argument("--window", type=int, default=DEFAULT_WINDOW)
    arguments = parser.parse_args(argv)

    sites, definitions = scan(arguments.corpus, arguments.operator, arguments.window)
    print(report(sites, definitions, arguments.operator), end="")
    return 0 if sites else 1


if __name__ == "__main__":
    import sys

    sys.path.insert(0, str(Path(__file__).resolve().parent))
    raise SystemExit(main())
