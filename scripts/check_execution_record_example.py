"""Check the real Rust example/CLI boundary using synthetic integer inputs only.

The supplied JSONL must be produced by the bound_execution_record example.
Temporary output is not an observed model result and is not uploaded as evidence.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("jsonl", type=Path)
    parser.add_argument("prospect", type=Path)
    args = parser.parse_args()
    with args.jsonl.open("rb") as source:
        data = source.read(65537)
    require(len(data) <= 65536, "synthetic example output exceeds bound")
    lines = data.splitlines()
    require(len(lines) == 2, "example must emit input and record JSON lines")
    prospect = args.prospect.resolve()
    with tempfile.TemporaryDirectory(prefix="prospect-record-contract-") as temporary:
        root = Path(temporary)
        bundle, record = root / "input.json", root / "record.json"
        bundle.write_bytes(lines[0])
        record.write_bytes(lines[1])
        argv = [str(prospect), "verify-execution-record", str(record), str(bundle)]
        result = subprocess.run(argv, capture_output=True, timeout=30, check=True)
        require(not result.stderr, "valid report unexpectedly emitted diagnostics")
        summary = json.loads(result.stdout)
        require(summary["state"] == "interrupted", "partial report was promoted")
        require(summary["successful_candidates"] == 1 and summary["never_started_candidates"] == 1,
                "incorrect lifecycle counts")
        require(summary["resume_authorized"] is False, "record unexpectedly authorized resume")
        require(summary["record_sha256"] == hashlib.sha256(lines[1]).hexdigest(), "record digest drift")
        require(summary["bundle_sha256"] == hashlib.sha256(lines[0]).hexdigest(), "input digest drift")
        # Same labels and interventions, different state: the old record must fail.
        changed = json.loads(lines[0])
        changed["state"] = 999
        bundle.write_text(json.dumps(changed, sort_keys=True, separators=(",", ":")), encoding="utf-8")
        rejected = subprocess.run(argv, capture_output=True, timeout=30, check=False)
        require(rejected.returncode == 1 and not rejected.stdout, "changed input reused the old report")
    print("Actual bound Rust producer -> release CLI passed; synthetic control fixture, no model execution")


if __name__ == "__main__":
    main()
