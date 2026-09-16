#![forbid(unsafe_code)]

use core::fmt;
use prospect_core::{DecisionPolicy, ProspectiveEngine};
use std::cmp::Reverse;

/// Exact probability scale used by discrete commercial scenario models.
pub const PROBABILITY_SCALE_PPM: u32 = 1_000_000;

/// Errors returned when commercial inputs are invalid or cannot be represented exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommercialError {
    NegativeField(&'static str),
    UnitDeltaOutOfRange(&'static str),
    ArithmeticOverflow(&'static str),
    EmptyOutcomeSet,
    ZeroProbabilityOutcome,
    ProbabilityMassMismatch { actual_ppm: u64 },
    InvalidDownsideTail { tail_ppm: u32 },
}

impl fmt::Display for CommercialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NegativeField(field) => write!(formatter, "{field} must be non-negative"),
            Self::UnitDeltaOutOfRange(field) => {
                write!(
                    formatter,
                    "{field} adjustment is outside the representable range"
                )
            }
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
            Self::EmptyOutcomeSet => {
                formatter.write_str("commercial outcome set must not be empty")
            }
            Self::ZeroProbabilityOutcome => {
                formatter.write_str("commercial outcomes must have non-zero probability mass")
            }
            Self::ProbabilityMassMismatch { actual_ppm } => write!(
                formatter,
                "commercial outcome probabilities must sum to {PROBABILITY_SCALE_PPM} ppm, got {actual_ppm} ppm"
            ),
            Self::InvalidDownsideTail { tail_ppm } => write!(
                formatter,
                "downside tail must be in 1..={PROBABILITY_SCALE_PPM} ppm, got {tail_ppm}"
            ),
        }
    }
}

impl std::error::Error for CommercialError {}

/// One-period commercial state for an exact unit-economics calculation.
///
/// Monetary values are integer minor currency units chosen by the caller
/// (for example euro cents). The engine does not infer demand, prices or costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitEconomicsState {
    pub demand_units: u64,
    pub capacity_units: u64,
    pub unit_price_minor: i64,
    pub unit_variable_cost_minor: i64,
    pub fixed_cost_minor: i64,
}

/// Explicit changes applied to a [`UnitEconomicsState`].
///
/// `one_time_cash_effect_minor` is positive for an inflow and negative for an
/// outflow. All other monetary deltas adjust the corresponding state field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitEconomicsIntervention {
    pub demand_delta_units: i64,
    pub capacity_delta_units: i64,
    pub unit_price_delta_minor: i64,
    pub unit_variable_cost_delta_minor: i64,
    pub fixed_cost_delta_minor: i64,
    pub one_time_cash_effect_minor: i64,
}

/// Deterministic commercial signature produced from one-period unit economics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitEconomicsSignature {
    pub served_units: u64,
    pub unmet_demand_units: u64,
    pub unused_capacity_units: u64,
    pub revenue_minor: i128,
    pub variable_cost_minor: i128,
    pub fixed_cost_minor: i128,
    pub operating_profit_minor: i128,
    pub net_cash_contribution_minor: i128,
}

/// Exact deterministic unit-economics engine.
///
/// This model is intentionally narrow. It evaluates caller-supplied commercial
/// assumptions; it is not a demand forecast, pricing model or market predictor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitEconomicsEngine;

impl ProspectiveEngine<UnitEconomicsState, UnitEconomicsIntervention> for UnitEconomicsEngine {
    type Signature = UnitEconomicsSignature;
    type Error = CommercialError;

    fn baseline(&self, state: &UnitEconomicsState) -> Result<Self::Signature, Self::Error> {
        validate_unit_state(state)?;
        evaluate_unit_state(state, 0)
    }

    fn evaluate(
        &self,
        state: &UnitEconomicsState,
        intervention: &UnitEconomicsIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        validate_unit_state(state)?;
        let adjusted = UnitEconomicsState {
            demand_units: apply_unit_delta(
                state.demand_units,
                intervention.demand_delta_units,
                "demand_units",
            )?,
            capacity_units: apply_unit_delta(
                state.capacity_units,
                intervention.capacity_delta_units,
                "capacity_units",
            )?,
            unit_price_minor: apply_nonnegative_money_delta(
                state.unit_price_minor,
                intervention.unit_price_delta_minor,
                "unit_price_minor",
            )?,
            unit_variable_cost_minor: apply_nonnegative_money_delta(
                state.unit_variable_cost_minor,
                intervention.unit_variable_cost_delta_minor,
                "unit_variable_cost_minor",
            )?,
            fixed_cost_minor: apply_nonnegative_money_delta(
                state.fixed_cost_minor,
                intervention.fixed_cost_delta_minor,
                "fixed_cost_minor",
            )?,
        };

        evaluate_unit_state(&adjusted, intervention.one_time_cash_effect_minor)
    }
}

fn validate_unit_state(state: &UnitEconomicsState) -> Result<(), CommercialError> {
    if state.unit_price_minor < 0 {
        return Err(CommercialError::NegativeField("unit_price_minor"));
    }
    if state.unit_variable_cost_minor < 0 {
        return Err(CommercialError::NegativeField("unit_variable_cost_minor"));
    }
    if state.fixed_cost_minor < 0 {
        return Err(CommercialError::NegativeField("fixed_cost_minor"));
    }
    Ok(())
}

fn apply_unit_delta(base: u64, delta: i64, field: &'static str) -> Result<u64, CommercialError> {
    let adjusted = i128::from(base) + i128::from(delta);
    if !(0..=i128::from(u64::MAX)).contains(&adjusted) {
        return Err(CommercialError::UnitDeltaOutOfRange(field));
    }
    u64::try_from(adjusted).map_err(|_| CommercialError::UnitDeltaOutOfRange(field))
}

fn apply_nonnegative_money_delta(
    base: i64,
    delta: i64,
    field: &'static str,
) -> Result<i64, CommercialError> {
    let adjusted = base
        .checked_add(delta)
        .ok_or(CommercialError::ArithmeticOverflow(field))?;
    if adjusted < 0 {
        return Err(CommercialError::NegativeField(field));
    }
    Ok(adjusted)
}

fn evaluate_unit_state(
    state: &UnitEconomicsState,
    one_time_cash_effect_minor: i64,
) -> Result<UnitEconomicsSignature, CommercialError> {
    let served_units = state.demand_units.min(state.capacity_units);
    let unmet_demand_units = state.demand_units - served_units;
    let unused_capacity_units = state.capacity_units - served_units;

    let revenue_minor = i128::from(served_units)
        .checked_mul(i128::from(state.unit_price_minor))
        .ok_or(CommercialError::ArithmeticOverflow("revenue"))?;
    let variable_cost_minor = i128::from(served_units)
        .checked_mul(i128::from(state.unit_variable_cost_minor))
        .ok_or(CommercialError::ArithmeticOverflow("variable cost"))?;
    let operating_profit_minor = revenue_minor
        .checked_sub(variable_cost_minor)
        .and_then(|value| value.checked_sub(i128::from(state.fixed_cost_minor)))
        .ok_or(CommercialError::ArithmeticOverflow("operating profit"))?;
    let net_cash_contribution_minor = operating_profit_minor
        .checked_add(i128::from(one_time_cash_effect_minor))
        .ok_or(CommercialError::ArithmeticOverflow("net cash contribution"))?;

    Ok(UnitEconomicsSignature {
        served_units,
        unmet_demand_units,
        unused_capacity_units,
        revenue_minor,
        variable_cost_minor,
        fixed_cost_minor: i128::from(state.fixed_cost_minor),
        operating_profit_minor,
        net_cash_contribution_minor,
    })
}

/// Policy that ranks deterministic commercial signatures by net cash contribution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizeNetCash;

impl DecisionPolicy<UnitEconomicsSignature> for MaximizeNetCash {
    type Score = i128;

    fn utility(&self, signature: &UnitEconomicsSignature) -> Self::Score {
        signature.net_cash_contribution_minor
    }
}

/// Policy that penalizes unmet demand while retaining exact monetary arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapacityAwareNetCash {
    pub unmet_demand_penalty_minor_per_unit: u64,
}

impl DecisionPolicy<UnitEconomicsSignature> for CapacityAwareNetCash {
    type Score = i128;

    fn utility(&self, signature: &UnitEconomicsSignature) -> Self::Score {
        let penalty = i128::from(signature.unmet_demand_units)
            .saturating_mul(i128::from(self.unmet_demand_penalty_minor_per_unit));
        signature.net_cash_contribution_minor.saturating_sub(penalty)
    }
}

/// One explicitly enumerated commercial outcome and its probability mass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeightedCommercialOutcome {
    pub probability_ppm: u32,
    pub net_cash_minor: i64,
}

/// Validated discrete probability distribution for commercial outcomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommercialDistribution {
    outcomes: Vec<WeightedCommercialOutcome>,
}

impl CommercialDistribution {
    pub fn new(outcomes: Vec<WeightedCommercialOutcome>) -> Result<Self, CommercialError> {
        if outcomes.is_empty() {
            return Err(CommercialError::EmptyOutcomeSet);
        }
        if outcomes.iter().any(|outcome| outcome.probability_ppm == 0) {
            return Err(CommercialError::ZeroProbabilityOutcome);
        }
        let actual_ppm: u64 = outcomes
            .iter()
            .map(|outcome| u64::from(outcome.probability_ppm))
            .sum();
        if actual_ppm != u64::from(PROBABILITY_SCALE_PPM) {
            return Err(CommercialError::ProbabilityMassMismatch { actual_ppm });
        }
        Ok(Self { outcomes })
    }

    #[must_use]
    pub fn outcomes(&self) -> &[WeightedCommercialOutcome] {
        &self.outcomes
    }
}

/// Intervention that replaces the baseline distribution with an explicitly
/// supplied candidate distribution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommercialDistributionIntervention {
    pub distribution: CommercialDistribution,
}

/// Exact summary of one discrete commercial outcome distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommercialRiskSignature {
    /// Numerator of the expected value in `minor_currency_unit * ppm`.
    pub expected_net_cash_weighted_minor_ppm: i128,
    /// Floor of the exact expected value in minor currency units.
    pub expected_net_cash_minor_floor: i128,
    pub loss_probability_ppm: u32,
    pub worst_case_net_cash_minor: i64,
    pub best_case_net_cash_minor: i64,
    pub downside_tail_ppm: u32,
    /// Numerator of the lower-tail mean in `minor_currency_unit * ppm`.
    pub downside_tail_weighted_minor_ppm: i128,
    /// Floor of the lower-tail mean in minor currency units.
    pub downside_tail_mean_minor_floor: i128,
}

/// Discrete expected-value and lower-tail risk engine.
///
/// The engine consumes probabilities supplied by the caller. It does not infer
/// those probabilities from historical data and does not claim calibrated risk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioRiskEngine {
    downside_tail_ppm: u32,
}

impl ScenarioRiskEngine {
    pub fn new(downside_tail_ppm: u32) -> Result<Self, CommercialError> {
        if downside_tail_ppm == 0 || downside_tail_ppm > PROBABILITY_SCALE_PPM {
            return Err(CommercialError::InvalidDownsideTail {
                tail_ppm: downside_tail_ppm,
            });
        }
        Ok(Self { downside_tail_ppm })
    }

    #[must_use]
    pub const fn downside_tail_ppm(&self) -> u32 {
        self.downside_tail_ppm
    }
}

impl ProspectiveEngine<CommercialDistribution, CommercialDistributionIntervention>
    for ScenarioRiskEngine
{
    type Signature = CommercialRiskSignature;
    type Error = CommercialError;

    fn baseline(&self, state: &CommercialDistribution) -> Result<Self::Signature, Self::Error> {
        Ok(summarize_distribution(state, self.downside_tail_ppm))
    }

    fn evaluate(
        &self,
        _state: &CommercialDistribution,
        intervention: &CommercialDistributionIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        Ok(summarize_distribution(
            &intervention.distribution,
            self.downside_tail_ppm,
        ))
    }
}

fn summarize_distribution(
    distribution: &CommercialDistribution,
    downside_tail_ppm: u32,
) -> CommercialRiskSignature {
    let expected_net_cash_weighted_minor_ppm: i128 = distribution
        .outcomes()
        .iter()
        .map(|outcome| i128::from(outcome.probability_ppm) * i128::from(outcome.net_cash_minor))
        .sum();
    let expected_net_cash_minor_floor =
        expected_net_cash_weighted_minor_ppm.div_euclid(i128::from(PROBABILITY_SCALE_PPM));

    let loss_probability_ppm = distribution
        .outcomes()
        .iter()
        .filter(|outcome| outcome.net_cash_minor < 0)
        .map(|outcome| outcome.probability_ppm)
        .sum();
    let worst_case_net_cash_minor = distribution
        .outcomes()
        .iter()
        .map(|outcome| outcome.net_cash_minor)
        .min()
        .expect("validated distributions are non-empty");
    let best_case_net_cash_minor = distribution
        .outcomes()
        .iter()
        .map(|outcome| outcome.net_cash_minor)
        .max()
        .expect("validated distributions are non-empty");

    let mut ordered = distribution.outcomes().to_vec();
    ordered.sort_by_key(|outcome| outcome.net_cash_minor);
    let mut remaining_tail_ppm = downside_tail_ppm;
    let mut downside_tail_weighted_minor_ppm = 0_i128;
    for outcome in ordered {
        if remaining_tail_ppm == 0 {
            break;
        }
        let consumed_ppm = outcome.probability_ppm.min(remaining_tail_ppm);
        downside_tail_weighted_minor_ppm +=
            i128::from(consumed_ppm) * i128::from(outcome.net_cash_minor);
        remaining_tail_ppm -= consumed_ppm;
    }
    debug_assert_eq!(remaining_tail_ppm, 0);
    let downside_tail_mean_minor_floor =
        downside_tail_weighted_minor_ppm.div_euclid(i128::from(downside_tail_ppm));

    CommercialRiskSignature {
        expected_net_cash_weighted_minor_ppm,
        expected_net_cash_minor_floor,
        loss_probability_ppm,
        worst_case_net_cash_minor,
        best_case_net_cash_minor,
        downside_tail_ppm,
        downside_tail_weighted_minor_ppm,
        downside_tail_mean_minor_floor,
    }
}

/// Policy that maximizes exact expected net cash across distributions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizeExpectedNetCash;

impl DecisionPolicy<CommercialRiskSignature> for MaximizeExpectedNetCash {
    type Score = i128;

    fn utility(&self, signature: &CommercialRiskSignature) -> Self::Score {
        signature.expected_net_cash_weighted_minor_ppm
    }
}

/// Conservative lexicographic policy: improve lower-tail mean first, then
/// expected net cash when the downside score ties.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DownsideFirst;

impl DecisionPolicy<CommercialRiskSignature> for DownsideFirst {
    type Score = (i128, i128);

    fn utility(&self, signature: &CommercialRiskSignature) -> Self::Score {
        (
            signature.downside_tail_mean_minor_floor,
            signature.expected_net_cash_weighted_minor_ppm,
        )
    }
}

/// Lexicographic policy that minimizes loss probability before maximizing
/// expected net cash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MinimizeLossProbability;

impl DecisionPolicy<CommercialRiskSignature> for MinimizeLossProbability {
    type Score = (Reverse<u32>, i128);

    fn utility(&self, signature: &CommercialRiskSignature) -> Self::Score {
        (
            Reverse(signature.loss_probability_ppm),
            signature.expected_net_cash_weighted_minor_ppm,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_state() -> UnitEconomicsState {
        UnitEconomicsState {
            demand_units: 120,
            capacity_units: 100,
            unit_price_minor: 1_000,
            unit_variable_cost_minor: 400,
            fixed_cost_minor: 20_000,
        }
    }

    #[test]
    fn unit_economics_baseline_is_exact() {
        let signature = UnitEconomicsEngine
            .baseline(&base_state())
            .expect("valid state");

        assert_eq!(signature.served_units, 100);
        assert_eq!(signature.unmet_demand_units, 20);
        assert_eq!(signature.unused_capacity_units, 0);
        assert_eq!(signature.revenue_minor, 100_000);
        assert_eq!(signature.variable_cost_minor, 40_000);
        assert_eq!(signature.operating_profit_minor, 40_000);
        assert_eq!(signature.net_cash_contribution_minor, 40_000);
    }

    #[test]
    fn unit_economics_intervention_applies_explicit_changes_only() {
        let intervention = UnitEconomicsIntervention {
            capacity_delta_units: 50,
            unit_price_delta_minor: 100,
            unit_variable_cost_delta_minor: -50,
            fixed_cost_delta_minor: 5_000,
            one_time_cash_effect_minor: -10_000,
            ..UnitEconomicsIntervention::default()
        };

        let signature = UnitEconomicsEngine
            .evaluate(&base_state(), &intervention)
            .expect("valid intervention");

        assert_eq!(signature.served_units, 120);
        assert_eq!(signature.unmet_demand_units, 0);
        assert_eq!(signature.unused_capacity_units, 30);
        assert_eq!(signature.revenue_minor, 132_000);
        assert_eq!(signature.variable_cost_minor, 42_000);
        assert_eq!(signature.operating_profit_minor, 65_000);
        assert_eq!(signature.net_cash_contribution_minor, 55_000);
    }

    #[test]
    fn unit_economics_rejects_negative_adjusted_price() {
        let intervention = UnitEconomicsIntervention {
            unit_price_delta_minor: -1_001,
            ..UnitEconomicsIntervention::default()
        };

        assert_eq!(
            UnitEconomicsEngine.evaluate(&base_state(), &intervention),
            Err(CommercialError::NegativeField("unit_price_minor"))
        );
    }

    #[test]
    fn distribution_requires_exact_probability_mass() {
        assert_eq!(
            CommercialDistribution::new(vec![WeightedCommercialOutcome {
                probability_ppm: 999_999,
                net_cash_minor: 10,
            }]),
            Err(CommercialError::ProbabilityMassMismatch {
                actual_ppm: 999_999
            })
        );
    }

    #[test]
    fn risk_engine_computes_expected_value_loss_probability_and_tail() {
        let distribution = CommercialDistribution::new(vec![
            WeightedCommercialOutcome {
                probability_ppm: 500_000,
                net_cash_minor: -1_000,
            },
            WeightedCommercialOutcome {
                probability_ppm: 500_000,
                net_cash_minor: 3_000,
            },
        ])
        .expect("probabilities sum to one");
        let engine = ScenarioRiskEngine::new(250_000).expect("valid tail");
        let signature = engine.baseline(&distribution).expect("valid distribution");

        assert_eq!(signature.expected_net_cash_minor_floor, 1_000);
        assert_eq!(signature.loss_probability_ppm, 500_000);
        assert_eq!(signature.worst_case_net_cash_minor, -1_000);
        assert_eq!(signature.best_case_net_cash_minor, 3_000);
        assert_eq!(signature.downside_tail_mean_minor_floor, -1_000);
    }

    #[test]
    fn downside_tail_supports_partial_outcome_mass() {
        let distribution = CommercialDistribution::new(vec![
            WeightedCommercialOutcome {
                probability_ppm: 100_000,
                net_cash_minor: -1_000,
            },
            WeightedCommercialOutcome {
                probability_ppm: 900_000,
                net_cash_minor: 100,
            },
        ])
        .expect("probabilities sum to one");
        let engine = ScenarioRiskEngine::new(250_000).expect("valid tail");
        let signature = engine.baseline(&distribution).expect("valid distribution");

        assert_eq!(signature.downside_tail_mean_minor_floor, -340);
    }

    #[test]
    fn policies_expose_distinct_commercial_preferences() {
        let risky = CommercialRiskSignature {
            expected_net_cash_weighted_minor_ppm: 2_000 * i128::from(PROBABILITY_SCALE_PPM),
            expected_net_cash_minor_floor: 2_000,
            loss_probability_ppm: 400_000,
            worst_case_net_cash_minor: -5_000,
            best_case_net_cash_minor: 8_000,
            downside_tail_ppm: 100_000,
            downside_tail_weighted_minor_ppm: -5_000 * 100_000,
            downside_tail_mean_minor_floor: -5_000,
        };
        let stable = CommercialRiskSignature {
            expected_net_cash_weighted_minor_ppm: 1_500 * i128::from(PROBABILITY_SCALE_PPM),
            expected_net_cash_minor_floor: 1_500,
            loss_probability_ppm: 0,
            worst_case_net_cash_minor: 1_000,
            best_case_net_cash_minor: 2_000,
            downside_tail_ppm: 100_000,
            downside_tail_weighted_minor_ppm: 1_000 * 100_000,
            downside_tail_mean_minor_floor: 1_000,
        };

        assert!(MaximizeExpectedNetCash.utility(&risky) > MaximizeExpectedNetCash.utility(&stable));
        assert!(DownsideFirst.utility(&stable) > DownsideFirst.utility(&risky));
        assert!(MinimizeLossProbability.utility(&stable) > MinimizeLossProbability.utility(&risky));
    }
}
