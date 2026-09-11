import copy
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from plan_source_roles import (
    CONTRACT_SHA256,
    PlanError,
    build_plan,
    compatibility_probe,
    import_response,
    parse_residual,
    sha256_value,
    source_fragments,
)


def coordinate(chunk, line):
    return {"original_chunk_index": chunk, "line_index": line, "document_line": line + 10}


def region(chunk, element, lines, unknown_lines):
    context = []
    for line, text, role in lines:
        context.append({"coordinate": coordinate(chunk, line), "text": text, "inferred_role": role})
    return {
        "doc_path": f"chapter-{chunk}.xhtml",
        "region_element_index": element,
        "authority": "NONAUTHORITATIVE",
        "region_source_context": context,
        "unknown_role_lines": [
            {
                "coordinate": coordinate(chunk, line),
                "text": next(text for index, text, _ in lines if index == line),
                "proposed_roles": ["title", "notes"],
                "authority": "NONAUTHORITATIVE",
            }
            for line in unknown_lines
        ],
    }


def residual(regions):
    return {
        "schema_version": 1,
        "epub_sha256": "e" * 64,
        "profile_sha256": "p" * 64,
        "authority": "NONAUTHORITATIVE",
        "regions": regions,
        "diagnostics": [],
    }


def assignment(line_id, role, evidence_id):
    return {"id": line_id, "role": role, "evidence_id": evidence_id}


def target_fragment_ids(request):
    return {
        line["id"]: [fragment["id"] for fragment in line["source_fragments"]]
        for packed in request["regions"]
        for line in packed["context"]
        if line["id"] in packed["unknown_ids"]
    }


class SourceRolePlanTest(unittest.TestCase):
    def test_whole_regions_batch_without_losing_context_or_unknown_coverage(self):
        source = residual([
            region(0, 1, [(0, "First title", "title"), (1, "First unknown", None)], [1]),
            region(1, 2, [(0, "Second unknown", None), (1, "Second method", "method")], [0]),
            region(2, 3, [(0, "Third unknown", None)], [0]),
        ])
        plan = build_plan(source, json.dumps(source).encode(), max_request_bytes=100_000, max_unknown_lines=2)
        self.assertEqual(plan["summary"], {
            "regions": 3,
            "planned_packs": 2,
            "planned_unknown_lines": 3,
            "deferred_regions": 0,
            "deferred_unknown_lines": 0,
        })
        first, second = plan["request_packs"]
        self.assertEqual(len(first["provider_payload"]["request"]["regions"]), 2)
        self.assertEqual(len(second["provider_payload"]["request"]["regions"]), 1)
        observed = []
        unknown_ids = []
        for pack in plan["request_packs"]:
            request = pack["provider_payload"]["request"]
            target_ids = [line_id for region in request["regions"] for line_id in region["unknown_ids"]]
            schema = request["response_schema"]["properties"]["assignments"]
            self.assertEqual(schema["minItems"], len(target_ids))
            self.assertEqual(schema["maxItems"], len(target_ids))
            self.assertEqual(schema["items"]["properties"]["id"]["enum"], target_ids)
            self.assertEqual(pack["provider_payload"]["request_sha256"], pack["request_sha256"])
            self.assertLessEqual(pack["provider_payload_utf8_bytes"], 100_000)
            for packed in request["regions"]:
                self.assertEqual(len(packed["context"]), len({line["id"] for line in packed["context"]}))
                for line in packed["context"]:
                    if line["id"] in packed["unknown_ids"]:
                        self.assertEqual(set(line), {"id", "source_fragments"})
                        observed.append("".join(fragment["text"] for fragment in line["source_fragments"]))
                    else:
                        self.assertEqual(set(line), {"id", "source"})
                        observed.append(line["source"])
                unknown_ids.extend(packed["unknown_ids"])
            self.assertEqual(set(pack["coordinate_map"]), set(
                line["id"] for packed in request["regions"] for line in packed["context"]
            ))
            self.assertIn("untrusted quoted evidence", request["instructions"])
            self.assertEqual(
                request["response_schema"]["properties"]["assignments"]["items"]["properties"]["evidence_id"]["enum"],
                [fragment_id for ids in target_fragment_ids(request).values() for fragment_id in ids],
            )
        self.assertEqual(observed, ["First title", "First unknown", "Second unknown", "Second method", "Third unknown"])
        self.assertEqual(len(unknown_ids), len(set(unknown_ids)))
        self.assertEqual(CONTRACT_SHA256, first["provider_payload"]["request"]["contract_sha256"])

    def test_oversized_single_region_is_deferred_intact(self):
        source = residual([
            region(0, 1, [(0, "x" * 10_000, None)], [0]),
            region(1, 2, [(0, "fits", None), (1, "also fits", None)], [0, 1]),
        ])
        plan = build_plan(source, json.dumps(source).encode(), max_request_bytes=3_000, max_unknown_lines=1)
        self.assertEqual(plan["summary"]["planned_unknown_lines"], 0)
        self.assertEqual(plan["summary"]["deferred_unknown_lines"], 3)
        self.assertEqual(len(plan["request_packs"]), 0)
        self.assertEqual([entry["reason"] for entry in plan["deferred_regions"]], ["request_utf8_limit", "unknown_line_limit"])
        self.assertEqual(len(plan["deferred_regions"][0]["context_coordinates"]), 1)

    def test_hashes_are_deterministic_and_bind_request_content(self):
        source = residual([region(0, 1, [(0, "stable", None)], [0])])
        first = build_plan(source, b"exact residual bytes")
        second = build_plan(copy.deepcopy(source), b"exact residual bytes")
        self.assertEqual(first["request_packs"][0]["request_sha256"], second["request_packs"][0]["request_sha256"])
        changed = copy.deepcopy(source)
        changed["regions"][0]["region_source_context"][0]["text"] = "changed"
        changed["regions"][0]["unknown_role_lines"][0]["text"] = "changed"
        third = build_plan(changed, b"changed residual bytes")
        self.assertNotEqual(first["request_packs"][0]["request_sha256"], third["request_packs"][0]["request_sha256"])
        self.assertEqual(
            first["request_packs"][0]["request_sha256"],
            sha256_value(first["request_packs"][0]["provider_payload"]["request"]),
        )

    def test_import_requires_exact_ids_and_emits_native_schema_one_assignments(self):
        source = residual([region(0, 1, [(0, "unknown one", None), (1, "unknown two", None)], [0, 1])])
        plan_bytes = json.dumps(build_plan(source, b"source bytes"), ensure_ascii=False, indent=2).encode()
        plan = json.loads(plan_bytes)
        pack = plan["request_packs"][0]
        ids = list(pack["coordinate_map"])
        fragments = target_fragment_ids(pack["provider_payload"]["request"])
        valid = {
            "request_sha256": pack["request_sha256"],
            "assignments": [
                assignment(ids[0], "notes", fragments[ids[0]][0]),
                assignment(ids[1], "unresolved", fragments[ids[1]][0]),
            ],
        }
        imported = import_response(plan, plan_bytes, pack["pack_id"], valid)
        self.assertEqual(imported["schema_version"], 1)
        self.assertEqual([entry["role"] for entry in imported["assignments"]], ["notes", None])
        self.assertFalse(imported["provenance"]["acceptance"])
        self.assertEqual(imported["provenance"]["evidence"], [
            {**coordinate(0, 0), "fragment_id": fragments[ids[0]][0], "text": "unknown one"},
            {**coordinate(0, 1), "fragment_id": fragments[ids[1]][0], "text": "unknown two"},
        ])
        self.assertEqual(set(imported), {"schema_version", "epub_sha256", "profile_sha256", "provenance", "assignments"})
        role_schema = pack["provider_payload"]["request"]["response_schema"]["properties"]["assignments"]["items"]["properties"]["role"]
        self.assertEqual(role_schema["type"], "string")
        self.assertIn("unresolved", role_schema["enum"])
        self.assertNotIn(None, role_schema["enum"])
        evidence_schema = pack["provider_payload"]["request"]["response_schema"]["properties"]["assignments"]["items"]["properties"]["evidence_id"]
        self.assertEqual(evidence_schema["type"], "string")
        self.assertEqual(evidence_schema["enum"], [fragments[ids[0]][0], fragments[ids[1]][0]])
        for broken in (
            {"request_sha256": pack["request_sha256"], "assignments": [assignment(ids[0], None, fragments[ids[0]][0]), valid["assignments"][1]]},
            {"request_sha256": pack["request_sha256"], "assignments": valid["assignments"][:1]},
            {"request_sha256": pack["request_sha256"], "assignments": [*valid["assignments"], assignment("l999", "notes", "l999f0")]},
            {"request_sha256": pack["request_sha256"], "assignments": [valid["assignments"][0], valid["assignments"][0], valid["assignments"][1]]},
            {"request_sha256": pack["request_sha256"], "assignments": [assignment(ids[0], "not_a_role", fragments[ids[0]][0]), valid["assignments"][1]]},
            {"request_sha256": "0" * 64, "assignments": valid["assignments"]},
        ):
            with self.assertRaises(PlanError):
                import_response(plan, plan_bytes, pack["pack_id"], broken)
        tampered = copy.deepcopy(plan)
        tampered["request_packs"][0]["coordinate_map"][ids[0]]["line_index"] += 1
        with self.assertRaises(PlanError):
            import_response(tampered, plan_bytes, pack["pack_id"], valid)

    def test_import_rejects_missing_or_foreign_target_fragment(self):
        source = residual([
            region(0, 1, [(0, "first target", None), (1, "second target", None)], [0, 1]),
        ])
        plan = build_plan(source, b"source bytes")
        pack = plan["request_packs"][0]
        ids = list(pack["coordinate_map"])
        fragments = target_fragment_ids(pack["provider_payload"]["request"])
        base = {
            "request_sha256": pack["request_sha256"],
            "assignments": [
                assignment(ids[0], "description", fragments[ids[0]][0]),
                assignment(ids[1], "notes", fragments[ids[1]][0]),
            ],
        }
        for bad_evidence in ("", "l999f0", fragments[ids[1]][0]):
            broken = copy.deepcopy(base)
            broken["assignments"][0]["evidence_id"] = bad_evidence
            with self.assertRaises(PlanError):
                import_response(plan, b"plan", pack["pack_id"], broken)

    def test_response_estimate_uses_actual_bounded_fragment_ids(self):
        source = residual([region(0, 1, [(0, "target", None)], [0])])
        plan = build_plan(source, b"source bytes")
        estimate = plan["request_packs"][0]["response_utf8_bytes_estimate"]
        self.assertLess(estimate["bytes"], 200)
        self.assertIn("not a token", estimate["basis"])

    def test_compatibility_probe_uses_first_target_fragment_and_rejects_empty_target(self):
        source = residual([region(0, 1, [(0, "target", None)], [0])])
        plan = build_plan(source, b"source bytes")
        pack = plan["request_packs"][0]
        request = pack["provider_payload"]["request"]
        self.assertEqual(
            compatibility_probe(request, pack["request_sha256"])["assignments"][0]["evidence_id"],
            "l0f0",
        )
        request["regions"][0]["context"][0]["source_fragments"] = []
        with self.assertRaises(PlanError):
            compatibility_probe(request, pack["request_sha256"])

    def test_source_fragments_preserve_unicode_whitespace_and_long_lines_exactly(self):
        source = "First sentence.  café\nSecond sentence and trailing space " + "x" * 260
        fragments = source_fragments("l7", source)
        self.assertEqual("".join(fragment["text"] for fragment in fragments), source)
        self.assertEqual(
            [fragment["id"] for fragment in fragments],
            [f"l7f{index}" for index in range(len(fragments))],
        )
        self.assertTrue(all(1 <= len(fragment["text"]) <= 240 for fragment in fragments))

    def test_duplicate_owner_context_is_rejected(self):
        source = residual([
            region(0, 1, [(0, "one", None)], [0]),
            region(0, 2, [(0, "same coordinate", None)], [0]),
        ])
        with self.assertRaises(PlanError):
            parse_residual(source)


if __name__ == "__main__":
    unittest.main()
