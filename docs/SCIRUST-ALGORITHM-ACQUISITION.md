# SciRust algorithm acquisition lifecycle

ProspectEngine is intended to become a commercially usable autonomous product. During development, however, SciRust is the shared scientific forge for the Memorithm ecosystem.

## Rule

A ProspectEngine feature may use SciRust freely during research, prototyping, differential validation and algorithm design. A production ProspectEngine capability must eventually be owned by ProspectEngine itself: its public contract, validation rules, implementation, tests, evidence model and release lifecycle must not require a SciRust runtime installation.

This is an acquisition workflow, not a prohibition on reuse.

## Acquisition stages

1. **Discover** — identify an existing SciRust primitive or algorithm that can support a ProspectEngine decision engine.
2. **Pin** — record the exact SciRust revision and source module used during qualification.
3. **Specify** — write the mathematical contract independently of the source implementation: inputs, outputs, invariants, numerical assumptions, failure modes and determinism requirements.
4. **Prototype** — use SciRust directly while the commercial decision surface is still being explored.
5. **Differentially validate** — compare the ProspectEngine implementation against the pinned SciRust reference on deterministic fixtures, adversarial cases and randomized/property-style cases where appropriate.
6. **Acquire** — implement or extract the minimal required capability into ProspectEngine under a ProspectEngine-owned API. Do not pull unrelated SciRust subsystems into the product.
7. **Freeze evidence** — preserve reference vectors, hashes, tolerances, source revision and acceptance criteria in ProspectEngine tests/docs.
8. **Decouple runtime** — before a commercial release claim, verify that the ProspectEngine runtime and release artifact do not require SciRust.
9. **Retain provenance** — keep the SciRust revision and qualification evidence so later algorithm changes remain auditable.

## Allowed dependency forms during development

- direct SciRust use in research branches and experiments;
- pinned SciRust Git dependencies in prototypes;
- SciRust as a development/test oracle;
- generators or reference implementations that emit frozen fixtures consumed by ProspectEngine;
- one-off migration tooling used to bootstrap a ProspectEngine-native implementation.

## Release boundary

A commercial release candidate must satisfy all of the following for every acquired algorithm:

- no required runtime link or dynamic load from SciRust;
- no hidden shell-out to SciRust tooling;
- no implicit network access to a SciRust service;
- stable ProspectEngine-owned types and errors;
- deterministic or explicitly stochastic provenance;
- independent unit and integration tests;
- differential or golden validation against the pinned scientific reference where such a reference exists;
- explicit numerical tolerances and domain limitations;
- release notes identifying algorithm revisions that materially change decisions.

## Commercial decision acquisition map

The initial acquisition targets are:

| ProspectEngine capability | SciRust development source | Product target |
| --- | --- | --- |
| constrained optimization / allocation | `scirust-solvers`, LP/simplex and numerical optimizers | native constrained commercial allocator |
| causal next-best-action / uplift | `scirust-causal` effect estimation and causal certificates | native causal commercial policy evidence |
| demand and time-series forecasting | `scirust-forecast` | native forecast adapter plus calibrated evidence contract |
| multi-objective search | `scirust-evo` NSGA-II and related optimizers | native Pareto candidate generator / selector |
| uncertainty and probability models | `scirust-stats` | native uncertainty summaries and scenario generators |
| adaptive action selection | `scirust-rl-algo` / bandit primitives | native exploration/exploitation commercial policy |

The commercial semantics stay in ProspectEngine. SciRust supplies scientific machinery during development; it does not decide what "profitable", "acceptable risk", "commercially eligible" or "strategically preferred" means.

## Non-goal

Acquisition does not mean copying an entire SciRust crate into ProspectEngine. The target is the smallest independently validated algorithmic surface needed by the product.
