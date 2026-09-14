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

## Milestone 0.3 — first operational adapter — complete

Target: ElasticXxx.

- [x] consume real ElasticXxx runtime observations through a pinned dependency;
- [x] preserve unsupported telemetry without fabricated values;
- [x] define a versioned probe contract requiring a validated, resource-declared plan and an explicit paired rollback intent;
- [x] compare resource actions before commit;
- [x] connect selected ProspectEngine decisions to PLAN -> VALIDATE -> ACT -> VERIFY -> COMMIT/ROLLBACK through the trusted ElasticXxx runtime;
- [x] preserve an explicit no-op baseline and rollback intent without claiming physical reversibility;
- [x] record policy utilities and observed transaction outcomes as separate canonical evidence linked by one run id.

Operational control loop:

```text
OBSERVE -> PROBE -> PROSPECT -> PLAN -> VALIDATE -> ACT -> VERIFY -> COMMIT/ROLLBACK
```

The trusted ElasticXxx `TransactionalActuator` remains authoritative for physical feasibility, verification, commit, and rollback. ProspectEngine does not infer reversibility from a transition declaration. Prospective decision evidence and observed transaction evidence remain separate records so predicted utility cannot be confused with physical outcome.

## Milestone 0.4 — AI-memory experiments

Targets: KVLab and FLAT-ATTENTION.

- [x] bounded Boolean KV/attention page-selection intervention scenarios through deterministic Hamming-threshold sweeps;
- [ ] explicit post-selection KV eviction scenarios with downstream numerical semantics;
- [x] first public FLAT Boolean attention masking/gating adapter using exact Hamming admission and an explicit dense baseline;
- [x] FLAT dependency pinned to reviewed commit `a5b6598ffe475c74c938f45feb86b009d0e4ad0a`;
- [x] prospective-model boundary that assigns no performance or quality meaning to sparsity by itself;
- [x] canonical replayable routing evidence binding exact Q/K Boolean signatures, routing mode, derived mask and mask-storage bytes;
- [x] canonical KVLab BKV handoff consumed and independently revalidated against FLAT routing, bound to KVLab handoff revision `0fb5adc5babfaea9077342db881ce775eacc4442`;
- [ ] prospective signatures backed by executed FLAT/KV experiments;
- [ ] evidence-backed comparisons against existing heuristics;
- [ ] representative benchmark gates for any speedup, traffic, TTFT/TPOT or quality claim.

The first adapter intentionally consumes only FLAT's public backend-neutral Boolean contracts (`BooleanAttentionSignature`, `HammingAdmissionRule`, and `BooleanAttentionMask`). Internal BIKV paged-selection implementation details are not treated as a stable cross-repository API. KVLab now provides the separate schema `kvlab.prospect-bkv-handoff/v1`, whose exact bit-packed signatures and candidate pages are independently revalidated by ProspectEngine before conversion into a FLAT mask.

The bounded scenario generator sorts and validates a caller-provided threshold set, emits stable scenario IDs, and keeps the dense all-admitted path as the explicit engine baseline. These scenarios represent Boolean page-selection interventions; they are not equivalent to physical KV eviction after numerical state has been materialized.

Routing and KVLab handoff evidence are structural and pre-execution: they prove which Boolean inputs and threshold produced a specific canonical mask. They do not by themselves prove numerical correctness, runtime speed, physical traffic reduction, or model-quality preservation.

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
