# R2 suite readiness qualification

This check exercises the actual no-CUDA path of the R2 launcher introduced by
KVLab PR #95. It is separate from unit tests that replace compilation or model
execution with test doubles. An input parser passing does not establish that a
pinned end-to-end launch command can build.

## Exact components

- KVLab launcher source: `e21289bcdcf8bc01dafc6873015b15552cf1091c` (PR #95 head).
- Frozen R2 campaign inputs: KVLab `216b49ae4d62ed4c4c2edfd1e88f929d0a0fd9e5`.
- Actual campaign executor: KVLab `404577ce939093767dc75d2d67de2fe3c16fa4dc`.
- Repaired NNIS F32 backend: `091aabbb3e132627cf64716720aae530442d2a32`.
- Launch-time ProspectEngine verifier with Cargo.lock:
  `298acdc91682ef1d09914b6f964e8934828825c0`.
- Rust build toolchain: `1.89.0`; both binaries use `--locked --release`.

The workflow checks out only explicit repository revisions, installs the explicit
Rust toolchain, and downloads the frozen SmolLM2 weights with a 512 MiB upper
bound and the exact source SHA-256. It then calls the real launcher with
`--preflight-only`, rather than approximating its steps with mock commands.
The launcher builds NNIS and the frozen ProspectEngine verifier in detached
worktrees and runs all three input specifications through the pinned KVLab and
ProspectEngine preflights. No model backend is invoked.

The workflow uses read-only repository permissions and does not commit generated
patches. It runs for changes to the readiness workflow, checker, tests or this
document, and supports manual dispatch. It does not run on an hourly schedule or
silently create another research automation.

## Receipt verification

The new `scripts/verify_r2_preflight_receipt.py` accepts only the canonical
`kvlab.smollm2-r2-position-suite-preflight/v1` receipt, optionally followed by one
CLI newline. It checks all declared repository/model identities, the three exact
input digests, their common trace and policy ordering. Unknown fields, duplicate
JSON keys, drifted values, oversized input, non-finite numbers, invalid device
ordinals, and float-valued retained counts fail closed.

Run its contract tests from the ProspectEngine repository root:

```bash
python3 -m unittest discover -s scripts/tests -v
```

Verify a receipt using:

```bash
python3 scripts/verify_r2_preflight_receipt.py /path/to/r2-preflight.json
```

The check hashes the exact supplied receipt bytes, including its final newline
when present. A successful workflow preserves the launcher receipt, the checker's
summary and `rustc -Vv` output as a seven-day CI artifact. It never uploads model
weights, credentials or an invented observed campaign directory.

## What a green run establishes

A green run establishes that this exact launcher can verify its source weight
file, build the two pinned binaries and validate the frozen input specifications
on that CPU CI environment, while leaving the requested result directory absent.
The stored receipt alone is an internally checked declaration, not independent
execution authentication; the workflow logs provide the corresponding executed
steps. CI green is not inferred merely because the workflow exists.

This does not validate decoder startup or CUDA execution, local model-directory
completeness beyond the checked weights, numerical parity, representative model
quality, policy superiority, latency, throughput, HBM release or physical traffic.
The seven scored targets in each short R2 campaign cannot establish general
quality. Real GPU observations and representative evaluation remain separate
uncompleted gates until their actual evidence is available.

The historical R1 inputs and tools are unchanged. The fixed
`prospect verify-kv-campaign-suite` is still an R1 consumer. R2 execution uses the
generic per-campaign verifier; the independent R2 whole-suite observed-evidence
consumer remains a separate development slice.
