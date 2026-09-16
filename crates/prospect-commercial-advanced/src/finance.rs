use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;
use prospect_core::{DecisionPolicy, ProspectiveEngine};
use std::cmp::Reverse;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FinanceError {
    EmptyCashFlowPlan,
    NegativeField(&'static str),
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for FinanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCashFlowPlan => formatter.write_str("cash-flow plan must not be empty"),
            Self::NegativeField(field) => write!(formatter, "{field} must be non-negative"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
        }
    }
}

impl std::error::Error for FinanceError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CashFlowPlan {
    cash_flows_minor: Vec<i64>,
}

impl CashFlowPlan {
    pub fn new(cash_flows_minor: Vec<i64>) -> Result<Self, FinanceError> {
        if cash_flows_minor.is_empty() {
            return Err(FinanceError::EmptyCashFlowPlan);
        }
        Ok(Self { cash_flows_minor })
    }

    #[must_use]
    pub fn cash_flows_minor(&self) -> &[i64] {
        &self.cash_flows_minor
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CashFlowIntervention {
    pub plan: CashFlowPlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscountedCashFlowSignature {
    pub undiscounted_total_minor: i128,
    pub npv_minor_trunc: i128,
    pub payback_period: Option<usize>,
    pub discounted_payback_period: Option<usize>,
    pub minimum_cumulative_cash_minor: i128,
    pub peak_capital_required_minor: i128,
    pub final_discount_factor_ppm: u64,
}

/// Deterministic DCF engine using an integer parts-per-million discount rate.
///
/// Each period's discount factor is represented in ppm and truncated toward
/// zero. This makes the calculation reproducible and avoids binary floating
/// point, at the cost of caller-visible fixed-point approximation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscountedCashFlowEngine {
    discount_rate_ppm: u32,
}

impl DiscountedCashFlowEngine {
    #[must_use]
    pub const fn new(discount_rate_ppm: u32) -> Self {
        Self { discount_rate_ppm }
    }

    #[must_use]
    pub const fn discount_rate_ppm(&self) -> u32 {
        self.discount_rate_ppm
    }
}

impl ProspectiveEngine<CashFlowPlan, CashFlowIntervention> for DiscountedCashFlowEngine {
    type Signature = DiscountedCashFlowSignature;
    type Error = FinanceError;

    fn baseline(&self, state: &CashFlowPlan) -> Result<Self::Signature, Self::Error> {
        summarize_cash_flows(state, self.discount_rate_ppm)
    }

    fn evaluate(
        &self,
        _state: &CashFlowPlan,
        intervention: &CashFlowIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        summarize_cash_flows(&intervention.plan, self.discount_rate_ppm)
    }
}

fn summarize_cash_flows(
    plan: &CashFlowPlan,
    discount_rate_ppm: u32,
) -> Result<DiscountedCashFlowSignature, FinanceError> {
    let scale = u64::from(PROBABILITY_SCALE_PPM);
    let denominator = scale
        .checked_add(u64::from(discount_rate_ppm))
        .ok_or(FinanceError::ArithmeticOverflow("discount denominator"))?;
    let mut discount_factor_ppm = scale;
    let mut undiscounted_total_minor = 0_i128;
    let mut npv_minor_trunc = 0_i128;
    let mut cumulative_cash_minor = 0_i128;
    let mut cumulative_discounted_minor = 0_i128;
    let mut minimum_cumulative_cash_minor = 0_i128;
    let mut payback_period = None;
    let mut discounted_payback_period = None;

    for (period, cash_flow_minor) in plan.cash_flows_minor().iter().copied().enumerate() {
        let cash = i128::from(cash_flow_minor);
        undiscounted_total_minor = undiscounted_total_minor
            .checked_add(cash)
            .ok_or(FinanceError::ArithmeticOverflow("cash-flow total"))?;
        cumulative_cash_minor = cumulative_cash_minor
            .checked_add(cash)
            .ok_or(FinanceError::ArithmeticOverflow("cumulative cash flow"))?;
        minimum_cumulative_cash_minor = minimum_cumulative_cash_minor.min(cumulative_cash_minor);
        if payback_period.is_none() && cumulative_cash_minor >= 0 {
            payback_period = Some(period);
        }

        let discounted = cash
            .checked_mul(i128::from(discount_factor_ppm))
            .ok_or(FinanceError::ArithmeticOverflow("discounted cash flow"))?
            / i128::from(scale);
        npv_minor_trunc = npv_minor_trunc
            .checked_add(discounted)
            .ok_or(FinanceError::ArithmeticOverflow("net present value"))?;
        cumulative_discounted_minor = cumulative_discounted_minor.checked_add(discounted).ok_or(
            FinanceError::ArithmeticOverflow("cumulative discounted cash flow"),
        )?;
        if discounted_payback_period.is_none() && cumulative_discounted_minor >= 0 {
            discounted_payback_period = Some(period);
        }

        discount_factor_ppm = discount_factor_ppm
            .checked_mul(scale)
            .ok_or(FinanceError::ArithmeticOverflow("discount factor"))?
            / denominator;
    }

    let peak_capital_required_minor = if minimum_cumulative_cash_minor < 0 {
        minimum_cumulative_cash_minor.saturating_neg()
    } else {
        0
    };

    Ok(DiscountedCashFlowSignature {
        undiscounted_total_minor,
        npv_minor_trunc,
        payback_period,
        discounted_payback_period,
        minimum_cumulative_cash_minor,
        peak_capital_required_minor,
        final_discount_factor_ppm: discount_factor_ppm,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizeNpv;

impl DecisionPolicy<DiscountedCashFlowSignature> for MaximizeNpv {
    type Score = i128;

    fn utility(&self, signature: &DiscountedCashFlowSignature) -> Self::Score {
        signature.npv_minor_trunc
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreferDiscountedPaybackThenNpv;

impl DecisionPolicy<DiscountedCashFlowSignature> for PreferDiscountedPaybackThenNpv {
    type Score = (bool, Reverse<usize>, i128);

    fn utility(&self, signature: &DiscountedCashFlowSignature) -> Self::Score {
        match signature.discounted_payback_period {
            Some(period) => (true, Reverse(period), signature.npv_minor_trunc),
            None => (false, Reverse(usize::MAX), signature.npv_minor_trunc),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BreakEvenState {
    pub current_units: u64,
    pub unit_price_minor: i64,
    pub unit_variable_cost_minor: i64,
    pub fixed_cost_minor: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BreakEvenIntervention {
    pub current_units_delta: i64,
    pub unit_price_delta_minor: i64,
    pub unit_variable_cost_delta_minor: i64,
    pub fixed_cost_delta_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BreakEvenSignature {
    pub contribution_margin_minor_per_unit: i128,
    pub break_even_units_ceil: Option<u64>,
    pub margin_of_safety_units: Option<i128>,
    pub current_operating_profit_minor: i128,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BreakEvenEngine;

impl ProspectiveEngine<BreakEvenState, BreakEvenIntervention> for BreakEvenEngine {
    type Signature = BreakEvenSignature;
    type Error = FinanceError;

    fn baseline(&self, state: &BreakEvenState) -> Result<Self::Signature, Self::Error> {
        summarize_break_even(*state)
    }

    fn evaluate(
        &self,
        state: &BreakEvenState,
        intervention: &BreakEvenIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        validate_break_even_state(state)?;
        let current_units = apply_u64_delta(
            state.current_units,
            intervention.current_units_delta,
            "current_units",
        )?;
        let adjusted = BreakEvenState {
            current_units,
            unit_price_minor: apply_money_delta(
                state.unit_price_minor,
                intervention.unit_price_delta_minor,
                "unit_price_minor",
            )?,
            unit_variable_cost_minor: apply_money_delta(
                state.unit_variable_cost_minor,
                intervention.unit_variable_cost_delta_minor,
                "unit_variable_cost_minor",
            )?,
            fixed_cost_minor: apply_money_delta(
                state.fixed_cost_minor,
                intervention.fixed_cost_delta_minor,
                "fixed_cost_minor",
            )?,
        };
        summarize_break_even(adjusted)
    }
}

fn validate_break_even_state(state: &BreakEvenState) -> Result<(), FinanceError> {
    for (name, value) in [
        ("unit_price_minor", state.unit_price_minor),
        ("unit_variable_cost_minor", state.unit_variable_cost_minor),
        ("fixed_cost_minor", state.fixed_cost_minor),
    ] {
        if value < 0 {
            return Err(FinanceError::NegativeField(name));
        }
    }
    Ok(())
}

fn apply_money_delta(base: i64, delta: i64, field: &'static str) -> Result<i64, FinanceError> {
    let adjusted = base
        .checked_add(delta)
        .ok_or(FinanceError::ArithmeticOverflow(field))?;
    if adjusted < 0 {
        return Err(FinanceError::NegativeField(field));
    }
    Ok(adjusted)
}

fn apply_u64_delta(base: u64, delta: i64, field: &'static str) -> Result<u64, FinanceError> {
    let adjusted = i128::from(base) + i128::from(delta);
    if !(0..=i128::from(u64::MAX)).contains(&adjusted) {
        return Err(FinanceError::NegativeField(field));
    }
    u64::try_from(adjusted).map_err(|_| FinanceError::ArithmeticOverflow(field))
}

fn summarize_break_even(state: BreakEvenState) -> Result<BreakEvenSignature, FinanceError> {
    validate_break_even_state(&state)?;
    let margin = i128::from(state.unit_price_minor) - i128::from(state.unit_variable_cost_minor);
    let current_operating_profit_minor = i128::from(state.current_units)
        .checked_mul(margin)
        .and_then(|value| value.checked_sub(i128::from(state.fixed_cost_minor)))
        .ok_or(FinanceError::ArithmeticOverflow("operating profit"))?;

    let break_even_units_ceil = if state.fixed_cost_minor == 0 {
        Some(0)
    } else if margin <= 0 {
        None
    } else {
        let fixed = i128::from(state.fixed_cost_minor);
        let units = fixed
            .checked_add(margin - 1)
            .ok_or(FinanceError::ArithmeticOverflow("break-even numerator"))?
            / margin;
        Some(
            u64::try_from(units)
                .map_err(|_| FinanceError::ArithmeticOverflow("break-even units"))?,
        )
    };
    let margin_of_safety_units = break_even_units_ceil
        .map(|break_even| i128::from(state.current_units) - i128::from(break_even));

    Ok(BreakEvenSignature {
        contribution_margin_minor_per_unit: margin,
        break_even_units_ceil,
        margin_of_safety_units,
        current_operating_profit_minor,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizeMarginOfSafety;

impl DecisionPolicy<BreakEvenSignature> for MaximizeMarginOfSafety {
    type Score = (bool, i128, i128);

    fn utility(&self, signature: &BreakEvenSignature) -> Self::Score {
        match signature.margin_of_safety_units {
            Some(margin) => (true, margin, signature.current_operating_profit_minor),
            None => (false, i128::MIN, signature.current_operating_profit_minor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dcf_zero_rate_matches_plain_total() {
        let plan = CashFlowPlan::new(vec![-10_000, 6_000, 6_000]).expect("valid plan");
        let signature = DiscountedCashFlowEngine::new(0)
            .baseline(&plan)
            .expect("valid dcf");
        assert_eq!(signature.undiscounted_total_minor, 2_000);
        assert_eq!(signature.npv_minor_trunc, 2_000);
        assert_eq!(signature.payback_period, Some(2));
        assert_eq!(signature.discounted_payback_period, Some(2));
    }

    #[test]
    fn dcf_applies_fixed_point_discounting() {
        let plan = CashFlowPlan::new(vec![-10_000, 6_000, 6_000]).expect("valid plan");
        let signature = DiscountedCashFlowEngine::new(100_000)
            .baseline(&plan)
            .expect("valid dcf");
        assert_eq!(signature.npv_minor_trunc, 412);
        assert_eq!(signature.discounted_payback_period, Some(2));
        assert_eq!(signature.peak_capital_required_minor, 10_000);
    }

    #[test]
    fn break_even_is_ceiled_exactly() {
        let state = BreakEvenState {
            current_units: 120,
            unit_price_minor: 1_000,
            unit_variable_cost_minor: 400,
            fixed_cost_minor: 20_000,
        };
        let signature = BreakEvenEngine.baseline(&state).expect("valid state");
        assert_eq!(signature.contribution_margin_minor_per_unit, 600);
        assert_eq!(signature.break_even_units_ceil, Some(34));
        assert_eq!(signature.margin_of_safety_units, Some(86));
        assert_eq!(signature.current_operating_profit_minor, 52_000);
    }

    #[test]
    fn break_even_is_unreachable_with_nonpositive_margin() {
        let state = BreakEvenState {
            current_units: 10,
            unit_price_minor: 400,
            unit_variable_cost_minor: 500,
            fixed_cost_minor: 1_000,
        };
        let signature = BreakEvenEngine.baseline(&state).expect("valid state");
        assert_eq!(signature.break_even_units_ceil, None);
        assert_eq!(signature.margin_of_safety_units, None);
    }
}
