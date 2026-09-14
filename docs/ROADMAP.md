# ProspectEngine Roadmap

## Bootstrap — complete

Goal: prove the architecture with the smallest reusable executable core.

- [x] Rust workspace
- [x] Domain-agnostic engine contracts
- [x] Batch scenario evaluation
- [x] Metric and policy extension points
- [x] Exact TDI adapter without copying TDI scientific code
- [x] TDI dependency pinned to a reviewed commit
- [x] Unit tests
- [x] CI for format, clippy and tests
- [x] PolyForm Noncommercial licensing aligned with SciRust

## Milestone 0.2 — evidence model

- [x] canonical scenario/evidence record;
- [x] deterministic serialization format;
- [x] model/adapter/dependency revision and optional content-hash fields;
- [x] replay contract;
- [x] explicit distinction between observed, simulated and inferred values;
- [x] no domain-specific safety claims.

## Milestone 0.3 — first operational adapter

Target: ElasticXxx.

- [x] consume real ElasticXxx runtime observations through a pinned dependency;
- [x] preserve unsupported telemetry without fabricated values;
- [x] define a versioned probe contract requiring a validated, resource-declared plan and an explicit paired rollback intent;
- [x] compare resource actions before commit;
- [ ] connect selected ProspectEngine decisions to PLAN -> VALIDATE -> ACT -> VERIFY -> COMMIT/ROLLBACK;
- [x] preserve an explicit no-op baseline and rollback intent without claiming physical reversibility.

Proposed control loop:

```text
OBSERVE -> PROBE -> PROSPECT -> PLAN -> VALIDATE -> ACT -> VERIFY -> COMMIT/ROLLBACK
```

The trusted ElasticXxx `TransactionalActuator` remains authoritative for physical feasibility, verification, commit, and rollback. ProspectEngine does not infer reversibility from a transition declaration.

## Milestone 0.4 — AI-memory experiments

Targets: KVLab and FLAT-ATTENTION.

- KV block eviction/intervention scenarios;
- attention branch masking/gating scenarios;
- prospective signatures before expensive execution;
- evidence-backed comparisons against existing heuristics;
- reject any speedup claim without benchmark evidence.

## Milestone 0.5 — plugin boundary

- stable adapter trait/versioning;
- capability metadata;
- metric registry;
- decision-policy registry;
- CLI/API surface;
- reproducible scenario bundles.

## Later research

Only after validation of the finite-state path:

- continuous/hybrid-system abstraction;
- cyber-resilience fault-injection adapters;
- digital-twin/manufacturing adapters;
- power-grid abstractions;
- robotics/logistics;
- multi-objective and constrained decision policies;
- Forge-assisted policy search where executed evidence remains authoritative.
