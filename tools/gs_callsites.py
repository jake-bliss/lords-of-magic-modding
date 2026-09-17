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

# How many tokens before the operator to show. The module's own motivating example --
# `cx cy f terrainsprites begin keep_ttype end addterrainsprite` -- is seven tokens, so a window of
# six silently cut the very case this exists to explain. Ten leaves headroom and still fits a line.
DEFAULT_WINDOW = 10

# Tokens that end an operand window: nothing before them can be an operand of this call. `}` is
# deliberately NOT here -- a procedure literal is an operand, and walking back over it to its `{`
# is what keeps the operand in front of it visible.
BOUNDARIES = frozenset({"def", "if", "ifelse", "for", "forall", "repeat"})


class Definitions:
    """Every `/name` definition in the corpus, split by whether it names a procedure or a value.

    A name bound to a procedure may consume operands; a name bound to a literal is exactly one
    operand. That distinction is the whole reason this module exists, so it is derived from the
    corpus rather than assumed.
    """

    def __init__(self) -> None:
        self.procedures: set[str] = set()
        self.values: set[str] = set()

    # How far after a `/name` to look for the `def` that makes it a definition. The shipped closure
    # idiom puts several tokens in between: `/great_temple{...}/dummy great_temple_array replace
    # bind def` needs five.
    DEFINITION_LOOKAHEAD = 8

    def add_file(self, file_tokens: list[str]) -> None:
        for index, token in enumerate(file_tokens):
            if len(token) < 2 or not token.startswith("/"):
                continue
            kind = self._definition_kind(file_tokens, index)
            if kind == "procedure":
                self.procedures.add(token[1:])
            elif kind == "value":
                self.values.add(token[1:])

    @classmethod
    def _definition_kind(cls, file_tokens: list[str], index: int) -> str | None:
        """Whether `/name` at `index` is defining something, and as what.

        A `/name` is not a definition merely because a `{` follows it. `/sprite_type get exec`
        looks up a dictionary key, and a dictionary literal is full of `key {procedure}` pairs.
        Treating those as definitions makes the classifier confidently wrong in the one direction
        that matters: a name wrongly recorded as a known `value` stops being reported as an
        unresolved engine operator. A definition is a `/name` with a `def` close behind it.
        """
        position = index + 1
        if position >= len(file_tokens):
            return None
        opener = file_tokens[position]
        kind = "value"
        if opener in ("{", "["):
            closer = "}" if opener == "{" else "]"
            kind = "procedure" if opener == "{" else "value"
            depth = 0
            while position < len(file_tokens):
                if file_tokens[position] == opener:
                    depth += 1
                elif file_tokens[position] == closer:
                    depth -= 1
                    if depth == 0:
                        position += 1
                        break
                position += 1
            else:
                return None
        limit = min(len(file_tokens), position + cls.DEFINITION_LOOKAHEAD)
        if "def" in file_tokens[position:limit]:
            return kind
        return None

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
    def __init__(
        self,
        path: Path,
        line: int,
        column: int,
        window: list[str],
        truncated: bool = False,
    ) -> None:
        self.path = path
        self.line = line
        self.column = column
        self.window = window
        # The window hit the token limit rather than a real boundary, so operands may be missing.
        self.truncated = truncated

    @property
    def pattern(self) -> str:
        """A blank pattern must never be the answer, and a cut window must never look complete."""
        parts = list(self.window)
        if self.truncated:
            parts.insert(0, "...")
        return " ".join(parts) if parts else "(no operands)"

    @property
    def where(self) -> str:
        """Line and column. Lines here run to thousands of characters; the column is not optional."""
        return f"{self.path.name}:{self.line}:{self.column}"


def _operand_window(names: list[str], index: int, window: int) -> tuple[list[str], bool]:
    """Walk back from a call, collecting operand tokens, and return them with a truncation flag.

    A procedure literal is one operand, so a closing `}` is not the end of the window -- the walk
    finds its matching `{` and carries on. Reporting `enumplayerarmies` as taking nothing, or as
    taking only a procedure, would both be wrong: it takes a player and a procedure.
    """
    collected: list[str] = []
    position = index - 1
    while position >= 0 and len(collected) < window:
        token = names[position]
        if token in BOUNDARIES:
            return list(reversed(collected)), False
        if token == "}":
            depth = 0
            while position >= 0:
                if names[position] == "}":
                    depth += 1
                elif names[position] == "{":
                    depth -= 1
                    if depth == 0:
                        break
                position -= 1
            if position < 0:
                # Unbalanced: the procedure starts before the file does. Stop rather than guess.
                return list(reversed(collected)), True
            collected.append("{...}")
            position -= 1
            continue
        if token == "{":
            # An unmatched opener: this call is inside a procedure and the window has reached its
            # start, so there is nothing further back that could be an operand.
            return list(reversed(collected)), False
        collected.append(token)
        position -= 1
    return list(reversed(collected)), position >= 0


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
            # No special case for a preceding `/name` here. It was written to skip a definition of
            # the operator itself, but `/myop` and `myop` are different tokens, so a definition
            # never produces the pattern it looked for -- while `/myop myop`, which is a literal
            # name followed by a real call, was silently dropped. The `token != operator` test
            # above already excludes every literal form.
            chunk, truncated = _operand_window(names, index, window)
            # A character offset, not a byte offset: these are indices into a `str`.
            offset = located[index][1]
            line = source.count("\n", 0, offset) + 1
            column = offset - (source.rfind("\n", 0, offset) + 1) + 1
            sites.append(CallSite(path, line, column, chunk, truncated))
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

    by_class: dict[str, set[str]] = collections.defaultdict(set)
    for site in sites:
        for token in site.window:
            by_class[definitions.classify(token)].add(token)

    if by_class["procedure"]:
        lines.append("")
        lines.append(
            "These names in the operand windows are PROCEDURES defined in the corpus, so the token"
        )
        lines.append(
            "count is not the operand count -- each may consume or produce operands. Read their"
        )
        lines.append("definitions before concluding anything about arity:")
        for name in sorted(by_class["procedure"]):
            lines.append(f"  {name}")

    if by_class["unknown"]:
        lines.append("")
        lines.append(
            "These are not defined anywhere in the corpus, so they are engine operators with stack"
        )
        lines.append(
            "effects of their own -- `terrainsprites /tower3 get` is three tokens and ONE operand."
        )
        lines.append("Check each against the recovered operator table before counting operands:")
        for name in sorted(by_class["unknown"]):
            lines.append(f"  {name}")

    if any(site.truncated for site in sites):
        lines.append("")
        lines.append(
            f"A window shown with a leading `...` hit the {DEFAULT_WINDOW}-token limit rather than a"
        )
        lines.append("real boundary; operands are missing from it. Re-run with a larger --window.")
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
