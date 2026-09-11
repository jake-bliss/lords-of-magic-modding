import tempfile
import unittest
from pathlib import Path

from tools.compare_trees import compare, inventory


class CompareTreesTest(unittest.TestCase):
    def test_classifies_added_removed_modified_and_unchanged_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            base = root / "base"
            variant = root / "variant"
            base.mkdir()
            variant.mkdir()

            (base / "same.gs").write_text("same")
            (variant / "same.gs").write_text("same")
            (base / "changed.gs").write_text("before")
            (variant / "changed.gs").write_text("after")
            (base / "removed.gs").write_text("removed")
            (variant / "added.gs").write_text("added")
            (base / "formatted.gs").write_text("/name 1 def")
            (variant / "formatted.gs").write_text("/name\n1 def ; comment\n")
            (base / "File00000001.xxx").write_text("catalogued")
            (variant / "units").mkdir()
            (variant / "units" / "known.gs").write_text("catalogued")

            differences = compare(inventory(base), inventory(variant))
            statuses = {difference.path: difference.status for difference in differences}

            self.assertEqual(
                statuses,
                {
                    "added.gs": "added",
                    "changed.gs": "modified",
                    "formatted.gs": "reformatted",
                    "removed.gs": "removed",
                    "same.gs": "unchanged",
                    "units/known.gs": "renamed",
                },
            )
            renamed = next(item for item in differences if item.status == "renamed")
            self.assertEqual(renamed.base.path, "File00000001.xxx")
            self.assertEqual(renamed.variant.path, "units/known.gs")


if __name__ == "__main__":
    unittest.main()
