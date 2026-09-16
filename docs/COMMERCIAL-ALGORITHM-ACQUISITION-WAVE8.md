# Commercial Algorithm Acquisition — Wave 8

Wave 8 extends `prospect-commercial-advanced` with a bounded/general LP front-end, a true LP-relaxation mixed-integer search core, exact pairwise disjunctive propagation, bounded continuous state-space likelihood search, and conditional instrumental-variable diagnostics.

The design rule remains unchanged: algorithms may evaluate explicit caller-supplied models, observations, bounds, constraints, and assumptions, but ProspectEngine must not fabricate economic effects, probabilities, causal validity, or guarantees.

## 1. Bounded and free-variable linear programming

Module: `bounded_linear_program`.

The wave-7 two-phase simplex core works in non-negative coordinates. Wave 8 adds an explicit transformation layer for original variables that may be free, one-sided bounded, or box constrained.

For an original variable `x`:

- finite lower bound `l`: `x = l + y`, with `y >= 0`;
- finite upper bound `u` only: `x = u - y`, with `y >= 0`;
- free variable: `x = y_plus - y_minus`, with both transformed variables non-negative;
- finite `l <= x <= u`: lower-shift plus the transformed constraint `y <= u - l`.

The objective and every linear constraint are transformed into the non-negative two-phase representation, solved there, reconstructed into original coordinates, and checked for original-space primal violation.

This is a semantic front-end, not a claim of revised simplex, dual simplex, presolve, scaling, basis-factorization engineering, or industrial numerical robustness.

## 2. LP-relaxation mixed-integer branch-and-bound

Module: `mixed_integer`.

Wave 8 replaces purely bounded enumeration for one important class with LP-relaxation branch-and-bound over mixed continuous and integer variables.

Each node:

1. applies caller-supplied or branch-derived finite bounds;
2. solves the continuous relaxation through `bounded_linear_program`;
3. rejects numerically invalid relaxations;
4. prunes by the incumbent objective bound when valid;
5. accepts a solution only when every integer variable is integral within the explicit tolerance;
6. otherwise branches on a fractional integer variable using `floor(x*)` and `ceil(x*)` bounds.

The search is deterministic and fails closed when the node budget is exhausted. An incumbent is never returned as though global optimality had been proved after budget exhaustion.

Deliberate omissions include cutting planes, pseudocosts, strong branching, presolve, incumbent heuristics, parallel search, conflict analysis, warm starts, and industrial MIP numerics.

## 3. Pairwise disjunctive scheduling propagation

Module: `cp_disjunctive`.

For a unary resource / `NoOverlap` constraint, each finite start-domain value of task `i` is retained only when every other task `j` has at least one remaining non-overlapping support. Propagation repeats to a fixed point.

After convergence, a precedence `i -> j` is emitted only when every supported non-overlapping pair places `i` completely before `j`.

This is exact pairwise finite-domain arc consistency for the disjunctive relation. It is stronger than singleton pruning, but it is not full multi-task edge-finding, detectable precedence over arbitrary subsets, energetic reasoning, or CP-SAT propagation.

## 4. Continuous bounded state-space likelihood search

Module: `state_space_optimization`.

Wave 7 introduced finite-grid likelihood calibration for the local-linear-trend Kalman model. Wave 8 adds deterministic bounded derivative-free search over:

- level process variance;
- trend process variance;
- measurement variance.

The optimizer starts from the midpoint of caller-supplied bounds, explores positive and negative coordinate moves, accepts strict log-likelihood improvements, and halves its normalized step when no coordinate improves. The number of likelihood evaluations is explicitly budgeted.

The result is a reproducible local bounded optimum candidate. It is not presented as a global maximum-likelihood estimate and does not provide Hessian-based parameter uncertainty, profile-likelihood intervals, Bayesian posterior inference, or automatic model-order selection.

## 5. Conditional instrumental-variable diagnostics

Module: `iv_partial`.

Wave 8 extends the earlier single-instrument diagnostic with explicit controls.

### Partial first-stage relevance

Restricted model:

`treatment ~ 1 + controls`

Full model:

`treatment ~ 1 + controls + instrument`

The engine reports restricted and full residual sums of squares, partial `R²`, and a one-instrument partial F statistic. A numerically exact full fit is treated as divergent only when adding the instrument produces a strictly positive reduction in SSE. If the restricted model already fits exactly and the instrument adds no explanatory power, the incremental F is zero.

### Anderson–Rubin-style diagnostic

For a caller-supplied null effect `beta0`, the transformed outcome

`y_null = y - beta0 * treatment`

is compared under controls-only versus controls-plus-instrument regressions. The returned F-style statistic measures the instrument's reduced-form association with the null outcome conditional on controls.

These diagnostics quantify conditional relevance or null compatibility under a supplied design. They do **not** establish exclusion, exogeneity, monotonicity, instrument validity, causal identification, finite-sample critical values, or p-values.

## 6. Evidence and determinism requirements

Wave-8 callers should retain, at minimum:

- original variable bounds and objective coefficients;
- original linear constraints and tolerance values;
- LP iteration budgets and MIP node budgets;
- integrality tolerance;
- transformed/reconstructed LP evidence when relevant;
- scheduling start domains and durations;
- state-space variance bounds, evaluation budget, and selected likelihood;
- IV instrument, treatment, outcome, controls, and tested null effect where applicable.

A numerical result without these inputs is not sufficient provenance for a prospective decision.

## 7. Validation gates

Wave 8 must pass the repository's standard qualification surface before merge:

```text
cargo +1.89.0 fmt --all -- --check
cargo +1.89.0 clippy --locked --workspace --all-targets -- -D warnings
cargo +1.89.0 test --locked --workspace
cargo +1.89.0 build --locked --release --workspace
```

The permanent CI additionally validates execution-record, live-journal, restart-preflight, immutable KVLab R2 input, and independent R2 interoperability contracts.

## 8. Remaining research boundaries

High-value follow-on work includes:

- LP/MIP presolve, scaling, basis reuse, cuts and stronger branching while preserving explicit numerical evidence;
- multi-task edge-finding and stronger cumulative/disjunctive propagation;
- multi-start or profile-likelihood state-space optimization and calibrated parameter uncertainty;
- multiple-instrument IV diagnostics, heteroskedasticity-robust inference, and weak-instrument-robust confidence procedures;
- benchmark comparisons against established solvers and statistical packages without claiming parity before evidence exists.
