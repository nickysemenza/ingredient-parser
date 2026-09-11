import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

TOOL = Path(__file__).with_name("report_automatic_cohort_envelope.py")
SPEC = importlib.util.spec_from_file_location("envelope", TOOL)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


def prepared(seed=False):
    return {
        "kind": "paid-throughput-evaluation-prepare-v2",
        "dispatch_requested": False,
        "source_identity": {
            "saved_run_sha256": "saved-run",
            "epub_sha256": "epub",
            "selected_source_sha256": "source",
            "document_set_sha256": "documents",
            "catalog_sha256": "catalog",
        },
        "policy": "source-verification-v4",
        "models": ["extract-a", "extract-b"],
        "max_attempts": 2,
        "trial_verifiers": False,
        "concurrency": 4,
        "selected_chunks": [
            {"source_index": index, "original_chunk_index": index, "original_chunk_id": f"chunk-{index}"}
            for index in range(8)
        ],
        "groups": [
            {
                "group": index,
                "source_indices": [index],
                "original_chunk_indices": [index],
                "chunk_count": 1,
                "seeded": seed,
            }
            for index in range(8)
        ],
        "automatic_branches": [
            {
                "extraction_model": "extract-a",
                "extraction_output_limit": 16000,
                "established_verifier_model": "verify-a",
                "established_verifier_output_limit": 16000,
            },
            {
                "extraction_model": "extract-b",
                "extraction_output_limit": 16000,
                "established_verifier_model": "verify-b",
                "established_verifier_output_limit": 16000,
            },
        ],
        "immediate_actions": [
            {
                "key": f"key-{index}",
                "group": index,
                "model": "extract-a",
                "chunk": index,
                "verification_chunk": None,
                "output_limit": 16000,
                "request_bytes": 100 + index,
                "reservation_usd": 0.01,
            }
            for index in range(8)
        ],
        "initial_wave_groups": [0, 1, 2, 3],
        "initial_wave_reservation_usd": 0.04,
    }


def ledger():
    return {
        "buckets": {
            "throughput": {
                "known_usd": 1.0,
                "reserved_usd": 0.5,
                "remaining_usd": 4.0,
            }
        }
    }


class EnvelopeTest(unittest.TestCase):
    def test_accepts_original_indices_with_outside_context(self):
        value = prepared()
        value["context_chunk_count"] = 42
        for index, chunk in enumerate(value["selected_chunks"]):
            chunk["source_index"] = index + 5
            chunk["original_chunk_index"] = index + 5
            value["groups"][index]["source_indices"] = [index + 5]
            value["groups"][index]["original_chunk_indices"] = [index + 5]
        report = MODULE.build_report(value, ledger(), "prepared", "ledger")
        self.assertEqual(report["exact_immediate_actions"]["reservation_usd"], 0.08)
        value["groups"][0]["source_indices"] = [0]
        with self.assertRaises(SystemExit):
            MODULE.build_report(value, ledger(), "prepared", "ledger")

    def test_reports_exact_wave_and_safe_non_usd_branch_bound(self):
        value = MODULE.build_report(prepared(), ledger(), "prepared-hash", "ledger-hash")
        self.assertEqual(value["exact_immediate_actions"]["reservation_usd"], 0.08)
        self.assertEqual(value["exact_immediate_actions"]["first_waves"]["4"]["reservation_usd"], 0.04)
        self.assertEqual(value["exact_immediate_actions"]["first_waves"]["8"]["reservation_usd"], 0.08)
        self.assertEqual(value["exact_immediate_actions"]["repeated_four_run_first_wave_reservation_floor_usd"], 0.24)
        bound = value["safe_branch_bound"]
        self.assertEqual(bound["maximum_extraction_attempts"], 32)
        self.assertEqual(bound["maximum_verification_attempts"], 32)
        self.assertEqual(bound["maximum_action_attempts"], 64)
        self.assertEqual(bound["maximum_provider_output_tokens"], 1024000)
        self.assertIsNone(bound["numeric_total_reservation_usd"])
        self.assertEqual(bound["full_book_envelope_status"], "unknown")
        self.assertEqual(value["bounded_cohort_admission"]["status"], "per-action-only")

    def test_rejects_manual_seed_and_too_small_cohort(self):
        with self.assertRaises(SystemExit):
            MODULE.build_report(prepared(seed=True), ledger(), "prepared", "ledger")
        value = prepared()
        value["groups"] = value["groups"][:7]
        with self.assertRaises(SystemExit):
            MODULE.build_report(value, ledger(), "prepared", "ledger")

    def test_rejects_trial_verifier_or_bad_source_group_mapping(self):
        value = prepared()
        value["trial_verifiers"] = True
        with self.assertRaises(SystemExit):
            MODULE.build_report(value, ledger(), "prepared", "ledger")
        value = prepared()
        value["groups"][0]["original_chunk_indices"] = [99]
        with self.assertRaises(SystemExit):
            MODULE.build_report(value, ledger(), "prepared", "ledger")


if __name__ == "__main__":
    unittest.main()
