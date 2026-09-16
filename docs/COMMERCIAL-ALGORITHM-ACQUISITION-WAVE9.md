# Commercial Algorithm Acquisition — Wave 9

Wave 9 is intentionally stacked on the frozen Wave-8 head while Wave 8 awaits GitHub-hosted runner capacity. It does not weaken or bypass Wave-8 qualification. Its PR must not target `main` until Wave 8 is merged and the branch ancestry is rechecked.

This wave adds five bounded, evidence-oriented research surfaces to `prospect-commercial-advanced`:

1. conservative mixed-integer bound presolve;
2. an explicit `presolve -> LP-relaxation branch-and-bound` pipeline preserving both evidence layers;
3. exact global finite-domain consistency for unary `NoOverlap`;
4. state-space profile likelihood over an explicit variance grid with nuisance reoptimization;
5. multi-instrument conditional relevance and Anderson–Rubin-style block diagnostics.

It also hardens the permanent CI workflows so superseded pull-request runs share a concurrency group and are cancelled, while each `main` push keeps a distinct SHA-scoped validation.

The shared design rule is unchanged: fail closed on exhausted budgets or singular numerical systems, keep caller assumptions explicit, and never turn a diagnostic into a causal or production guarantee.

## 1. Conservative MIP bound presolve

Module: `mip_presolve`.

For a finitely bounded mixed-integer problem, each linear row is evaluated over the current variable box using interval arithmetic. For a row

`a_1 x_1 + ... + a_n x_n <= b`,

presolve computes the minimum contribution attainable by all variables except a selected target `x_i`. This yields a safe one-variable implied bound. The same mechanism is applied to `>=` rows by sign reversal and to equality rows in both directions.

For integer variables, implied bounds are rounded inward after allowing the explicit numerical tolerance. Integer-domain bounds must remain exact integer-valued `f64` values within `|x| <= 2^53`, matching the downstream Wave-8 mixed-integer solver. An integer variable is reported fixed only when its lower and upper bounds are exactly equal; tolerance-based fixedness remains reserved for continuous variables.

Propagation repeats until the variable box stops changing or the caller-supplied pass budget is exhausted.

The report preserves:

- number of passes;
- number of bound tightenings;
- variables fixed by the resulting box;
- rows that are redundant throughout the final box within the explicit presolve tolerance.

Rows reported as redundant are deliberately **not deleted** from the returned model. This preserves dimensionality and provenance and avoids claiming a full model-reduction presolver before substitution, coefficient reduction, duplicate-row elimination, scaling, singleton-column logic, and postsolve reconstruction are implemented.

The presolver can prove infeasibility when the attainable row interval lies completely outside a constraint relation.

### 1.1 Presolved mixed-integer pipeline

Module: `mip_pipeline`.

The pipeline composes the conservative presolve with the Wave-8 LP-relaxation branch-and-bound solver and returns both objects:

- the complete presolve report and tightened model;
- the final mixed-integer solution and branch-and-bound counters.

Nothing is hidden as an implementation detail. Callers can bind evidence separately to the original model, tightened bounds, pass count, node count, relaxation count and final incumbent.

The pipeline preserves the Wave-8 rule that a numerically near-integral LP leaf is snapped to exact integer coordinates and revalidated against the original retained linear constraints before becoming an incumbent.

## 2. Exact global finite-domain `NoOverlap` consistency

Module: `cp_disjunctive_global`.

Wave 8 added exact pairwise arc consistency for disjunctive scheduling. Wave 9 adds a stronger finite-domain global support test.

For every candidate start of every task, the propagator asks:

> Does there exist a complete assignment of one start to every task such that all intervals are pairwise disjoint?

Support is searched by deterministic depth-first search using minimum-remaining-values task selection. A candidate without a complete global support is removed. The process repeats to a fixed point.

This is generalized arc consistency for the supplied finite-domain `NoOverlap` constraint. It detects interactions that pairwise propagation cannot, including Hall-like occupancy failures where each pair is locally compatible but the full task set is not jointly schedulable.

The worst-case cost is exponential. Therefore:

- search nodes are counted globally across all support checks;
- the caller supplies a total node budget;
- budget exhaustion returns `SearchBudgetExceeded`;
- budget exhaustion is never reported as successful partial propagation.

This algorithm should not be mislabeled as polynomial edge-finding. Established CP systems use specialized disjunctive, precedence, timetable-edge-finding, and energetic propagators; this Wave-9 component is an exact bounded finite-domain oracle with different complexity characteristics.

## 3. State-space profile likelihood

Module: `state_space_profile`.

Wave 8 added continuous bounded pattern search for the local-linear-trend Kalman variance components. Wave 9 adds deterministic likelihood profiling for one selected variance component.

For each caller-supplied profile value:

1. the selected variance component is fixed at that value;
2. the other variance components remain inside their caller-supplied bounds;
3. the bounded likelihood optimizer reoptimizes those nuisance components;
4. the resulting log likelihood is recorded.

If `ell_max` is the best log likelihood among evaluated profile points, each point receives the likelihood-ratio deviance

`D(theta) = 2 * (ell_max - ell(theta))`.

A helper can return the discrete envelope of points satisfying a caller-supplied cutoff. The cutoff has **no implicit confidence level**. Mapping a likelihood-ratio threshold to a statistical confidence region requires regularity assumptions and an inferential convention outside this routine.

The grid is explicit, duplicate values are rejected, profile values must stay inside the original optimization box, and a total likelihood-evaluation budget is enforced across the complete profile.

This is not a claim of global continuous profile optimization, Hessian uncertainty, Bayesian posterior inference, or asymptotically calibrated coverage.

## 4. Multi-instrument conditional diagnostics

Module: `iv_multi`.

For one endogenous treatment with multiple instruments and optional controls, Wave 9 compares:

Restricted model:

`treatment ~ 1 + controls`

Full model:

`treatment ~ 1 + controls + instrument_block`

The diagnostic reports:

- restricted and full residual sums of squares;
- partial R² of the full instrument block;
- ordinary homoskedastic joint F statistic;
- heteroskedasticity-robust HC0 sandwich Wald statistic for the instrument block.

The ordinary F uses the standard block-restriction construction. If the full model is numerically exact after a strictly positive SSE reduction, the finite F ratio diverges and is represented explicitly rather than fabricated. If the restricted model already fits exactly and the instruments add no explanatory power, the incremental F is zero.

The HC0 statistic is formed from

`(X'X)^(-1) [sum_i e_i^2 x_i x_i'] (X'X)^(-1)`

and the quadratic form of the instrument coefficient block. If the robust covariance block is singular, no finite robust-Wald statistic is claimed.

The same block machinery is exposed for an Anderson–Rubin-style supplied null effect `beta0` by analyzing

`y_null = y - beta0 * treatment`.

These surfaces intentionally do **not** attach p-values or reference distributions. Modern IV toolchains distinguish ordinary F tests from covariance-adjusted Wald tests, and interpretation depends on the covariance estimator, number of instruments, sample size, and identification assumptions.

Most importantly, strong first-stage or reduced-form statistics do not establish:

- exclusion restriction;
- exogeneity;
- monotonicity;
- instrument validity;
- causal identification.

## 5. Evidence requirements

Wave-9 evidence should retain at minimum:

### MIP presolve and pipeline
- original and tightened variable bounds;
- linear rows and relations;
- pass budget and tolerance;
- number of passes and tightenings;
- fixed-variable and redundant-row reports;
- branch-and-bound node and LP-relaxation budgets;
- final solve counters and incumbent coordinates.

### Global disjunctive propagation
- exact start domains and task durations;
- total search-node budget;
- search nodes consumed;
- removed start values;
- number of fixed-point passes.

### State-space profile
- profiled parameter identity;
- explicit profile grid;
- original nuisance-parameter bounds;
- per-point and total evaluation budgets;
- nuisance optimum and log likelihood at every point;
- caller-supplied deviance cutoff, if a region is reported.

### Multi-instrument diagnostics
- complete instrument matrix;
- treatment vector;
- controls matrix;
- outcome and null effect for AR-style diagnostics;
- restricted/full SSE;
- partial R²;
- ordinary F and robust-Wald status/statistic.

## 6. Qualification and merge discipline

Because Wave 9 is stacked on an unmerged Wave-8 head, its draft PR may exercise CI against the exact Wave-8 branch but must not be treated as independently mergeable to `main` yet.

After Wave 8 merges:

1. verify the Wave-9 merge base contains the merged Wave-8 content;
2. retarget the Wave-9 PR from `feat/commercial-acquisition-suite-8` to `main`;
3. verify that the resulting file diff contains only Wave-9 changes;
4. rerun the standard Rust 1.89 gates on the retargeted stable head:

```text
cargo +1.89.0 fmt --all -- --check
cargo +1.89.0 clippy --locked --workspace --all-targets -- -D warnings
cargo +1.89.0 test --locked --workspace
cargo +1.89.0 build --locked --release --workspace
```

5. run permanent execution-record, journal, restart-preflight, immutable R2-input, and independent R2 interoperability gates;
6. merge only from a stable head for which all required gates are green.

For pull requests, the Wave-9 workflow versions use PR-scoped concurrency groups with `cancel-in-progress=true`; superseded PR commits should therefore stop consuming validation capacity. Pushes to `main` are SHA-scoped and are never intentionally cancelled by this rule.

## 7. Remaining boundaries

The next meaningful steps after evidence supports Waves 8 and 9 are:

- safe column/row elimination with explicit postsolve reconstruction;
- coefficient scaling and numerical conditioning diagnostics;
- MIP pseudocosts, strong-branching experiments, cuts and incumbent heuristics;
- specialized polynomial disjunctive edge-finding / detectable-precedence algorithms to complement the exact exponential global-support oracle;
- multi-start and continuous profile optimization with calibrated uncertainty studies;
- richer weak-instrument-robust inference and heteroskedasticity/autocorrelation-aware covariance families;
- comparative benchmarks against established solvers/statistical packages, with no parity claim before measured evidence exists.
