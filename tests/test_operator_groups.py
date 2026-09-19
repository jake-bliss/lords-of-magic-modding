import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

PROJECT_DIR = Path(__file__).resolve().parents[1]

from tools.operator_groups import (
    call_site_agreement,
    caller_group_agreement,
    caller_index,
    dominant_directories,
    name_stem,
    read_table_order,
    stem_agreement,
    stem_runs,
)


class NameStemTest(unittest.TestCase):
    def test_strips_a_leading_verb_so_accessors_share_a_subject(self) -> None:
        self.assertEqual(name_stem("getcastdata"), "castdata")
        self.assertEqual(name_stem("setcastdata"), "castdata")
        self.assertEqual(name_stem("initcastdata"), "castdata")

    def test_strips_predicate_punctuation(self) -> None:
        self.assertEqual(name_stem("ismovingstealthily?"), "movingstealthily")
        self.assertEqual(name_stem("canhire?"), "hire")

    def test_keeps_a_name_whose_remainder_would_be_too_short_to_be_evidence(self) -> None:
        # Stripping "set" from "sets" leaves "s", which would collide with unrelated names.
        self.assertEqual(name_stem("sets"), "sets")

    def test_leaves_a_name_with_no_verb_prefix_alone(self) -> None:
        self.assertEqual(name_stem("cameraposition"), "cameraposition")


class ReadTableOrderTest(unittest.TestCase):
    def test_takes_operator_rows_in_order_and_ignores_summary_rows(self) -> None:
        output = "\n".join(
            [
                "operator-table-runs\t2",
                "distinct-operators\t3",
                "operator\tPop\t0x004ca010\t1\t0\twell-formed",
                "operator\tdup\t0x004c9f40\t1\t2\twell-formed",
                "operator\texch\t0x004ca7e0\t2\t2\twell-formed",
            ]
        )
        self.assertEqual(read_table_order(output), ["pop", "dup", "exch"])


class CallerIndexTest(unittest.TestCase):
    def test_indexes_calls_and_ignores_names_that_only_appear_in_comments(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.gs").write_text("getcastdata setcastdata")
            (root / "b.gs").write_text("; getcastdata is only mentioned here\nsetcastdata")
            callers = caller_index(root, {"getcastdata", "setcastdata"})
        self.assertEqual(callers["getcastdata"], {"a.gs"})
        self.assertEqual(callers["setcastdata"], {"a.gs", "b.gs"})


class AgreementTest(unittest.TestCase):
    def test_call_site_agreement_exceeds_the_baseline_when_neighbours_share_callers(self) -> None:
        # Neighbours share a caller; distant entries do not, so a random pair usually scores zero.
        order = ["alpha", "beta", "gamma", "delta"]
        callers = {
            "alpha": {"one.gs"},
            "beta": {"one.gs"},
            "gamma": {"two.gs"},
            "delta": {"two.gs"},
        }
        result = call_site_agreement(order, callers)
        self.assertEqual(result["pairs"], 3)
        self.assertGreater(result["adjacent_mean"], result["baseline_mean"])

    def test_call_site_agreement_matches_the_baseline_when_order_carries_nothing(self) -> None:
        # Every operator has the same caller set, so adjacency tells you nothing extra.
        order = ["alpha", "beta", "gamma", "delta"]
        callers = {name: {"one.gs"} for name in order}
        result = call_site_agreement(order, callers)
        self.assertAlmostEqual(result["ratio"], 1.0, places=6)

    def test_stem_agreement_reports_the_observed_share_and_a_baseline(self) -> None:
        order = ["getcastdata", "setcastdata", "cameraposition", "ambientlight"]
        result = stem_agreement(order)
        # One of the three adjacent pairs shares a stem.
        self.assertAlmostEqual(result["observed"], 1 / 3, places=6)
        self.assertGreater(result["ratio"], 1.0)


class StemRunsTest(unittest.TestCase):
    def test_groups_consecutive_entries_sharing_a_stem(self) -> None:
        order = [
            "unrelated",
            "getslider",
            "setslider",
            "slider",
            "cameraposition",
        ]
        self.assertEqual(
            stem_runs(order),
            [("slider", ["getslider", "setslider", "slider"])],
        )

    def test_does_not_group_matching_entries_that_are_not_adjacent(self) -> None:
        order = ["getslider", "cameraposition", "setslider"]
        self.assertEqual(stem_runs(order), [])


class CallerGroupDeterminismTest(unittest.TestCase):
    """The dominant-caller-directory row has to be the same number twice.

    `caller_group_agreement` picks each operator's dominant caller directory out of a `Counter`.
    `Counter.most_common` breaks a tie by insertion order, and the insertion order came from
    iterating a `set` of file names, which varies with Python's string hash randomisation. The
    published table moved between runs because of it: five runs over one corpus with one unchanged
    tokenizer gave observed 1.43-1.45 and ratio 1.31-1.32x, and a tokenizer change was credited
    with a difference that was noise. The tie is now broken by name.
    """

    SEED_PROBE = """
import sys
sys.path.insert(0, {project!r})
from tools.operator_groups import caller_group_agreement

order = [f"op{{index}}" for index in range(12)]
callers = {{
    name: {{f"{{letter}}__dir__{{name}}.gs" for letter in "abcdefgh"}}
    for name in order
}}
print(caller_group_agreement(order, callers)["observed"])
"""

    def test_the_dominant_directory_does_not_depend_on_the_hash_seed(self) -> None:
        """Run it under eight hash seeds and demand one answer.

        This cannot be tested inside one process: a set with the same elements iterates the same
        way every time within a run, and the defect is that the way *changes between runs*. Eight
        subprocesses with different `PYTHONHASHSEED` values are the instrument, and the fixture
        gives every operator an eight-way tie so a hash-ordered tie-break has every chance to
        disagree with itself. Reverting the tie-break to `Counter.most_common` makes this fail.
        """
        answers = set()
        for seed in range(8):
            environment = dict(os.environ, PYTHONHASHSEED=str(seed), PYTHONDONTWRITEBYTECODE="1")
            completed = subprocess.run(
                [sys.executable, "-c", self.SEED_PROBE.format(project=str(PROJECT_DIR))],
                capture_output=True,
                check=True,
                env=environment,
                text=True,
            )
            answers.add(completed.stdout.strip())
        self.assertEqual(
            len(answers),
            1,
            f"the dominant-caller-directory measurement depends on the hash seed: {answers}",
        )

    def test_a_tie_is_broken_by_name_rather_than_by_iteration_order(self) -> None:
        order = ["alpha", "beta"]
        callers = {
            # One caller in each of two directories -- a tie, which has to resolve to `a\\y`.
            # The names carry three components because the grouping depth is two.
            "alpha": {"b__x__one.gs", "a__y__two.gs"},
            "beta": {"a__y__three.gs", "b__x__four.gs"},
        }
        first = caller_group_agreement(order, callers)

        # The same sets, built in the opposite order, are the same sets -- which is the point: the
        # result must not depend on how they were built or on how they happen to iterate.
        reversed_callers = {
            "alpha": {"a__y__two.gs", "b__x__one.gs"},
            "beta": {"b__x__four.gs", "a__y__three.gs"},
        }
        self.assertEqual(first, caller_group_agreement(order, reversed_callers))

        # Naming the winner, because a run length of 2 is satisfied by EITHER tie outcome as long
        # as both operators pick the same one -- which is not the property under test.
        self.assertEqual(dominant_directories(order, callers), ["a\\y", "a\\y"])
        self.assertEqual(first["observed"], 2.0)

    def test_a_clear_majority_still_wins(self) -> None:
        order = ["alpha"]
        callers = {"alpha": {"b__x__one.gs", "b__x__two.gs", "a__y__three.gs"}}
        self.assertEqual(caller_group_agreement(order, callers)["observed"], 1.0)


if __name__ == "__main__":
    unittest.main()
