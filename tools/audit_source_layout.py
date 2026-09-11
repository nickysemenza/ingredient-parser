"""Compare unverified layout candidates with a pre-existing source inventory.

This measures ingredient/method preservation only, not semantic correctness or
whole-book acceptance. The report includes every inventory recipe, including
those without candidates. It never feeds expected labels into extraction.
"""

import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path


def normalized(text):
    return " ".join(text.split())


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def compare(candidate_report, inventory):
    if candidate_report["epub_sha256"] != inventory["sha256"]:
        raise ValueError("candidate and frozen inventory EPUB hashes differ")
    by_title = defaultdict(list)
    for candidate in candidate_report["candidates"]:
        lines = candidate["region_chunk"]["text"].splitlines()
        selections = candidate["indexed_payload"]["recipes"]
        lowered = (candidate.get("lowered_payload") or {}).get("recipes", [])
        if len(selections) != len(lowered):
            raise ValueError("candidate selections and lowered recipes differ")
        for selection, recipe in zip(selections, lowered):
            titles = selection["title"]
            if not titles:
                raise ValueError("region candidate lacks a source title")
            # Match only a complete first authored title line. Ambiguous or
            # unmatched titles stay unscored; never infer an owner by substring.
            key = (candidate["doc_path"], normalized(lines[titles[0]]).casefold())
            by_title[key].append(recipe)
    results = []
    matched_keys = set()
    for expected in inventory["recipes"]:
        key = (expected["document"], normalized(expected["title"]).casefold())
        found = by_title.get(key, [])
        row = {"document": key[0], "source_title_sha256": digest(key[1])}
        if len(found) != 1:
            row["status"] = "no_candidate" if not found else "ambiguous_candidate_match"
        else:
            matched_keys.add(key)
            recipe = found[0]
            row["fields"] = {}
            for expected_field, candidate_field in [("ingredients", "ingredients"), ("methods", "instructions")]:
                wanted = Counter(normalized(line) for line in expected[expected_field])
                actual = Counter(normalized(line) for section in recipe["sections"] for line in section[candidate_field])
                row["fields"][expected_field] = {
                    "missing": [{"sha256": digest(line), "count": count} for line, count in sorted((wanted - actual).items())],
                    "extra": [{"sha256": digest(line), "count": count} for line, count in sorted((actual - wanted).items())],
                    "expected_count": sum(wanted.values()),
                    "candidate_count": sum(actual.values()),
                }
            row["status"] = "field_inventory_match" if all(not field["missing"] and not field["extra"] for field in row["fields"].values()) else "field_inventory_mismatch"
        results.append(row)
    return {
        "scope": "frozen ingredient/method inventory comparison; no semantic, ownership, variation, headnote, or whole-book certification",
        "verified": False,
        "inventory_recipes": len(results),
        "status_counts": dict(Counter(row["status"] for row in results)),
        "unmatched_candidate_title_groups": len(set(by_title) - matched_keys),
        "results": results,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate_report", type=Path)
    parser.add_argument("frozen_inventory", type=Path)
    args = parser.parse_args()
    candidate_bytes = args.candidate_report.read_bytes()
    inventory_bytes = args.frozen_inventory.read_bytes()
    result = compare(json.loads(candidate_bytes), json.loads(inventory_bytes))
    result["candidate_report_sha256"] = hashlib.sha256(candidate_bytes).hexdigest()
    result["frozen_inventory_sha256"] = hashlib.sha256(inventory_bytes).hexdigest()
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
