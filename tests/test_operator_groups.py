import tempfile
import unittest
from pathlib import Path

from tools.operator_groups import (
    call_site_agreement,
    caller_index,
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


if __name__ == "__main__":
    unittest.main()
