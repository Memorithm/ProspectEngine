# Constraint-first multi-objective decisions

`prospect_scenario::decision` is an additive decision layer for completed typed
`BatchResult` values. It exists for cases where one scalar utility is insufficient
and some requirements are mandatory rather than tradeable.

The module does not assign domain meaning to scores. Applications supply constraint
logic, objective values, units and acceptance thresholds. ProspectEngine only
preserves the declared structure and applies deterministic generic ordering rules.

## Alternatives and baseline

A decision set contains the baseline/no-intervention outcome plus every evaluated
scenario. The baseline is explicit rather than treated as an implicit zero score.
This permits a domain to retain no intervention when candidates do not improve the
ordered objectives, and exact lexicographic ties conservatively retain the earlier
alternative. Because baseline is assessed first, it wins an exact tie.

Scenario IDs must be unique before constraint/objective callbacks run. The decision
layer performs this identity preflight across the complete batch and rejects a
duplicate `AlternativeId` before evaluating any domain constraint or objective.
This prevents two distinct interventions from becoming indistinguishable in the
selected ID or Pareto front even when a caller constructed a batch outside the
normal scenario-bundle validation path.

## Mandatory constraints

`assess_decision_set` calls the application once for baseline and then once for each
scenario in batch order. The callback receives the alternative identity, baseline
signature and alternative signature.

- `Err(reason)` rejects that alternative under a mandatory domain constraint.
- `Ok(objectives)` admits it and supplies its optimization objectives.

Rejected alternatives expose no objectives and cannot be selected by either
lexicographic or Pareto selection. A high value on another objective never overrides
a mandatory rejection.

This distinction is deliberate. A memory budget, quality floor, safety gate or
other requirement is only a mandatory constraint when the calling domain explicitly
defines it as such. ProspectEngine does not invent those semantics or thresholds.

## Objective contract

Every admissible alternative must expose the same non-empty ordered objective
schema. Each objective has:

- a non-empty stable ID;
- a direction: `Maximize` or `Minimize`;
- a value whose Rust type implements `Ord` for selection.

Blank or duplicate IDs fail closed. Name, order, count or direction drift between
admissible alternatives rejects the complete decision set instead of comparing
semantically incompatible vectors.

Applications needing floating-point objectives should first map observations into
a domain-approved total-order representation; this module does not silently impose
NaN semantics or floating-point tolerances.

## Lexicographic selection

`ConstrainedDecisionSet::lexicographic_best` compares objective zero first. Only
when it is equal does objective one matter, and so on. Direction is respected for
each component. Exact objective-vector ties retain the earlier alternative.

This is deterministic priority ordering, not a weighted sum. It avoids fabricating
conversion factors between unrelated dimensions.

## Pareto front

`ConstrainedDecisionSet::pareto_front` returns every admissible non-dominated
alternative in deterministic input order. Alternative A dominates B only when A is
no worse on every declared objective and strictly better on at least one, respecting
each objective's min/max direction.

Equal vectors remain together on the front. The module does not secretly choose a
winner from a Pareto set; a domain must add an explicit rule if one is required.

## Example

```rust
use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};
use prospect_scenario::{
    evaluate_batch,
    decision::{AlternativeId, Objective, assess_decision_set},
};

struct Add;
impl ProspectiveEngine<i32, i32> for Add {
    type Signature = i32;
    type Error = core::convert::Infallible;
    fn baseline(&self, state: &i32) -> Result<i32, Self::Error> { Ok(*state) }
    fn evaluate(&self, state: &i32, action: &i32) -> Result<i32, Self::Error> {
        Ok(*state + *action)
    }
}

let batch = evaluate_batch(&Add, &10, vec![
    Scenario::new(ScenarioId::new("safe").unwrap(), 2),
    Scenario::new(ScenarioId::new("too-high").unwrap(), 50),
]).unwrap();

let decision = assess_decision_set(&batch, |id, _baseline, signature| {
    if *signature > 20 {
        Err("application-defined hard limit")
    } else {
        let intervention_cost = match id {
            AlternativeId::Baseline => 0,
            AlternativeId::Scenario(_) => 1,
        };
        Ok(vec![
            Objective::maximize("utility", *signature),
            Objective::minimize("intervention_cost", intervention_cost),
        ])
    }
}).unwrap();

assert_eq!(decision.lexicographic_best().unwrap().id().to_string(), "safe");
assert_eq!(decision.rejected_count(), 1);
```

## Evidence and operational boundary

This module consumes only a completed `BatchResult`. Controlled partial runs,
terminal records, live journals and restart preflights do not become decision input
unless a later verified assembly has reconstructed a complete typed batch.

Constraint-first selection is software decision structure, not proof that a
constraint is scientifically valid or physically satisfied. Versioned canonical
recording/replay of this decision structure is a separate evidence layer. Physical
actuation authority remains outside this module, including ElasticXxx transactional
validation/commit/rollback boundaries.
