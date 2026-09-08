#!/usr/bin/env python3
import json
import tempfile
import unittest
from pathlib import Path

from compare_cookbook_eval import compare


FIELDS = ("name", "amounts", "modifier", "optional", "usage")


def row(row_id, **overrides):
    fields = {field: True for field in FIELDS}
    fields.update(overrides)
    return {"id": row_id, "fields": fields}


class CompareCookbookEvalTest(unittest.TestCase):
    def write(self, directory, name, rows):
        path = Path(directory, name)
        path.write_text(json.dumps({"rows": rows}))
        return path

    def test_reports_exact_and_field_transitions(self):
        with tempfile.TemporaryDirectory() as directory:
            before = self.write(
                directory,
                "before.json",
                [row("wok-01", name=False), row("wok-02"), row("rintaro-01")],
            )
            after = self.write(
                directory,
                "after.json",
                [row("wok-01"), row("wok-02", modifier=False), row("rintaro-01")],
            )
            result = compare(before, after)
            self.assertEqual(result["rows"], 3)
            self.assertEqual(result["exact"], {
                "before": 2,
                "after": 2,
                "delta": 0,
                "improved": ["wok-01"],
                "regressed": ["wok-02"],
            })
            self.assertEqual(result["per_field"]["name"]["improved"], ["wok-01"])
            self.assertEqual(result["per_field"]["modifier"]["regressed"], ["wok-02"])

    def test_restricts_to_book_cohort(self):
        with tempfile.TemporaryDirectory() as directory:
            before = self.write(directory, "before.json", [row("wok-01", name=False), row("rintaro-01")])
            after = self.write(directory, "after.json", [row("wok-01"), row("rintaro-01", name=False)])
            result = compare(before, after, ["wok"])
            self.assertEqual(result["book_ids"], ["wok"])
            self.assertEqual(result["rows"], 1)
            self.assertEqual(result["exact"]["improved"], ["wok-01"])
            self.assertEqual(result["exact"]["regressed"], [])

    def test_rejects_mismatched_cohort_without_filter(self):
        with tempfile.TemporaryDirectory() as directory:
            before = self.write(directory, "before.json", [row("wok-01")])
            after = self.write(directory, "after.json", [row("rintaro-01")])
            with self.assertRaisesRegex(ValueError, "row IDs differ"):
                compare(before, after)

    def test_hyphenated_books_and_unknown_cohorts(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.write(directory, "run.json", [row("home-kitchen-01"), row("wok-01")])
            self.assertEqual(compare(path, path, ["home-kitchen"])["rows"], 1)
            with self.assertRaisesRegex(ValueError, "requested books absent"):
                compare(path, path, ["unknown"])

    def test_rejects_non_boolean_scores(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.write(directory, "run.json", [row("wok-01", name="false")])
            with self.assertRaisesRegex(ValueError, "must be booleans"):
                compare(path, path)


if __name__ == "__main__":
    unittest.main()
