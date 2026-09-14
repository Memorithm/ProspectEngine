# ProspectEngine

ProspectEngine is a domain-agnostic prospective dynamics and intervention engine built around experimentally validated primitives from the Memorithm ecosystem.

The project is intended to answer a narrow operational question:

> Given a current system state and a set of admissible interventions, what prospective structure does each intervention induce, and what evidence supports selecting one intervention over another?

ProspectEngine separates scientific primitives from domain models. TDI is the initial scientific foundation; adapters translate concrete systems into the generic scenario interface without modifying or duplicating frozen TDI research code.

## Bootstrap architecture

```text
Domain state
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
| Prospective core  |
| / TDI adapter     |
+---------+---------+
          |
          v
Signatures -> metrics -> domain policy -> ranked evidence
```

Current workspace:

- `prospect-core`: generic scenario, engine, metric and decision-policy contracts;
- `prospect-scenario`: baseline/candidate batch evaluation and ranking;
- `prospect-tdi`: thin adapter over the exact `tdi-core` finite-state primitives.

The TDI dependency is pinned to a reviewed commit. ProspectEngine does not copy the TDI exploration or signature algorithms.

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

## Validate the bootstrap

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Status

Early bootstrap. No production, safety, regulatory, financial-performance or universal-prediction claim is made at this stage.

## License

Copyright 2026 Tarek Zekriti.

PolyForm Noncommercial License 1.0.0. See `LICENSE` and `LICENSE.md`.
