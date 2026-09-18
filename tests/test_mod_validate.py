"""Validation findings, against archives and profiles that do not exist.

Every fixture here is a manifest row and a fact record built in memory. That is deliberate: the
validator's inputs are all *produced* by other tools, so its logic can be exercised with no game
installed, no StormLib, and no archive on disk -- which is what lets the refusal cases be asserted
one at a time instead of hoped for.
"""

import tempfile
import unittest
from pathlib import Path

from tools.mod_tree import load
from tools.mod_validate import ERROR, WARNING, validate
from tools.mpq_shape import Member

MANIFEST = """\
id = "example"
name = "A mod"
version = "0.1.0"
base_profile = "vanilla"
"""

ORINF_SHA = "4eadbc3cf9e11c46c8b3824103ef1c99aab4e5b72c588f63c6d349089e6f5874"


def base_member(path: str, *, block_index: int = 10, sha256: str = ORINF_SHA) -> Member:
    return Member(
        path=path,
        block_index=block_index,
        hash_index=block_index,
        size=1798,
        compressed_size=677,
        flags="0x80010100",
        locale=0,
        sha256=sha256,
    )


def facts(
    name: str,
    *,
    definitions: dict | None = None,
    executables: dict | None = None,
    runs: list | None = None,
    paths: list | None = None,
    parse_error: dict | None = None,
    line_endings: dict | None = None,
    high: dict | None = None,
    control: dict | None = None,
    valid_utf8: bool = True,
    anomalies: list | None = None,
) -> dict:
    return {
        "name": name,
        "bytes": 1798,
        "sha256": ORINF_SHA,
        "token_sha256": "t" * 64,
        "parse_error": parse_error,
        "token_count": 200,
        "comment_count": 0,
        "string_count": 2,
        "number_count": 33,
        "maximum_procedure_depth": 2,
        "procedure_anomalies": anomalies or [],
        "definition_names": definitions or {},
        "scalar_definitions": {},
        "executable_names": executables or {},
        "literal_names": {},
        "static_run_dependencies": runs or [],
        "path_strings": paths or [],
        "non_path_string_count": 2,
        "line_endings": line_endings or {"crlf": 0, "bare_cr": 0, "bare_lf": 0},
        "alphabet": {
            "tabs": 0,
            "valid_utf8": valid_utf8,
            "high": high or {},
            "unexpected_control": control or {},
        },
    }


class ValidateTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.root = Path(self._temporary.name) / "example"
        self.root.mkdir(parents=True)
        (self.root / "mod.toml").write_text(MANIFEST, encoding="utf-8")
        (self.root / "archives" / "gs.mpq" / "units").mkdir(parents=True)

    def tearDown(self) -> None:
        self._temporary.cleanup()

    def write(self, relative: str, content: bytes = b"/a 1 def") -> str:
        path = self.root / "archives" / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return f"archives/{relative}"

    def manifest(self, text: str) -> None:
        (self.root / "mod.toml").write_text(text, encoding="utf-8")

    def run_validate(
        self,
        *,
        manifests=None,
        base_facts=None,
        mod_facts=None,
        vocabulary=None,
    ):
        return validate(
            load(self.root),
            manifests if manifests is not None else {"gs.mpq": []},
            base_facts or {},
            mod_facts or {},
            vocabulary or {},
        )

    def findings(self, report, check: str) -> list:
        return [finding for finding in report.findings if finding.check == check]


class MemberResolutionTest(ValidateTestCase):
    def test_a_member_that_resolves_exactly_produces_no_finding(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            mod_facts={relative: facts(relative)},
        )
        self.assertEqual(self.findings(report, "missing-member"), [])
        self.assertEqual(self.findings(report, "case-mismatch"), [])
        self.assertTrue(report.ok)

    def test_a_member_the_base_archive_does_not_have_is_an_error(self) -> None:
        relative = self.write("gs.mpq/units/invented.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            mod_facts={relative: facts(relative)},
        )
        finding = self.findings(report, "missing-member")[0]
        self.assertEqual(finding.severity, ERROR)
        self.assertIn("new_members", finding.message)
        self.assertFalse(report.ok)

    def test_a_case_difference_from_the_manifest_is_an_error_naming_both(self) -> None:
        relative = self.write("gs.mpq/units/ORINF.GS")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            mod_facts={relative: facts(relative)},
        )
        finding = self.findings(report, "case-mismatch")[0]
        self.assertEqual(finding.severity, ERROR)
        # `repr` doubles the backslash, so the assertion matches what a reader actually sees.
        self.assertIn(r"units\\orinf.gs", finding.message)
        self.assertIn(r"units\\ORINF.GS", finding.message)

    def test_a_name_the_archive_holds_twice_is_refused_by_block_index(self) -> None:
        """The PIC5R3 `portrait\\AIpotM.lbm` case: two entries, one name, different content.

        A source tree has one file per name, so it cannot say which block it means. Refusing is
        the only honest answer -- a repack that picked one would silently pick.
        """
        relative = self.write("pic.mpq/portrait/AIpotM.lbm", b"\x00")
        report = self.run_validate(
            manifests={
                "pic.mpq": [
                    base_member("portrait\\AIpotM.lbm", block_index=1108, sha256="9dc0" + "0" * 60),
                    base_member("portrait\\AIpotM.lbm", block_index=1144, sha256="884f" + "0" * 60),
                ]
            },
            mod_facts={relative: facts(relative)},
        )
        finding = self.findings(report, "ambiguous-member")[0]
        self.assertEqual(finding.severity, ERROR)
        self.assertIn("1108", finding.message)
        self.assertIn("1144", finding.message)

    def test_an_added_member_is_refused_unless_declared_and_allowed(self) -> None:
        relative = self.write("gs.mpq/units/new.gs")
        # Neither declared nor allowed.
        report = self.run_validate(
            manifests={"gs.mpq": []}, mod_facts={relative: facts(relative)}
        )
        self.assertEqual(self.findings(report, "missing-member")[0].severity, ERROR)

        # Declared but not allowed: still refused, and the message says why.
        self.manifest(MANIFEST + 'new_members = ["units\\\\new.gs"]\n')
        report = self.run_validate(
            manifests={"gs.mpq": []}, mod_facts={relative: facts(relative)}
        )
        finding = self.findings(report, "new-member")[0]
        self.assertEqual(finding.severity, ERROR)
        self.assertIn("allow_new_members", finding.message)

        # Declared and allowed: permitted, and warned about as untested against the engine.
        self.manifest(
            MANIFEST + 'new_members = ["units\\\\new.gs"]\nallow_new_members = true\n'
        )
        report = self.run_validate(
            manifests={"gs.mpq": []}, mod_facts={relative: facts(relative)}
        )
        finding = self.findings(report, "new-member")[0]
        self.assertEqual(finding.severity, WARNING)
        self.assertIn("No evidence exists", finding.message)
        self.assertTrue(report.ok)

    def test_a_missing_base_manifest_is_an_error_rather_than_a_pass(self) -> None:
        relative = self.write("pic.mpq/LBM/ART.lbm", b"\x00")
        report = self.run_validate(
            manifests={"gs.mpq": []}, mod_facts={relative: facts(relative)}
        )
        self.assertEqual(self.findings(report, "base-manifest")[0].severity, ERROR)


class SyntaxTest(ValidateTestCase):
    def test_a_file_that_does_not_lex_is_an_error_at_line_and_column(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            mod_facts={
                relative: facts(
                    relative,
                    parse_error={
                        "message": "unterminated string",
                        "offset": 42,
                        "line": 7,
                        "column": 3,
                    },
                )
            },
        )
        finding = self.findings(report, "lex")[0]
        self.assertEqual(finding.severity, ERROR)
        self.assertEqual(finding.location, f"{relative}:7:3")
        self.assertIn("unterminated string", finding.message)

    def test_an_unclosed_procedure_is_an_error(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            mod_facts={
                relative: facts(
                    relative,
                    anomalies=[
                        {
                            "message": "unclosed procedure delimiter {",
                            "offset": 3,
                            "line": 1,
                            "column": 4,
                        }
                    ],
                )
            },
        )
        self.assertEqual(self.findings(report, "lex")[0].severity, ERROR)

    def test_a_gamescript_file_with_no_facts_is_an_error_not_a_silence(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]}, mod_facts={}
        )
        self.assertEqual(self.findings(report, "lex")[0].severity, ERROR)


class EncodingTest(ValidateTestCase):
    def base_and_mod(self, **mod_kwargs):
        relative = self.write("gs.mpq/units/orinf.gs")
        return relative, {"units\\orinf.gs": facts("units\\orinf.gs")}, {
            relative: facts(relative, **mod_kwargs)
        }

    def test_a_control_byte_no_shipped_member_contains_is_an_error(self) -> None:
        """Measured across all 4,692 .gs members: only TAB, CR and LF appear."""
        relative, base, mod = self.base_and_mod(control={"0": 1})
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts=base,
            mod_facts=mod,
        )
        finding = self.findings(report, "encoding")[0]
        self.assertEqual(finding.severity, ERROR)
        self.assertIn("0x00", finding.message)

    def test_a_reflow_of_a_single_line_member_is_reported_by_name(self) -> None:
        """`units\\orinf.gs` has ZERO line endings. A CR-versus-LF rule cannot see this."""
        relative, base, mod = self.base_and_mod(
            line_endings={"crlf": 0, "bare_cr": 0, "bare_lf": 34}
        )
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts=base,
            mod_facts=mod,
        )
        finding = self.findings(report, "line-endings")[0]
        self.assertEqual(finding.severity, WARNING)
        self.assertIn("no line ending at all", finding.message)
        self.assertIn("34 bare LF", finding.message)

    def test_an_unchanged_line_ending_census_produces_no_finding(self) -> None:
        """Asserted in both directions, so the check cannot be a constant WARNING."""
        relative, base, mod = self.base_and_mod()
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts=base,
            mod_facts=mod,
        )
        self.assertEqual(self.findings(report, "line-endings"), [])

    def test_a_utf8_resave_of_a_high_byte_member_is_reported(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        base = {"units\\orinf.gs": facts("units\\orinf.gs", high={"252": 3}, valid_utf8=False)}
        mod = {relative: facts(relative, high={}, valid_utf8=True)}
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts=base,
            mod_facts=mod,
        )
        messages = [finding.message for finding in self.findings(report, "encoding")]
        self.assertTrue(any("re-saving the file as UTF-8" in message for message in messages))

    def test_an_introduced_high_byte_is_reported(self) -> None:
        relative, base, mod = self.base_and_mod(high={"233": 1}, valid_utf8=False)
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts=base,
            mod_facts=mod,
        )
        messages = [finding.message for finding in self.findings(report, "encoding")]
        self.assertTrue(any("0xe9" in message for message in messages))


class SymbolTest(ValidateTestCase):
    def test_a_name_no_member_defines_and_the_vocabulary_lacks_is_an_error(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={"units\\orinf.gs": facts("units\\orinf.gs")},
            mod_facts={relative: facts(relative, executables={"hit_ponits": 1})},
            vocabulary={"hit_points": "x", "armor": "x", "mps": "x"},
        )
        finding = self.findings(report, "unresolved-reference")[0]
        self.assertEqual(finding.severity, ERROR)
        self.assertIn("hit_ponits", finding.message)
        # The size is reported from the vocabulary actually loaded, not from a literal. It was
        # hardcoded as "14,080" and went stale the moment PR #64 added `INF` to the table; it had
        # also never been right for patch302 (15,176) or gs5r3 (16,856).
        self.assertIn("3-name executable vocabulary", finding.message)

    def test_a_name_the_vocabulary_knows_is_accepted_and_counted(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={"units\\orinf.gs": facts("units\\orinf.gs")},
            mod_facts={relative: facts(relative, executables={"def": 36, "HUMAN": 1})},
            vocabulary={"def": "language-primitive", "HUMAN": "constant-or-data"},
        )
        self.assertEqual(self.findings(report, "unresolved-reference"), [])
        self.assertEqual(
            report.coverage.counts["references-resolved-by-vocabulary-presence-only"], 2
        )

    def test_a_member_that_calls_what_it_defines_is_not_reported(self) -> None:
        """Every unit record has this shape: `units\\orinf.gs` calls `orinf` 18 times."""
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={
                "units\\orinf.gs": facts(
                    "units\\orinf.gs", definitions={"orinf": 1}, executables={"orinf": 18}
                )
            },
            mod_facts={
                relative: facts(relative, definitions={"orinf": 1}, executables={"orinf": 18})
            },
        )
        self.assertEqual(self.findings(report, "unresolved-reference"), [])

    def test_defining_a_name_another_member_already_defines_is_an_error(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={
                "units\\orinf.gs": facts("units\\orinf.gs"),
                "units\\orcav.gs": facts("units\\orcav.gs", definitions={"orcav": 1}),
            },
            mod_facts={relative: facts(relative, definitions={"orcav": 1})},
        )
        finding = self.findings(report, "duplicate-definition")[0]
        self.assertEqual(finding.severity, ERROR)
        self.assertIn("units\\orcav.gs", finding.message)

    def test_a_definition_nothing_else_defines_is_not_a_duplicate(self) -> None:
        """The other direction. Without this, a check that always fires passes the suite.

        Caught by mutation: `if elsewhere:` -> `if True:` survived until this test existed.
        """
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={
                "units\\orinf.gs": facts("units\\orinf.gs"),
                "units\\orcav.gs": facts("units\\orcav.gs", definitions={"orcav": 1}),
            },
            mod_facts={relative: facts(relative, definitions={"orinf_new_stat": 1})},
        )
        self.assertEqual(self.findings(report, "duplicate-definition"), [])
        self.assertTrue(report.ok)

    def test_two_mod_files_defining_the_same_name_is_an_error(self) -> None:
        first = self.write("gs.mpq/units/orinf.gs")
        second = self.write("gs.mpq/units/orcav.gs")
        report = self.run_validate(
            manifests={
                "gs.mpq": [base_member("units\\orinf.gs"), base_member("units\\orcav.gs")]
            },
            base_facts={
                "units\\orinf.gs": facts("units\\orinf.gs"),
                "units\\orcav.gs": facts("units\\orcav.gs"),
            },
            mod_facts={
                first: facts(first, definitions={"shared": 1}),
                second: facts(second, definitions={"shared": 1}),
            },
        )
        finding = self.findings(report, "duplicate-definition")[0]
        self.assertEqual(finding.severity, ERROR)
        self.assertIn("in this same", finding.message)

    def test_dropping_a_definition_others_call_is_an_error_that_names_the_callers(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={
                "units\\orinf.gs": facts("units\\orinf.gs", definitions={"orinf": 1}),
                "gs\\army.gs": facts("gs\\army.gs", executables={"orinf": 2}),
            },
            mod_facts={relative: facts(relative, definitions={})},
            vocabulary={"orinf": "script-definition"},
        )
        finding = self.findings(report, "dropped-definition")[0]
        self.assertEqual(finding.severity, ERROR)
        self.assertIn("gs\\army.gs", finding.message)

    def test_dropping_a_definition_nobody_calls_is_only_a_note(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={"units\\orinf.gs": facts("units\\orinf.gs", definitions={"unused": 1})},
            mod_facts={relative: facts(relative, definitions={})},
        )
        finding = self.findings(report, "dropped-definition")[0]
        self.assertEqual(finding.severity, "note")
        self.assertTrue(report.ok)

    def test_base_members_that_do_not_lex_are_counted_not_ignored(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={
                "units\\orinf.gs": facts("units\\orinf.gs"),
                "gs\\broken.gs": facts(
                    "gs\\broken.gs",
                    parse_error={"message": "x", "offset": 0, "line": 1, "column": 1},
                ),
            },
            mod_facts={relative: facts(relative)},
        )
        self.assertEqual(report.coverage.counts["base-members-that-do-not-lex"], 1)


class RunTargetTest(ValidateTestCase):
    def test_a_run_target_that_names_nothing_is_an_error(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={"units\\orinf.gs": facts("units\\orinf.gs")},
            mod_facts={relative: facts(relative, runs=["gs\\absent.gs"])},
        )
        finding = self.findings(report, "missing-run-target")[0]
        self.assertEqual(finding.severity, ERROR)

    def test_a_run_target_resolves_case_insensitively(self) -> None:
        """An MPQ's name hash is case-insensitive; this is the one place case is folded."""
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={
                "gs.mpq": [base_member("units\\orinf.gs"), base_member("gs\\standard.gs")]
            },
            base_facts={"units\\orinf.gs": facts("units\\orinf.gs")},
            mod_facts={relative: facts(relative, runs=["GS/STANDARD.GS"])},
        )
        self.assertEqual(self.findings(report, "missing-run-target"), [])

    def test_a_run_target_the_mod_itself_adds_resolves(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        added = self.write("gs.mpq/units/extra.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={"units\\orinf.gs": facts("units\\orinf.gs")},
            mod_facts={
                relative: facts(relative, runs=["units\\extra.gs"]),
                added: facts(added),
            },
        )
        self.assertEqual(self.findings(report, "missing-run-target"), [])

    def test_an_unresolved_asset_path_is_a_warning_not_an_error(self) -> None:
        """Some literals are assembled at run time, so this cannot be an error."""
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={"units\\orinf.gs": facts("units\\orinf.gs")},
            mod_facts={relative: facts(relative, paths=["LBM\\ABSENT.lbm"])},
        )
        finding = self.findings(report, "missing-asset")[0]
        self.assertEqual(finding.severity, WARNING)
        self.assertTrue(report.ok)


class CoverageTest(ValidateTestCase):
    def test_a_pic_member_carries_the_engine_acceptance_warning(self) -> None:
        relative = self.write("pic.mpq/LBM/ART.lbm", b"\x00")
        report = self.run_validate(
            manifests={"gs.mpq": [], "pic.mpq": [base_member("LBM\\ART.lbm")]},
            mod_facts={},
        )
        finding = self.findings(report, "engine-acceptance")[0]
        self.assertEqual(finding.severity, WARNING)
        self.assertIn("NEVER", finding.message)

    def test_a_binary_member_is_counted_as_unvalidated(self) -> None:
        relative = self.write("pic.mpq/LBM/ART.lbm", b"\x00")
        report = self.run_validate(
            manifests={"gs.mpq": [], "pic.mpq": [base_member("LBM\\ART.lbm")]},
            mod_facts={},
        )
        self.assertEqual(report.coverage.counts["members-with-no-content-validation"], 1)

    def test_an_empty_tree_is_refused_rather_than_passing(self) -> None:
        """A build from a tree that maps nothing would change nothing and report success."""
        report = self.run_validate()
        self.assertFalse(report.ok)
        self.assertEqual(self.findings(report, "tree-shape")[0].severity, ERROR)

    def test_string_literals_not_examined_as_paths_are_counted(self) -> None:
        relative = self.write("gs.mpq/units/orinf.gs")
        report = self.run_validate(
            manifests={"gs.mpq": [base_member("units\\orinf.gs")]},
            base_facts={"units\\orinf.gs": facts("units\\orinf.gs")},
            mod_facts={relative: facts(relative)},
        )
        self.assertEqual(report.coverage.counts["string-literals-not-examined-as-paths"], 2)


if __name__ == "__main__":
    unittest.main()
