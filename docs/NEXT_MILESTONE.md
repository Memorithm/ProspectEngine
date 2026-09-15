# ProspectEngine next milestone

Updated 2026-09-15. This replaces the obsolete post-bootstrap plan: canonical
decision evidence, the pinned ElasticXxx observation/transaction bridge and the
versioned plugin/registry/bundle/typed-dispatch boundary are already implemented.
Do not restart those completed slices.

## Current engineering slice: fail-closed R2 publication

The R2 whole-suite Rust consumer is implemented by ProspectEngine #40, with the
position-native metric-delta guard in #41. The separate legacy eviction consumer
has the same finite guard and its own regression tests in #42. These are input
integrity changes, not experimental results.

KVLab #100 adds the mandatory global check before the launcher publishes its
staged suite. The historical per-campaign verifier remains pinned at
`298acdc91682ef1d09914b6f964e8934828825c0`; the additional publication verifier is
pinned separately at `ca9685cd98f3a0a23e8c4f7e368736bb3aa28d0c`. Experiment bytes,
KVLab/NNIS execution pins and v1 suite manifests remain unchanged.

Engineering acceptance requires the final-head Python/Rust tests, actual locked
three-binary builds in CPU readiness, and a cross-repository publication test.
The latter must accept a valid synthetic suite and reject individually valid
campaigns whose baselines differ across budgets **before** final rename. A
workflow existing or unit tests using mocks are not enough to mark this gate met.
See [readiness and publication qualification](R2-SUITE-READINESS.md).

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
4. Report only the metrics actually observed for the exact run, with explicit
   baseline/candidate/delta semantics and the limit of seven scored targets.

Do not mark representative quality, general policy superiority, physical HBM
release, traffic reduction, latency or throughput gates complete from these short
technical checks. Such claims require separately preregistered, representative
measurement campaigns. R1/R2 inputs and historical pins remain immutable.

## Further coding, kept separate from execution claims

- Bounded file loading and explicit resource policies across all evidence readers,
  beyond the current static file-type checks and post-capture stdout bound.
- Richer versioned execution provenance when needed, with a defined trust model;
  a file hash or self-reported revision is not independent GPU authentication.
- Representative multi-trace/control evaluation and domain policies only after
  their scientific protocol and acceptance criteria are specified.

The existing TDI, ElasticXxx, FLAT-ATTENTION and KVLab contracts remain the reused
foundations. No generic score is relabelled as safety, risk, model quality or
physical performance. Production actuation authority remains in ElasticXxx's
provided transactional actuator, not inferred from a prospective score.

## Stop condition

The publication engineering slice ends only after its final checks pass and
reviewed changes are merged. The empirical slice ends only after actual execution
artifacts pass independent verification, or a concrete failure is recorded with
reproduction evidence. A missing GPU execution is not silently replaced by a
synthetic fixture, a preflight receipt, or an estimated measurement.
