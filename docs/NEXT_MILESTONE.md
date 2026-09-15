# ProspectEngine next milestone

Updated 2026-09-15. Canonical decision evidence, the pinned ElasticXxx
observation/transaction bridge and the versioned plugin/registry/bundle/typed
execution boundary are implemented. Do not restart completed slices.

## Completed foundation: fail-closed R2 publication

The R2 whole-suite Rust consumer is implemented by ProspectEngine #40, with the
position-native metric-delta guard in #41 and the legacy eviction-consumer guard
in #42. KVLab #100 adds mandatory global verification before publication, and
ProspectEngine #43 qualifies that integration with actual locked CPU builds and
synthetic positive/negative publication tests. These are input-integrity and
software-integration results, not model-execution results.

The historical per-campaign verifier remains pinned at
`298acdc91682ef1d09914b6f964e8934828825c0`; the additional publication verifier is
pinned separately at `ca9685cd98f3a0a23e8c4f7e368736bb3aa28d0c`. Experiment bytes,
KVLab/NNIS execution pins and v1 suite manifests remain unchanged. See
[readiness and publication qualification](R2-SUITE-READINESS.md).

## Current engineering slice: bounded verification inputs

ProspectEngine #44 introduces a shared exact-byte reader for every file-based
CLI entry point: 16 MiB per file, 128 MiB of cumulative text per operation, and
at most 1,024 entries per generic campaign directory. Whole-suite context and
baseline rereads consume that same budget, rather than starting a new one.
Oversized streams fail without accepting a truncated prefix; accepted UTF-8
bytes and canonical evidence identities remain unchanged. Reserved campaign
inputs now receive static symlink and regular-file checks as well.

The public shared-budget API can compose campaign verifiers under one explicit
policy. Final-head Rust CI and the existing Python/Rust publication integration
must pass before this slice is considered merged and complete. See the precise
[input limits and trust boundary](VERIFICATION-INPUT-LIMITS.md).

This slice does not cap parser/node allocations, process RSS, child-process
output or I/O time, and it does not provide race-free filesystem isolation.
Historical pinned verifier binaries still execute their original code. Updating
a launcher's verifier requires a separate explicit, reviewed successor pin.

## Next empirical slice: exact R2 CUDA qualification

Use the frozen 7/27, 14/27 and 20/27 campaigns, not regenerated or relabelled inputs.

1. Verify the actual GPU host, model directory/configuration and exact weights,
   checked-out revisions, compiler/runtime environment and available resources.
   CPU readiness verifies builds and the source-weight hash only; it does not
   prove model-directory completeness or decoder admission.
2. Execute the pinned NNIS backend on the frozen trace with the LRU and seeded
   random controls. Preserve failures rather than fabricating missing outcomes.
3. Retain the self-contained campaign/suite files and the separate launch log,
   including the global verifier's stage-consistency receipt. Independently run
   `verify-kv-campaign-suite-r2` on the published suite.
4. Report only metrics actually observed for the exact run, with explicit
   baseline/candidate/delta semantics and the limit of seven scored targets.

Do not mark representative quality, general policy superiority, physical HBM
release, traffic reduction, latency or throughput gates complete from these short
technical checks. Such claims require separately preregistered, representative
measurement campaigns. R1/R2 inputs and historical pins remain immutable.

## Further coding, kept separate from execution claims

- Hard subprocess-output bounds, parser/work quotas and deadline policy where
  needed, beyond the raw file-byte admission limits already implemented here.
- Richer versioned execution provenance with a defined trust model; a file hash
  or self-reported revision is not independent GPU authentication.
- Representative multi-trace/control evaluation and domain policies only after
  their scientific protocol and acceptance criteria are specified.

The existing TDI, ElasticXxx, FLAT-ATTENTION and KVLab contracts remain the reused
foundations. No generic score is relabelled as safety, risk, model quality or
physical performance. Production actuation authority remains in ElasticXxx's
provided transactional actuator, not inferred from a prospective score.

## Stop condition

Each engineering slice ends only after its final checks pass and the reviewed
changes are merged. The empirical slice ends only after actual execution
artifacts pass independent verification, or a concrete failure is recorded with
reproduction evidence. A missing GPU execution is not replaced by a synthetic
fixture, a preflight receipt, or an estimated measurement.
