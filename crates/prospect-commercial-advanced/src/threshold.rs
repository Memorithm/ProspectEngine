use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;
use prospect_core::{DecisionPolicy, ProspectiveEngine};
use std::cmp::Reverse;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThresholdError {
    EmptyCaseSet,
    ScoreOutOfRange(u32),
    ThresholdOutOfRange(u32),
    NegativeEconomicsField(&'static str),
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for ThresholdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCaseSet => formatter.write_str("threshold tuning requires at least one case"),
            Self::ScoreOutOfRange(value) => write!(
                formatter,
                "case score must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::ThresholdOutOfRange(value) => write!(
                formatter,
                "threshold must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::NegativeEconomicsField(field) => write!(formatter, "{field} must be non-negative"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
        }
    }
}

impl std::error::Error for ThresholdError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScoredBinaryCase {
    pub score_ppm: u32,
    pub actual_positive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinaryDecisionEconomics {
    pub true_positive_gain_minor: i64,
    pub true_negative_gain_minor: i64,
    pub false_positive_cost_minor: i64,
    pub false_negative_cost_minor: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThresholdTuningState {
    cases: Vec<ScoredBinaryCase>,
    pub economics: BinaryDecisionEconomics,
    pub baseline_threshold_ppm: u32,
}

impl ThresholdTuningState {
    pub fn new(
        cases: Vec<ScoredBinaryCase>,
        economics: BinaryDecisionEconomics,
        baseline_threshold_ppm: u32,
    ) -> Result<Self, ThresholdError> {
        if cases.is_empty() {
            return Err(ThresholdError::EmptyCaseSet);
        }
        for case in &cases {
            if case.score_ppm > PROBABILITY_SCALE_PPM {
                return Err(ThresholdError::ScoreOutOfRange(case.score_ppm));
            }
        }
        validate_threshold(baseline_threshold_ppm)?;
        validate_economics(economics)?;
        Ok(Self {
            cases,
            economics,
            baseline_threshold_ppm,
        })
    }

    #[must_use]
    pub fn cases(&self) -> &[ScoredBinaryCase] {
        &self.cases
    }

    /// Returns every score boundary that can change a `score >= threshold`
    /// decision on the supplied finite sample.
    #[must_use]
    pub fn candidate_thresholds_ppm(&self) -> Vec<u32> {
        let mut thresholds = Vec::with_capacity(self.cases.len().saturating_mul(2) + 2);
        thresholds.push(0);
        thresholds.push(PROBABILITY_SCALE_PPM);
        for case in &self.cases {
            thresholds.push(case.score_ppm);
            if case.score_ppm < PROBABILITY_SCALE_PPM {
                thresholds.push(case.score_ppm + 1);
            }
        }
        thresholds.sort_unstable();
        thresholds.dedup();
        thresholds
    }
}

fn validate_threshold(threshold_ppm: u32) -> Result<(), ThresholdError> {
    if threshold_ppm > PROBABILITY_SCALE_PPM {
        return Err(ThresholdError::ThresholdOutOfRange(threshold_ppm));
    }
    Ok(())
}

fn validate_economics(economics: BinaryDecisionEconomics) -> Result<(), ThresholdError> {
    for (field, value) in [
        ("true_positive_gain_minor", economics.true_positive_gain_minor),
        ("true_negative_gain_minor", economics.true_negative_gain_minor),
        ("false_positive_cost_minor", economics.false_positive_cost_minor),
        ("false_negative_cost_minor", economics.false_negative_cost_minor),
    ] {
        if value < 0 {
            return Err(ThresholdError::NegativeEconomicsField(field));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThresholdIntervention {
    pub threshold_ppm: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThresholdSignature {
    pub threshold_ppm: u32,
    pub true_positive: u64,
    pub true_negative: u64,
    pub false_positive: u64,
    pub false_negative: u64,
    pub predicted_positive: u64,
    pub empirical_business_utility_minor: i128,
}

/// Evaluates a decision cut-off against labelled evidence and explicit business
/// economics. This is an empirical policy evaluator, not a calibrated predictor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThresholdDecisionEngine;

impl ProspectiveEngine<ThresholdTuningState, ThresholdIntervention> for ThresholdDecisionEngine {
    type Signature = ThresholdSignature;
    type Error = ThresholdError;

    fn baseline(&self, state: &ThresholdTuningState) -> Result<Self::Signature, Self::Error> {
        evaluate_threshold(state, state.baseline_threshold_ppm)
    }

    fn evaluate(
        &self,
        state: &ThresholdTuningState,
        intervention: &ThresholdIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        evaluate_threshold(state, intervention.threshold_ppm)
    }
}

fn evaluate_threshold(
    state: &ThresholdTuningState,
    threshold_ppm: u32,
) -> Result<ThresholdSignature, ThresholdError> {
    validate_threshold(threshold_ppm)?;
    let mut true_positive = 0_u64;
    let mut true_negative = 0_u64;
    let mut false_positive = 0_u64;
    let mut false_negative = 0_u64;

    for case in state.cases() {
        let predicted_positive = case.score_ppm >= threshold_ppm;
        match (predicted_positive, case.actual_positive) {
            (true, true) => true_positive += 1,
            (false, false) => true_negative += 1,
            (true, false) => false_positive += 1,
            (false, true) => false_negative += 1,
        }
    }

    let economics = state.economics;
    let gain = i128::from(true_positive)
        .checked_mul(i128::from(economics.true_positive_gain_minor))
        .and_then(|value| {
            value.checked_add(
                i128::from(true_negative) * i128::from(economics.true_negative_gain_minor),
            )
        })
        .ok_or(ThresholdError::ArithmeticOverflow("classification gain"))?;
    let cost = i128::from(false_positive)
        .checked_mul(i128::from(economics.false_positive_cost_minor))
        .and_then(|value| {
            value.checked_add(
                i128::from(false_negative) * i128::from(economics.false_negative_cost_minor),
            )
        })
        .ok_or(ThresholdError::ArithmeticOverflow("classification cost"))?;
    let empirical_business_utility_minor = gain
        .checked_sub(cost)
        .ok_or(ThresholdError::ArithmeticOverflow("business utility"))?;

    Ok(ThresholdSignature {
        threshold_ppm,
        true_positive,
        true_negative,
        false_positive,
        false_negative,
        predicted_positive: true_positive + false_positive,
        empirical_business_utility_minor,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaximizeEmpiricalBusinessUtility;

impl DecisionPolicy<ThresholdSignature> for MaximizeEmpiricalBusinessUtility {
    type Score = i128;

    fn utility(&self, signature: &ThresholdSignature) -> Self::Score {
        signature.empirical_business_utility_minor
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FalseNegativeFirst;

impl DecisionPolicy<ThresholdSignature> for FalseNegativeFirst {
    type Score = (Reverse<u64>, i128);

    fn utility(&self, signature: &ThresholdSignature) -> Self::Score {
        (
            Reverse(signature.false_negative),
            signature.empirical_business_utility_minor,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ThresholdTuningState {
        ThresholdTuningState::new(
            vec![
                ScoredBinaryCase {
                    score_ppm: 900_000,
                    actual_positive: true,
                },
                ScoredBinaryCase {
                    score_ppm: 700_000,
                    actual_positive: false,
                },
                ScoredBinaryCase {
                    score_ppm: 400_000,
                    actual_positive: true,
                },
                ScoredBinaryCase {
                    score_ppm: 100_000,
                    actual_positive: false,
                },
            ],
            BinaryDecisionEconomics {
                true_positive_gain_minor: 100,
                true_negative_gain_minor: 0,
                false_positive_cost_minor: 200,
                false_negative_cost_minor: 50,
            },
            500_000,
        )
        .expect("valid tuning state")
    }

    #[test]
    fn threshold_engine_uses_business_costs() {
        let baseline = ThresholdDecisionEngine
            .baseline(&state())
            .expect("valid baseline");
        let conservative = ThresholdDecisionEngine
            .evaluate(
                &state(),
                &ThresholdIntervention {
                    threshold_ppm: 800_000,
                },
            )
            .expect("valid threshold");
        assert_eq!(baseline.empirical_business_utility_minor, -150);
        assert_eq!(conservative.empirical_business_utility_minor, 50);
        assert!(
            MaximizeEmpiricalBusinessUtility.utility(&conservative)
                > MaximizeEmpiricalBusinessUtility.utility(&baseline)
        );
    }

    #[test]
    fn candidate_thresholds_cover_both_sides_of_observed_scores() {
        let thresholds = state().candidate_thresholds_ppm();
        assert!(thresholds.contains(&700_000));
        assert!(thresholds.contains(&700_001));
        assert_eq!(thresholds.first(), Some(&0));
        assert_eq!(thresholds.last(), Some(&PROBABILITY_SCALE_PPM));
    }
}
