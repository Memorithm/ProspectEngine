use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;
use prospect_core::{DecisionPolicy, ProspectiveEngine};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextBestActionError {
    EmptyActionSet,
    InvalidActionIndex(usize),
    IneligibleAction(usize),
    PropensityOutOfRange(u32),
    NegativeField(&'static str),
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for NextBestActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyActionSet => formatter.write_str("next-best-action set must not be empty"),
            Self::InvalidActionIndex(index) => write!(formatter, "invalid action index: {index}"),
            Self::IneligibleAction(index) => write!(formatter, "action {index} is not eligible"),
            Self::PropensityOutOfRange(value) => write!(
                formatter,
                "action propensity must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::NegativeField(field) => write!(formatter, "{field} must be non-negative"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
        }
    }
}

impl std::error::Error for NextBestActionError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionCandidate {
    pub eligible: bool,
    /// Caller-supplied probability/propensity of success.
    pub success_propensity_ppm: u32,
    pub success_value_minor: i64,
    pub failure_value_minor: i64,
    pub attempt_cost_minor: i64,
    /// Explicit reserve for compliance, churn, capacity or other downside.
    pub risk_reserve_minor: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextBestActionState {
    actions: Vec<ActionCandidate>,
    baseline_action_index: Option<usize>,
}

impl NextBestActionState {
    pub fn new(
        actions: Vec<ActionCandidate>,
        baseline_action_index: Option<usize>,
    ) -> Result<Self, NextBestActionError> {
        if actions.is_empty() {
            return Err(NextBestActionError::EmptyActionSet);
        }
        for action in &actions {
            validate_action(*action)?;
        }
        if let Some(index) = baseline_action_index {
            if index >= actions.len() {
                return Err(NextBestActionError::InvalidActionIndex(index));
            }
            if !actions[index].eligible {
                return Err(NextBestActionError::IneligibleAction(index));
            }
        }
        Ok(Self {
            actions,
            baseline_action_index,
        })
    }

    #[must_use]
    pub fn actions(&self) -> &[ActionCandidate] {
        &self.actions
    }

    #[must_use]
    pub fn eligible_action_indices(&self) -> Vec<usize> {
        self.actions
            .iter()
            .enumerate()
            .filter_map(|(index, action)| action.eligible.then_some(index))
            .collect()
    }
}

fn validate_action(action: ActionCandidate) -> Result<(), NextBestActionError> {
    if action.success_propensity_ppm > PROBABILITY_SCALE_PPM {
        return Err(NextBestActionError::PropensityOutOfRange(
            action.success_propensity_ppm,
        ));
    }
    if action.attempt_cost_minor < 0 {
        return Err(NextBestActionError::NegativeField("attempt_cost_minor"));
    }
    if action.risk_reserve_minor < 0 {
        return Err(NextBestActionError::NegativeField("risk_reserve_minor"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NextBestActionIntervention {
    pub action_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NextBestActionSignature {
    pub action_index: Option<usize>,
    pub success_propensity_ppm: u32,
    pub expected_value_weighted_minor_ppm: i128,
    pub expected_value_minor_trunc: i128,
    pub certain_cost_minor: i128,
}

/// One-step next-best-action evaluator. Predictive propensities remain external
/// evidence; the engine performs deterministic arbitration over supplied values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NextBestActionEngine;

impl ProspectiveEngine<NextBestActionState, NextBestActionIntervention> for NextBestActionEngine {
    type Signature = NextBestActionSignature;
    type Error = NextBestActionError;

    fn baseline(&self, state: &NextBestActionState) -> Result<Self::Signature, Self::Error> {
        match state.baseline_action_index {
            Some(index) => summarize_action(state, index),
            None => Ok(NextBestActionSignature {
                action_index: None,
                success_propensity_ppm: 0,
                expected_value_weighted_minor_ppm: 0,
                expected_value_minor_trunc: 0,
                certain_cost_minor: 0,
            }),
        }
    }

    fn evaluate(
        &self,
        state: &NextBestActionState,
        intervention: &NextBestActionIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        summarize_action(state, intervention.action_index)
    }
}

fn summarize_action(
    state: &NextBestActionState,
    action_index: usize,
) -> Result<NextBestActionSignature, NextBestActionError> {
    let action = *state
        .actions()
        .get(action_index)
        .ok_or(NextBestActionError::InvalidActionIndex(action_index))?;
    if !action.eligible {
        return Err(NextBestActionError::IneligibleAction(action_index));
    }
    let scale = i128::from(PROBABILITY_SCALE_PPM);
    let success_weight = i128::from(action.success_propensity_ppm);
    let failure_weight = scale - success_weight;
    let certain_cost_minor = i128::from(action.attempt_cost_minor)
        .checked_add(i128::from(action.risk_reserve_minor))
        .ok_or(NextBestActionError::ArithmeticOverflow("certain action cost"))?;
    let expected_value_weighted_minor_ppm = i128::from(action.success_value_minor)
        .checked_mul(success_weight)
        .and_then(|value| {
            value.checked_add(i128::from(action.failure_value_minor) * failure_weight)
        })
        .and_then(|value| value.checked_sub(certain_cost_minor * scale))
        .ok_or(NextBestActionError::ArithmeticOverflow(
            "next-best-action expected value",
        ))?;

    Ok(NextBestActionSignature {
        action_index: Some(action_index),
        success_propensity_ppm: action.success_propensity_ppm,
        expected_value_weighted_minor_ppm,
        expected_value_minor_trunc: expected_value_weighted_minor_ppm / scale,
        certain_cost_minor,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizeNextBestActionValue;

impl DecisionPolicy<NextBestActionSignature> for MaximizeNextBestActionValue {
    type Score = i128;

    fn utility(&self, signature: &NextBestActionSignature) -> Self::Score {
        signature.expected_value_weighted_minor_ppm
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PropensityThenValue;

impl DecisionPolicy<NextBestActionSignature> for PropensityThenValue {
    type Score = (u32, i128);

    fn utility(&self, signature: &NextBestActionSignature) -> Self::Score {
        (
            signature.success_propensity_ppm,
            signature.expected_value_weighted_minor_ppm,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> NextBestActionState {
        NextBestActionState::new(
            vec![
                ActionCandidate {
                    eligible: true,
                    success_propensity_ppm: 200_000,
                    success_value_minor: 10_000,
                    failure_value_minor: 0,
                    attempt_cost_minor: 500,
                    risk_reserve_minor: 100,
                },
                ActionCandidate {
                    eligible: true,
                    success_propensity_ppm: 600_000,
                    success_value_minor: 4_000,
                    failure_value_minor: 0,
                    attempt_cost_minor: 200,
                    risk_reserve_minor: 100,
                },
                ActionCandidate {
                    eligible: false,
                    success_propensity_ppm: 900_000,
                    success_value_minor: 100_000,
                    failure_value_minor: 0,
                    attempt_cost_minor: 0,
                    risk_reserve_minor: 0,
                },
            ],
            None,
        )
        .expect("valid action set")
    }

    #[test]
    fn next_best_action_uses_expected_business_value() {
        let first = NextBestActionEngine
            .evaluate(&state(), &NextBestActionIntervention { action_index: 0 })
            .expect("eligible action");
        let second = NextBestActionEngine
            .evaluate(&state(), &NextBestActionIntervention { action_index: 1 })
            .expect("eligible action");
        assert_eq!(first.expected_value_minor_trunc, 1_400);
        assert_eq!(second.expected_value_minor_trunc, 2_100);
        assert!(MaximizeNextBestActionValue.utility(&second) > MaximizeNextBestActionValue.utility(&first));
    }

    #[test]
    fn no_action_baseline_is_explicit() {
        let baseline = NextBestActionEngine.baseline(&state()).expect("valid no-op");
        assert_eq!(baseline.action_index, None);
        assert_eq!(baseline.expected_value_minor_trunc, 0);
    }

    #[test]
    fn ineligible_action_fails_closed() {
        assert_eq!(
            NextBestActionEngine.evaluate(
                &state(),
                &NextBestActionIntervention { action_index: 2 }
            ),
            Err(NextBestActionError::IneligibleAction(2))
        );
    }
}
