# Commercial algorithm acquisition suite 3

This document records the third commercial-decision acquisition wave for ProspectEngine.

## Product boundary

SciRust is permitted as a development and scientific qualification forge. ProspectEngine commercial releases must remain executable without SciRust. Algorithms acquired into ProspectEngine therefore receive ProspectEngine-owned contracts, tests and implementation boundaries.

Scientific reference revision used for this wave: SciRust `aa13f62ea829c5c42e370a32d51143f016a0dc44` (2026-09-16).

Relevant SciRust research surfaces at that revision include `scirust-solvers`, `scirust-evo`/NSGA-II, `scirust-forecast`, `scirust-causal`, `scirust-stats`, `scirust-learning`, `scirust-rl-algo` and the broader deterministic scientific-computing stack. ProspectEngine does not take a runtime dependency on those crates in this acquisition.

## Acquired capabilities

### Prescriptive optimization

`optimization` adds a bounded exact binary linear optimizer for 0/1 decisions with linear constraints. It deliberately rejects problems beyond its declared enumeration budget rather than silently returning a heuristic result.

The same module adds:

- non-dominated sorting;
- NSGA-II environmental selection with crowding distance for an already generated candidate population;
- explicit scenario expected value, downside-tail and chance-constraint summaries;
- distributionally robust worst-case expectation under probability intervals;
- exact bounded discrete portfolio selection over scenario payoffs.

This is not a general replacement for CPLEX, CP-SAT or a full evolutionary search runtime. Generic large-scale MILP/CP and generational NSGA-II remain separate scalability milestones.

### Forecasting, causal policy learning and adaptive decisioning

`learning` adds:

- fixed-point simple exponential smoothing;
- fixed-point Holt linear trend forecasting;
- doubly robust/AIPW average treatment effect estimation over caller-supplied propensities and outcome models;
- segment-level uplift summaries;
- doubly robust evaluation of a deterministic treatment policy;
- contextual UCB over explicit discrete context keys;
- Bayesian action selection;
- expected value of perfect information;
- binary experiment expected value of sample information and experiment selection.

The causal estimators do not establish identification. Graph identification, hidden-confounding analysis and certificates remain separate evidence requirements. Forecasting does not yet include acquired ARIMA/SARIMA/ETS implementations.

### Customer economics

`customer` adds:

- discounted survival-based customer lifetime value;
- churn/retention intervention value from explicit causal uplift;
- promotion/discount economics from supplied response quantities;
- exact bounded assortment selection under capital and shelf/slot constraints.

No price elasticity, churn probability or promotion response is fabricated by these engines.

### Operations

`operations` adds:

- service-level reorder point and safety stock from an explicit lead-time demand distribution;
- exact bounded workforce task assignment with eligibility and worker capacities;
- exact bounded closed-route optimization;
- scenario-based real-option action selection across exercise, defer, expand and abandon actions.

The bounded exact workforce/routing solvers are correctness references and small-problem product engines, not claims of large industrial scheduling or vehicle-routing scalability.

### Markets

`market` adds:

- competitive-response payoff evaluation with either explicit competitor probabilities or maximin decisioning;
- first-price bid expected-profit evaluation and candidate selection.

Opponent behavior and bid-win probabilities remain supplied evidence.

### Stress and simulation

`stress` adds:

- deterministic seeded non-parametric Monte Carlo bootstrap over caller-supplied empirical/simulated payoffs;
- replayable empirical quantiles and loss probability;
- weighted stress-case evaluation with explicit probability mass.

The bootstrap does not assert a parametric data-generating distribution.

## Acquisition rule

For every future SciRust-derived commercial capability:

1. identify the exact SciRust revision and scientific contract used during development;
2. define ProspectEngine's narrower product contract;
3. construct parity/reference fixtures where the contracts overlap;
4. implement the autonomous ProspectEngine capability;
5. keep assumptions, numerical conventions and bounds explicit;
6. remove SciRust from the release dependency graph;
7. require ProspectEngine CI and independent evidence before making production claims.

## Remaining high-scale milestones

The following are not claimed by this wave: arbitrary-scale MILP/CP solving, full generational NSGA-II search, ARIMA/SARIMA acquisition, causal identification from raw observational data, continuous-context linear/Thompson bandits, continuous mean-variance optimization, large VRP/scheduling, calibrated market-response models, or production financial-performance guarantees.
