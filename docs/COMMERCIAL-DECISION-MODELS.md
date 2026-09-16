# Commercial decision models

`prospect-commercial` adds auditable commercial decision primitives without changing the domain-agnostic semantics of `prospect-core`.

The models answer narrower questions than a generic business predictor. They evaluate commercial assumptions supplied by the caller and preserve the distinction between assumptions, prospective signatures and later observed outcomes.

## 1. Deterministic unit economics

`UnitEconomicsEngine` evaluates one commercial period from:

- demand units;
- capacity units;
- unit price;
- unit variable cost;
- fixed cost;
- an optional one-time cash effect attached to an intervention.

All monetary values are integer minor currency units chosen by the caller. The engine uses exact integer arithmetic and never estimates demand, price elasticity, market growth or cost evolution.

For served volume

```text
served = min(demand, capacity)
```

the signature records

```text
revenue = served * unit_price
variable_cost = served * unit_variable_cost
operating_profit = revenue - variable_cost - fixed_cost
net_cash_contribution = operating_profit + one_time_cash_effect
```

It also records unmet demand and unused capacity so a policy can distinguish high cash contribution from an intervention that simply leaves demand unserved.

Two initial policies are supplied:

- `MaximizeNetCash`;
- `CapacityAwareNetCash`, which applies an explicit caller-provided penalty per unit of unmet demand.

These policies express preferences. They are not assertions that either preference is universally correct.

## 2. Explicit discrete uncertainty

`ScenarioRiskEngine` evaluates a validated discrete distribution of commercial net-cash outcomes. Each outcome carries probability mass in parts per million and the total must equal exactly 1,000,000 ppm.

The engine records:

- exact expected-net-cash numerator;
- floored expected net cash in minor currency units;
- loss probability;
- worst and best case;
- a caller-selected downside-tail mass;
- exact weighted downside-tail cash;
- floored downside-tail mean.

The lower tail is constructed from the worst outcomes first and can consume only part of the probability mass of the boundary outcome. This provides an Expected-Shortfall-like downside summary without claiming that the supplied probabilities are statistically calibrated.

Three initial policies are supplied:

- `MaximizeExpectedNetCash`;
- `DownsideFirst`, which lexicographically prioritizes the lower-tail mean and then expected net cash;
- `MinimizeLossProbability`, which lexicographically minimizes loss probability and then maximizes expected net cash.

## Trust boundary

The crate deliberately does **not** provide any of the following yet:

- demand forecasting;
- price elasticity estimation;
- customer conversion prediction;
- market-size estimation;
- competitor-response prediction;
- calibrated probability estimation;
- discounted multi-period cash flow, NPV or IRR;
- portfolio optimization under capital constraints;
- automatic authorization to execute a commercial action.

Those capabilities require separate models, data provenance, validation and evidence. ProspectEngine should continue to distinguish predicted utility from observed commercial outcomes.

## Intended examples

The first model is suitable for explicit what-if questions such as:

- add production capacity;
- change a selling price under a caller-supplied demand assumption;
- change procurement or production cost;
- compare outsourcing with internal capacity;
- evaluate a one-time investment or subsidy effect over one declared period.

The risk model is suitable when the caller already has a justified scenario distribution, for example pessimistic/base/optimistic demand outcomes or multiple contract-margin outcomes with externally established probabilities.

## Next commercial slices

Future work should remain modular. Candidate additions are:

1. multi-period discounted cash-flow signatures with explicit discount-rate provenance;
2. break-even and payback analysis;
3. price/volume elasticity adapters backed by observed data;
4. Bayesian or empirical probability estimators kept separate from decision policy;
5. constrained portfolio selection across projects, products or industrial investments;
6. multi-criteria policies combining financial, capacity, resilience and strategic constraints;
7. observed-outcome records to compare prospective commercial signatures with actual realized results.
