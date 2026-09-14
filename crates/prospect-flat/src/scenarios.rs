use core::fmt;

use prospect_core::{InvalidScenarioId, Scenario, ScenarioId};

use crate::{FlatBooleanAttentionState, FlatBooleanContractError, FlatBooleanIntervention};

#[derive(Debug)]
pub enum FlatBooleanScenarioError {
    EmptyThresholds,
    DuplicateThreshold { threshold: usize },
    Contract(FlatBooleanContractError),
    InvalidScenarioId(InvalidScenarioId),
}

/// Build a deterministic bounded scenario set over exact FLAT Hamming rules.
///
/// Thresholds are sorted in ascending order before scenario construction and
/// duplicates are rejected. The dense baseline is intentionally not emitted as
/// a scenario because [`crate::FlatBooleanAttentionEngine::baseline`] already
/// owns that reference path.
pub fn hamming_threshold_scenarios(
    state: &FlatBooleanAttentionState,
    thresholds: impl IntoIterator<Item = usize>,
) -> Result<Vec<Scenario<FlatBooleanIntervention>>, FlatBooleanScenarioError> {
    let mut thresholds = thresholds.into_iter().collect::<Vec<_>>();
    if thresholds.is_empty() {
        return Err(FlatBooleanScenarioError::EmptyThresholds);
    }
    thresholds.sort_unstable();
    if let Some(pair) = thresholds.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(FlatBooleanScenarioError::DuplicateThreshold {
            threshold: pair[0],
        });
    }

    let mut scenarios = Vec::with_capacity(thresholds.len());
    for threshold in thresholds {
        state
            .hamming_mask(threshold)
            .map_err(FlatBooleanScenarioError::Contract)?;
        let id = ScenarioId::new(format!("flat-hamming-distance-{threshold}"))
            .map_err(FlatBooleanScenarioError::InvalidScenarioId)?;
        scenarios.push(Scenario::new(id, FlatBooleanIntervention::new(threshold)));
    }
    Ok(scenarios)
}

impl fmt::Display for FlatBooleanScenarioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyThresholds => formatter.write_str("at least one Hamming threshold is required"),
            Self::DuplicateThreshold { threshold } => {
                write!(formatter, "duplicate Hamming threshold {threshold}")
            }
            Self::Contract(error) => write!(formatter, "invalid FLAT Boolean scenario: {error}"),
            Self::InvalidScenarioId(error) => write!(formatter, "invalid scenario id: {error}"),
        }
    }
}

impl std::error::Error for FlatBooleanScenarioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(error) => Some(error),
            Self::InvalidScenarioId(error) => Some(error),
            Self::EmptyThresholds | Self::DuplicateThreshold { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use flat_attention::api::boolean_attention_signature::BooleanAttentionSignature;

    use super::{FlatBooleanScenarioError, hamming_threshold_scenarios};
    use crate::FlatBooleanAttentionState;

    fn signature(value: u64) -> BooleanAttentionSignature {
        BooleanAttentionSignature::new(4, vec![value]).expect("4-bit signature")
    }

    fn state() -> FlatBooleanAttentionState {
        FlatBooleanAttentionState::new(
            signature(0b1010),
            vec![
                signature(0b1010),
                signature(0b1110),
                signature(0b0000),
                signature(0b1011),
            ],
        )
        .expect("valid state")
    }

    #[test]
    fn sorts_thresholds_and_builds_stable_scenario_ids() {
        let scenarios = hamming_threshold_scenarios(&state(), [2, 0, 1]).expect("scenarios");
        let ids = scenarios
            .iter()
            .map(|scenario| scenario.id().as_str())
            .collect::<Vec<_>>();
        let thresholds = scenarios
            .iter()
            .map(|scenario| scenario.intervention().max_hamming_distance())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "flat-hamming-distance-0",
                "flat-hamming-distance-1",
                "flat-hamming-distance-2"
            ]
        );
        assert_eq!(thresholds, vec![0, 1, 2]);
    }

    #[test]
    fn rejects_empty_duplicate_and_out_of_range_threshold_sets() {
        assert!(matches!(
            hamming_threshold_scenarios(&state(), []),
            Err(FlatBooleanScenarioError::EmptyThresholds)
        ));
        assert!(matches!(
            hamming_threshold_scenarios(&state(), [1, 1]),
            Err(FlatBooleanScenarioError::DuplicateThreshold { threshold: 1 })
        ));
        assert!(matches!(
            hamming_threshold_scenarios(&state(), [5]),
            Err(FlatBooleanScenarioError::Contract(_))
        ));
    }
}
