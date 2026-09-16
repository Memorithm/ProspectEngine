# Commercial algorithm acquisition — wave 6

Wave 6 advances the explicit depth targets left after wave 5 while keeping ProspectEngine product-autonomous. SciRust remains a development/reference forge only. The inspected SciRust reference for this wave is `aa13f62ea829c5c42e370a32d51143f016a0dc44` on `master`.

No SciRust crate is introduced as a ProspectEngine runtime dependency.

## Linear programming relaxation

`linear_program` adds a deterministic primal-simplex engine for the canonical form:

```text
maximize     c^T x
subject to   A x <= b
             x >= 0
             b >= 0
```

The implementation includes:

- deterministic Bland-style entering-variable selection;
- minimum-ratio leaving selection with deterministic ties;
- explicit slack-basis construction;
- finite pivot budgets that fail closed;
- numerical-breakdown detection;
- recovered primal values, basis indices and objective value;
- measured maximum primal violation.

The supported form is intentionally narrow. Negative right-hand sides, free variables, equalities and `>=` constraints are rejected rather than silently pretending that phase-I feasibility has been solved.

SciRust contains a basic simplex implementation, but its own capability audit explicitly notes that a production MIP solver requires much more than simplex plus branch-and-bound: presolve, numerical scaling, basis management, cuts, heuristics, node selection, conflict processing and extensive safeguards. ProspectEngine therefore does **not** claim HiGHS/SCIP/CPLEX-class capability from this wave.

`integer_relaxation` adds an auditable capacity-style bridge from bounded integer models into that canonical LP:

- `<=` constraints only;
- non-negative capacity coefficients only;
- explicit lower-bound shifting;
- upper bounds emitted as LP constraints;
- numerical relaxed objective upper bound;
- explicit list of fractional variables;
- exact-integer-magnitude guard before conversion to `f64`;
- fail-closed rejection when the lower-bound assignment already violates a non-negative capacity constraint.

This report is a **numerical relaxation diagnostic**, not a formal mixed-integer optimality certificate. Exact bounded-integer decisions remain the responsibility of the integer solver from wave 5.

## Stronger global CP propagation

`cp_global` adds two stronger finite-domain propagators.

### `AllDifferent` generalized arc consistency

Each candidate value is retained only if the complete variable set admits a perfect bipartite matching while that candidate is fixed. This detects Hall-set structure that singleton propagation cannot see and removes unsupported values deterministically.

### Cumulative time-table propagation

For finite start-time domains, durations and resource demands, the propagator:

- computes mandatory task parts;
- constructs mandatory resource profiles;
- detects capacity overloads;
- removes start candidates whose execution would exceed capacity together with other tasks' mandatory load;
- iterates to a fixed point;
- reports removed starts, propagation passes and mandatory peak load.

This is time-table propagation. It does not claim edge-finding, energetic reasoning, lazy-clause generation or CP-SAT-scale propagation.

## State-space forecasting and SARIMA validation

`state_space` adds a local-linear-trend Gaussian state-space model with a two-dimensional Kalman recursion:

```text
level_t = level_{t-1} + trend_{t-1} + eta_level
trend_t = trend_{t-1} + eta_trend
y_t     = level_t + epsilon_t
```

The model exposes:

- explicit level/trend process variances;
- explicit measurement variance;
- Kalman innovations;
- Gaussian innovation log-likelihood;
- final filtered level and trend;
- multi-step forecast means;
- predictive observation variances that grow through the state transition.

The parameters are caller-supplied in this wave. There is no hidden maximum-likelihood optimizer pretending to infer variance components automatically.

The same module adds deterministic holdout selection across caller-supplied `SeasonalArimaOrder` candidates. Candidate models are fit only on the training prefix and scored on an untouched holdout using MAE and RMSE. Ties are deterministic.

This is explicit model-family validation, not unrestricted automatic SARIMA order discovery. The selected SARIMA implementation remains the additive-lag Hannan-Rissanen model already documented in `timeseries`; this wave does not turn it into a full multiplicative state-space likelihood implementation.

## Non-Gaussian adaptive decisions

`adaptive_count` adds Gamma-Poisson Thompson Sampling for event-count rewards:

```text
lambda_a ~ Gamma(alpha, beta)
count    ~ Poisson(lambda_a * exposure)
```

The engine supports:

- explicit Gamma shape/rate priors;
- action-specific observed counts;
- caller-defined exposure units;
- exact conjugate posterior updates;
- seeded deterministic posterior sampling;
- posterior mean rates and sampled rates in decision evidence;
- fail-closed handling of invalid actions and exposure.

This is suited to count-style commercial or industrial rewards such as orders, leads, defects, failures or arrivals. Rates are only comparable when exposure units have the same semantics across actions. The model does not infer overdispersion; Negative-Binomial or hierarchical count models remain separate work.

## Causal sensitivity diagnostics

`causal_sensitivity` adds robustness diagnostics after causal identification/estimation, without changing the identification status itself.

It includes:

- E-values for harmful or protective risk ratios;
- confidence-limit E-values, returning one when the interval reaches/crosses the null;
- explicit confounder-outcome / confounder-exposure bias factors;
- attenuation of an observed risk ratio toward the null under a supplied bias factor.

These quantities answer a limited question: *how strong would an unmeasured confounder have to be, under the chosen stylized sensitivity model, to explain away an observed risk-ratio association?*

They do **not** prove causal validity, discover latent variables, repair an invalid DAG, or replace front-door/backdoor/IV identification checks.

## Product rule

Wave 6 preserves the acquisition boundary:

```text
SciRust / literature / research implementation
                 |
                 v
       reviewed mathematical contract
                 |
                 v
      ProspectEngine-owned algorithm
                 |
                 v
      deterministic tests + limits
                 |
                 v
        autonomous product release
```

## Remaining depth targets

- phase-I/phase-II or dual simplex for general LP feasibility;
- presolve, scaling, cuts, incumbent heuristics and mixed continuous/integer variables before any stronger MIP claim;
- edge-finding / energetic cumulative scheduling and richer global constraints;
- variance-component estimation and calibrated state-space predictive intervals;
- full multiplicative SARIMA/state-space likelihood and constrained automatic order search;
- Negative-Binomial, hierarchical and contextual generalized Bayesian bandits;
- quantitative IV strength diagnostics and weak-instrument handling;
- causal sensitivity beyond risk-ratio E-values, including continuous-outcome and partial-R² robustness diagnostics;
- causal graph discovery only as a separate evidence-producing subsystem with explicit assumptions.
