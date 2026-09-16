use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;
use prospect_core::{DecisionPolicy, ProspectiveEngine};
use std::cmp::Reverse;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryError {
    EmptyDemandDistribution,
    ZeroProbabilityOutcome,
    ProbabilityMassMismatch { actual_ppm: u64 },
    NegativeField(&'static str),
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDemandDistribution => {
                formatter.write_str("demand distribution must not be empty")
            }
            Self::ZeroProbabilityOutcome => {
                formatter.write_str("demand outcomes must have non-zero probability mass")
            }
            Self::ProbabilityMassMismatch { actual_ppm } => write!(
                formatter,
                "demand probabilities must sum to {PROBABILITY_SCALE_PPM} ppm, got {actual_ppm} ppm"
            ),
            Self::NegativeField(field) => write!(formatter, "{field} must be non-negative"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
        }
    }
}

impl std::error::Error for InventoryError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemandOutcome {
    pub probability_ppm: u32,
    pub demand_units: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemandDistribution {
    outcomes: Vec<DemandOutcome>,
}

impl DemandDistribution {
    pub fn new(outcomes: Vec<DemandOutcome>) -> Result<Self, InventoryError> {
        if outcomes.is_empty() {
            return Err(InventoryError::EmptyDemandDistribution);
        }
        if outcomes.iter().any(|outcome| outcome.probability_ppm == 0) {
            return Err(InventoryError::ZeroProbabilityOutcome);
        }
        let actual_ppm: u64 = outcomes
            .iter()
            .map(|outcome| u64::from(outcome.probability_ppm))
            .sum();
        if actual_ppm != u64::from(PROBABILITY_SCALE_PPM) {
            return Err(InventoryError::ProbabilityMassMismatch { actual_ppm });
        }
        Ok(Self { outcomes })
    }

    #[must_use]
    pub fn outcomes(&self) -> &[DemandOutcome] {
        &self.outcomes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryState {
    pub demand: DemandDistribution,
    pub baseline_order_quantity_units: u64,
    pub unit_price_minor: i64,
    pub unit_purchase_cost_minor: i64,
    pub unit_salvage_value_minor: i64,
    pub stockout_penalty_minor_per_unit: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InventoryIntervention {
    pub order_quantity_units: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InventorySignature {
    pub order_quantity_units: u64,
    pub expected_profit_weighted_minor_ppm: i128,
    pub expected_profit_minor_trunc: i128,
    pub expected_units_sold_weighted_ppm: u128,
    pub expected_lost_sales_units_weighted_ppm: u128,
    pub expected_leftover_units_weighted_ppm: u128,
    pub stockout_probability_ppm: u32,
    pub leftover_probability_ppm: u32,
}

/// Single-period inventory/newsvendor evaluator over an explicit demand distribution.
///
/// It does not estimate demand probabilities. It only evaluates order quantities
/// against probabilities and economics supplied by the caller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InventoryEngine;

impl ProspectiveEngine<InventoryState, InventoryIntervention> for InventoryEngine {
    type Signature = InventorySignature;
    type Error = InventoryError;

    fn baseline(&self, state: &InventoryState) -> Result<Self::Signature, Self::Error> {
        evaluate_inventory(state, state.baseline_order_quantity_units)
    }

    fn evaluate(
        &self,
        state: &InventoryState,
        intervention: &InventoryIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        evaluate_inventory(state, intervention.order_quantity_units)
    }
}

fn validate_state(state: &InventoryState) -> Result<(), InventoryError> {
    for (field, value) in [
        ("unit_price_minor", state.unit_price_minor),
        ("unit_purchase_cost_minor", state.unit_purchase_cost_minor),
        ("unit_salvage_value_minor", state.unit_salvage_value_minor),
        (
            "stockout_penalty_minor_per_unit",
            state.stockout_penalty_minor_per_unit,
        ),
    ] {
        if value < 0 {
            return Err(InventoryError::NegativeField(field));
        }
    }
    Ok(())
}

fn evaluate_inventory(
    state: &InventoryState,
    order_quantity_units: u64,
) -> Result<InventorySignature, InventoryError> {
    validate_state(state)?;
    let mut expected_profit_weighted_minor_ppm = 0_i128;
    let mut expected_units_sold_weighted_ppm = 0_u128;
    let mut expected_lost_sales_units_weighted_ppm = 0_u128;
    let mut expected_leftover_units_weighted_ppm = 0_u128;
    let mut stockout_probability_ppm = 0_u32;
    let mut leftover_probability_ppm = 0_u32;

    let purchase_cost = i128::from(order_quantity_units)
        .checked_mul(i128::from(state.unit_purchase_cost_minor))
        .ok_or(InventoryError::ArithmeticOverflow(
            "inventory purchase cost",
        ))?;

    for outcome in state.demand.outcomes() {
        let sold = order_quantity_units.min(outcome.demand_units);
        let lost = outcome.demand_units - sold;
        let leftover = order_quantity_units - sold;
        let revenue = i128::from(sold)
            .checked_mul(i128::from(state.unit_price_minor))
            .ok_or(InventoryError::ArithmeticOverflow("inventory revenue"))?;
        let salvage = i128::from(leftover)
            .checked_mul(i128::from(state.unit_salvage_value_minor))
            .ok_or(InventoryError::ArithmeticOverflow("inventory salvage"))?;
        let stockout_penalty = i128::from(lost)
            .checked_mul(i128::from(state.stockout_penalty_minor_per_unit))
            .ok_or(InventoryError::ArithmeticOverflow("stockout penalty"))?;
        let profit = revenue
            .checked_add(salvage)
            .and_then(|value| value.checked_sub(purchase_cost))
            .and_then(|value| value.checked_sub(stockout_penalty))
            .ok_or(InventoryError::ArithmeticOverflow("inventory profit"))?;
        let probability = i128::from(outcome.probability_ppm);
        expected_profit_weighted_minor_ppm = expected_profit_weighted_minor_ppm
            .checked_add(profit.checked_mul(probability).ok_or(
                InventoryError::ArithmeticOverflow("weighted inventory profit"),
            )?)
            .ok_or(InventoryError::ArithmeticOverflow(
                "expected inventory profit",
            ))?;

        let probability_u128 = u128::from(outcome.probability_ppm);
        expected_units_sold_weighted_ppm = expected_units_sold_weighted_ppm
            .checked_add(u128::from(sold).saturating_mul(probability_u128))
            .ok_or(InventoryError::ArithmeticOverflow("expected units sold"))?;
        expected_lost_sales_units_weighted_ppm = expected_lost_sales_units_weighted_ppm
            .checked_add(u128::from(lost).saturating_mul(probability_u128))
            .ok_or(InventoryError::ArithmeticOverflow("expected lost sales"))?;
        expected_leftover_units_weighted_ppm = expected_leftover_units_weighted_ppm
            .checked_add(u128::from(leftover).saturating_mul(probability_u128))
            .ok_or(InventoryError::ArithmeticOverflow("expected leftover"))?;

        if lost > 0 {
            stockout_probability_ppm = stockout_probability_ppm
                .checked_add(outcome.probability_ppm)
                .ok_or(InventoryError::ArithmeticOverflow("stockout probability"))?;
        }
        if leftover > 0 {
            leftover_probability_ppm = leftover_probability_ppm
                .checked_add(outcome.probability_ppm)
                .ok_or(InventoryError::ArithmeticOverflow("leftover probability"))?;
        }
    }

    let expected_profit_minor_trunc =
        expected_profit_weighted_minor_ppm / i128::from(PROBABILITY_SCALE_PPM);

    Ok(InventorySignature {
        order_quantity_units,
        expected_profit_weighted_minor_ppm,
        expected_profit_minor_trunc,
        expected_units_sold_weighted_ppm,
        expected_lost_sales_units_weighted_ppm,
        expected_leftover_units_weighted_ppm,
        stockout_probability_ppm,
        leftover_probability_ppm,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizeExpectedInventoryProfit;

impl DecisionPolicy<InventorySignature> for MaximizeExpectedInventoryProfit {
    type Score = i128;

    fn utility(&self, signature: &InventorySignature) -> Self::Score {
        signature.expected_profit_weighted_minor_ppm
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServiceLevelFirst;

impl DecisionPolicy<InventorySignature> for ServiceLevelFirst {
    type Score = (Reverse<u32>, i128);

    fn utility(&self, signature: &InventorySignature) -> Self::Score {
        (
            Reverse(signature.stockout_probability_ppm),
            signature.expected_profit_weighted_minor_ppm,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> InventoryState {
        InventoryState {
            demand: DemandDistribution::new(vec![
                DemandOutcome {
                    probability_ppm: 500_000,
                    demand_units: 50,
                },
                DemandOutcome {
                    probability_ppm: 500_000,
                    demand_units: 100,
                },
            ])
            .expect("valid demand distribution"),
            baseline_order_quantity_units: 50,
            unit_price_minor: 1_000,
            unit_purchase_cost_minor: 400,
            unit_salvage_value_minor: 100,
            stockout_penalty_minor_per_unit: 50,
        }
    }

    #[test]
    fn inventory_engine_accounts_for_stockouts() {
        let signature = InventoryEngine.baseline(&state()).expect("valid state");
        assert_eq!(signature.order_quantity_units, 50);
        assert_eq!(signature.stockout_probability_ppm, 500_000);
        assert_eq!(signature.leftover_probability_ppm, 0);
        assert_eq!(signature.expected_profit_minor_trunc, 28_750);
    }

    #[test]
    fn larger_order_quantity_changes_profit_and_leftovers() {
        let signature = InventoryEngine
            .evaluate(
                &state(),
                &InventoryIntervention {
                    order_quantity_units: 100,
                },
            )
            .expect("valid order quantity");
        assert_eq!(signature.stockout_probability_ppm, 0);
        assert_eq!(signature.leftover_probability_ppm, 500_000);
        assert_eq!(signature.expected_profit_minor_trunc, 37_500);
    }

    #[test]
    fn demand_distribution_requires_unit_probability_mass() {
        assert_eq!(
            DemandDistribution::new(vec![DemandOutcome {
                probability_ppm: 900_000,
                demand_units: 10,
            }]),
            Err(InventoryError::ProbabilityMassMismatch {
                actual_ppm: 900_000
            })
        );
    }
}
