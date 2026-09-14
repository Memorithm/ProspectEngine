# ProspectEngine Architecture

ProspectEngine is intentionally split between scientific primitives and operational orchestration.

## Design rule

A domain adapter must provide enough information to evaluate candidate interventions, but ProspectEngine must not pretend that one domain model is universal.

The stable flow is:

```text
observed state
    -> candidate interventions
    -> prospective engine
    -> signatures
    -> metrics
    -> domain policy
    -> ranked evidence
```

## Crates

### `prospect-core`

Domain-agnostic contracts only:

- `ScenarioId`
- `Scenario<I>`
- `ProspectiveEngine<State, Intervention>`
- `SignatureMetric<Signature>`
- `DecisionPolicy<Signature>`

This crate must not depend on TDI, ElasticXxx, KVLab, FLAT-ATTENTION, grid models, industrial protocols, or UI concerns.

### `prospect-scenario`

Operational orchestration:

- compute a baseline signature;
- evaluate a finite batch of candidate interventions;
- compare candidates against baseline through a supplied metric;
- rank candidates through a supplied decision policy.

This layer does not define what "safe", "resilient", "optimal", or "profitable" means. Those semantics belong to domain policy modules and require independent validation.

### `prospect-tdi`

Thin adapter to the exact finite-state primitives in `tdi-core`.

The dependency is pinned to a specific TDI commit. ProspectEngine must not copy or mutate frozen TDI research code. Updating the TDI revision is an explicit scientific dependency change and should be reviewed as such.

## Future adapters

Candidate adapters include:

- ElasticXxx resource/runtime state;
- KVLab cache interventions;
- FLAT-ATTENTION masking/gating interventions;
- cyber-resilience fault injection;
- manufacturing/digital-twin models;
- grid and power-system abstractions;
- robotics and logistics state machines.

Continuous and hybrid systems require a validated abstraction or a dedicated continuous primitive. They must not be silently forced into the exact finite-state TDI model.

## Evidence and provenance

A production scenario record should eventually include at least:

- model identifier and hash;
- initial state identifier;
- intervention identifier and parameters;
- prospective horizon/action schedule;
- engine and adapter versions;
- scientific dependency revisions;
- signature output;
- metric and policy identifiers;
- selected action, if any;
- deterministic seed or stochastic provenance where relevant.

The bootstrap deliberately stops before claiming any production, safety, regulatory, financial, or control-system validation.
