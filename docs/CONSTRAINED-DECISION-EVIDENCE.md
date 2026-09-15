# Canonical constrained-decision evidence

`prospect_evidence::constrained` records the exact software decision structure
produced by `prospect_scenario::decision`. It is a provenance and replay contract,
not a scientific validation of the application's constraints or objectives.

The schema is `prospect.constrained-decision-evidence/v1`.

## Recorded structure

A record contains:

- a run ID;
- sorted, deduplicated `EvidenceSource` provenance entries;
- the baseline alternative followed by every scenario alternative in decision order;
- for each admissible alternative, its complete ordered objective vector, including
  stable objective IDs, maximize/minimize direction and application value;
- for each rejected alternative, the application-provided rejection reason instead
  of an objective vector;
- the derived lexicographic selection, when at least one alternative is admissible;
- the complete Pareto front in deterministic decision order.

The record does not invent thresholds, units, risk meaning, quality meaning, or
weights. Those remain properties of the application protocol which produced the
constraint outcomes and objective values.

## Capture

`ConstrainedDecisionEvidence::from_decision_set` consumes an already validated
`ConstrainedDecisionSet`. It copies the full decision state and recomputes the
lexicographic selection and Pareto front from that state before constructing the
record. Provenance sources are sorted for canonical output and exact duplicates are
rejected.

The baseline must remain the first alternative. Alternative identities must be
unique. Every admissible alternative must retain one common non-empty objective
schema. Empty or duplicate objective IDs, schema drift, unknown selections and
invalid Pareto references fail closed.

## Canonical JSON and independent parsing

`canonical_json` emits the versioned wire representation with deterministic field
order. `from_canonical_json` parses under `deny_unknown_fields`, validates the full
record, recomputes the derived lexicographic/Pareto results from the recorded
objective values, reserializes canonically and requires byte equality with the
supplied input.

Consequently a record cannot change a derived selection while leaving the objective
values unchanged and still pass validation. Likewise a stale or incomplete Pareto
front is rejected. Canonical encoding is a consistency property, not a signature or
trust root.

## Replay comparison

`verify_replay` compares two already validated records while deliberately ignoring
their run identity. It classifies drift independently as:

- provenance/source drift;
- alternative/constraint/objective drift;
- lexicographic-selection drift;
- Pareto-front drift.

This mirrors the existing evidence convention where a repeated run can have a new
run ID while the substantive evidence must remain equal for an exact replay.

## Trust boundary

A rejection reason such as `quality_floor`, `memory_budget`, or `safety_gate` is
only a label/value supplied by the application. ProspectEngine does not establish
that the underlying observation is correct, representative or physically enforced.
An `Observed` source describes the application's evidence classification; it is not
hardware attestation by itself.

Similarly, canonical evidence showing that one candidate is Pareto-optimal does not
establish real-world superiority. It establishes only that, for the exact recorded
objective values and directions, the deterministic generic Pareto relation produces
that front.

This evidence layer consumes completed decision sets only. Partial controlled runs,
open journals or interrupted parent/child chains must first pass the corresponding
completion/assembly contracts before entering decision evaluation.

## Validation

The implementation includes regression coverage for canonical round-trip, unknown
or noncanonical fields, unsupported schemas, stale lexicographic selections, stale
Pareto fronts, duplicate provenance sources, replay-mismatch classification and
baseline exact-tie behavior.

All current tests use software fixtures. They do not constitute CUDA, model-quality,
physical-memory, latency, throughput or safety evidence.
