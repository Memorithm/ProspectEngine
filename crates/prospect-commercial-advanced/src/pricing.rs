use core::fmt;
use prospect_core::{DecisionPolicy, ProspectiveEngine};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PricingError {
    EmptyCurve,
    DuplicatePrice(i64),
    PriceNotInCurve(i64),
    NegativeField(&'static str),
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for PricingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCurve => formatter.write_str("price-demand curve must not be empty"),
            Self::DuplicatePrice(price) => write!(formatter, "duplicate price point: {price}"),
            Self::PriceNotInCurve(price) => {
                write!(formatter, "price is not present in curve: {price}")
            }
            Self::NegativeField(field) => write!(formatter, "{field} must be non-negative"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
        }
    }
}

impl std::error::Error for PricingError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriceDemandPoint {
    pub price_minor: i64,
    pub demand_units: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceDemandCurve {
    points: Vec<PriceDemandPoint>,
}

impl PriceDemandCurve {
    pub fn new(mut points: Vec<PriceDemandPoint>) -> Result<Self, PricingError> {
        if points.is_empty() {
            return Err(PricingError::EmptyCurve);
        }
        if points.iter().any(|point| point.price_minor < 0) {
            return Err(PricingError::NegativeField("price_minor"));
        }
        points.sort_by_key(|point| point.price_minor);
        for pair in points.windows(2) {
            if pair[0].price_minor == pair[1].price_minor {
                return Err(PricingError::DuplicatePrice(pair[0].price_minor));
            }
        }
        Ok(Self { points })
    }

    #[must_use]
    pub fn points(&self) -> &[PriceDemandPoint] {
        &self.points
    }

    fn point(&self, price_minor: i64) -> Option<PriceDemandPoint> {
        self.points
            .binary_search_by_key(&price_minor, |point| point.price_minor)
            .ok()
            .map(|index| self.points[index])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingState {
    pub curve: PriceDemandCurve,
    pub baseline_price_minor: i64,
    pub capacity_units: u64,
    pub unit_variable_cost_minor: i64,
    pub fixed_cost_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PricingIntervention {
    pub price_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PricingSignature {
    pub price_minor: i64,
    pub demand_units: u64,
    pub served_units: u64,
    pub unmet_demand_units: u64,
    pub revenue_minor: i128,
    pub variable_cost_minor: i128,
    pub contribution_margin_minor_per_unit: i128,
    pub operating_profit_minor: i128,
}

/// Evaluates caller-supplied price-demand points without fitting or interpolating
/// a demand curve. Elasticity estimation belongs to a separate evidence layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PricingEngine;

impl ProspectiveEngine<PricingState, PricingIntervention> for PricingEngine {
    type Signature = PricingSignature;
    type Error = PricingError;

    fn baseline(&self, state: &PricingState) -> Result<Self::Signature, Self::Error> {
        evaluate_price(state, state.baseline_price_minor)
    }

    fn evaluate(
        &self,
        state: &PricingState,
        intervention: &PricingIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        evaluate_price(state, intervention.price_minor)
    }
}

fn validate_state(state: &PricingState) -> Result<(), PricingError> {
    if state.unit_variable_cost_minor < 0 {
        return Err(PricingError::NegativeField("unit_variable_cost_minor"));
    }
    if state.fixed_cost_minor < 0 {
        return Err(PricingError::NegativeField("fixed_cost_minor"));
    }
    Ok(())
}

fn evaluate_price(
    state: &PricingState,
    price_minor: i64,
) -> Result<PricingSignature, PricingError> {
    validate_state(state)?;
    let point = state
        .curve
        .point(price_minor)
        .ok_or(PricingError::PriceNotInCurve(price_minor))?;
    let served_units = point.demand_units.min(state.capacity_units);
    let unmet_demand_units = point.demand_units - served_units;
    let revenue_minor = i128::from(served_units)
        .checked_mul(i128::from(point.price_minor))
        .ok_or(PricingError::ArithmeticOverflow("pricing revenue"))?;
    let variable_cost_minor = i128::from(served_units)
        .checked_mul(i128::from(state.unit_variable_cost_minor))
        .ok_or(PricingError::ArithmeticOverflow("pricing variable cost"))?;
    let operating_profit_minor = revenue_minor
        .checked_sub(variable_cost_minor)
        .and_then(|value| value.checked_sub(i128::from(state.fixed_cost_minor)))
        .ok_or(PricingError::ArithmeticOverflow("pricing operating profit"))?;

    Ok(PricingSignature {
        price_minor: point.price_minor,
        demand_units: point.demand_units,
        served_units,
        unmet_demand_units,
        revenue_minor,
        variable_cost_minor,
        contribution_margin_minor_per_unit: i128::from(point.price_minor)
            - i128::from(state.unit_variable_cost_minor),
        operating_profit_minor,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizePricingProfit;

impl DecisionPolicy<PricingSignature> for MaximizePricingProfit {
    type Score = i128;

    fn utility(&self, signature: &PricingSignature) -> Self::Score {
        signature.operating_profit_minor
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizePricingRevenue;

impl DecisionPolicy<PricingSignature> for MaximizePricingRevenue {
    type Score = i128;

    fn utility(&self, signature: &PricingSignature) -> Self::Score {
        signature.revenue_minor
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfitThenServedVolume;

impl DecisionPolicy<PricingSignature> for ProfitThenServedVolume {
    type Score = (i128, u64);

    fn utility(&self, signature: &PricingSignature) -> Self::Score {
        (signature.operating_profit_minor, signature.served_units)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PricingState {
        PricingState {
            curve: PriceDemandCurve::new(vec![
                PriceDemandPoint {
                    price_minor: 800,
                    demand_units: 150,
                },
                PriceDemandPoint {
                    price_minor: 1_000,
                    demand_units: 120,
                },
                PriceDemandPoint {
                    price_minor: 1_200,
                    demand_units: 80,
                },
            ])
            .expect("valid curve"),
            baseline_price_minor: 1_000,
            capacity_units: 100,
            unit_variable_cost_minor: 400,
            fixed_cost_minor: 20_000,
        }
    }

    #[test]
    fn pricing_engine_respects_capacity() {
        let signature = PricingEngine
            .baseline(&state())
            .expect("valid pricing state");
        assert_eq!(signature.served_units, 100);
        assert_eq!(signature.unmet_demand_units, 20);
        assert_eq!(signature.revenue_minor, 100_000);
        assert_eq!(signature.operating_profit_minor, 40_000);
    }

    #[test]
    fn explicit_higher_price_can_be_compared_without_interpolation() {
        let signature = PricingEngine
            .evaluate(&state(), &PricingIntervention { price_minor: 1_200 })
            .expect("known price point");
        assert_eq!(signature.served_units, 80);
        assert_eq!(signature.operating_profit_minor, 44_000);
    }

    #[test]
    fn unknown_price_fails_closed() {
        assert_eq!(
            PricingEngine.evaluate(&state(), &PricingIntervention { price_minor: 1_100 }),
            Err(PricingError::PriceNotInCurve(1_100))
        );
    }
}
