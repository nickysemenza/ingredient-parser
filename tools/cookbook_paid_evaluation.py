#!/usr/bin/env python3
"""Durably account for bounded cookbook evaluation calls.

This generic tool deliberately has no cookbook, source text, expected recipe,
mutation, or provider knowledge. Private evaluation manifests own those
inputs. A caller reserves before dispatch, settles in its original bucket, and
leaves unknown charges held.
"""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import math
import os
import tempfile
from datetime import datetime, timezone
from pathlib import Path


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_json(path: Path) -> dict:
    return json.loads(path.read_text())


def write_json(path: Path, value: dict) -> None:
    """Atomically replace JSON and persist the file and directory entries."""
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(value, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def update_ledger(args: argparse.Namespace) -> None:
    path = Path(args.ledger)
    amount = float(args.amount)
    if not math.isfinite(amount) or amount < 0:
        raise SystemExit("amount must be finite and non-negative")
    amount = round(amount, 8)
    lock_path = path.with_suffix(path.suffix + ".lock")
    with lock_path.open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            ledger = read_json(path)
            bucket = ledger["buckets"][args.bucket]
            existing = next((e for e in ledger["entries"] if e["id"] == args.entry), None)
            if args.action == "reserve":
                if amount <= 0:
                    raise SystemExit("reservation must be positive")
                if existing is not None:
                    raise SystemExit("reservation entry id already exists")
                if amount > bucket["remaining_usd"] + 1e-9:
                    raise SystemExit(f"{args.bucket} cap would be exceeded")
                bucket["reserved_usd"] = round(bucket["reserved_usd"] + amount, 8)
                bucket["remaining_usd"] = round(bucket["remaining_usd"] - amount, 8)
                ledger["entries"].append({"id": args.entry, "bucket": args.bucket, "reserved_usd": amount, "status": "reserved", "reserved_at": datetime.now(timezone.utc).isoformat(), "purpose": args.purpose})
            else:
                if existing is None or existing["status"] != "reserved":
                    raise SystemExit(f"{args.action} requires an existing reserved entry")
                if existing["bucket"] != args.bucket:
                    raise SystemExit("settlement bucket must match the reservation bucket")
                held = existing["reserved_usd"]
                if args.action == "settle":
                    if amount > held + 1e-9:
                        raise SystemExit("known charge exceeds its conservative reservation")
                    bucket["reserved_usd"] = round(bucket["reserved_usd"] - held, 8)
                    bucket["known_usd"] = round(bucket["known_usd"] + amount, 8)
                    bucket["remaining_usd"] = round(bucket["remaining_usd"] + held - amount, 8)
                    existing.update({"status": "settled", "known_usd": amount, "settled_at": datetime.now(timezone.utc).isoformat()})
                else:
                    if amount != held:
                        raise SystemExit("unknown charge must retain the full original reservation")
                    existing.update({"status": "unknown", "unknown_at": datetime.now(timezone.utc).isoformat()})
            write_json(path, ledger)
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
    print(digest(path))


def main() -> None:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(required=True)
    ledger = commands.add_parser("ledger")
    ledger.add_argument("--ledger", required=True)
    ledger.add_argument("--action", choices=("reserve", "settle", "unknown"), required=True)
    ledger.add_argument("--bucket", choices=("verifier", "throughput"), required=True)
    ledger.add_argument("--entry", required=True)
    ledger.add_argument("--amount", required=True)
    ledger.add_argument("--purpose", default="")
    ledger.set_defaults(func=update_ledger)
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
