# Independent R2 whole-suite verification

The R2 suite emitted by KVLab's `prospect_smollm2_r2_suite` now has an independent Rust consumer:

```bash
cargo +1.89.0 run --locked -p prospect-cli --bin prospect -- \
  verify-kv-campaign-suite-r2 suite-output
```

This command reads an already produced directory. It does not run the launcher,
load a model, start CUDA, perform policy selection, or publish scientific evidence.
Its success means the supplied files satisfy the frozen R2 consistency contract.

## Fixed contract

The input schema is `kvlab.smollm2-r2-position-suite-result/v1`, and the output
summary schema is `prospect.kv-campaign-suite-r2-verification/v1`.

The supported producer contract is KVLab #95, merged at
`cefe129f126c865819545ea94b3d3510400d8964`. The verifier binds the three exact
campaign input SHA-256 values from preregistration revision
`216b49ae4d62ed4c4c2edfd1e88f929d0a0fd9e5`, plus these declared execution pins:

- KVLab executor: `404577ce939093767dc75d2d67de2fe3c16fa4dc`;
- repaired NNIS runtime: `091aabbb3e132627cf64716720aae530442d2a32`;
- launch-time per-campaign ProspectEngine verifier:
  `298acdc91682ef1d09914b6f964e8934828825c0`.

The supported producer revision in the output describes this software contract;
it is not an authenticated statement about which launcher actually ran. The
manifest does not carry independent executable or hardware authentication.

The original `verify-kv-campaign-suite` command remains R1-only. The two commands
share one verification implementation but use separate private immutable contract
profiles. Neither accepts the other's schema, and neither rewrites historical pins.

## Checks performed

The consumer requires the canonical suite manifest and exactly three budget
directories, ordered 7/27, 14/27 and 20/27, with their published verification
summaries. Each budget contains the exact `lru` and `random_seeded` selections.

It independently verifies per-campaign manifests, record digests, embedded
selections, logical accounting and metric deltas. Published summaries cannot
override the independently recomputed summaries. Within each budget, candidates
must have the same experimental context, one exact full-cache baseline and equal
retained logical budgets. Across budgets, the trace, baseline output digest,
metric identities and bitwise baseline values must agree.

The logical KV payload remains 46,080 bytes per token: 1,244,160 bytes for the full
27-position prefix, with retained budgets of 322,560, 645,120 and 921,600 bytes.
These are arithmetic budgets, not measured HBM allocation or traffic reductions.

Exact preregistration hashes prevent a substituted input, retained-position set
or evaluation identifier from passing merely because all downstream checksums
were recomputed consistently. Wrong schemas, unknown fields, missing/extra files,
path substitutions, malformed counts and out-of-i32 device ordinals fail closed.
Static symlinks and non-regular evidence entries are rejected before file reads.
The same file-boundary hardening applies to R1.

The directory and its parent must be trusted and remain unmodified during
verification. File-type checks do not constitute race-free filesystem isolation,
a resource-limit boundary, or protection against a hostile concurrent writer.

## Validation

Rust regression tests cover complete R2 suites, deterministic summaries, R1/R2
separation, frozen-input substitutions, provenance drift, schema/manifest errors,
published-summary tampering, cross-budget baseline drift and symbolic links.
All existing R1 regression tests remain in place.

The separate CI workflow `R2 suite interoperability (synthetic only)` checks out
the actual pinned Python KVLab producer, uses frozen R2 input bytes, and produces
temporary campaign records with an explicitly synthetic backend. Fractional
metrics exercise the Python-to-Rust serialization boundary. The actual release
CLI must accept the valid suite, reject it through the R1 command, and reject a
tampered published summary. No fixture outputs are committed or uploaded as model
results. This is distinct from the real-launcher CPU readiness check described
in [R2-SUITE-READINESS.md](R2-SUITE-READINESS.md).

## Still not established

An internally consistent file set does not independently authenticate model or
GPU execution. A checksum is not proof that weights were loaded or KV rows were
physically compacted. No speedup, latency, throughput, HBM saving, physical traffic
reduction, representative model quality or general policy superiority follows
from accepting the suite. The seven scored targets per R2 campaign remain a
small technical check, not a representative language-model evaluation.
