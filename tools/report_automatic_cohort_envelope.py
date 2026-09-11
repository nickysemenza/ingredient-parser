#!/usr/bin/env python3
"""Report a read-only conservative envelope for an automatic cold-cache cohort.

The prepared JSON must come from the native throughput harness in prepare-only
mode.  This tool never reserves money, opens a provider connection, writes a
cache, or reads cookbook prose.  It reports exact immediate Action
reservations and a safe structural bound for later recovery actions.

A numeric all-branch USD total is deliberately absent unless the engine has an
enforced byte cap for generated candidates and feedback.  The recovery action
reservation uses serialized request bytes; an output-token limit alone cannot
bound arbitrary UTF-8 candidate or finding strings in that formula.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
from typing import Any


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_object(path: Path, name: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"{name} is not readable JSON: {error}") from error
    if not isinstance(value, dict):
        raise SystemExit(f"{name} must be a JSON object")
    return value


def finite(value: Any, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise SystemExit(f"{name} must be finite")
    return float(value)


def nonnegative(value: Any, name: str) -> float:
    parsed = finite(value, name)
    if parsed < 0:
        raise SystemExit(f"{name} must be non-negative")
    return parsed


def integer(value: Any, name: str, minimum: int = 0) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise SystemExit(f"{name} must be an integer >= {minimum}")
    return value


def object_list(value: Any, name: str) -> list[dict[str, Any]]:
    if not isinstance(value, list) or any(not isinstance(item, dict) for item in value):
        raise SystemExit(f"{name} must be an array of objects")
    return value


def action_entry(action: dict[str, Any], index: int) -> dict[str, Any]:
    key = action.get("key")
    model = action.get("model")
    # The native preparation manifest intentionally preserves the recovery
    # Action shape. A populated `chunk` is extraction; a verifier action has
    # no extraction chunk and instead names `verification_chunk`.
    chunk = action.get("chunk")
    verification_chunk = action.get("verification_chunk")
    if chunk is not None and verification_chunk is not None:
        raise SystemExit(f"immediate_actions[{index}] cannot be extraction and verification")
    operation = "extract" if chunk is not None else "verify" if verification_chunk is not None else None
    group = action.get("group")
    if not isinstance(key, str) or not key or not isinstance(model, str) or not model:
        raise SystemExit(f"immediate_actions[{index}] needs nonempty key and model")
    if operation not in ("extract", "verify"):
        raise SystemExit(f"immediate_actions[{index}].operation must be extract or verify")
    return {
        "key": key,
        "group": integer(group, f"immediate_actions[{index}].group"),
        "model": model,
        "operation": operation,
        "output_limit": integer(action.get("output_limit"), f"immediate_actions[{index}].output_limit", 1),
        "request_bytes": integer(action.get("request_bytes"), f"immediate_actions[{index}].request_bytes"),
        "reservation_usd": nonnegative(action.get("reservation_usd"), f"immediate_actions[{index}].reservation_usd"),
    }


def first_wave(actions: list[dict[str, Any]], concurrency: int) -> dict[str, Any]:
    chosen: list[dict[str, Any]] = []
    groups: set[int] = set()
    for action in actions:
        if action["group"] in groups:
            continue
        groups.add(action["group"])
        chosen.append(action)
        if len(chosen) == concurrency:
            break
    return {
        "requested_concurrency": concurrency,
        "distinct_ready_groups": len(chosen),
        "reservation_usd": round(sum(item["reservation_usd"] for item in chosen), 8),
        "action_keys": [item["key"] for item in chosen],
    }


def model_limits(prepared: dict[str, Any], models: list[str]) -> tuple[dict[str, dict[str, int]], dict[str, list[str]]] | None:
    branches = prepared.get("automatic_branches")
    if branches is None:
        # v1 emits only exact, immediate Actions. It does not make up a
        # future verifier request before a candidate exists.
        return None
    branch_entries = object_list(branches, "automatic_branches")
    parsed_limits: dict[str, dict[str, int]] = {}
    parsed_verifiers: dict[str, list[str]] = {}
    for index, value in enumerate(branch_entries):
        model = value.get("extraction_model")
        verifier = value.get("established_verifier_model")
        if not isinstance(model, str) or not model or not isinstance(verifier, str) or not verifier:
            raise SystemExit(f"automatic_branches[{index}] needs extraction_model and established_verifier_model")
        if model in parsed_verifiers:
            raise SystemExit(f"automatic_branches repeats {model}")
        parsed_limits[model] = {
            "extract": integer(value.get("extraction_output_limit"), f"automatic_branches[{index}].extraction_output_limit", 1),
            "verify": 0,
        }
        parsed_verifiers[model] = [verifier]
        parsed_limits[verifier] = {
            "extract": 0,
            "verify": integer(value.get("established_verifier_output_limit"), f"automatic_branches[{index}].established_verifier_output_limit", 1),
        }
    if set(parsed_verifiers) != set(models):
        raise SystemExit("automatic_branches must cover each configured automatic extraction model exactly once")
    return parsed_limits, parsed_verifiers


def branch_token_ceiling(
    groups: list[dict[str, Any]],
    models: list[str],
    max_attempts: int,
    limits: dict[str, dict[str, int]],
    verifiers: dict[str, list[str]],
) -> dict[str, Any]:
    extraction_actions = 0
    verification_actions = 0
    output_tokens = 0
    for group in groups:
        chunks = integer(group.get("selected_chunk_count"), "groups[].selected_chunk_count", 1)
        for model in models:
            extraction_actions += chunks * max_attempts
            output_tokens += chunks * max_attempts * limits[model]["extract"]
            # Default production has one established verifier per candidate
            # family. The calculation still accepts an explicit future list.
            for verifier in verifiers[model]:
                verification_actions += chunks * max_attempts
                output_tokens += chunks * max_attempts * integer(
                    limits.get(verifier, {}).get("verify"),
                    f"operation_output_limits.{verifier}.verify",
                    1,
                )
    return {
        "maximum_extraction_attempts": extraction_actions,
        "maximum_verification_attempts": verification_actions,
        "maximum_action_attempts": extraction_actions + verification_actions,
        "maximum_provider_output_tokens": output_tokens,
        "basis": "Every automatic candidate model, every selected source chunk, every established verifier stage, and MAX_ATTEMPTS per Action.",
    }


def build_report(prepared: dict[str, Any], ledger: dict[str, Any], prepared_sha: str, ledger_sha: str) -> dict[str, Any]:
    if prepared.get("kind") not in ("paid-throughput-evaluation-prepare-v1", "paid-throughput-evaluation-prepare-v2"):
        raise SystemExit("unexpected prepare manifest kind")
    if prepared.get("dispatch_requested") is not False:
        raise SystemExit("envelope input must be a prepare-only manifest")
    source_identity = prepared.get("source_identity")
    if not isinstance(source_identity, dict):
        raise SystemExit("prepare report needs source_identity")
    policy = prepared.get("policy")
    models = prepared.get("models")
    if not isinstance(policy, str) or not isinstance(models, list) or not models or any(not isinstance(m, str) or not m for m in models):
        raise SystemExit("prepare report needs policy and nonempty models")
    # v1's runner is hard-wired to `set_trial_verifiers(false)`; v2 makes the
    # assertion visible in the artifact. A token ceiling depends on the v2
    # proof that no opt-in verifier stages are in the branch.
    if prepared.get("kind") == "paid-throughput-evaluation-prepare-v2" and prepared.get("trial_verifiers") is not False:
        raise SystemExit("automatic cohort report requires trial_verifiers=false")
    max_attempts = integer(prepared.get("max_attempts"), "max_attempts", 1)
    groups = object_list(prepared.get("groups"), "groups")
    if len(groups) < 8:
        raise SystemExit("automatic paired 4/8 cohorts require at least eight recovery groups")
    selected_chunks = object_list(prepared.get("selected_chunks"), "selected_chunks")
    original_chunks = [
        integer(item.get("original_chunk_index"), f"selected_chunks[{index}].original_chunk_index")
        for index, item in enumerate(selected_chunks)
    ]
    source_indices = [
        integer(item.get("source_index"), f"selected_chunks[{index}].source_index")
        for index, item in enumerate(selected_chunks)
    ]
    if len(original_chunks) < 8:
        raise SystemExit("selected_chunks needs at least eight original_chunk_index values")
    if len(set(original_chunks)) != len(original_chunks) or len(set(source_indices)) != len(source_indices):
        raise SystemExit("selected_chunks must have unique source and original indices")
    original_by_source = dict(zip(source_indices, original_chunks))
    if any(group.get("seeded") is not False for group in groups):
        raise SystemExit("automatic cohort cannot include manually seeded groups")

    normalized_groups = []
    for index, group in enumerate(groups):
        group_source_indices = group.get("source_indices")
        group_original_indices = group.get("original_chunk_indices")
        if not isinstance(group_source_indices, list) or not isinstance(group_original_indices, list):
            raise SystemExit(f"groups[{index}] needs source and original chunk mappings")
        parsed_source_indices = [
            integer(value, f"groups[{index}].source_indices[]") for value in group_source_indices
        ]
        parsed_original_indices = [
            integer(value, f"groups[{index}].original_chunk_indices[]") for value in group_original_indices
        ]
        chunk_count = integer(group.get("chunk_count"), f"groups[{index}].chunk_count", 1)
        if len(parsed_source_indices) != chunk_count or len(parsed_original_indices) != chunk_count:
            raise SystemExit(f"groups[{index}] chunk_count does not match its mappings")
        if any(source_index not in original_by_source for source_index in parsed_source_indices):
            raise SystemExit(f"groups[{index}] refers outside selected_chunks")
        if [original_by_source[source_index] for source_index in parsed_source_indices] != parsed_original_indices:
            raise SystemExit(f"groups[{index}] source and original mappings disagree")
        normalized_groups.append({
            "group": integer(group.get("group"), f"groups[{index}].group"),
            "selected_chunk_count": chunk_count,
            "seeded": False,
        })
    if len({group["group"] for group in normalized_groups}) != len(normalized_groups):
        raise SystemExit("groups must have unique group IDs")
    if sorted(source_index for group in groups for source_index in group["source_indices"]) != sorted(source_indices):
        raise SystemExit("groups must cover every selected source chunk exactly once")
    limits_and_verifiers = model_limits(prepared, models)
    actions = [action_entry(action, index) for index, action in enumerate(object_list(prepared.get("immediate_actions"), "immediate_actions"))]
    if not actions:
        raise SystemExit("prepare report has no immediate actions")
    group_ids = {group["group"] for group in normalized_groups}
    if any(action["group"] not in group_ids for action in actions):
        raise SystemExit("immediate Action refers to an unknown group")

    buckets = ledger.get("buckets")
    if not isinstance(buckets, dict) or not isinstance(buckets.get("throughput"), dict):
        raise SystemExit("ledger needs buckets.throughput")
    throughput = buckets["throughput"]
    remaining = nonnegative(throughput.get("remaining_usd"), "ledger.buckets.throughput.remaining_usd")
    known = nonnegative(throughput.get("known_usd"), "ledger.buckets.throughput.known_usd")
    held = nonnegative(throughput.get("reserved_usd"), "ledger.buckets.throughput.reserved_usd")

    waves = {str(concurrency): first_wave(actions, concurrency) for concurrency in (4, 8)}
    manifest_initial_groups = prepared.get("initial_wave_groups")
    manifest_initial_reservation = prepared.get("initial_wave_reservation_usd")
    if not isinstance(manifest_initial_groups, list) or any(not isinstance(group, int) for group in manifest_initial_groups):
        raise SystemExit("prepare report needs initial_wave_groups")
    if manifest_initial_groups != [action["group"] for action in actions[:len(manifest_initial_groups)]]:
        raise SystemExit("initial_wave_groups does not match immediate Action order")
    configured_concurrency = integer(prepared.get("concurrency"), "concurrency", 1)
    if abs(nonnegative(manifest_initial_reservation, "initial_wave_reservation_usd") - first_wave(actions, configured_concurrency)["reservation_usd"]) > 1e-8:
        raise SystemExit("initial_wave_reservation_usd does not match immediate Actions")
    for wave in waves.values():
        wave["fits_current_remaining_usd"] = wave["reservation_usd"] <= remaining + 1e-9
    repeated_four_run_first_wave_floor = round(
        2 * waves["4"]["reservation_usd"] + 2 * waves["8"]["reservation_usd"], 8
    )

    return {
        "schema_version": 1,
        "kind": "automatic-cohort-reservation-envelope-v1",
        "read_only": True,
        "authorization": "This report does not reserve, dispatch, settle, or authorize a paid call.",
        "inputs": {
            "prepare_sha256": prepared_sha,
            "ledger_sha256": ledger_sha,
            "policy": policy,
            "models": models,
            "max_attempts_per_action": max_attempts,
            "source_identity": source_identity,
            "original_chunk_indices": original_chunks,
            "groups": normalized_groups,
        },
        "ledger_snapshot": {
            "bucket": "throughput",
            "known_usd": known,
            "unknown_or_pending_held_usd": held,
            "remaining_usd": remaining,
        },
        "exact_immediate_actions": {
            "count": len(actions),
            "reservation_usd": round(sum(item["reservation_usd"] for item in actions), 8),
            "actions": actions,
            "first_waves": waves,
            "repeated_four_run_first_wave_reservation_floor_usd": repeated_four_run_first_wave_floor,
            "repeated_four_run_basis": "Two cold c4 and two cold c8 runs of this identical cohort. This is only the minimum reservation required to start every first wave, not a run or full-book total.",
        },
        "safe_branch_bound": {
            **(
                branch_token_ceiling(normalized_groups, models, max_attempts, *limits_and_verifiers)
                if limits_and_verifiers is not None
                else {
                    "maximum_extraction_attempts": None,
                    "maximum_verification_attempts": None,
                    "maximum_action_attempts": None,
                    "maximum_provider_output_tokens": None,
                    "basis": "The v1 manifest supplies exact immediate Actions but no complete automatic-model/verifier output-cap map.",
                }
            ),
            "numeric_total_reservation_usd": None,
            "full_book_envelope_status": "unknown",
            "reason": (
                "The current recovery contract has no enforced maximum serialized byte size for "
                "candidate outputs or provider finding text. Future verifier Action reservations "
                "depend on those bytes, so a token output cap alone cannot soundly bound their "
                "input reservation in USD."
            ),
            "safe_bound_exposed": (
                "maximum provider output tokens and Action attempts only"
                if limits_and_verifiers is not None
                else "exact immediate Action reservations only; later Action token ceiling unavailable from this manifest"
            ),
        },
        "bounded_cohort_admission": {
            "status": "per-action-only",
            "first_c4_wave_fits_ledger_snapshot": waves["4"]["fits_current_remaining_usd"],
            "first_c8_wave_fits_ledger_snapshot": waves["8"]["fits_current_remaining_usd"],
            "rule": "A bounded cohort may reserve each exact next Action and stop when the ledger rejects it. A budget-constrained or incomplete run remains evidence but cannot advance the concurrency speed gate.",
        },
        "cohort_gate": {
            "paired_cold_cache_requirement": "Run identical current-source cohort once at 4 and once at 8 only with isolated empty caches and new checkpoint prefixes.",
            "default_change_requirement": "Two matched complete observations per cohort A and B, at least 20% lower 8-way median, unchanged quality, no more failures, and no higher cost per verified recipe.",
            "current_report_limit": "This report prechecks only the immediately ready wave. Every later Action must still be ledger-reserved immediately before dispatch. A budget-constrained or incomplete run is diagnostic and cannot advance the speed gate.",
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--prepared", required=True, type=Path, help="prepare-only native throughput JSON")
    parser.add_argument("--ledger", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    if args.out.exists():
        raise SystemExit("refusing to overwrite envelope report")
    prepared = read_object(args.prepared, "prepared report")
    ledger = read_object(args.ledger, "ledger")
    report = build_report(prepared, ledger, sha256(args.prepared), sha256(args.ledger))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"report": str(args.out), "sha256": sha256(args.out)}, indent=2))


if __name__ == "__main__":
    main()
