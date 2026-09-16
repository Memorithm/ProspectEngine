# Commercial algorithm acquisition — wave 4

This wave moves several commercial-decision capabilities from bounded prototypes toward complete, autonomous ProspectEngine engines while preserving the project rule that SciRust is a development/scientific forge rather than a product runtime dependency.

## Scientific provenance

Development reference: Memorithm/SciRust `aa13f62ea829c5c42e370a32d51143f016a0dc44`.

Relevant upstream research surfaces inspected during acquisition:

- `scirust-evo::Nsga2`: seeded generational NSGA-II with crowded tournament selection, offspring generation and elitist P ∪ Q survival;
- `scirust-forecast::arima`: ARIMA(p,d,q) with Hannan-Rissanen two-stage estimation and recursive point forecasting;
- `scirust-causal::adjustment`: Pearl backdoor criterion, canonical parent adjustment and minimal adjustment-set search using d-separation in a graph with outgoing treatment edges removed.

ProspectEngine does not link to those crates at runtime. The implementations in `prospect-commercial-advanced` have ProspectEngine-owned contracts, tests and explicit limitations.

## Acquired product engines

### Prescriptive solvers

`solver` adds:

- exact 0/1 branch-and-bound maximization;
- incumbent upper-bound pruning;
- partial linear-constraint feasibility bounds;
- explicit node budgets that fail closed rather than returning an unproven optimum;
- finite-domain constraint programming with deterministic search;
- `AllDifferent`, linear equality/inequality and interval `NoOverlap` constraints.

This materially improves on wave 3's full assignment enumeration. It is not an arbitrary mixed-integer continuous optimizer and does not claim CPLEX/CP-SAT scale.

### Generational multi-objective optimization

`evolution` adds a complete seeded NSGA-II loop:

1. population initialization;
2. objective evaluation;
3. non-dominated sorting;
4. crowding-distance assignment;
5. crowded tournament selection;
6. deterministic single-point crossover;
7. seeded Gaussian mutation;
8. offspring evaluation;
9. elitist P ∪ Q environmental survival;
10. repeated generations with reproducible trajectories.

The implementation supports explicit maximize/minimize directions. It does not claim advanced constraint-domination, reference-point variants or large distributed evolutionary execution.

### Time-series forecasting

`timeseries` adds:

- ARIMA(p,d,q) fit using a self-contained Hannan-Rissanen-style two-stage AR/MA regression;
- recursive original-scale point forecasting after regular differencing;
- seasonal ARIMA-style additive seasonal AR/MA lags plus seasonal differencing and reintegration;
- additive Holt-Winters / ETS-style level, trend and seasonal state updates.

The seasonal model is intentionally labelled as an additive-lag seasonal ARIMA implementation. It is not a full multiplicative SARIMA maximum-likelihood/state-space estimator. Holt-Winters is additive only; multiplicative and damped variants remain separate acquisition targets.

### Adaptive decision policies

`adaptive` adds:

- seeded Beta-Bernoulli Thompson Sampling by discrete context;
- explicit posterior alpha/beta evidence;
- LinUCB with per-action ridge regression, matrix inversion, prediction and uncertainty terms.

Thompson Sampling currently assumes binary outcomes and discrete context keys. LinUCB assumes a linear reward model and shared fixed-width feature vectors.

### Causal identification

`causal` adds:

- validated acyclic directed graphs;
- parent, child and descendant traversal;
- deterministic graph fingerprinting;
- d-separation through the ancestral-moral-graph reduction;
- Pearl backdoor criterion in the graph with treatment-outgoing edges removed;
- canonical parent adjustment sets;
- bounded minimal adjustment-set discovery;
- explicit identification certificates and assumptions.

This layer answers whether a supplied DAG identifies an effect by adjustment. It does not infer the DAG from raw data, prove causal sufficiency, test positivity, or turn a misspecified graph into a valid causal claim. Wave 3's doubly-robust estimators remain downstream of this identification boundary.

## Product boundary

The acquisition flow remains:

```text
SciRust research/reference
        |
        v
algorithm contract + differential reasoning
        |
        v
ProspectEngine-owned implementation
        |
        v
ProspectEngine tests + CI + evidence
        |
        v
autonomous commercial product release
```

No SciRust dependency is introduced into the ProspectEngine workspace by this wave.

## Remaining high-value acquisitions

The next depth upgrades are deliberately narrower:

- mixed integer variables and stronger LP relaxations/cuts for branch-and-bound;
- CP propagation stronger than simple domain/bound checks;
- multiplicative SARIMA/state-space likelihood and forecast intervals;
- damped/multiplicative ETS and automatic model selection;
- continuous/generalized Thompson models and disjoint/hybrid LinUCB variants;
- front-door, IV and additional graphical identification criteria;
- causal graph discovery remains a separate evidence-producing component, not an implicit decision-engine assumption.
