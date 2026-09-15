# ProspectEngine

ProspectEngine is a domain-agnostic prospective dynamics and intervention engine built around primitives from the Memorithm ecosystem.

The project answers a narrow operational question:

> Given a current system state and a set of admissible interventions, what prospective structure does each intervention induce, and what evidence supports selecting one intervention over another?

Scientific primitives, domain models, prospective decisions and observed outcomes remain separate. TDI is the initial finite-state foundation; adapters reuse pinned upstream contracts rather than copying scientific or runtime algorithms.

## Implemented surfaces

The workspace includes generic scenario evaluation (`prospect-core`, `prospect-scenario`), canonical evidence (`prospect-evidence`), versioned adapter metadata, metric/policy registries, reproducible bundles and typed dispatch (`prospect-adapter`, `prospect-registry`, `prospect-bundle`, `prospect-dispatch`).

Domain bridges cover exact TDI finite-state signatures, ElasticXxx observations and transaction gating, FLAT Boolean routing, and KVLab logical/synthetic/observed KV contracts. Capability declarations do not establish model quality, physical effects or performance.

The verification-first `prospect` CLI exposes:

```text
list-adapters
preflight-scenario-bundle <bundle.json> <catalog.json>
verify-scenario-bundle <bundle.json>
verify-kv-campaign-spec <campaign.json>
verify-kv-campaign <campaign-directory>
verify-kv-campaign-suite <suite-directory>
verify-kv-campaign-suite-r2 <suite-directory>
```

The typed execution API resolves software contracts before invoking a registered engine. The CLI does not dynamically load arbitrary plugins or execute shell commands from untyped scenario data. See [CLI contracts](docs/CLI.md) and [roadmap](docs/ROADMAP.md).

## Architecture

```text
Domain observations -> adapter -> admissible candidate interventions
                                      |
                                      v
                           pluggable prospective model
                                      |
                                      v
                       signatures -> metrics -> domain policy
                                      |
                                      v
                          prospective decision evidence

Observed execution outcomes are recorded and verified separately.
```

TDI, ElasticXxx and FLAT-ATTENTION dependencies are pinned to reviewed revisions. The workspace tracks `Cargo.lock` for reproducible CLI dependency resolution.

## ElasticXxx bridge

Valid `elastic-runtime::ObservationSnapshot` values become deterministic source/signal keys. Unsupported telemetry stays explicit; duplicate signals and non-finite values fail closed.

A selected probe can pass through `execute_selected_probe` into the existing ElasticXxx `Runtime::cycle` transaction boundary. Action-time validation, actuation, verification, commit and rollback remain the responsibility of the supplied `TransactionalActuator`. A no-op does not touch the actuator. A declared rollback intention is not evidence of physical reversibility.

A concrete `ElasticProspectiveModel` must supply and validate the domain-specific prospective behavior. ProspectEngine does not invent a universal forecast model.

## KVLab / NNIS campaign status

The fixed SmolLM2 R1 suite verifier binds all three campaign inputs to the exact preregistered SHA-256 values, independently verifies their evidence records, and checks common trace/baseline and matched policy budgets. Coherently rehashing substituted inputs does not make them the frozen experiment.

R1 remains frozen under its original pins. Its NNIS backend preserved BF16 weights before calling an F32-only decoder, and its pinned ProspectEngine revision had no committed dependency lockfile. Input-only preflight was therefore not end-to-end execution qualification.

[KVLab R2 preregistrations](https://github.com/Memorithm/KVLab/tree/216b49ae4d62ed4c4c2edfd1e88f929d0a0fd9e5/experiments/prospect/smollm2-r2) use repaired NNIS revision `091aabbb3e132627cf64716720aae530442d2a32`, with explicit BF16-source/F32-execution separation. They preserve the 27-token prefix, eight-token evaluation trace, seed, controls and 7/27, 14/27 and 20/27 retained-row budgets. Each has seven scored teacher-forced targets.

CI builds the locked release binary and checks those exact R2 inputs against their immutable digests and expected budgets. This is cross-repository input/build qualification, not a CUDA run. Generic per-campaign verification supports R2; the original whole-suite command remains R1-only.

The dedicated [R2 whole-suite verifier](docs/R2-SUITE-VERIFICATION.md), `verify-kv-campaign-suite-r2`, independently checks complete R2 result directories against their frozen inputs, exact baseline, published summaries and equal per-policy budgets. R1 and R2 share the verification implementation, not their identities. Their commands reject one another's schemas. A separate Python-producer/Rust-consumer integration test uses explicitly synthetic outputs and never executes a model.

The [R2 launcher readiness check](docs/R2-SUITE-READINESS.md) exercises the actual KVLab R2 `--preflight-only` launcher against the repaired NNIS runtime and the locked ProspectEngine verifier. It includes strict receipt-contract tests, real pinned builds and verification of the frozen source-weight digest. Its artifact is input-only readiness evidence, not an observed model result. A successful workflow run, rather than the presence of the workflow, is required to claim that this path has passed. Representative model-quality, physical-memory, traffic, latency and throughput gates remain open.

## Controlled campaign evaluation

`prospect_scenario::controlled::evaluate_batch_controlled` adds an explicit
candidate quota, shared cancellation signal, monotonic deadline checkpoints
and synchronous progress notifications. It preserves successful work, the
actual failed candidate/error and never-started input separately. Only a
completed report converts to the existing rankable `BatchResult`.

`prospect_dispatch::execution::evaluate_registered_bundle_controlled`
reuses full typed requirement resolution before engine calls. This new API
is evaluation-only: it does not invoke metrics/policies or rank a prefix.
The original batch and full bundle-execution APIs remain unchanged.
See [controlled evaluation](docs/CONTROLLED-EVALUATION.md) for runnable
examples, terminal states and cooperative-control limitations. Persistent
checkpoints, automatic retry and GPU/domain validation are separate work.

## Validate and build

```bash
cargo +1.89.0 fmt --all -- --check
cargo +1.89.0 clippy --locked --workspace --all-targets -- -D warnings
cargo +1.89.0 test --locked --workspace
cargo +1.89.0 build --locked --release -p prospect-cli --bin prospect
```

See [architecture](docs/ARCHITECTURE.md), [roadmap](docs/ROADMAP.md), and [next milestone](docs/NEXT_MILESTONE.md). Continuous/hybrid systems, cyber-resilience, manufacturing, power-system and robotics adapters remain research targets requiring separately validated abstractions.

## Status

Research implementation. No production, safety, regulatory, financial-performance or universal-prediction claim is made.

## License

Copyright 2026 Tarek Zekriti.

PolyForm Noncommercial License 1.0.0. See `LICENSE` and `LICENSE.md`.
