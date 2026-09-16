# Commercial algorithm acquisition — wave 5

Wave 5 deepens four areas left deliberately bounded after wave 4. SciRust remains a development/reference forge; ProspectEngine owns the release implementations and has no runtime dependency on SciRust.

## Prescriptive optimization

`integer_solver` adds exact branch-and-bound for bounded integer variables with:

- arbitrary finite integer lower/upper bounds;
- linear equality and inequality constraints;
- objective upper bounds from remaining variable domains;
- sign-aware value ordering;
- deterministic variable ordering by potential objective impact/domain size;
- explicit node budgets that fail closed rather than returning an unproven optimum.

`constraint_propagation` adds fixed-point propagation for finite-domain CP:

- singleton propagation for `AllDifferent`;
- candidate-value filtering from linear min/max support bounds;
- interval-value filtering for `NoOverlap`;
- propagation before every MRV branch;
- explicit counts for passes, removed values and explored nodes.

These are exact bounded integer/finite-domain solvers. They are not a continuous LP/MILP engine and do not claim industrial CPLEX/CP-SAT scale or cutting-plane strength.

## Forecasting

`forecast_advanced` adds:

- damped Holt trend;
- multiplicative Holt-Winters with optional trend damping;
- deterministic holdout selection across damped, additive seasonal and multiplicative seasonal families;
- empirical residual quantile intervals around point forecasts.

This improves family coverage and model selection without claiming calibrated probabilistic intervals or maximum-likelihood state-space estimation. Full multiplicative SARIMA likelihood, automatic ARIMA order search and statistically calibrated interval coverage remain separate work.

## Bayesian adaptive decisions

`adaptive_bayes` adds:

- Gaussian Thompson Sampling for continuous scalar rewards with explicit prior and observation precision;
- Bayesian linear Thompson Sampling per action;
- ridge/Gaussian posterior precision matrices;
- deterministic posterior sampling from a seeded normal generator;
- explicit posterior means, variances and observation counts.

These models assume Gaussian/linear reward structures. They do not infer whether those assumptions are valid for a deployment.

## Causal identification

`causal_advanced` adds explicit certificates for:

- the single-mediator front-door criterion;
- a conservative sufficient graphical instrumental-variable criterion;
- structural relevance, exclusion and instrument-outcome backdoor checks;
- explicit assumptions and graph fingerprints.

Front-door and IV certification consume a caller-supplied validated DAG. They do not discover the graph, prove absence of latent variables, establish statistical instrument strength, or estimate an effect by themselves. Identification remains upstream of estimation.

## Product rule

Every wave follows the same acquisition boundary:

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
       deterministic tests + evidence
                 |
                 v
        autonomous product release
```

No SciRust crate is introduced into ProspectEngine by this wave.

## Remaining depth targets

- LP relaxations, cuts and mixed continuous/integer variables;
- stronger global CP propagators and scheduling constraints;
- multiplicative SARIMA/state-space likelihood and automatic ARIMA/SARIMA order search;
- probabilistically calibrated forecast intervals;
- generalized Bayesian bandits for non-Gaussian rewards;
- conditional/multiple-mediator front-door, richer IV criteria and sensitivity analysis;
- causal graph discovery as a separate evidence-producing subsystem rather than an implicit assumption.
