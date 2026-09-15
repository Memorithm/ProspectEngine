"""Test Python producer -> Rust consumer interoperability with synthetic outputs.

This never loads a model. Fixtures are temporary and must not be published as
observed scientific results, regardless of their wire schema's name.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

EXECUTION = "404577ce939093767dc75d2d67de2fe3c16fa4dc"
PREREGISTRATION = "216b49ae4d62ed4c4c2edfd1e88f929d0a0fd9e5"
RUNTIME = "091aabbb3e132627cf64716720aae530442d2a32"
VERIFIER = "298acdc91682ef1d09914b6f964e8934828825c0"
MODEL_SHA = "80521b40281d6ce74e35c9282c22539e75aa0ac8578892b2a59955ef78d55da1"
TRACE = "3411f378fb3c7010eb94361128c019206fb47bda4271f721498d517eb07ca65f"
DIGESTS = {
    7: "d826e0ca1869b6f3134e8b34bb65db14aa034d2518bf9559810ae80f19012346",
    14: "01eeec54e02bf3f56bbd2e75175e2f04fc4593701875a21b90981b40cfa4eff5",
    20: "e09b14c8479bac98b93625f3d98f667943d8318ff769dbbbf3db989959dcba07",
}


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


def run(*argv):
    return subprocess.run([str(arg) for arg in argv], capture_output=True, text=True, timeout=30, check=True).stdout


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--producer", type=Path, required=True)
    parser.add_argument("--prospect", type=Path, required=True)
    parser.add_argument("--publication-launcher", type=Path,
                        help="also exercise this launcher's actual global gate and publication path")
    args = parser.parse_args()
    producer, prospect = args.producer.resolve(), args.prospect.resolve()
    require(run("git", "-C", producer, "rev-parse", "HEAD").strip() == EXECUTION, "producer revision drifted")
    sys.path.insert(0, str(producer))
    from kvlab.prospect_real_model_campaign_v4 import PositionCampaignSpecV1, execute_position_campaign_v4
    from kvlab.prospect_real_model_runner import BackendMetricValue
    from kvlab.prospect_real_model_runner_v4 import BackendObservationV4

    class SyntheticContractBackend:
        def execute(self, request):
            # Baseline identity is independent of experiment/budget; no fabricated
            # baseline differences are hidden by recomputing suite metadata.
            policy = request["policy"]
            nll, accuracy = {None: (1.0, 4 / 7), "lru": (1.125, 3 / 7), "random_seeded": (1.25, 2 / 7)}[policy]
            artifact = canonical({
                "synthetic_contract_fixture": True, "mode": request["mode"],
                "policy": policy, "retained_positions": request["retained_positions"],
                "trace_sha256": request["trace_sha256"],
            }).encode()
            return BackendObservationV4(
                request_sha256=hashlib.sha256(canonical(request).encode()).hexdigest(),
                applied_mode=request["mode"], applied_policy=policy,
                applied_retained_positions=tuple(request["retained_positions"]),
                artifact_bytes=artifact, artifact_sha256=hashlib.sha256(artifact).hexdigest(),
                metrics=(BackendMetricValue("mean_nll", "quality", "nat_per_token", "lower_is_better", nll),
                         BackendMetricValue("token_accuracy", "quality", "ratio", "higher_is_better", accuracy)),
            )

    with tempfile.TemporaryDirectory(prefix="synthetic-r2-contract-only-") as temporary:
        root = Path(temporary)
        records = []
        for count, expected_digest in DIGESTS.items():
            stem = f"retain-{count:02d}-of-27"
            path = f"experiments/prospect/smollm2-r2/{stem}.json"
            payload = run("git", "-C", producer, "show", f"{PREREGISTRATION}:{path}")
            require(hashlib.sha256(payload.encode()).hexdigest() == expected_digest, "frozen input digest mismatch")
            spec = PositionCampaignSpecV1.from_canonical_json(payload)
            execute_position_campaign_v4(spec=spec, backend=SyntheticContractBackend(), output_dir=root / stem)
            summary = json.loads(run(prospect, "verify-kv-campaign", root / stem))
            require(summary["campaign_spec_sha256"] == expected_digest, "verified input digest mismatch")
            require(summary["trace_sha256"] == TRACE, "trace mismatch")
            filename = f"verification-{stem}.json"
            (root / filename).write_text(canonical(summary), encoding="utf-8")
            records.append(dict(retained_count=count, campaign_path=path, output_directory=stem,
                                campaign_spec_sha256=expected_digest, trace_sha256=TRACE,
                                record_count=2, policies=["lru", "random_seeded"], verification_file=filename))
        manifest = dict(
            schema="kvlab.smollm2-r2-position-suite-result/v1",
            kvlab_preregistration_revision=PREREGISTRATION, kvlab_execution_revision=EXECUTION,
            nnis_runtime_revision=RUNTIME, prospect_verifier_revision=VERIFIER,
            model_id="HuggingFaceTB/SmolLM2-135M", model_revision="93efa2f097d58c2a74874c7e644dbc9b0cee75a2",
            source_model_sha256=MODEL_SHA, runtime_backend="nnis-kvlab-v4", bytes_per_token=46080,
            device_ordinal=0, campaigns=records,
        )
        (root / "suite-manifest.json").write_text(canonical(manifest), encoding="utf-8")
        verified = json.loads(run(prospect, "verify-kv-campaign-suite-r2", root))
        require(verified["schema"] == "prospect.kv-campaign-suite-r2-verification/v1", "wrong summary schema")
        require([c["retained_count"] for c in verified["campaigns"]] == [7, 14, 20], "budget order mismatch")
        require(verified["trace_sha256"] == TRACE, "suite trace mismatch")
        if args.publication_launcher is not None:
            print(run(sys.executable, Path(__file__).with_name("check_r2_publication_gate.py"),
                      "--launcher", args.publication_launcher.resolve(),
                      "--prospect", prospect, "--fixtures", root).strip())
        old = subprocess.run([str(prospect), "verify-kv-campaign-suite", str(root)], capture_output=True, timeout=30)
        require(old.returncode == 1 and not old.stdout, "R1 accepted an R2 suite")
        # A substituted published report must not override independently replayed values.
        published = root / "verification-retain-07-of-27.json"
        changed = json.loads(published.read_text())
        changed["observations"][0]["metrics"][0]["candidate_value"] = 99.0
        published.write_text(canonical(changed), encoding="utf-8")
        rejected = subprocess.run([str(prospect), "verify-kv-campaign-suite-r2", str(root)], capture_output=True, timeout=30)
        require(rejected.returncode == 1 and not rejected.stdout, "tampered report was accepted")
    print("Python producer -> Rust R2 suite consumer passed; synthetic outputs only, no model executed")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stderr or str(error), file=sys.stderr)
        raise SystemExit(1)
