"""Qualify software journal persistence, never model execution or automatic resume."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def invoke(argv, code=0):
    result = subprocess.run([str(value) for value in argv], capture_output=True, text=True, timeout=30)
    if result.returncode != code:
        raise RuntimeError(f"unexpected exit {result.returncode}, expected {code}: {result.stderr[:4000]}")
    if code not in (0, 23) and result.stdout:
        raise AssertionError("rejected inspection emitted stdout")
    return result


def inspect(prospect, journal, bundle):
    before = hashlib.sha256(journal.read_bytes()).hexdigest()
    summary = json.loads(invoke([prospect, "inspect-execution-journal", journal, bundle]).stdout)
    assert summary["journal_sha256"] == before
    assert hashlib.sha256(journal.read_bytes()).hexdigest() == before, "inspection modified the journal"
    assert summary["resume_authorized"] is False
    assert summary["evidence_kind"] == "journal_consistency_only"
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prospect", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    prospect, fixture = args.prospect.resolve(), args.fixture.resolve()
    if len(args.revision) != 40 or any(c not in "0123456789abcdef" for c in args.revision):
        raise ValueError("expected an exact lower-case Git revision")
    fixture_digest = hashlib.sha256(fixture.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="prospect-journal-interop-") as directory:
        root = Path(directory)
        for label, flag, exit_code, expected in [
            ("complete", None, 0, "completed"),
            ("failure", "--fail-second", 0, "failed"),
            ("abrupt", "--exit-during-second", 23, "unknown_call_result"),
        ]:
            journal, bundle = root / f"{label}.jsonl", root / f"{label}.bundle.json"
            command = [fixture, journal, bundle, args.revision]
            if flag:
                command.append(flag)
            invoke(command, exit_code)
            summary = inspect(prospect, journal, bundle)
            assert summary["state"] == expected
            assert summary["implementation"]["artifact_sha256"] == fixture_digest
            assert summary["implementation"]["revision"] == args.revision
            if label == "abrupt":
                assert summary["unknown_call_result"] == {"kind": "scenario", "id": "s2"}
                assert summary["successful_candidates"] == 1
                assert summary["never_started_candidates"] == 1
                assert summary["terminal_recorded"] is False
                assert summary["verified_entries"] == 6
            elif label == "failure":
                assert summary["failed_call"] == {"kind": "scenario", "id": "s2"}
                assert summary["successful_candidates"] == 1
                assert summary["terminal_recorded"] is True
            else:
                assert summary["successful_candidates"] == 3
                assert summary["terminal_recorded"] is True
            print(f"{label}: exact call lifecycle and no-resume classification verified")

        complete = root / "complete.jsonl"
        bundle = root / "complete.bundle.json"
        partial = root / "partial-tail.jsonl"
        partial.write_bytes(complete.read_bytes()[:-1])
        summary = inspect(prospect, partial, bundle)
        assert summary["state"] == "incomplete_tail" and summary["incomplete_tail_bytes"] > 0
        assert summary["terminal_recorded"] is False and summary["successful_candidates"] == 3

        altered = root / "altered.bundle.json"
        value = json.loads(bundle.read_text())
        value["state"] = 11
        altered.write_text(json.dumps(value, sort_keys=True, separators=(",", ":")))
        invoke([prospect, "inspect-execution-journal", complete, altered], 1)

        broken = root / "broken.jsonl"
        lines = complete.read_bytes().splitlines(keepends=True)
        broken.write_bytes(b"".join(lines[:3] + lines[4:]))
        invoke([prospect, "inspect-execution-journal", broken, bundle], 1)
        broken.write_bytes(complete.read_bytes() + b"x")
        invoke([prospect, "inspect-execution-journal", broken, bundle], 1)
        broken.write_bytes((root / "abrupt.jsonl").read_bytes() + b"{broken}\n")
        invoke([prospect, "inspect-execution-journal", broken, root / "abrupt.bundle.json"], 1)
        link = root / "linked.jsonl"
        link.symlink_to(complete)
        invoke([prospect, "inspect-execution-journal", link, bundle], 1)
        original = complete.read_bytes()
        invoke([fixture, complete, root / "new.bundle.json", args.revision], 1)
        assert complete.read_bytes() == original, "existing journal was overwritten"
        print("torn tail, changed input, deleted event, malformed event, final tail, link and no-clobber checks passed")
    print("Software/process persistence only; no engine replay, GPU evidence or power-loss guarantee.")


if __name__ == "__main__":
    main()
