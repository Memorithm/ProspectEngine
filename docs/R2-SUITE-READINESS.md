# R2 suite readiness and publication qualification

The CPU readiness workflow exercises the actual no-CUDA path of the globally
gated launcher introduced by KVLab #100. It is distinct from tests replacing
model execution or compilation with fixtures. Merely parsing a campaign does
not establish that its pinned tools can build.

## Exact components

- KVLab launcher source: `0318e4fc09b17eea3705534927917c4d34818258` (reviewed #100 head).
- Frozen R2 inputs: KVLab `216b49ae4d62ed4c4c2edfd1e88f929d0a0fd9e5`.
- Actual campaign executor: KVLab `404577ce939093767dc75d2d67de2fe3c16fa4dc`.
- Repaired NNIS F32 backend: `091aabbb3e132627cf64716720aae530442d2a32`.
- Historical per-campaign verifier: ProspectEngine
  `298acdc91682ef1d09914b6f964e8934828825c0`.
- Additional whole-suite publication verifier: ProspectEngine
  `ca9685cd98f3a0a23e8c4f7e368736bb3aa28d0c`.
- Rust: `1.89.0`; all three binaries use `--locked --release`.

The additional verifier is a separate checkout, not a replacement for the
historical verifier named in each v1 suite manifest. Frozen R1/R2 campaign bytes,
model/runtime pins and output schemas remain unchanged. Old launcher revisions
are not retrospectively repaired.

## Real build/readiness check

The read-only workflow `.github/workflows/r2-suite-readiness.yml` downloads the
frozen SmolLM2 weight artifact within a 512 MiB bound and verifies its SHA-256.
It calls the actual launcher with `--preflight-only`: the launcher builds NNIS,
the historical per-campaign verifier and the additional publication verifier in
separate detached worktrees, then preflights all three frozen inputs. The model
backend and observed-suite publication gate are not invoked in preflight mode.
The requested output directory must remain absent.

The original v1 preflight receipt stays compatible with
`scripts/verify_r2_preflight_receipt.py`. That checker rejects noncanonical JSON,
duplicate/unknown fields, changed identities, numeric type drift and oversized
input. The workflow also records the launcher source revision and additional
publication-verifier revision in `r2-publication-tools.json`, outside the receipt.
Successful runs retain these two identity records, the checker summary and
`rustc -Vv` as seven-day CI artifacts. No weights or credentials are uploaded.

```bash
python3 -m unittest discover -s scripts/tests -v
python3 scripts/verify_r2_preflight_receipt.py /path/to/r2-preflight.json
```

A green readiness run establishes actual pinned builds and input preflight on
that CI host, not decoder startup, complete model-directory admission or CUDA
execution. The stored receipt is a declaration tied to the executed workflow
steps, not independent hardware or executable authentication.

## Real global verifier at the publication boundary

The separate `.github/workflows/r2-suite-interop.yml` retains the original
Python-producer/current-Rust-consumer test. It additionally builds the exact
publication verifier at `ca9685cd98f3a0a23e8c4f7e368736bb3aa28d0c` and runs
`scripts/check_r2_publication_gate.py` through the existing producer-fixture harness.

The fixture producer is the real frozen KVLab executor, supplied with explicitly
synthetic backend outcomes. The publication test replaces Git/model probes,
compilation and numerical backend execution with test doubles. It does **not**
replace the global Rust subprocess, staging, manifest writing, final rename,
publication-lock cleanup or failure handling.

A valid synthetic suite must publish. In the negative case, both policy records
of the 20/27 budget are changed to claim a different baseline output, and their
checksums and published summary are recomputed. Every campaign must still pass
individual Rust verification. The actual global gate must reject the cross-budget
baseline mismatch, leave no result directory and remove its temporary stage and
lock. No synthetic output suite is retained as scientific evidence.

On success, the launcher emits a separate canonical stderr receipt with
`phase=stage_verified`, the publication-verifier revision, binary SHA-256,
manifest SHA-256 and exact verifier-stdout SHA-256. stdout and the strict v1 suite
file set remain unchanged. Preserve the launch log outside the suite to retain
that receipt; receipt emission alone does not prove final publication succeeded.

## Boundaries and remaining gates

A passing unit, interoperability or readiness check does not establish an actual
model run. No new CUDA quality, HBM, traffic, latency or throughput result is
created by these workflows. Each short R2 campaign has seven scored targets and
cannot establish representative quality or general policy superiority.

The binary and directories must be trusted and unmodified. Static file-type
checks are not race-free filesystem isolation. The gate's stdout size check is
after capture, not a hard subprocess-memory limit. Its receipt is not a GPU
attestation or a crash-durability guarantee. See
[R2-SUITE-VERIFICATION.md](R2-SUITE-VERIFICATION.md) and the current
[next milestone](NEXT_MILESTONE.md).
