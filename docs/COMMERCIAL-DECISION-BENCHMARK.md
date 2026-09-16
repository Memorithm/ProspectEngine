# Commercial decision-engine benchmark

This document records external product/research patterns used to define ProspectEngine's commercial decision surface. It is a capability benchmark, not a claim of feature parity.

## External reference families

### Prescriptive mathematical optimization

IBM Decision Optimization / CPLEX and Google OR-Tools expose linear, mixed-integer, constraint and combinatorial optimization for planning, scheduling, assignment and resource allocation.

Relevant ProspectEngine targets:

- constrained capital/resource allocation;
- product-mix and capacity planning;
- assignment and scheduling;
- portfolio selection under hard business constraints;
- infeasibility explanations and explicit termination status.

Development acquisition path: `scirust-solvers`, LP/simplex and other numerical optimizers, followed by ProspectEngine-native product contracts.

References:

- https://www.ibm.com/products/decision-optimization
- https://www.ibm.com/products/ilog-cplex-optimization-studio
- https://developers.google.com/optimization
- https://developers.google.com/optimization/math_opt

### Real-time next-best-action / decision arbitration

Pega Customer Decision Hub combines eligibility/engagement rules, constraints, predictive or adaptive propensities and business arbitration to select the next action. SAS Intelligent Decisioning similarly combines business rules, analytics, governance and real-time decision processing.

Relevant ProspectEngine targets:

- action eligibility and suppression;
- propensity/value/cost arbitration;
- explicit no-action baseline;
- channel/capacity/budget constraints;
- adaptive action-selection policies;
- auditable decision traces.

References:

- https://www.pega.com/technology/next-best-action
- https://academy.pega.com/module/customer-decision-hub-overview/v6
- https://support.sas.com/en/software/intelligent-decisioning-support.html

### Causal decision and policy learning

PyWhy DoWhy separates causal identification from effect estimation and supports intervention reasoning. EconML includes heterogeneous treatment-effect estimators and doubly-robust policy trees/forests for treatment selection.

Relevant ProspectEngine targets:

- incremental/uplift rather than raw response propensity;
- treatment-effect uncertainty;
- explicit identifiability status;
- causal policy value relative to a no-action/control baseline;
- abstention when causal assumptions are not established.

Development acquisition path: SciRust `scirust-causal`, especially its identification/certificate/effect-estimation layers, then a ProspectEngine-native commercial causal-policy evidence format.

References:

- https://www.pywhy.org/dowhy/
- https://www.pywhy.org/EconML/
- https://www.pywhy.org/EconML/reference.html

### Multi-objective / Pareto search

pymoo and related optimization libraries expose NSGA-II and other multi-objective algorithms that preserve Pareto structure instead of prematurely collapsing every objective into one scalar.

Relevant ProspectEngine targets:

- profit versus risk versus service-level fronts;
- growth versus cash consumption versus capacity fronts;
- explicit dominance and Pareto rank;
- reference-point or aspiration-based selection;
- decision-policy separation from candidate generation.

Development acquisition path: SciRust `scirust-evo` NSGA-II, followed by a ProspectEngine-owned Pareto evidence and candidate-generation boundary.

Reference:

- https://pymoo.org/algorithms/moo/nsga2.html

## ProspectEngine commercial engine inventory

### Already implemented in `prospect-commercial`

- deterministic unit economics;
- explicit commercial scenario distributions;
- expected value;
- probability of loss;
- lower-tail/downside summaries;
- cash-first, downside-first and loss-probability decision policies.

### Advanced suite under development

- discounted cash flow and payback;
- break-even and margin of safety;
- inventory/newsvendor decisions;
- explicit price-demand point evaluation;
- robust maximin / minimax-regret / Hurwicz decisions;
- normalized multi-criteria decision analysis with vetoes;
- cost-sensitive threshold tuning;
- next-best-action expected-value arbitration.

### Acquisition backlog

The following families remain high priority:

1. linear and mixed-integer commercial allocation;
2. constraint programming and scheduling;
3. Pareto/NSGA-II candidate generation;
4. demand forecasting with forecast-error evidence;
5. causal uplift / heterogeneous treatment effects;
6. doubly-robust policy learning;
7. contextual and non-contextual multi-armed bandits;
8. Bayesian decision models and value of information;
9. portfolio optimization and risk budgeting;
10. stochastic programming and chance constraints;
11. distributionally robust optimization;
12. Monte Carlo scenario generation and stress testing;
13. customer lifetime-value decision models;
14. churn/retention economics;
15. promotion and discount optimization;
16. assortment/product-mix optimization;
17. replenishment and safety-stock models;
18. workforce/capacity scheduling;
19. routing/logistics commercial optimization;
20. real-options / staged investment decisions;
21. game-theoretic competitor-response scenarios;
22. negotiation/bid/auction decision models;
23. value-of-information / experiment-selection policies;
24. ensemble decision arbitration across several independent commercial engines.

## Product boundary

SciRust may be used freely as a scientific dependency during development. Commercial release artifacts must acquire the required algorithmic capabilities into ProspectEngine-owned implementations and contracts, following `SCIRUST-ALGORITHM-ACQUISITION.md`.
