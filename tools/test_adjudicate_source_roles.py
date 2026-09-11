import copy
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from adjudicate_source_roles import (
    PlanError,
    build_review_plan,
    import_review_response,
)
from plan_source_roles import build_plan, import_response, sha256_value


def coordinate(chunk, line):
    return {"original_chunk_index": chunk, "line_index": line, "document_line": line + 100}


def region(chunk, count):
    lines = [
        {"coordinate": coordinate(chunk, line), "text": f"Source paragraph {chunk}:{line}.", "inferred_role": None}
        for line in range(count)
    ]
    return {
        "doc_path": f"chapter-{chunk}.xhtml",
        "region_element_index": chunk,
        "authority": "NONAUTHORITATIVE",
        "region_source_context": lines,
        "unknown_role_lines": [
            {
                "coordinate": line["coordinate"],
                "text": line["text"],
                "proposed_roles": ["method", "description"],
                "authority": "NONAUTHORITATIVE",
            }
            for line in lines
        ],
    }


def residual():
    return {
        "schema_version": 1,
        "epub_sha256": "e" * 64,
        "profile_sha256": "p" * 64,
        "authority": "NONAUTHORITATIVE",
        "regions": [region(index, count) for index, count in enumerate([4, 3, 3, 3, 4])],
        "diagnostics": [],
    }


def first_stage(plan, plan_bytes):
    pack = plan["request_packs"][0]
    request = pack["provider_payload"]["request"]
    ids = [line_id for region in request["regions"] for line_id in region["unknown_ids"]]
    # Six method hypotheses across every one of the five owner regions.
    method_indexes = {0, 3, 4, 7, 10, 15}
    fragments = {
        line["id"]: line["source_fragments"][0]["id"]
        for region in request["regions"]
        for line in region["context"]
        if line["id"] in region["unknown_ids"]
    }
    response = {
        "request_sha256": pack["request_sha256"],
        "assignments": [
            {
                "id": line_id,
                "role": "method" if index in method_indexes else "description",
                "evidence_id": fragments[line_id],
            }
            for index, line_id in enumerate(ids)
        ],
    }
    imported = import_response(plan, plan_bytes, pack["pack_id"], response)
    return imported, json.dumps(imported, ensure_ascii=False, indent=2).encode(), ids, method_indexes


class SourceRoleAdjudicationTest(unittest.TestCase):
    def setUp(self):
        self.plan = build_plan(residual(), b"residual bytes")
        self.plan_bytes = json.dumps(self.plan, ensure_ascii=False, indent=2).encode()
        self.assignments, self.assignments_bytes, self.ids, self.method_indexes = first_stage(
            self.plan, self.plan_bytes,
        )
        self.review = build_review_plan(
            self.plan, self.plan_bytes, self.assignments, self.assignments_bytes,
        )
        self.review_bytes = json.dumps(self.review, ensure_ascii=False, indent=2).encode()
        self.pack = self.review["request_packs"][0]
        self.request = self.pack["provider_payload"]["request"]

    def response(self, role="description"):
        return {
            "request_sha256": self.pack["request_sha256"],
            "assignments": [
                {
                    "id": target["id"],
                    "role": role,
                    "evidence_id": f"{target['id']}f0",
                }
                for target in self.request["review_targets"]
            ],
        }

    def test_selects_all_method_hypotheses_with_complete_owner_regions(self):
        targets = self.request["review_targets"]
        self.assertEqual([target["proposed_role"] for target in targets], ["method"] * 6)
        self.assertEqual([target["authority"] for target in targets], ["NONAUTHORITATIVE"] * 6)
        self.assertEqual(self.pack["unknown_line_count"], 6)
        expected = {
            (entry["original_chunk_index"], entry["line_index"], entry["document_line"])
            for entry in self.assignments["assignments"] if entry["role"] == "method"
        }
        selected = {
            tuple(
                self.pack["coordinate_map"][target["id"]][key]
                for key in ("original_chunk_index", "line_index", "document_line")
            )
            for target in targets
        }
        self.assertEqual(selected, expected)
        self.assertEqual(len(self.request["regions"]), 5)
        self.assertEqual(sum(len(region["context"]) for region in self.request["regions"]), 17)
        self.assertEqual(self.review["classification_stage"], "source_role_adjudication")
        self.assertFalse(self.review["first_stage_import"].get("acceptance", False))

    def test_review_coordinate_map_is_limited_to_complete_selected_owners(self):
        narrowed = copy.deepcopy(self.assignments)
        for index, entry in enumerate(narrowed["assignments"]):
            entry["role"] = "method" if index == 0 else "description"
        narrowed_bytes = json.dumps(narrowed, ensure_ascii=False, indent=2).encode()
        review = build_review_plan(self.plan, self.plan_bytes, narrowed, narrowed_bytes)
        request = review["request_packs"][0]["provider_payload"]["request"]
        self.assertEqual(len(request["regions"]), 1)
        self.assertEqual(set(review["request_packs"][0]["coordinate_map"]), {"l0", "l1", "l2", "l3"})

    def test_merge_only_changes_selected_targets_and_records_fragment_provenance(self):
        merged = import_review_response(
            self.review, self.review_bytes, self.pack["pack_id"], self.response(),
        )
        before = self.assignments["assignments"]
        after = merged["assignments"]
        self.assertEqual(len(after), 17)
        selected_coordinates = {
            (entry["original_chunk_index"], entry["line_index"], entry["document_line"])
            for entry in merged["provenance"]["evidence"]
        }
        for old, new in zip(before, after):
            key = (old["original_chunk_index"], old["line_index"], old["document_line"])
            if key in selected_coordinates:
                self.assertEqual(new["role"], "description")
            else:
                self.assertEqual(new, old)
        self.assertFalse(merged["provenance"]["acceptance"])
        self.assertEqual(len(merged["provenance"]["evidence"]), 6)
        self.assertTrue(all(item["text"].startswith("Source paragraph") for item in merged["provenance"]["evidence"]))

    def test_null_review_role_is_preserved_as_unresolved(self):
        merged = import_review_response(
            self.review, self.review_bytes, self.pack["pack_id"], self.response("unresolved"),
        )
        selected = {
            (entry["original_chunk_index"], entry["line_index"], entry["document_line"])
            for entry in merged["provenance"]["evidence"]
        }
        for entry in merged["assignments"]:
            key = (entry["original_chunk_index"], entry["line_index"], entry["document_line"])
            if key in selected:
                self.assertIsNone(entry["role"])

    def test_rejects_stale_first_stage_foreign_fragment_and_missing_review_target(self):
        stale = copy.deepcopy(self.assignments)
        stale["provenance"]["plan_sha256"] = "0" * 64
        with self.assertRaises(PlanError):
            build_review_plan(self.plan, self.plan_bytes, stale, self.assignments_bytes)

        foreign = self.response()
        foreign["assignments"][0]["evidence_id"] = f"{foreign['assignments'][1]['id']}f0"
        with self.assertRaises(PlanError):
            import_review_response(self.review, self.review_bytes, self.pack["pack_id"], foreign)

        missing = self.response()
        missing["assignments"].pop()
        with self.assertRaises(PlanError):
            import_review_response(self.review, self.review_bytes, self.pack["pack_id"], missing)

    def test_rejects_foreign_first_stage_coordinate(self):
        foreign = copy.deepcopy(self.assignments)
        foreign["assignments"][0]["line_index"] = 999
        with self.assertRaises(PlanError):
            build_review_plan(self.plan, self.plan_bytes, foreign, json.dumps(foreign).encode())

    def test_review_request_hash_binds_first_stage_import(self):
        changed = copy.deepcopy(self.review)
        changed["first_stage_import"]["response_sha256"] = "0" * 64
        with self.assertRaises(PlanError):
            import_review_response(changed, self.review_bytes, self.pack["pack_id"], self.response())
        changed = copy.deepcopy(self.review)
        changed["first_stage_assignments"]["assignments"][0]["role"] = "ignored"
        with self.assertRaises(PlanError):
            import_review_response(changed, self.review_bytes, self.pack["pack_id"], self.response())
        self.assertEqual(self.pack["request_sha256"], sha256_value(self.request))


if __name__ == "__main__":
    unittest.main()
