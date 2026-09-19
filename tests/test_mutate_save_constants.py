"""The mutation harness must not silently stop covering a constant.

A sweep whose coverage quietly shrinks reports a better number than it earned. These tests are
about the harness's *reach*, not about `save.rs` being correct -- they never build anything.
"""

import importlib.util
import pathlib
import re
import unittest

REPO = pathlib.Path(__file__).resolve().parent.parent
HARNESS = REPO / "tools" / "mutate_save_constants.py"
SOURCE = REPO / "spikes" / "asset-viewer" / "src" / "save.rs"


def load_harness():
    spec = importlib.util.spec_from_file_location("mutate_save_constants", HARNESS)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


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

    def test_expected_survivors_are_named_and_justified(self):
        # An expected survivor is a claim that no test *can* catch it. If the constant stops
        # existing, the exemption must go too, or it silently excuses a real gap.
        for name in self.harness.EXPECTED_SURVIVORS:
            self.assertIn(f"const {name}: i32", self.text)


if __name__ == "__main__":
    unittest.main()
