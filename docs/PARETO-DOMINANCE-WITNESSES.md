# Pareto dominance witnesses

`prospect_scenario::decision::ConstrainedDecisionSet::pareto_dominance_witnesses`
explains why admissible alternatives are outside the Pareto front without changing
the decision rule itself.

## Witness contract

Each `ParetoDominanceWitness` identifies:

- the admissible alternative acting as `dominator`;
- the admissible alternative being `dominated`;
- the ordered objective IDs on which the dominator is strictly preferred.

A witness exists only when the dominator is no worse on every declared objective and
strictly better on at least one, using the existing maximize/minimize direction of
each objective. Objectives omitted from `strictly_better_objective_ids` are equal
between those two alternatives.

The implementation returns every valid domination relation, in deterministic order:
dominated-alternative order first, then dominator order. It does not invent a
"principal" dominator or rank several valid witnesses.

## Constraint boundary

Alternatives rejected by mandatory constraints never participate in Pareto dominance.
Constraint rejection and Pareto domination are different facts: a rejected alternative
has no optimization vector and therefore cannot dominate or be Pareto-dominated.

Equal objective vectors do not dominate one another and produce no witness. They may
coexist on the Pareto front exactly as before.

## What does not change

This feature does not alter:

- `pareto_front` membership;
- lexicographic selection;
- baseline exact-tie behavior;
- objective ordering or directions;
- application constraint logic;
- any score or threshold.

It adds auditability only. No composite score, distance, preference weight, tie breaker
or cross-objective conversion factor is introduced.

## Evidence boundary

A witness proves only the deterministic relation among the exact objective values
provided to the generic decision layer. It does not establish that those values are
scientifically valid, representative, physically measured, safe, or sufficient for
actuation. Canonical persistence of witnesses, if required, belongs in an explicit
evidence schema rather than being inferred from this API.
