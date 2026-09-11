#!/usr/bin/env python3
"""Create source-anchored evaluation artifacts outside the repository.

The input is a saved candidate run and the output is only the selected chunk's
candidate array. It can also split one exact source chunk at a reviewed line,
retaining the original source identity in each fragment for continuation QA.
It never changes the source run or frozen expectations.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", required=True, type=Path)
    parser.add_argument("--chunk", required=True, type=int)
    parser.add_argument("--recipe")
    parser.add_argument("--mutation", choices=("drop-instruction", "move-instruction-to-description", "append-invented-ingredient", "copy-foreign-ingredient", "invented-recipe"))
    parser.add_argument("--instruction", type=int, default=0)
    parser.add_argument("--from-recipe")
    parser.add_argument("--source-group", action="store_true", help="write a source-derived two-fragment group rather than a candidate mutation")
    parser.add_argument("--split-before-line", type=int, help="zero-based source line beginning the second fragment")
    parser.add_argument("--shared-title", help="exact source title used as the linked group title_hint")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    if args.output.exists():
        raise SystemExit(f"refusing to overwrite candidate evidence: {args.output}")
    run = json.loads(args.run.read_text())
    chunk = run["chunks"][args.chunk]
    candidate = copy.deepcopy(chunk["output"])
    if candidate is None:
        raise SystemExit("selected source chunk has no candidate output")
    if args.source_group:
        if args.mutation or args.recipe or args.from_recipe:
            raise SystemExit("--source-group cannot combine with a candidate mutation")
        if args.split_before_line is None or not args.shared_title:
            raise SystemExit("--source-group requires --split-before-line and --shared-title")
        lines = chunk["source"]["text"].splitlines()
        if not 0 < args.split_before_line < len(lines):
            raise SystemExit("--split-before-line must be inside the exact source chunk")
        if args.shared_title not in lines:
            raise SystemExit("--shared-title must be an exact source line")
        source_sha256 = hashlib.sha256(chunk["source"]["text"].encode()).hexdigest()
        fragments = []
        for suffix, selected_lines, output in (
            ("head", lines[:args.split_before_line], candidate),
            ("tail", lines[args.split_before_line:], []),
        ):
            source = copy.deepcopy(chunk["source"])
            source["text"] = "\n".join(selected_lines)
            # This title is exact source prose. Both fragments must carry it so
            # State::new derives one linked continuation group from source.
            source["title_hint"] = args.shared_title
            fragments.append({
                "original_chunk_index": args.chunk,
                "original_chunk_id": chunk["id"],
                "fragment": suffix,
                "source": source,
                "source_sha256": hashlib.sha256(source["text"].encode()).hexdigest(),
                "candidate_output": output,
            })
        artifact = {
            "schema_version": 1,
            "kind": "source-derived-linked-group",
            "source_run_sha256": hashlib.sha256(args.run.read_bytes()).hexdigest(),
            "original_source_sha256": source_sha256,
            "split_before_line": args.split_before_line,
            "shared_title_source_line": args.shared_title,
            "fragments": fragments,
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(artifact, ensure_ascii=False, indent=2) + "\n")
        print(json.dumps({"source_group_sha256": hashlib.sha256(args.output.read_bytes()).hexdigest(), "source_run_sha256": artifact["source_run_sha256"], "chunk": chunk["id"], "fragments": len(fragments)}, indent=2))
        return
    if not args.recipe or not args.mutation:
        raise SystemExit("--recipe and --mutation are required unless --source-group is used")
    recipe = next((item for item in candidate if item.get("title") == args.recipe), None)
    if recipe is None and args.mutation != "invented-recipe":
        raise SystemExit("selected recipe is absent from the selected source chunk")
    section = None if recipe is None else next((item for item in recipe.get("sections", []) if item.get("instructions")), None)
    if args.mutation == "invented-recipe":
        candidate.append({"title": args.recipe, "description": None, "recipe_yield": None, "sections": [{"ingredients": ["1 cup invented food"], "instructions": ["Invent a method that has no source support."]}]})
    elif args.mutation == "append-invented-ingredient":
        if not recipe.get("sections"):
            raise SystemExit("recipe has no section for invented ingredient")
        recipe["sections"][0].setdefault("ingredients", []).append("1 cup invented food")
    elif args.mutation == "copy-foreign-ingredient":
        if not args.from_recipe:
            raise SystemExit("copy-foreign-ingredient requires --from-recipe")
        owner = next((item for item in candidate if item.get("title") == args.from_recipe), None)
        if owner is None or not owner.get("sections") or not owner["sections"][0].get("ingredients"):
            raise SystemExit("foreign recipe has no available ingredient")
        recipe["sections"][0].setdefault("ingredients", []).append(owner["sections"][0]["ingredients"][0])
    else:
        if section is None or args.instruction >= len(section["instructions"]):
            raise SystemExit("requested method paragraph is absent")
        moved = section["instructions"].pop(args.instruction)
        if args.mutation == "move-instruction-to-description":
            recipe["description"] = "\n".join(part for part in (recipe.get("description"), moved) if part)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(candidate, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"candidate_sha256": hashlib.sha256(args.output.read_bytes()).hexdigest(), "source_run_sha256": hashlib.sha256(args.run.read_bytes()).hexdigest(), "chunk": chunk["id"]}, indent=2))


if __name__ == "__main__":
    main()
