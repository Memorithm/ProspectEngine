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
- [x] exact post-materialization logical KV eviction adapter and replayable KVLab handoff;
- [x] explicit downstream-model boundary so logical eviction never implies numerical, physical-memory or performance effects by itself;
- [x] replayable synthetic numerical evaluation of post-materialization KV eviction through KVLab's additive oracle;
- [ ] executed real-model numerical/quality evaluation of post-materialization KV eviction scenarios;
- [x] canonical observed real-model evidence contract and fail-closed ProspectEngine consumer backend;
- [x] first public FLAT Boolean attention masking/gating adapter using exact Hamming admission and an explicit dense baseline;
- [x] FLAT dependency pinned to reviewed commit `a5b6598ffe475c74c938f45feb86b009d0e4ad0a`;
- [x] prospective-model boundary that assigns no performance or quality meaning to sparsity by itself;
- [x] canonical replayable routing evidence binding exact Q/K Boolean signatures, routing mode, derived mask and mask-storage bytes;
- [x] canonical KVLab BKV handoff consumed and independently revalidated against FLAT routing, bound to KVLab handoff revision `0fb5adc5babfaea9077342db881ce775eacc4442`;
- [x] FLAT BKV-K6.3 executed evidence envelope ingestion with checksum, provenance, accounting, gate and promotion-decision revalidation;
- [x] comparable multi-threshold BIKV observed sweeps with exact paired-dense normalization and no interpolation;
- [x] synthetic KV prospective signatures backed only by replayed KVLab eviction-effect records;
- [ ] representative real-model prospective signatures and quality evidence;
- [x] evidence-backed synthetic comparisons against existing LRU, magnitude, sensitivity-per-byte and seeded-random heuristics;
- [ ] representative real-model heuristic comparisons;
- [ ] representative benchmark gates for any speedup, traffic, TTFT/TPOT or quality claim.

The first adapter intentionally consumes only FLAT's public backend-neutral Boolean contracts (`BooleanAttentionSignature`, `HammingAdmissionRule`, and `BooleanAttentionMask`). Internal BIKV paged-selection implementation details are not treated as a stable cross-repository API. KVLab provides the separate schema `kvlab.prospect-bkv-handoff/v1`, whose exact bit-packed signatures and candidate pages are independently revalidated by ProspectEngine before conversion into a FLAT mask.

The bounded routing-scenario generator sorts and validates a caller-provided threshold set, emits stable scenario IDs, and keeps the dense all-admitted path as the explicit engine baseline. These scenarios represent Boolean page-selection interventions; they are not equivalent to physical KV eviction after numerical state has been materialized.

Post-materialization logical eviction is a separate `prospect-kv` adapter. It consumes KVLab schema `kvlab.prospect-kv-eviction/v1`, bound to merged KVLab revision `e53a09e9b5923bb95527036d5148735f973eefc9`, and independently replays `oldest_first` retention, exact token identities and logical byte accounting. The engine keeps a no-eviction baseline and emits deterministic `kv-retain-N` interventions. A pluggable `KvEvictionProspectiveModel` must supply any downstream numerical, quality, latency or physical-memory interpretation; `logical_evicted_bytes` is never treated as freed HBM, avoided physical traffic or preserved model quality.

KVLab also provides schema `kvlab.prospect-kv-eviction-effect/v1` at merged revision `e9c10e38a57657e8910b42f42e3625ca0e4f1bbc`. The envelope binds an exact logical eviction to a one-to-one token/region mapping and the existing additive synthetic oracle, then records full-cache output, retained output and replayed L2 delta. ProspectEngine revalidates the embedded eviction, region order/storage, contribution geometry, numerical outputs and L2 delta before exposing a `Simulated` evidence source. `SyntheticKvEvictionEvidenceModel` fails closed when no exact measured outcome exists, so logical evicted bytes are never converted into a fabricated numerical effect. This is synthetic numerical evidence only, not real-model quality evidence.

Budget-matched heuristic comparison is represented by KVLab schema `kvlab.prospect-kv-heuristic-comparison/v1`, merged at revision `407c6f2e15a9cb4b1bcbdfa273e41a930483af39`. ProspectEngine consumes the embedded eviction-effect record, independently recomputes the LRU, magnitude and synthetic-sensitivity-per-byte selections in Rust, re-evaluates every recorded retained set against the embedded additive oracle, verifies byte accounting and recomputes the best policy from the validated L2 values. The seeded-random retained set is not regenerated because CPython's `Random.shuffle` is producer-specific; its seed is retained as provenance and its membership, budget and numerical consequence are independently validated. The resulting source remains `Simulated` evidence and is not a real-model policy ranking.

Observed real-model eviction evidence uses KVLab schema `kvlab.prospect-kv-real-model-eviction/v1`, merged at revision `6c1ee30e016827de507e3428387f750931eab5fa`. The dedicated `prospect-kv-observed` backend independently replays the embedded logical eviction, validates model/tokenizer/runtime/trace provenance, output digests, exact logical byte accounting, finite named numerical/quality metrics and `candidate - baseline` deltas, then exposes the source as `Observed`. Evidence sets must share one experimental context and one paired full-cache baseline; `baseline()` comes from that observed baseline and candidate evaluation fails closed when the exact logical outcome is absent. This contract/consumer path does not itself prove that a representative real-model execution has occurred, so the real-model execution and representative-signature gates remain open.

Routing and KVLab handoff evidence are structural and pre-execution: they prove which Boolean inputs and threshold produced a specific canonical mask. They do not by themselves prove numerical correctness, runtime speed, physical traffic reduction, or model-quality preservation.

Executed BKV-K6.3 evidence is handled separately. ProspectEngine revalidates the candidate/dense benchmark checksums, shared commit/environment/problem/protocol, exact logical KV-byte accounting, correctness/quality gates, scope declarations and top-level evidence checksum. The observed signature uses the candidate and dense **end-to-end benchmark medians**; the sum of per-phase medians remains diagnostic and is never substituted for an end-to-end latency measurement. Logical numerical KV bytes avoided remain logical accounting unless the source evidence explicitly claims physical DRAM traffic measurement.

Measured multi-threshold sweeps remain empirical. Records are comparable only when commit, benchmark identities, device/driver/backend, attention problem, measurement protocol, signature width, selection policy and measurement scope match. Ranking uses each candidate's paired dense baseline via exact integer cross multiplication, so baseline drift between runs is not silently ignored. Missing thresholds are never interpolated or synthesized.

## Milestone 0.5 — plugin boundary

- [x] stable adapter trait/versioning;
- [x] capability metadata;
- [ ] metric registry;
- [ ] decision-policy registry;
- [ ] CLI/API surface;
- [ ] reproducible scenario bundles.

`prospect-adapter` defines adapter contract `1.0`, strict machine-stable namespaced identifiers, explicit upstream revisions, versioned capabilities and compatibility checks. The first built-in metadata implementations cover TDI, ElasticXxx, FLAT Boolean attention and logical KV eviction without treating capability declarations as evidence of performance, safety or physical effects. The CLI surface has started with `verify-kv-campaign`, but the broader CLI/API milestone remains open.

## Later research

Only after validation of the finite-state path:

- continuous/hybrid-system abstraction;
- cyber-resilience fault-injection adapters;
- digital-twin/manufacturing adapters;
- power-grid abstractions;
- robotics/logistics;
- multi-objective and constrained decision policies;
- Forge-assisted policy search where executed evidence remains authoritative.
