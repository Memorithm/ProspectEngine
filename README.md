# ProspectEngine

ProspectEngine is a domain-agnostic prospective dynamics and intervention engine built around experimentally validated primitives from the Memorithm ecosystem.

The project is intended to answer a narrow operational question:

> Given a current system state and a set of admissible interventions, what prospective structure does each intervention induce, and what evidence supports selecting one intervention over another?

ProspectEngine separates scientific primitives from domain models. TDI is the initial scientific foundation; adapters translate concrete systems into the generic scenario interface without modifying or duplicating frozen TDI research code.

## Architecture

```text
Domain observations
   |
   v
Domain adapter
   |
   v
Candidate interventions
   |
   v
+-------------------+
| ProspectEngine    |
| scenario engine   |
+---------+---------+
          |
          v
+-------------------+
| Prospective model |
| / TDI adapter     |
+---------+---------+
          |
          v
Signatures -> metrics -> domain policy -> decision evidence
```

Current workspace:

- `prospect-core`: generic scenario, engine, metric and decision-policy contracts;
- `prospect-scenario`: baseline/candidate batch evaluation and ranking;
- `prospect-evidence`: canonical run/source/candidate/selection evidence records;
- `prospect-tdi`: thin adapter over the exact `tdi-core` finite-state primitives;
- `prospect-elastic`: bridge from real ElasticXxx runtime observations to pluggable prospective models.

The TDI and ElasticXxx dependencies are pinned to reviewed commits. ProspectEngine does not copy their scientific or runtime algorithms.

## ElasticXxx bridge

The first operational bridge consumes `elastic-runtime::ObservationSnapshot` directly. Valid observations are converted into deterministic source/signal keys. Unsupported telemetry remains explicit and is never converted to zero. Duplicate observations and non-finite values fail closed.

This bridge does **not** execute ElasticXxx actuation yet and does not invent a universal forecast model. A concrete `ElasticProspectiveModel` must provide the prospective behavior and be validated for its domain.

## Intended adapters

The architecture is designed to support independent adapters for systems such as:

- ElasticXxx resource/runtime control;
- KVLab cache experiments;
- FLAT-ATTENTION gating/masking experiments;
- cyber-resilience fault injection;
- manufacturing and digital twins;
- power-system abstractions;
- robotics and logistics.

These are targets, not validated capabilities. Continuous and hybrid systems require a separately validated abstraction or primitive.

## Validate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), [`docs/ROADMAP.md`](docs/ROADMAP.md), and [`docs/NEXT_MILESTONE.md`](docs/NEXT_MILESTONE.md).

## Status

Early implementation. No production, safety, regulatory, financial-performance or universal-prediction claim is made at this stage.

## License

Copyright 2026 Tarek Zekriti.

PolyForm Noncommercial License 1.0.0. See `LICENSE` and `LICENSE.md`.
