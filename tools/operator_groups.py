"""Test whether the engine's operator table is grouped by subject, and label the groups.

`lomse.exe` registers its GameScript operators in a table whose order is fixed at build time. If
that order carries meaning — if neighbouring entries belong to the same subsystem — then the table
is a free outline of the host API. This module measures whether that is true rather than assuming
it, by comparing the real table order against a shuffled baseline on two independent signals:

* **call sites** — do adjacent operators get called from the same scripts?
* **name morphology** — do adjacent operators share a name stem, as `getcastdata` and `setcastdata`
  do?

Both comparisons need a null model, because any two operators share some callers by chance. The
shuffled baseline supplies it: the same names, the same caller sets, a random order.
"""

from __future__ import annotations

import argparse
import random
import re
import statistics
from collections import Counter
from pathlib import Path

from tools.gs_syntax import tokens

# Verb prefixes the engine uses to name accessors around a shared subject. Stripping them is what
# lets `getcastdata`, `setcastdata` and `initcastdata` be recognised as one group.
VERB_PREFIXES = (
    "get", "set", "enum", "is", "can", "has", "add", "remove", "create", "destroy",
    "do", "play", "draw", "calc", "find", "make", "clear", "reset", "update", "init",
    "load", "save", "send", "show", "hide", "open", "close", "start", "stop", "toggle",
    "enable", "disable", "inc", "dec",
)
_PREFIX = re.compile("^(" + "|".join(VERB_PREFIXES) + ")")

# A stem shorter than this is not evidence of a shared subject; `set` and `sets` would collide with
# anything. Three characters is the shortest subject name that appears in the table (`imp`).
MINIMUM_STEM = 3

# Draws used to estimate the chance level. Large enough that the baseline mean is stable to the
# precision reported.
BASELINE_DRAWS = 20_000
BASELINE_SHUFFLES = 300


def name_stem(name: str) -> str:
    """Strip a leading verb and any predicate suffix, leaving the subject the operator acts on."""
    subject = name.rstrip("?!")
    match = _PREFIX.match(subject)
    if match and len(subject) - match.end() >= MINIMUM_STEM:
        return subject[match.end() :]
    return subject


def read_table_order(scan_output: str) -> list[str]:
    """Extract operator names, in table order, from `--scan-natives` output."""
    order = []
    for line in scan_output.splitlines():
        fields = line.split("\t")
        if fields[0] == "operator" and len(fields) >= 2:
            order.append(fields[1].lower())
    return order


def caller_index(script_directory: Path, operators: set[str]) -> dict[str, set[str]]:
    """Map each operator to the set of script files that name it.

    Tokenising with the project lexer rather than a regex matters: it drops `;`-to-end-of-line
    comments, so a name that only appears in prose is not counted as a call site.
    """
    callers: dict[str, set[str]] = {}
    for path in sorted(script_directory.iterdir()):
        if not path.is_file():
            continue
        source = path.read_text(errors="replace")
        for token in {token.lower() for token in tokens(source)}:
            if token in operators:
                callers.setdefault(token, set()).add(path.name)
    return callers


def _jaccard(left: set[str], right: set[str]) -> float:
    union = len(left | right)
    return len(left & right) / union if union else 0.0


def call_site_agreement(
    order: list[str], callers: dict[str, set[str]], seed: int = 7
) -> dict[str, float]:
    """Compare caller-set overlap for adjacent operators against randomly paired ones."""
    adjacent = [
        _jaccard(callers[left], callers[right])
        for left, right in zip(order, order[1:])
        if left in callers and right in callers
    ]
    pool = [name for name in order if name in callers]
    generator = random.Random(seed)
    baseline = [
        _jaccard(callers[generator.choice(pool)], callers[generator.choice(pool)])
        for _ in range(BASELINE_DRAWS)
    ]
    adjacent_mean = statistics.mean(adjacent) if adjacent else 0.0
    baseline_mean = statistics.mean(baseline) if baseline else 0.0
    return {
        "pairs": len(adjacent),
        "adjacent_mean": adjacent_mean,
        "baseline_mean": baseline_mean,
        "ratio": adjacent_mean / baseline_mean if baseline_mean else 0.0,
        "adjacent_sharing_any": sum(1 for value in adjacent if value > 0) / len(adjacent)
        if adjacent
        else 0.0,
        "baseline_sharing_any": sum(1 for value in baseline if value > 0) / len(baseline)
        if baseline
        else 0.0,
    }


def stem_agreement(order: list[str], seed: int = 3) -> dict[str, float]:
    """Compare how often adjacent operators share a name stem against a shuffled order."""
    stems = [name_stem(name) for name in order]

    def adjacent_share(sequence: list[str]) -> float:
        if len(sequence) < 2:
            return 0.0
        return sum(1 for left, right in zip(sequence, sequence[1:]) if left == right) / (
            len(sequence) - 1
        )

    observed = adjacent_share(stems)
    generator = random.Random(seed)
    shuffled = list(stems)
    baseline = []
    for _ in range(BASELINE_SHUFFLES):
        generator.shuffle(shuffled)
        baseline.append(adjacent_share(shuffled))
    baseline_mean = statistics.mean(baseline)
    return {
        "observed": observed,
        "baseline": baseline_mean,
        "ratio": observed / baseline_mean if baseline_mean else 0.0,
    }


def stem_runs(order: list[str], minimum: int = 3) -> list[tuple[str, list[str]]]:
    """Find consecutive table entries that share a name stem."""
    stems = [name_stem(name) for name in order]
    runs = []
    index = 0
    while index < len(order):
        end = index
        while end + 1 < len(order) and stems[end + 1] == stems[index]:
            end += 1
        if end - index + 1 >= minimum:
            runs.append((stems[index], order[index : end + 1]))
        index = end + 1
    return runs


def caller_group_agreement(
    order: list[str], callers: dict[str, set[str]], depth: int = 2, seed: int = 11
) -> dict[str, float]:
    """Compare run structure of each operator's dominant caller directory against a shuffle.

    Reported because it is the obvious hypothesis and it does **not** hold: the script tree is
    organised for the mod's authors, not along the engine's internal seams.
    """
    dominant: list[str | None] = []
    for name in order:
        files = callers.get(name)
        if not files:
            dominant.append(None)
            continue
        # Deterministic tie-break. `files` is a set, so its iteration order varies with Python's
        # string hash randomisation, and `Counter.most_common` breaks ties by insertion order --
        # which made this row, and only this row, move between runs of the same corpus with the
        # same tokenizer: observed 1.43-1.45 and ratio 1.31-1.32x over five runs. A published
        # measurement a reader cannot reproduce is not a measurement. Measured 2026-09-18.
        groups = Counter("\\".join(file.split("__")[:depth]) for file in sorted(files))
        dominant.append(min(groups.items(), key=lambda group: (-group[1], group[0]))[0])

    def mean_run(sequence: list[str | None]) -> float:
        runs = []
        current = 1
        for left, right in zip(sequence, sequence[1:]):
            if left == right and left is not None:
                current += 1
            else:
                runs.append(current)
                current = 1
        runs.append(current)
        return statistics.mean(runs)

    observed = mean_run(dominant)
    generator = random.Random(seed)
    shuffled = list(dominant)
    baseline = []
    for _ in range(BASELINE_SHUFFLES):
        generator.shuffle(shuffled)
        baseline.append(mean_run(shuffled))
    baseline_mean = statistics.mean(baseline)
    return {
        "observed": observed,
        "baseline": baseline_mean,
        "ratio": observed / baseline_mean if baseline_mean else 0.0,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scan_output", type=Path, help="saved output of --scan-natives")
    parser.add_argument("script_directory", type=Path, help="directory of extracted .gs files")
    parser.add_argument(
        "--skip",
        type=int,
        default=104,
        help="leading entries to ignore; the default skips the interpreter primitive table",
    )
    arguments = parser.parse_args()

    order = read_table_order(arguments.scan_output.read_text())[arguments.skip :]
    callers = caller_index(arguments.script_directory, set(order))

    calls = call_site_agreement(order, callers)
    stems = stem_agreement(order)
    groups = caller_group_agreement(order, callers)

    print(f"operators\t{len(order)}")
    print(f"operators-with-callers\t{len(callers)}")
    print(
        "call-site-agreement\t"
        f"pairs={calls['pairs']}\tadjacent={calls['adjacent_mean']:.4f}\t"
        f"baseline={calls['baseline_mean']:.4f}\tratio={calls['ratio']:.1f}x"
    )
    print(
        "call-site-sharing-any\t"
        f"adjacent={calls['adjacent_sharing_any']:.3f}\tbaseline={calls['baseline_sharing_any']:.3f}"
    )
    print(
        "name-stem-agreement\t"
        f"observed={stems['observed']:.4f}\tbaseline={stems['baseline']:.5f}\t"
        f"ratio={stems['ratio']:.0f}x"
    )
    print(
        "caller-directory-runs\t"
        f"observed={groups['observed']:.2f}\tbaseline={groups['baseline']:.2f}\t"
        f"ratio={groups['ratio']:.2f}x"
    )
    for stem, members in stem_runs(order):
        print(f"stem-run\t{stem}\t{len(members)}\t{','.join(members)}")


if __name__ == "__main__":
    main()
