import unittest

from audit_source_layout import compare


class SourceLayoutAuditTest(unittest.TestCase):
    def test_missing_method_in_notes_and_deferred_recipe_are_not_passes(self):
        inventory = {"sha256": "book", "recipes": [
            {"document": "chapter", "title": "Example Soup", "ingredients": ["1 cup water"], "methods": ["Stir.", "Cover."]},
            {"document": "chapter", "title": "Other Soup", "ingredients": [], "methods": ["Serve."]},
        ]}
        candidates = {"epub_sha256": "book", "candidates": [{
            "doc_path": "chapter", "region_chunk": {"text": "Example Soup\n1 cup water\nStir.\nCover."},
            "indexed_payload": {"recipes": [{"title": [0]}]},
            "lowered_payload": {"recipes": [{"sections": [{"ingredients": ["1 cup water"], "instructions": ["Stir."]}], "notes": ["Cover."]}]},
        }]}
        result = compare(candidates, inventory)
        self.assertEqual(result["status_counts"], {"field_inventory_mismatch": 1, "no_candidate": 1})
        self.assertEqual(result["results"][0]["fields"]["methods"]["missing"][0]["count"], 1)
        self.assertFalse(result["verified"])

    def test_mismatched_book_is_rejected(self):
        with self.assertRaises(ValueError):
            compare({"epub_sha256": "other"}, {"sha256": "book"})


if __name__ == "__main__":
    unittest.main()
