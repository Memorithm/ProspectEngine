"""Exercise the real restart CLI against real fixture files; no model execution.

The harness controls the writer, binary, declared revision and fixture metadata.
It retains expectations BEFORE modifying copies. This is not a production command
for deriving trusted expectations from an arbitrary untrusted journal.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def run(args, expected=0):
    result = subprocess.run([str(arg) for arg in args], stdin=subprocess.DEVNULL,
                            capture_output=True, timeout=30)
    if result.returncode != expected:
        raise AssertionError((args, result.returncode, result.stdout, result.stderr))
    if expected:
        assert not result.stdout, result.stdout
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prospect", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    prospect, fixture = args.prospect.resolve(), args.fixture.resolve()
    # Read the actual test executable separately; no value comes from the log header.
    with fixture.open("rb") as stream:
        binary_sha = hashlib.file_digest(stream, "sha256").hexdigest()
    adapter = dict(adapter_id="example.journal", contract_version=dict(major=1, minor=0),
                   upstream=None, capabilities=[dict(id="example.evaluate", version=dict(major=1, minor=0))])
    with tempfile.TemporaryDirectory(prefix="prospect-restart-") as temporary:
        root = Path(temporary)
        for label, mode, writer_exit, expected_blocker in (
            ("quota", "--quota-one", 0, None),
            ("crashed", "--exit-during-second", 23, "unknown_call_result"),
            ("failed", "--fail-second", 0, "failed_call_requires_reconciliation"),
            ("complete", "", 0, "already_completed"),
        ):
            case = root / label
            case.mkdir()
            journal, bundle, trusted = [case / name for name in ("run.jsonl", "bundle.json", "trusted.json")]
            command = [fixture, journal, bundle, args.revision] + ([mode] if mode else [])
            run(command, writer_exit)
            original_journal, original_bundle = journal.read_bytes(), bundle.read_bytes()
            expected = dict(
                schema="prospect.restart-expectations/v1",
                anchors=dict(run_id="fixture-run", journal_sha256=sha(original_journal),
                             bundle_sha256=sha(original_bundle), adapter_sha256=sha(canonical(adapter))),
                implementation=dict(component="example.journal_fixture", revision=args.revision,
                                    artifact_sha256=binary_sha),
                codecs=dict(signature="example.i32.v1", error="example.error.v1"),
                semantics="pure_independent",
            )
            # This controlled harness is the external trust source for this fixture.
            trusted.write_bytes(canonical(expected))
            command = [prospect, "preflight-execution-restart", journal, bundle, trusted, fixture]
            summary = json.loads(run(command).stdout)
            assert summary["resume_authorized"] is False
            assert summary["artifact_sha256"] == binary_sha
            assert summary["expectation_sha256"] == sha(trusted.read_bytes())
            assert journal.read_bytes() == original_journal
            assert bundle.read_bytes() == original_bundle
            if expected_blocker is None:
                assert summary["continuation_preparation_allowed"] is True
                assert summary["never_started_scenario_ids"] == ["s2", "s3"]
                assert summary["successful_candidates"] == 1
            else:
                assert expected_blocker in summary["blockers"]
                assert summary["continuation_preparation_allowed"] is False
                if label == "crashed":
                    assert summary["never_started_scenario_ids"] == ["s3"]
                    assert summary["source_state"] == "unknown_call_result"
            print(f"{label}: actual files checked, no automatic resume")

            if label != "quota":
                continue
            # Independently retained expected hash detects removal of whole entries.
            lines = original_journal.splitlines(keepends=True)
            journal.write_bytes(b"".join(lines[:-1]))
            run(command, 1)
            journal.write_bytes(original_journal)
            # Coherently rehash a different returned value: structurally plausible,
            # but still rejected against the externally retained complete-log anchor.
            previous = None
            changed = []
            for raw in lines:
                entry = json.loads(raw)
                if entry["event"]["event"] == "call_succeeded":
                    entry["event"]["payload"] = "different-result"
                entry["previous_sha256"] = previous
                encoded = canonical(entry)
                previous = sha(encoded)
                changed.append(encoded + b"\n")
            journal.write_bytes(b"".join(changed))
            run(command, 1)
            journal.write_bytes(original_journal)
            altered = json.loads(original_bundle)
            altered["state"] = 99
            bundle.write_bytes(canonical(altered))
            run(command, 1)
            bundle.write_bytes(original_bundle)
            wrong_artifact = case / "substituted-artifact"
            wrong_artifact.write_bytes(b"not the implementation")
            run(command[:-1] + [wrong_artifact], 1)
            for path in (journal, bundle, trusted, wrong_artifact):
                # POSIX CI: reject all static final-component links, not just log links.
                alias = case / (path.name + ".link")
                alias.symlink_to(path)
                linked = [alias if arg == path else arg for arg in command]
                if path == wrong_artifact:
                    linked = command[:-1] + [alias]
                run(linked, 1)
                alias.unlink()
            wrong_artifact.write_bytes(b"")
            run(command[:-1] + [wrong_artifact], 1)
            wrong_artifact.unlink()
            # Sparse oversized artifact is rejected without scanning its full payload.
            with wrong_artifact.open("wb") as stream:
                stream.truncate(1024 * 1024 * 1024 + 1)
            run(command[:-1] + [wrong_artifact], 1)
            wrong_artifact.unlink()
            trusted.write_bytes(b" " * (16 * 1024 + 1))
            run(command, 1)
            trusted.write_bytes(canonical(expected))
            expected["semantics"] = "requires_reconciliation"
            trusted.write_bytes(canonical(expected))
            blocked = json.loads(run(command).stdout)
            assert blocked["blockers"] == ["stateful_or_effectful_engine"]
            assert blocked["continuation_preparation_allowed"] is False
        for count in (0, 1, 2, 3, 5):
            run([prospect, "preflight-execution-restart", *(["unused"] * count)], 2)
    print("restart preflight: binary binding, unknown-call rejection, no file mutation and strict CLI verified")


if __name__ == "__main__":
    main()
