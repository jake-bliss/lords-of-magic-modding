#!/usr/bin/env python3
"""Compare extracted MPQ trees without modifying the source archives."""

from __future__ import annotations

import argparse
import csv
import hashlib
from collections import Counter
from dataclasses import dataclass
from itertools import combinations
from pathlib import Path

try:
    from .gs_syntax import normalized_bytes
except ImportError:  # Direct execution: python3 tools/compare_trees.py
    from gs_syntax import normalized_bytes


@dataclass(frozen=True)
class FileRecord:
    path: str
    size: int
    sha256: str
    token_sha256: str


@dataclass(frozen=True)
class Difference:
    status: str
    path: str
    base: FileRecord | None
    variant: FileRecord | None


def hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def token_hash(path: Path, raw_hash: str) -> str:
    if path.suffix.casefold() != ".gs":
        return raw_hash
    return hashlib.sha256(normalized_bytes(path.read_bytes())).hexdigest()


def inventory(root: Path) -> dict[str, FileRecord]:
    records: dict[str, FileRecord] = {}
    for path in sorted(candidate for candidate in root.rglob("*") if candidate.is_file()):
        relative = path.relative_to(root).as_posix()
        key = relative.casefold()
        if key in records:
            raise ValueError(
                f"case-insensitive path collision: {records[key].path!r} and {relative!r}"
            )
        raw_hash = hash_file(path)
        records[key] = FileRecord(
            relative, path.stat().st_size, raw_hash, token_hash(path, raw_hash)
        )
    return records


def compare(
    base: dict[str, FileRecord], variant: dict[str, FileRecord]
) -> list[Difference]:
    differences: list[Difference] = []
    shared_keys = base.keys() & variant.keys()
    for key in shared_keys:
        base_record = base[key]
        variant_record = variant[key]
        if base_record.sha256 == variant_record.sha256:
            status = "unchanged"
        elif base_record.token_sha256 == variant_record.token_sha256:
            status = "reformatted"
        else:
            status = "modified"
        differences.append(
            Difference(status, variant_record.path, base_record, variant_record)
        )

    unmatched_base = [base[key] for key in base.keys() - variant.keys()]
    unmatched_variant = [variant[key] for key in variant.keys() - base.keys()]
    base_by_hash: dict[str, list[FileRecord]] = {}
    variant_by_hash: dict[str, list[FileRecord]] = {}
    for record in unmatched_base:
        base_by_hash.setdefault(record.sha256, []).append(record)
    for record in unmatched_variant:
        variant_by_hash.setdefault(record.sha256, []).append(record)

    matched_base: set[str] = set()
    matched_variant: set[str] = set()
    for sha256 in base_by_hash.keys() & variant_by_hash.keys():
        base_records = sorted(base_by_hash[sha256], key=lambda record: record.path)
        variant_records = sorted(variant_by_hash[sha256], key=lambda record: record.path)
        for base_record, variant_record in zip(base_records, variant_records):
            matched_base.add(base_record.path.casefold())
            matched_variant.add(variant_record.path.casefold())
            differences.append(
                Difference("renamed", variant_record.path, base_record, variant_record)
            )

    differences.extend(
        Difference("removed", record.path, record, None)
        for record in unmatched_base
        if record.path.casefold() not in matched_base
    )
    differences.extend(
        Difference("added", record.path, None, record)
        for record in unmatched_variant
        if record.path.casefold() not in matched_variant
    )
    return sorted(differences, key=lambda difference: (difference.status, difference.path))


def write_csv(path: Path, differences: list[Difference]) -> None:
    with path.open("w", newline="", encoding="utf-8") as output:
        writer = csv.writer(output)
        writer.writerow(
            [
                "status",
                "base_path",
                "variant_path",
                "base_size",
                "variant_size",
                "base_sha256",
                "variant_sha256",
                "base_token_sha256",
                "variant_token_sha256",
            ]
        )
        for difference in differences:
            writer.writerow(
                [
                    difference.status,
                    difference.base.path if difference.base else "",
                    difference.variant.path if difference.variant else "",
                    difference.base.size if difference.base else "",
                    difference.variant.size if difference.variant else "",
                    difference.base.sha256 if difference.base else "",
                    difference.variant.sha256 if difference.variant else "",
                    difference.base.token_sha256 if difference.base else "",
                    difference.variant.token_sha256 if difference.variant else "",
                ]
            )


def changed_extension_counts(differences: list[Difference]) -> Counter[str]:
    counts: Counter[str] = Counter()
    for difference in differences:
        if difference.status in {"unchanged", "renamed", "reformatted"}:
            continue
        suffix = Path(difference.path).suffix.lower() or "(none)"
        counts[suffix] += 1
    return counts


def parse_named_path(value: str) -> tuple[str, Path]:
    try:
        label, raw_path = value.split("=", 1)
    except ValueError as error:
        raise argparse.ArgumentTypeError("expected LABEL=PATH") from error
    path = Path(raw_path).expanduser().resolve()
    if not label or not path.is_dir():
        raise argparse.ArgumentTypeError(f"not a directory: {raw_path}")
    return label, path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True, type=parse_named_path)
    parser.add_argument("--variant", required=True, action="append", type=parse_named_path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    base_label, base_path = args.base
    args.output.mkdir(parents=True, exist_ok=True)
    inventories = {base_label: inventory(base_path)}
    for label, path in args.variant:
        if label in inventories:
            parser.error(f"duplicate label: {label}")
        inventories[label] = inventory(path)

    report_rows: list[str] = [
        "# Extracted archive comparison",
        "",
        "Content equality is determined with SHA-256. Paths are matched "
        "case-insensitively to reflect the default macOS filesystem.",
        "",
    ]
    table_rows: list[str] = [
        "| Comparison | Added | Removed | Modified | Layout/comments only | Renamed/catalogued | Unchanged |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    detail_rows: list[str] = []

    labels = [base_label, *(label for label, _ in args.variant)]
    for comparison_base, variant_label in combinations(labels, 2):
        differences = compare(
            inventories[comparison_base], inventories[variant_label]
        )
        counts = Counter(item.status for item in differences)
        filename = f"{variant_label}-vs-{comparison_base}.csv"
        write_csv(args.output / filename, differences)
        table_rows.append(
            f"| {variant_label} vs {comparison_base} | {counts['added']} | "
            f"{counts['removed']} | {counts['modified']} | {counts['reformatted']} | "
            f"{counts['renamed']} | {counts['unchanged']} |"
        )

        detail_rows.extend(
            ["", f"## {variant_label} vs {comparison_base}: content-changing file types", ""]
        )
        extension_counts = changed_extension_counts(differences)
        if extension_counts:
            detail_rows.extend(
                f"- `{extension}`: {count}"
                for extension, count in extension_counts.most_common()
            )
        else:
            detail_rows.append("None.")

    report_rows.extend(table_rows)
    report_rows.extend(detail_rows)
    (args.output / "summary.md").write_text(
        "\n".join(report_rows) + "\n", encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
