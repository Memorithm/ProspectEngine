# Commercial algorithm acquisition — wave 7

Wave 7 deepens four explicit limits left by wave 6 while preserving ProspectEngine as an autonomous product. The SciRust development/reference pin remains `aa13f62ea829c5c42e370a32d51143f016a0dc44` on `master`; no SciRust runtime dependency is introduced.

## General LP feasibility: deterministic two-phase simplex

`general_linear_program` extends the narrow canonical relaxation from wave 6 to mixed linear relations:

```text
maximize c^T x
subject to
    A_le x <= b_le
    A_ge x >= b_ge
    A_eq x  = b_eq
    x >= 0
```

The solver:

- normalizes negative right-hand sides and flips inequality direction explicitly;
- introduces slack, surplus and artificial variables according to constraint type;
- runs phase I by maximizing the negative sum of artificial variables;
- refuses phase II unless artificial mass is numerically zero;
- pivots artificial basic variables out when possible and removes redundant zero rows;
- deletes artificial columns before restoring the original objective;
- shares one pivot budget across phase I and phase II;
- uses deterministic Bland-style entering selection and deterministic leaving ties;
- reports recovered primal values, objective, phase-I artificial sum, pivot count, final basis and maximum primal violation;
- distinguishes infeasible, unbounded, pivot-budget and numerical failures.

This is a materially more general LP engine, but it is still not a production HiGHS/CPLEX-class implementation. Variables are non-negative; free variables and explicit finite variable bounds require caller transformation or a future bounded-variable front end. No dual simplex, revised simplex, presolve, scaling, sparse factorization or basis warm-start machinery is claimed.

## Energetic cumulative propagation

`cp_energy` strengthens scheduling propagation beyond mandatory-part time tables.

For each interval generated from current start/completion endpoints, the engine computes the minimum overlap energy each task must place inside that interval across all remaining starts. It then compares:

```text
required mandatory energy <= capacity * interval length
```

The propagator:

- detects global energetic overloads;
- fixes each candidate start temporarily and removes it when any generated interval becomes energetically impossible;
- iterates filtering to a fixed point;
- reports removed starts, passes, interval evaluations and final minimum energy slack;
- uses `i128` energy arithmetic with overflow checks;
- requires an explicit interval budget and fails closed before combinatorial interval growth exceeds that budget.

The reasoning is exact for the generated finite-domain interval family. It is not a lazy-clause CP-SAT engine and does not claim the full family of edge-finding, not-first/not-last or advanced energetic scheduling algorithms.

## Likelihood calibration of local-linear-trend state space

`state_space_calibration` adds deterministic likelihood-based selection of the variance components already exposed by the wave-6 Kalman model.

Callers supply finite grids for:

- level process variance;
- trend process variance;
- measurement variance;
- one explicit initial variance.

Every Cartesian candidate is fit with the same local-linear-trend Kalman recursion. Candidates are ranked by Gaussian innovation log-likelihood with deterministic variance tie-breaking. The engine reports the selected configuration, complete ranking, attempted count and failed count.

A caller-supplied candidate budget is checked before grid execution. This makes the procedure replayable and auditable.

This is **finite-grid likelihood calibration**, not continuous maximum-likelihood optimization and not a proof of globally optimal variance components.

The module also converts predictive variances into symmetric Gaussian intervals using an explicit standard-deviation multiplier. It deliberately does not hard-code a confidence level or claim empirical coverage calibration.

## Instrumental-variable first-stage diagnostics

`iv_diagnostics` adds a statistical relevance diagnostic complementary to the graphical IV certificate introduced previously.

For one instrument `z`, it fits:

```text
treatment = intercept + slope * z + error
```

and records:

- sample size;
- first-stage intercept and instrument slope;
- treatment and instrument means;
- treatment R²;
- residual and total sums of squares;
- the one-instrument first-stage F statistic;
- an explicit representation of a perfect first-stage fit, where the finite F statistic diverges.

The implementation uses:

```text
F = (R² / (1 - R²)) * (n - 2)
```

for a non-perfect one-regressor first stage.

A separate threshold-assessment function accepts the threshold as caller data. ProspectEngine does **not** hard-code `F >= 10`: that value is a common heuristic, not a universal validity theorem.

Most importantly, a strong first stage is only a relevance diagnostic. It does not establish instrument exogeneity, exclusion, monotonicity, treatment-effect homogeneity or causal identification. Those assumptions remain separate evidence/contracts.

## Product acquisition rule

Wave 7 continues the same boundary:

```text
research / SciRust reference
          |
          v
reviewed mathematical contract
          |
          v
ProspectEngine-owned implementation
          |
          v
strict deterministic validation
          |
          v
autonomous product capability
```

## Remaining depth targets

- bounded/free-variable transformations, revised/dual simplex, presolve and numerical scaling;
- mixed continuous/integer branch-and-bound using LP relaxation, then cuts and incumbent heuristics before any industrial MIP claim;
- stronger scheduling edge-finding and search integration around the propagators;
- continuous optimization of state-space variance components and empirical interval calibration;
- hierarchical / Negative-Binomial / generalized contextual bandits;
- partial-F diagnostics with covariates, weak-IV robust inference and multi-instrument diagnostics;
- quantitative continuous-outcome causal sensitivity and partial-R² robustness measures;
- causal graph discovery only as a separate assumption-explicit evidence generator.
