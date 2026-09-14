# ProspectEngine next milestone

This document records the implementation slice that follows the initial bootstrap.

## Scope

1. Add a canonical decision-evidence envelope that records run identity, source revisions, candidate scores, and the selected scenario without inventing domain semantics.
2. Add a first ElasticXxx bridge that consumes real `elastic-runtime` observation snapshots while keeping the prospective model pluggable.
3. Pin the ElasticXxx dependency to an exact revision for reproducibility.

## Non-goals

- ProspectEngine does not execute ElasticXxx physical actuation in this slice.
- ProspectEngine does not reinterpret unsupported ElasticXxx telemetry as zero.
- No generic score is claimed to be a safety, resilience, or risk measure until a domain-specific validation contract exists.
- TDI scientific code remains external and unchanged.

## Definition of done

- the workspace builds on the minimum Rust version required by the pinned ElasticXxx runtime;
- valid ElasticXxx observations are converted deterministically into ProspectEngine state;
- unsupported telemetry remains explicit;
- a pluggable Elastic prospective model can be evaluated through `ProspectiveEngine`;
- decision evidence records provenance and candidate scores;
- formatting, Clippy, and tests pass in CI.
