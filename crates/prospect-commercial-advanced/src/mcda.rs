use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;
use prospect_core::{DecisionPolicy, ProspectiveEngine};
use std::cmp::Reverse;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McdaError {
    EmptyCriteria,
    EmptyAlternatives,
    WeightMassMismatch { actual_ppm: u64 },
    ValueOutOfRange { value_ppm: u32 },
    AlternativeWidthMismatch,
    InvalidAlternativeIndex(usize),
}

impl fmt::Display for McdaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCriteria => formatter.write_str("MCDA requires at least one criterion"),
            Self::EmptyAlternatives => {
                formatter.write_str("MCDA requires at least one alternative")
            }
            Self::WeightMassMismatch { actual_ppm } => write!(
                formatter,
                "criterion weights must sum to {PROBABILITY_SCALE_PPM} ppm, got {actual_ppm} ppm"
            ),
            Self::ValueOutOfRange { value_ppm } => write!(
                formatter,
                "normalized criterion value must be in 0..={PROBABILITY_SCALE_PPM}, got {value_ppm}"
            ),
            Self::AlternativeWidthMismatch => {
                formatter.write_str("every MCDA alternative must provide one value per criterion")
            }
            Self::InvalidAlternativeIndex(index) => {
                write!(formatter, "invalid MCDA alternative index: {index}")
            }
        }
    }
}

impl std::error::Error for McdaError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CriterionDirection {
    Maximize,
    Minimize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Criterion {
    pub weight_ppm: u32,
    pub direction: CriterionDirection,
    /// Optional veto expressed after direction normalization: zero is worst,
    /// one million ppm is best.
    pub minimum_desirability_ppm: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McdaProblem {
    criteria: Vec<Criterion>,
    alternatives: Vec<Vec<u32>>,
    baseline_alternative_index: usize,
}

impl McdaProblem {
    pub fn new(
        criteria: Vec<Criterion>,
        alternatives: Vec<Vec<u32>>,
        baseline_alternative_index: usize,
    ) -> Result<Self, McdaError> {
        if criteria.is_empty() {
            return Err(McdaError::EmptyCriteria);
        }
        if alternatives.is_empty() {
            return Err(McdaError::EmptyAlternatives);
        }
        let actual_ppm: u64 = criteria
            .iter()
            .map(|criterion| u64::from(criterion.weight_ppm))
            .sum();
        if actual_ppm != u64::from(PROBABILITY_SCALE_PPM) {
            return Err(McdaError::WeightMassMismatch { actual_ppm });
        }
        for criterion in &criteria {
            if let Some(value) = criterion.minimum_desirability_ppm {
                validate_normalized(value)?;
            }
        }
        for alternative in &alternatives {
            if alternative.len() != criteria.len() {
                return Err(McdaError::AlternativeWidthMismatch);
            }
            for value in alternative {
                validate_normalized(*value)?;
            }
        }
        if baseline_alternative_index >= alternatives.len() {
            return Err(McdaError::InvalidAlternativeIndex(
                baseline_alternative_index,
            ));
        }
        Ok(Self {
            criteria,
            alternatives,
            baseline_alternative_index,
        })
    }

    #[must_use]
    pub fn criteria(&self) -> &[Criterion] {
        &self.criteria
    }

    #[must_use]
    pub fn alternatives(&self) -> &[Vec<u32>] {
        &self.alternatives
    }
}

fn validate_normalized(value_ppm: u32) -> Result<(), McdaError> {
    if value_ppm > PROBABILITY_SCALE_PPM {
        return Err(McdaError::ValueOutOfRange { value_ppm });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McdaIntervention {
    pub alternative_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McdaSignature {
    pub alternative_index: usize,
    pub feasible: bool,
    pub violated_veto_count: u32,
    pub weighted_utility_numerator: u64,
    pub weighted_utility_ppm_trunc: u32,
    pub worst_desirability_ppm: u32,
}

/// Weighted normalized multi-criteria evaluator with explicit direction and veto
/// thresholds. Input normalization is a caller responsibility and is evidence,
/// not something the engine invents from heterogeneous units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct McdaEngine;

impl ProspectiveEngine<McdaProblem, McdaIntervention> for McdaEngine {
    type Signature = McdaSignature;
    type Error = McdaError;

    fn baseline(&self, state: &McdaProblem) -> Result<Self::Signature, Self::Error> {
        summarize_alternative(state, state.baseline_alternative_index)
    }

    fn evaluate(
        &self,
        state: &McdaProblem,
        intervention: &McdaIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        summarize_alternative(state, intervention.alternative_index)
    }
}

fn summarize_alternative(
    problem: &McdaProblem,
    alternative_index: usize,
) -> Result<McdaSignature, McdaError> {
    let alternative = problem
        .alternatives()
        .get(alternative_index)
        .ok_or(McdaError::InvalidAlternativeIndex(alternative_index))?;
    let mut violated_veto_count = 0_u32;
    let mut weighted_utility_numerator = 0_u64;
    let mut worst_desirability_ppm = PROBABILITY_SCALE_PPM;

    for (criterion, value) in problem.criteria().iter().zip(alternative) {
        let desirability = match criterion.direction {
            CriterionDirection::Maximize => *value,
            CriterionDirection::Minimize => PROBABILITY_SCALE_PPM - *value,
        };
        worst_desirability_ppm = worst_desirability_ppm.min(desirability);
        if criterion
            .minimum_desirability_ppm
            .is_some_and(|minimum| desirability < minimum)
        {
            violated_veto_count = violated_veto_count.saturating_add(1);
        }
        weighted_utility_numerator = weighted_utility_numerator.saturating_add(
            u64::from(criterion.weight_ppm).saturating_mul(u64::from(desirability)),
        );
    }

    Ok(McdaSignature {
        alternative_index,
        feasible: violated_veto_count == 0,
        violated_veto_count,
        weighted_utility_numerator,
        weighted_utility_ppm_trunc: u32::try_from(
            weighted_utility_numerator / u64::from(PROBABILITY_SCALE_PPM),
        )
        .expect("weighted normalized utility is bounded by one million ppm"),
        worst_desirability_ppm,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeasibleWeightedUtility;

impl DecisionPolicy<McdaSignature> for FeasibleWeightedUtility {
    type Score = (bool, Reverse<u32>, u64);

    fn utility(&self, signature: &McdaSignature) -> Self::Score {
        (
            signature.feasible,
            Reverse(signature.violated_veto_count),
            signature.weighted_utility_numerator,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BalancedWorstCriterionFirst;

impl DecisionPolicy<McdaSignature> for BalancedWorstCriterionFirst {
    type Score = (bool, u32, u64);

    fn utility(&self, signature: &McdaSignature) -> Self::Score {
        (
            signature.feasible,
            signature.worst_desirability_ppm,
            signature.weighted_utility_numerator,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem() -> McdaProblem {
        McdaProblem::new(
            vec![
                Criterion {
                    weight_ppm: 600_000,
                    direction: CriterionDirection::Maximize,
                    minimum_desirability_ppm: Some(300_000),
                },
                Criterion {
                    weight_ppm: 400_000,
                    direction: CriterionDirection::Minimize,
                    minimum_desirability_ppm: None,
                },
            ],
            vec![vec![700_000, 500_000], vec![900_000, 800_000]],
            0,
        )
        .expect("valid MCDA problem")
    }

    #[test]
    fn mcda_applies_direction_before_weighting() {
        let signature = McdaEngine.baseline(&problem()).expect("valid baseline");
        assert!(signature.feasible);
        assert_eq!(signature.weighted_utility_ppm_trunc, 620_000);
        assert_eq!(signature.worst_desirability_ppm, 500_000);
    }

    #[test]
    fn weighted_utility_can_differ_from_worst_criterion_policy() {
        let baseline = McdaEngine.baseline(&problem()).expect("valid baseline");
        let alternative = McdaEngine
            .evaluate(
                &problem(),
                &McdaIntervention {
                    alternative_index: 1,
                },
            )
            .expect("valid alternative");
        assert!(
            FeasibleWeightedUtility.utility(&alternative)
                > FeasibleWeightedUtility.utility(&baseline)
        );
        assert!(
            BalancedWorstCriterionFirst.utility(&baseline)
                > BalancedWorstCriterionFirst.utility(&alternative)
        );
    }

    #[test]
    fn weights_must_sum_to_one() {
        assert_eq!(
            McdaProblem::new(
                vec![Criterion {
                    weight_ppm: 999_999,
                    direction: CriterionDirection::Maximize,
                    minimum_desirability_ppm: None,
                }],
                vec![vec![500_000]],
                0,
            ),
            Err(McdaError::WeightMassMismatch {
                actual_ppm: 999_999
            })
        );
    }
}
