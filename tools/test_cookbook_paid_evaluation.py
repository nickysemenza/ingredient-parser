import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


TOOL = Path(__file__).with_name("cookbook_paid_evaluation.py")


def initial_ledger(path):
    path.write_text(json.dumps({"buckets": {name: {"cap_usd": 1.0, "known_usd": 0.0, "reserved_usd": 0.0, "remaining_usd": 1.0} for name in ("verifier", "throughput")}, "entries": []}))


class LedgerTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.ledger = Path(self.directory.name) / "ledger.json"
        initial_ledger(self.ledger)

    def tearDown(self):
        self.directory.cleanup()

    def invoke(self, *args, ok=True):
        result = subprocess.run([sys.executable, "-B", str(TOOL), "ledger", "--ledger", str(self.ledger), *args], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0 if ok else 1, result.stderr)
        return result

    def test_duplicate_nonfinite_and_cross_bucket_are_rejected(self):
        self.invoke("--action", "reserve", "--bucket", "verifier", "--entry", "one", "--amount", "0.4")
        self.invoke("--action", "reserve", "--bucket", "verifier", "--entry", "one", "--amount", "0.1", ok=False)
        self.invoke("--action", "reserve", "--bucket", "verifier", "--entry", "nan", "--amount", "nan", ok=False)
        self.invoke("--action", "settle", "--bucket", "throughput", "--entry", "one", "--amount", "0.1", ok=False)
        value = json.loads(self.ledger.read_text())
        self.assertEqual(value["buckets"]["verifier"]["reserved_usd"], 0.4)
        self.assertEqual(value["buckets"]["throughput"]["reserved_usd"], 0.0)

    def test_zero_known_charge_releases_reservation(self):
        self.invoke("--action", "reserve", "--bucket", "verifier", "--entry", "free", "--amount", "0.4")
        self.invoke("--action", "settle", "--bucket", "verifier", "--entry", "free", "--amount", "0")
        value = json.loads(self.ledger.read_text())
        self.assertEqual(value["buckets"]["verifier"]["reserved_usd"], 0.0)
        self.assertEqual(value["buckets"]["verifier"]["remaining_usd"], 1.0)

    def test_concurrent_admission_is_serialized_and_unknown_charge_stays_held(self):
        commands = [
            [sys.executable, "-B", str(TOOL), "ledger", "--ledger", str(self.ledger), "--action", "reserve", "--bucket", "throughput", "--entry", entry, "--amount", "0.7"]
            for entry in ("first", "second")
        ]
        processes = [subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) for command in commands]
        results = [process.communicate() for process in processes]
        self.assertEqual(sorted(process.returncode for process in processes), [0, 1], results)
        value = json.loads(self.ledger.read_text())
        held = value["entries"][0]
        self.invoke("--action", "unknown", "--bucket", "throughput", "--entry", held["id"], "--amount", "0.7")
        value = json.loads(self.ledger.read_text())
        self.assertEqual(value["buckets"]["throughput"]["reserved_usd"], 0.7)
        self.assertEqual(value["entries"][0]["status"], "unknown")
