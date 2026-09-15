"""Exercise real Rust verification inside KVLab's staging/publication path.

The input suites contain explicitly synthetic backend outcomes. Git/model probes,
builds and input preflight are mocked here (real build/preflight has separate CI).
The global verifier subprocess, output files, staging, cleanup and rename are real.
No model is loaded and no synthetic fixture is retained as scientific evidence.
"""
from __future__ import annotations

import argparse
from contextlib import ExitStack, contextmanager, redirect_stderr
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from unittest.mock import patch


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


@contextmanager
def temporary_checkout(repo, revision, destination):
    destination.mkdir()
    yield destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--launcher", type=Path, required=True)
    parser.add_argument("--prospect", type=Path, required=True)
    parser.add_argument("--fixtures", type=Path, required=True)
    args = parser.parse_args()
    launcher, prospect, fixtures = (p.resolve() for p in (args.launcher, args.prospect, args.fixtures))
    sys.path.insert(0, str(launcher))
    from kvlab import prospect_smollm2_r2_suite as suite
    from kvlab.prospect_r2_publication_gate import PublicationGateError

    require(suite.PUBLICATION_VERIFIER_REVISION == "ca9685cd98f3a0a23e8c4f7e368736bb3aa28d0c", "global verifier pin drift")
    require(suite.VERIFIER_REVISION == "298acdc91682ef1d09914b6f964e8934828825c0", "historical verifier pin drift")

    with tempfile.TemporaryDirectory(prefix="synthetic-publication-contract-") as temporary:
        root = Path(temporary)
        for drift in (False, True):
            destination = root / ("rejected" if drift else "accepted")
            generic_verified = []

            def read_input(repo, revision, repository_path):
                require(revision == suite.PREREGISTRATION_REVISION, "input revision drift")
                stem = Path(repository_path).stem
                return (fixtures / stem / "campaign.json").read_text(encoding="utf-8")

            def execute_fixture(*arguments):
                campaign, stage = arguments[4], arguments[6]
                stem = Path(campaign.repository_path).stem
                target = stage / stem
                shutil.copytree(fixtures / stem, target)
                if drift and campaign.retained_count == 20:
                    # Every campaign remains individually valid, but the last
                    # budget now claims a different full-cache baseline output.
                    manifest_path = target / "manifest.json"
                    manifest = json.loads(manifest_path.read_text())
                    for entry in manifest["records"]:
                        path = target / entry["filename"]
                        record = json.loads(path.read_text())
                        record["baseline_output_sha256"] = "9" * 64
                        raw = canonical(record).encode()
                        path.write_bytes(raw)
                        entry["sha256"] = hashlib.sha256(raw).hexdigest()
                    manifest_path.write_text(canonical(manifest), encoding="utf-8")
                result = subprocess.run([str(prospect), "verify-kv-campaign", str(target)],
                                        capture_output=True, check=True, timeout=30)
                summary = json.loads(result.stdout)
                generic_verified.append(campaign.retained_count)
                filename = f"verification-{stem}.json"
                (stage / filename).write_text(canonical(summary), encoding="utf-8")
                return suite.CampaignVerification(
                    campaign.retained_count, campaign.repository_path, stem,
                    campaign.campaign_spec_sha256, campaign.trace_sha256,
                    2, suite.POLICIES, filename,
                )

            with ExitStack() as stack:
                stack.enter_context(patch.object(suite, "require_git_commit"))
                stack.enter_context(patch.object(suite, "require_model_artifact"))
                stack.enter_context(patch.object(suite, "read_git_file", side_effect=read_input))
                stack.enter_context(patch.object(suite, "detached_worktree", side_effect=temporary_checkout))
                stack.enter_context(patch.object(suite, "_build", return_value=prospect))
                stack.enter_context(patch.object(suite, "_preflight"))
                stack.enter_context(patch.object(suite, "_execute", side_effect=execute_fixture))
                log = io.StringIO()
                with redirect_stderr(log):
                    try:
                        result = suite.run_suite(kvlab_repo=launcher, nnis_repo=root,
                                                 prospect_repo=root, model_dir=root,
                                                 output_directory=destination)
                    except PublicationGateError as error:
                        require(drift, f"valid synthetic suite rejected: {error}")
                        require("baseline" in str(error), f"wrong rejection: {error}")
                    else:
                        require(not drift, "coherently drifted baseline was published")
                        require(destination.is_dir(), "valid suite was not published")
                        manifest = (destination / "suite-manifest.json").read_bytes()
                        require(manifest == canonical(result).encode(), "v1 manifest was changed")
                        receipt = json.loads(log.getvalue())
                        require(receipt["phase"] == "stage_verified", "wrong receipt phase")
                        require(receipt["suite_manifest_sha256"] == hashlib.sha256(manifest).hexdigest(), "receipt detached from output")
                        with prospect.open("rb") as stream:
                            binary_sha = hashlib.file_digest(stream, "sha256").hexdigest()
                        require(receipt["verifier_binary_sha256"] == binary_sha, "receipt binary hash mismatch")
                require(generic_verified == [7, 14, 20], "not all campaigns passed individual verification")
                if drift:
                    require(not destination.exists(), "failed suite destination exists")
                    require(not log.getvalue(), "failed global gate emitted a success receipt")
                require(not any(p.name.startswith('.') for p in root.iterdir()), "stage or publication lock leaked")
    print("Actual Rust gate accepted a valid synthetic suite and prevented publication of individually valid baseline-drifted campaigns; no model executed")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stderr or str(error), file=sys.stderr)
        raise SystemExit(1)
