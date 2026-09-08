#!/usr/bin/env python3
"""Compare two JSON outputs from the cookbook evaluator.

The evaluator emits one row per labeled input. This tool compares only those
rows, validates that both runs scored the identical IDs and fields, and emits
stable JSON suitable for a PR description or a later audit. It never reads or
changes labels.
"""

import argparse
import json
from pathlib import Path


FIELDS = ("name", "amounts", "modifier", "optional", "usage")


def load_rows(path: Path):
    document = json.loads(path.read_text())
    rows = document.get("rows")
    if not isinstance(rows, list):
        raise ValueError(f"{path}: evaluator JSON lacks a rows array")
    result = {}
    for row in rows:
        row_id = row.get("id")
        fields = row.get("fields")
        if not isinstance(row_id, str) or not isinstance(fields, dict):
            raise ValueError(f"{path}: every row needs string id and fields object")
        if row_id in result:
            raise ValueError(f"{path}: duplicate row id {row_id}")
        unknown = set(fields) - set(FIELDS)
        missing = set(FIELDS) - set(fields)
        if unknown or missing:
            raise ValueError(
                f"{path}: {row_id}: expected fields {FIELDS}, "
                f"missing={sorted(missing)}, unknown={sorted(unknown)}"
            )
        if any(type(value) is not bool for value in fields.values()):
            raise ValueError(f"{path}: {row_id}: field scores must be booleans")
        result[row_id] = fields
    return result


def compare(before_path: Path, after_path: Path, book_ids=()):
    before = load_rows(before_path)
    after = load_rows(after_path)
    selected = set(book_ids)
    if selected:
        for rows in (before, after):
            missing_books = selected - {row_id.rsplit("-", 1)[0] for row_id in rows}
            if missing_books:
                raise ValueError(f"requested books absent: {sorted(missing_books)}")
        before = {
            row_id: fields
            for row_id, fields in before.items()
            if row_id.rsplit("-", 1)[0] in selected
        }
        after = {
            row_id: fields
            for row_id, fields in after.items()
            if row_id.rsplit("-", 1)[0] in selected
        }
    if set(before) != set(after):
        raise ValueError(
            "evaluator row IDs differ: "
            f"only_before={sorted(set(before) - set(after))}, "
            f"only_after={sorted(set(after) - set(before))}"
        )

    exact_before = {row_id: all(before[row_id].values()) for row_id in before}
    exact_after = {row_id: all(after[row_id].values()) for row_id in after}
    per_field = {}
    for field in FIELDS:
        before_ok = {row_id for row_id in before if before[row_id][field]}
        after_ok = {row_id for row_id in after if after[row_id][field]}
        per_field[field] = {
            "before": len(before_ok),
            "after": len(after_ok),
            "delta": len(after_ok) - len(before_ok),
            "improved": sorted(after_ok - before_ok),
            "regressed": sorted(before_ok - after_ok),
        }
    return {
        "rows": len(before),
        "exact": {
            "before": sum(exact_before.values()),
            "after": sum(exact_after.values()),
            "delta": sum(exact_after.values()) - sum(exact_before.values()),
            "improved": sorted(row_id for row_id in before if exact_after[row_id] and not exact_before[row_id]),
            "regressed": sorted(row_id for row_id in before if exact_before[row_id] and not exact_after[row_id]),
        },
        "per_field": per_field,
        "book_ids": sorted(selected),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument(
        "--book-id",
        action="append",
        default=[],
        help="restrict comparison to this book ID; repeat for a cohort",
    )
    args = parser.parse_args()
    print(json.dumps(compare(args.before, args.after, args.book_id), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
