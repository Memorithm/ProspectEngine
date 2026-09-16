use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;
use prospect_core::{DecisionPolicy, ProspectiveEngine};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RobustError {
    EmptyActionSet,
    EmptyScenarioSet,
    RaggedPayoffMatrix,
    InvalidActionIndex(usize),
    InvalidOptimismPpm(u32),
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for RobustError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyActionSet => {
                formatter.write_str("robust decision requires at least one action")
            }
            Self::EmptyScenarioSet => {
                formatter.write_str("robust decision requires at least one scenario")
            }
            Self::RaggedPayoffMatrix => {
                formatter.write_str("all robust action payoff rows must have equal length")
            }
            Self::InvalidActionIndex(index) => write!(formatter, "invalid action index: {index}"),
            Self::InvalidOptimismPpm(value) => write!(
                formatter,
                "optimism must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
        }
    }
}

impl std::error::Error for RobustError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RobustDecisionProblem {
    payoffs_minor: Vec<Vec<i64>>,
    baseline_action_index: usize,
}

impl RobustDecisionProblem {
    pub fn new(
        payoffs_minor: Vec<Vec<i64>>,
        baseline_action_index: usize,
    ) -> Result<Self, RobustError> {
        if payoffs_minor.is_empty() {
            return Err(RobustError::EmptyActionSet);
        }
        if baseline_action_index >= payoffs_minor.len() {
            return Err(RobustError::InvalidActionIndex(baseline_action_index));
        }
        let scenario_count = payoffs_minor[0].len();
        if scenario_count == 0 {
            return Err(RobustError::EmptyScenarioSet);
        }
        if payoffs_minor.iter().any(|row| row.len() != scenario_count) {
            return Err(RobustError::RaggedPayoffMatrix);
        }
        Ok(Self {
            payoffs_minor,
            baseline_action_index,
        })
    }

    #[must_use]
    pub fn payoffs_minor(&self) -> &[Vec<i64>] {
        &self.payoffs_minor
    }

    #[must_use]
    pub const fn baseline_action_index(&self) -> usize {
        self.baseline_action_index
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RobustIntervention {
    pub action_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RobustSignature {
    pub action_index: usize,
    pub worst_case_payoff_minor: i64,
    pub best_case_payoff_minor: i64,
    pub average_payoff_minor_trunc: i128,
    pub maximum_regret_minor: i128,
    pub hurwicz_weighted_minor_ppm: i128,
}

/// Decision engine for deep uncertainty where scenario probabilities are not
/// asserted. It exposes maximin, minimax-regret, average and Hurwicz summaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RobustDecisionEngine {
    optimism_ppm: u32,
}

impl RobustDecisionEngine {
    pub fn new(optimism_ppm: u32) -> Result<Self, RobustError> {
        if optimism_ppm > PROBABILITY_SCALE_PPM {
            return Err(RobustError::InvalidOptimismPpm(optimism_ppm));
        }
        Ok(Self { optimism_ppm })
    }

    #[must_use]
    pub const fn optimism_ppm(&self) -> u32 {
        self.optimism_ppm
    }
}

impl ProspectiveEngine<RobustDecisionProblem, RobustIntervention> for RobustDecisionEngine {
    type Signature = RobustSignature;
    type Error = RobustError;

    fn baseline(&self, state: &RobustDecisionProblem) -> Result<Self::Signature, Self::Error> {
        summarize_action(state, state.baseline_action_index(), self.optimism_ppm)
    }

    fn evaluate(
        &self,
        state: &RobustDecisionProblem,
        intervention: &RobustIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        summarize_action(state, intervention.action_index, self.optimism_ppm)
    }
}

fn summarize_action(
    problem: &RobustDecisionProblem,
    action_index: usize,
    optimism_ppm: u32,
) -> Result<RobustSignature, RobustError> {
    let selected = problem
        .payoffs_minor()
        .get(action_index)
        .ok_or(RobustError::InvalidActionIndex(action_index))?;
    let worst_case_payoff_minor = *selected
        .iter()
        .min()
        .expect("validated non-empty scenario row");
    let best_case_payoff_minor = *selected
        .iter()
        .max()
        .expect("validated non-empty scenario row");
    let payoff_sum = selected.iter().try_fold(0_i128, |sum, payoff| {
        sum.checked_add(i128::from(*payoff))
            .ok_or(RobustError::ArithmeticOverflow("average payoff"))
    })?;
    let average_payoff_minor_trunc =
        payoff_sum / i128::try_from(selected.len()).expect("usize fits i128");

    let mut maximum_regret_minor = 0_i128;
    for scenario_index in 0..selected.len() {
        let best_scenario_payoff = problem
            .payoffs_minor()
            .iter()
            .map(|row| row[scenario_index])
            .max()
            .expect("validated non-empty action set");
        let regret = i128::from(best_scenario_payoff) - i128::from(selected[scenario_index]);
        maximum_regret_minor = maximum_regret_minor.max(regret);
    }

    let pessimism_ppm = PROBABILITY_SCALE_PPM - optimism_ppm;
    let hurwicz_weighted_minor_ppm = i128::from(best_case_payoff_minor)
        .checked_mul(i128::from(optimism_ppm))
        .and_then(|value| {
            value.checked_add(i128::from(worst_case_payoff_minor) * i128::from(pessimism_ppm))
        })
        .ok_or(RobustError::ArithmeticOverflow("Hurwicz utility"))?;

    Ok(RobustSignature {
        action_index,
        worst_case_payoff_minor,
        best_case_payoff_minor,
        average_payoff_minor_trunc,
        maximum_regret_minor,
        hurwicz_weighted_minor_ppm,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Maximin;

impl DecisionPolicy<RobustSignature> for Maximin {
    type Score = i64;

    fn utility(&self, signature: &RobustSignature) -> Self::Score {
        signature.worst_case_payoff_minor
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MinimaxRegret;

impl DecisionPolicy<RobustSignature> for MinimaxRegret {
    type Score = i128;

    fn utility(&self, signature: &RobustSignature) -> Self::Score {
        signature.maximum_regret_minor.saturating_neg()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizeHurwicz;

impl DecisionPolicy<RobustSignature> for MaximizeHurwicz {
    type Score = i128;

    fn utility(&self, signature: &RobustSignature) -> Self::Score {
        signature.hurwicz_weighted_minor_ppm
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizeScenarioAverage;

impl DecisionPolicy<RobustSignature> for MaximizeScenarioAverage {
    type Score = i128;

    fn utility(&self, signature: &RobustSignature) -> Self::Score {
        signature.average_payoff_minor_trunc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem() -> RobustDecisionProblem {
        RobustDecisionProblem::new(
            vec![
                vec![100, 100, 100],
                vec![-100, 300, 500],
                vec![50, 150, 250],
            ],
            0,
        )
        .expect("valid payoff matrix")
    }

    #[test]
    fn robust_engine_computes_regret_and_extremes() {
        let engine = RobustDecisionEngine::new(500_000).expect("valid optimism");
        let signature = engine
            .evaluate(&problem(), &RobustIntervention { action_index: 1 })
            .expect("valid action");
        assert_eq!(signature.worst_case_payoff_minor, -100);
        assert_eq!(signature.best_case_payoff_minor, 500);
        assert_eq!(signature.average_payoff_minor_trunc, 233);
        assert_eq!(signature.maximum_regret_minor, 200);
        assert_eq!(signature.hurwicz_weighted_minor_ppm, 200_000_000);
    }

    #[test]
    fn maximin_and_minimax_regret_can_disagree_with_average() {
        let engine = RobustDecisionEngine::new(500_000).expect("valid optimism");
        let safe = engine.baseline(&problem()).expect("valid baseline");
        let risky = engine
            .evaluate(&problem(), &RobustIntervention { action_index: 1 })
            .expect("valid risky action");
        assert!(Maximin.utility(&safe) > Maximin.utility(&risky));
        assert!(MaximizeScenarioAverage.utility(&risky) > MaximizeScenarioAverage.utility(&safe));
    }
}
