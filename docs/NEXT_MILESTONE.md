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

## Completed input-boundary foundation

ProspectEngine #44 is merged at `fc69820e6e50f3b8e9d4a9e5a3565337d3ce598c`.
Every file-based CLI input now uses an exact-byte shared read budget: 16 MiB
per file and 128 MiB cumulative, with a 1,024-entry generic campaign cap.
Whole-suite rereads share the same budget. See
[input limits](VERIFICATION-INPUT-LIMITS.md). These limits are not parser,
process-RSS, child-output or hard I/O-deadline limits. Historical pinned
verifiers still execute their historical code.

## Current engineering slice: controlled scenario evaluation

Add cooperative control to the existing generic and registered typed engine
boundaries. Preserve successful baseline/candidate work on interruption or
failure; identify the failed candidate separately from never-started input.
Resolve all typed bundle requirements before any engine call. Reject
duplicate direct-batch IDs and prevent incomplete reports from entering the
existing rankable BatchResult path. The new registered API evaluates only;
it does not automatically score or select a policy winner.

Acceptance requires Rust 1.89 formatting, locked workspace Clippy/tests,
executable examples and the existing R2 producer/consumer/publication
integration. Regression coverage must include a control request after the
last successful call, not just before the next candidate. Deadlines and
cancellation are checkpoints, not preemption or physical rollback.
See [controlled evaluation](CONTROLLED-EVALUATION.md).

Persistent evidence/checkpoints, run-bound input identities, safe restart
and explicit retry/idempotency rules are the next distinct functional
slice. An in-memory partial report is not a durable resumable experiment.
Existing evaluate_batch and execute_registered_bundle are not rewritten.

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
