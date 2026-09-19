"""The mutation harness must not silently stop measuring.

A sweep whose coverage quietly shrinks, or whose baseline is red, reports a better number than it
earned. These tests are about the harness's *reach and honesty*, not about `save.rs` being
correct -- they never build anything.
"""

import importlib.util
import pathlib
import re
import subprocess
import unittest

REPO = pathlib.Path(__file__).resolve().parent.parent
HARNESS = REPO / "tools" / "mutate_save_constants.py"
SOURCE = REPO / "spikes" / "asset-viewer" / "src" / "save.rs"


def load_harness():
    spec = importlib.util.spec_from_file_location("mutate_save_constants", HARNESS)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def completed(returncode, stdout="", stderr=""):
    return subprocess.CompletedProcess(["cargo"], returncode, stdout, stderr)


class GateCoverage(unittest.TestCase):
    def setUp(self):
        self.harness = load_harness()
        self.text = SOURCE.read_text()

    def test_every_sprite_gate_constant_is_mutated_in_both_directions(self):
        # Counted from the source independently of the harness's own regex, so a regex that
        # narrowed would be caught rather than agreeing with itself.
        declared = set(
            re.findall(r"^const (SPR_\w+): i32 = 0x[0-9A-Fa-f]+;$", self.text, re.MULTILINE)
        )
        self.assertGreater(len(declared), 25, "the gate ladder should not have shrunk this far")

        mutated = {}
        for _, _, label in self.harness.gate_mutations(self.text):
            name, delta = label.rsplit(" ", 1)
            mutated.setdefault(name, set()).add(delta)

        self.assertEqual(set(mutated), declared)
        for name, deltas in mutated.items():
            self.assertEqual(deltas, {"+1", "-1"}, f"{name} is not swept in both directions")

    def test_every_structural_mutation_still_matches_the_source_exactly_once(self):
        for old, new, label in self.harness.STRUCTURAL:
            with self.subTest(label=label):
                self.assertEqual(
                    self.text.count(old),
                    1,
                    f"{label}: the pattern no longer matches the source uniquely, so this "
                    f"mutation would be skipped and the sweep would silently under-report",
                )
                self.assertNotEqual(old, new)

    def test_mutation_ids_are_unique_so_results_can_be_attributed(self):
        labels = [label for _, _, label in self.harness.gate_mutations(self.text)]
        labels += [label for _, _, label in self.harness.STRUCTURAL]
        self.assertEqual(len(labels), len(set(labels)))

    def test_each_unguarded_callsite_has_its_own_structural_mutation(self):
        # One shared mutation would let two of the three call sites go untested while the sweep
        # stayed green. That is exactly what a review found, so it is pinned here.
        unguarded = [
            label for _, _, label in self.harness.STRUCTURAL if "unguarded" in label
        ]
        self.assertEqual(len(unguarded), self.text.count("spr_unguarded_count(cursor.u32()?)"))
        self.assertEqual(len(unguarded), 3)


class CountTaxonomy(unittest.TestCase):
    """Every file-declared number in `LS_SPR_` must go through one of the three markers.

    The markers are how the engine's three guard shapes stay visible at the call site. A reader
    added with a bare `cursor.u32()? as usize` is behaviourally fine and silently outside the
    classification, which is how the convention rots -- a review found exactly two such sites.
    """

    def setUp(self):
        self.text = SOURCE.read_text()
        start = self.text.index("// 0x004F6B00 -- the base reader")
        end = self.text.index("/// The unit, army and hero table.")
        self.readers = self.text[start:end]

    # Reads that are not quantities and so are not in the taxonomy. Matched by EQUALITY and
    # required to be present, so this cannot quietly grow into a place to hide a real count.
    NOT_A_QUANTITY = {
        # An enumerant that selects code, not a number that drives a loop or a read length. The
        # engine bounds it with `cmp ecx,3 / ja` inside the factory at 0x0044B4F0, and
        # `spr_nested` mirrors that with an explicit error arm citing the bound.
        "let type_id = cursor.u32()?;",
    }

    def test_the_not_a_quantity_exemptions_all_still_exist(self):
        for line in self.NOT_A_QUANTITY:
            with self.subTest(line=line):
                self.assertIn(
                    line,
                    self.readers,
                    "an exemption for a line that no longer exists excuses nothing; delete it",
                )

    def test_no_sprite_reader_takes_a_bare_length_or_count(self):
        offenders = []
        for line in self.readers.splitlines():
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            # A `u32`/`u16` read bound to a name is a count or a length; one consumed directly by
            # `take` of a fixed size is not.
            if re.search(r"let \w+ = cursor\.u(?:16|32)\(\)\?", stripped) and not re.search(
                r"spr_(?:signed_count16|signed_count|unguarded_count|unguarded_length)", stripped
            ):
                if stripped not in self.NOT_A_QUANTITY:
                    offenders.append(stripped)
        self.assertEqual(
            offenders,
            [],
            "these file-declared numbers bypass the three-way guard taxonomy:\n  "
            + "\n  ".join(offenders),
        )

    def test_all_three_markers_exist_and_are_used(self):
        for marker in (
            "spr_signed_count",
            "spr_signed_count16",
            "spr_unguarded_count",
            "spr_unguarded_length",
        ):
            with self.subTest(marker=marker):
                self.assertIn(f"fn {marker}(", self.text)
                self.assertGreaterEqual(
                    self.readers.count(f"{marker}("), 1, f"{marker} is declared but unused"
                )


class ExpectedSurvivors(unittest.TestCase):
    def setUp(self):
        self.harness = load_harness()
        self.text = SOURCE.read_text()

    def test_expected_survivors_are_full_mutation_ids_not_prefixes(self):
        # Substring matching is the hazard: a future gate named `<existing> _EXTRA` would produce
        # labels CONTAINING an expected id and be excused in silence. Every exemption must be a
        # complete, producible mutation id.
        producible = {label for _, _, label in self.harness.gate_mutations(self.text)}
        producible |= {label for _, _, label in self.harness.STRUCTURAL}
        for label in self.harness.EXPECTED_SURVIVORS:
            with self.subTest(label=label):
                self.assertIn(
                    label,
                    producible,
                    "an exemption that no mutation can produce excuses nothing and hides a gap",
                )

    def test_every_exemption_carries_a_reason(self):
        for label, reason in self.harness.EXPECTED_SURVIVORS.items():
            with self.subTest(label=label):
                self.assertTrue(reason and reason.strip(), f"{label} has no stated reason")

    def test_a_new_gate_whose_name_extends_an_exempt_one_is_not_excused(self):
        # The concrete attack, run rather than described.
        extended = self.text.replace(
            "const SPR_NESTED_LAST_WORD_MIN: i32 = 0x41;",
            "const SPR_NESTED_LAST_WORD_MIN: i32 = 0x41;\n"
            "const SPR_NESTED_LAST_WORD_MIN_EXTRA: i32 = 0x41;",
            1,
        )
        labels = {label for _, _, label in self.harness.gate_mutations(extended)}
        self.assertIn("SPR_NESTED_LAST_WORD_MIN_EXTRA +1", labels)
        # The new label must NOT be excused, even though it contains an exempt one as a prefix.
        for label in labels:
            if label.startswith("SPR_NESTED_LAST_WORD_MIN_EXTRA"):
                self.assertNotIn(label, self.harness.EXPECTED_SURVIVORS)


class BaselineHonesty(unittest.TestCase):
    """The failure this harness is most likely to have is reporting a perfect score having run
    nothing at all. `classify` must never call a non-running suite a kill."""

    def setUp(self):
        self.harness = load_harness()

    def test_a_clean_exit_is_a_survivor(self):
        self.assertEqual(
            self.harness.classify(completed(0, "test result: ok. 1 passed")),
            self.harness.SURVIVED,
        )

    def test_a_real_test_failure_is_a_kill(self):
        self.assertEqual(
            self.harness.classify(completed(101, "test result: FAILED. 1 failed")),
            self.harness.KILLED,
        )

    def test_a_build_failure_is_not_counted_as_a_kill(self):
        for stdout, stderr in [
            ("", "error[E0308]: mismatched types\nerror: could not compile `x`"),
            ("", "error: could not compile `lom-asset-viewer`"),
        ]:
            with self.subTest(stderr=stderr[:30]):
                self.assertEqual(
                    self.harness.classify(completed(101, stdout, stderr)),
                    self.harness.UNCOMPILABLE,
                )

    def test_success_is_the_exit_code_and_never_a_substring_of_the_output(self):
        # A suite that cannot run prints no `test result: ok`. Inferring "killed" from that is
        # what turns a broken baseline into a perfect score, so a zero exit must win even when
        # the output says nothing recognisable.
        self.assertEqual(self.harness.classify(completed(0, "")), self.harness.SURVIVED)
        self.assertEqual(
            self.harness.classify(completed(0, "warning: unused import")),
            self.harness.SURVIVED,
        )

    def test_a_skipped_mutation_is_not_folded_into_the_survivor_list(self):
        # The third fail-open path: a mutation whose pattern stops matching must not be absorbed
        # as an expected survivor, or the sweep stops running it while the number holds.
        text = HARNESS.read_text()
        self.assertIn("SKIPPED", text)
        self.assertNotIn("outcomes[label] = SURVIVED", text)
        self.assertIn("unexpected or vanished or skipped", text)

    def test_both_signed_helpers_have_a_width_mutation(self):
        # The 32-bit one was missing, and `(raw as i16)` passed the whole suite because of it.
        widths = [
            label for _, _, label in self.harness.STRUCTURAL if "wrong width" in label
        ]
        self.assertEqual(sorted(widths), [
            "16-bit count clamped at the wrong width",
            "32-bit count clamped at the wrong width",
        ])

    def test_the_harness_checks_its_baseline_before_and_after(self):
        text = HARNESS.read_text()
        self.assertIn("check_baseline(\"before\")", text)
        self.assertIn("check_baseline(\"after restore\")", text)
        self.assertIn("result.returncode != 0", text)


if __name__ == "__main__":
    unittest.main()
