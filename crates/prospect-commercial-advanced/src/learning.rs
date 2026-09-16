use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LearningError {
    EmptySeries,
    InvalidSmoothingPpm(u32),
    ArithmeticOverflow(&'static str),
    EmptyCausalSample,
    InvalidPropensityPpm(u32),
    InvalidBanditConfiguration,
    NonFiniteExploration,
    EmptyBayesianProblem,
    ProbabilityMassMismatch { actual_ppm: u64 },
    UtilityWidthMismatch,
    InvalidLikelihoodPpm(u32),
    ExperimentWidthMismatch,
    NegativeExperimentCost,
}

impl fmt::Display for LearningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySeries => formatter.write_str("forecast series must not be empty"),
            Self::InvalidSmoothingPpm(value) => write!(
                formatter,
                "smoothing coefficient must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
            Self::EmptyCausalSample => formatter.write_str("causal sample must not be empty"),
            Self::InvalidPropensityPpm(value) => write!(
                formatter,
                "treatment propensity must be in 1..{PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::InvalidBanditConfiguration => {
                formatter.write_str("bandit requires at least one action")
            }
            Self::NonFiniteExploration => {
                formatter.write_str("bandit exploration coefficient must be finite and non-negative")
            }
            Self::EmptyBayesianProblem => {
                formatter.write_str("Bayesian decision problem must not be empty")
            }
            Self::ProbabilityMassMismatch { actual_ppm } => write!(
                formatter,
                "prior probabilities must sum to {PROBABILITY_SCALE_PPM} ppm, got {actual_ppm} ppm"
            ),
            Self::UtilityWidthMismatch => {
                formatter.write_str("every action must define one utility per hypothesis")
            }
            Self::InvalidLikelihoodPpm(value) => write!(
                formatter,
                "experiment likelihood must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::ExperimentWidthMismatch => formatter.write_str(
                "experiment likelihood width must match Bayesian hypothesis count",
            ),
            Self::NegativeExperimentCost => {
                formatter.write_str("experiment cost must be non-negative")
            }
        }
    }
}

impl std::error::Error for LearningError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleExponentialSmoothing {
    alpha_ppm: u32,
}

impl SimpleExponentialSmoothing {
    pub fn new(alpha_ppm: u32) -> Result<Self, LearningError> {
        if alpha_ppm > PROBABILITY_SCALE_PPM {
            return Err(LearningError::InvalidSmoothingPpm(alpha_ppm));
        }
        Ok(Self { alpha_ppm })
    }

    pub fn forecast(&self, series: &[i64], horizon: usize) -> Result<Vec<i128>, LearningError> {
        if series.is_empty() {
            return Err(LearningError::EmptySeries);
        }
        let scale = i128::from(PROBABILITY_SCALE_PPM);
        let alpha = i128::from(self.alpha_ppm);
        let complement = scale - alpha;
        let mut level_scaled = i128::from(series[0])
            .checked_mul(scale)
            .ok_or(LearningError::ArithmeticOverflow("SES level"))?;
        for observation in &series[1..] {
            let observed = i128::from(*observation)
                .checked_mul(alpha)
                .ok_or(LearningError::ArithmeticOverflow("SES observation"))?;
            let retained = level_scaled
                .checked_mul(complement)
                .ok_or(LearningError::ArithmeticOverflow("SES retained level"))?
                / scale;
            level_scaled = observed
                .checked_add(retained)
                .ok_or(LearningError::ArithmeticOverflow("SES level"))?;
        }
        let forecast = level_scaled / scale;
        Ok(vec![forecast; horizon])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoltLinearTrend {
    alpha_ppm: u32,
    beta_ppm: u32,
}

impl HoltLinearTrend {
    pub fn new(alpha_ppm: u32, beta_ppm: u32) -> Result<Self, LearningError> {
        if alpha_ppm > PROBABILITY_SCALE_PPM {
            return Err(LearningError::InvalidSmoothingPpm(alpha_ppm));
        }
        if beta_ppm > PROBABILITY_SCALE_PPM {
            return Err(LearningError::InvalidSmoothingPpm(beta_ppm));
        }
        Ok(Self { alpha_ppm, beta_ppm })
    }

    pub fn forecast(&self, series: &[i64], horizon: usize) -> Result<Vec<i128>, LearningError> {
        if series.is_empty() {
            return Err(LearningError::EmptySeries);
        }
        let scale = i128::from(PROBABILITY_SCALE_PPM);
        let alpha = i128::from(self.alpha_ppm);
        let beta = i128::from(self.beta_ppm);
        let mut level = i128::from(series[0])
            .checked_mul(scale)
            .ok_or(LearningError::ArithmeticOverflow("Holt level"))?;
        let mut trend = if series.len() >= 2 {
            i128::from(series[1] - series[0])
                .checked_mul(scale)
                .ok_or(LearningError::ArithmeticOverflow("Holt trend"))?
        } else {
            0
        };

        for observation in &series[1..] {
            let old_level = level;
            let prediction = level
                .checked_add(trend)
                .ok_or(LearningError::ArithmeticOverflow("Holt prediction"))?;
            let observed_component = i128::from(*observation)
                .checked_mul(alpha)
                .ok_or(LearningError::ArithmeticOverflow("Holt observation"))?;
            let retained_component = prediction
                .checked_mul(scale - alpha)
                .ok_or(LearningError::ArithmeticOverflow("Holt retained level"))?
                / scale;
            level = observed_component
                .checked_add(retained_component)
                .ok_or(LearningError::ArithmeticOverflow("Holt level"))?;
            let trend_delta = level
                .checked_sub(old_level)
                .ok_or(LearningError::ArithmeticOverflow("Holt trend delta"))?;
            trend = trend_delta
                .checked_mul(beta)
                .ok_or(LearningError::ArithmeticOverflow("Holt trend update"))?
                / scale
                + trend
                    .checked_mul(scale - beta)
                    .ok_or(LearningError::ArithmeticOverflow("Holt retained trend"))?
                    / scale;
        }

        (1..=horizon)
            .map(|step| {
                let future = level
                    .checked_add(
                        trend
                            .checked_mul(i128::try_from(step).expect("usize fits i128"))
                            .ok_or(LearningError::ArithmeticOverflow("Holt horizon"))?,
                    )
                    .ok_or(LearningError::ArithmeticOverflow("Holt forecast"))?;
                Ok(future / scale)
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DoublyRobustRecord {
    pub segment_id: u64,
    pub treated: bool,
    pub outcome_minor: i64,
    pub treatment_propensity_ppm: u32,
    pub modeled_control_outcome_minor: i64,
    pub modeled_treated_outcome_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentUplift {
    pub segment_id: u64,
    pub sample_count: u64,
    pub doubly_robust_uplift_minor_trunc: i128,
}

pub fn doubly_robust_average_treatment_effect(
    records: &[DoublyRobustRecord],
) -> Result<i128, LearningError> {
    if records.is_empty() {
        return Err(LearningError::EmptyCausalSample);
    }
    let total = records.iter().try_fold(0_i128, |sum, record| {
        let estimate = doubly_robust_effect(*record)?;
        sum.checked_add(estimate)
            .ok_or(LearningError::ArithmeticOverflow("doubly robust ATE"))
    })?;
    Ok(total / i128::try_from(records.len()).expect("usize fits i128"))
}

pub fn doubly_robust_segment_uplift(
    records: &[DoublyRobustRecord],
) -> Result<Vec<SegmentUplift>, LearningError> {
    if records.is_empty() {
        return Err(LearningError::EmptyCausalSample);
    }
    let mut grouped: BTreeMap<u64, (i128, u64)> = BTreeMap::new();
    for record in records {
        let estimate = doubly_robust_effect(*record)?;
        let entry = grouped.entry(record.segment_id).or_insert((0, 0));
        entry.0 = entry
            .0
            .checked_add(estimate)
            .ok_or(LearningError::ArithmeticOverflow("segment uplift"))?;
        entry.1 += 1;
    }
    Ok(grouped
        .into_iter()
        .map(|(segment_id, (sum, count))| SegmentUplift {
            segment_id,
            sample_count: count,
            doubly_robust_uplift_minor_trunc: sum / i128::from(count),
        })
        .collect())
}

fn doubly_robust_effect(record: DoublyRobustRecord) -> Result<i128, LearningError> {
    let propensity = record.treatment_propensity_ppm;
    if propensity == 0 || propensity >= PROBABILITY_SCALE_PPM {
        return Err(LearningError::InvalidPropensityPpm(propensity));
    }
    let scale = i128::from(PROBABILITY_SCALE_PPM);
    let modeled_uplift = i128::from(record.modeled_treated_outcome_minor)
        - i128::from(record.modeled_control_outcome_minor);
    let correction = if record.treated {
        let residual = i128::from(record.outcome_minor)
            - i128::from(record.modeled_treated_outcome_minor);
        residual
            .checked_mul(scale)
            .ok_or(LearningError::ArithmeticOverflow("treated causal correction"))?
            / i128::from(propensity)
    } else {
        let residual = i128::from(record.outcome_minor)
            - i128::from(record.modeled_control_outcome_minor);
        -(residual
            .checked_mul(scale)
            .ok_or(LearningError::ArithmeticOverflow("control causal correction"))?
            / i128::from(PROBABILITY_SCALE_PPM - propensity))
    };
    modeled_uplift
        .checked_add(correction)
        .ok_or(LearningError::ArithmeticOverflow("doubly robust effect"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyLearningRecord {
    pub observed_treatment: bool,
    pub policy_treatment: bool,
    pub outcome_minor: i64,
    pub treatment_propensity_ppm: u32,
    pub modeled_control_outcome_minor: i64,
    pub modeled_treated_outcome_minor: i64,
}

pub fn doubly_robust_policy_value(
    records: &[PolicyLearningRecord],
) -> Result<i128, LearningError> {
    if records.is_empty() {
        return Err(LearningError::EmptyCausalSample);
    }
    let scale = i128::from(PROBABILITY_SCALE_PPM);
    let total = records.iter().try_fold(0_i128, |sum, record| {
        let propensity = record.treatment_propensity_ppm;
        if propensity == 0 || propensity >= PROBABILITY_SCALE_PPM {
            return Err(LearningError::InvalidPropensityPpm(propensity));
        }
        let modeled_policy = if record.policy_treatment {
            record.modeled_treated_outcome_minor
        } else {
            record.modeled_control_outcome_minor
        };
        let observed_matches = record.observed_treatment == record.policy_treatment;
        let correction = if observed_matches {
            let modeled_observed = if record.observed_treatment {
                record.modeled_treated_outcome_minor
            } else {
                record.modeled_control_outcome_minor
            };
            let action_probability = if record.observed_treatment {
                propensity
            } else {
                PROBABILITY_SCALE_PPM - propensity
            };
            (i128::from(record.outcome_minor) - i128::from(modeled_observed))
                .checked_mul(scale)
                .ok_or(LearningError::ArithmeticOverflow("policy correction"))?
                / i128::from(action_probability)
        } else {
            0
        };
        let value = i128::from(modeled_policy)
            .checked_add(correction)
            .ok_or(LearningError::ArithmeticOverflow("policy value"))?;
        sum.checked_add(value)
            .ok_or(LearningError::ArithmeticOverflow("policy value"))
    })?;
    Ok(total / i128::try_from(records.len()).expect("usize fits i128"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BanditObservation {
    pub context_key: u64,
    pub action: u32,
    pub reward_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BanditRecommendation {
    pub action: u32,
    pub ucb_score: f64,
    pub matching_context_observations: u64,
}

pub fn contextual_ucb_recommend(
    observations: &[BanditObservation],
    context_key: u64,
    action_count: u32,
    exploration: f64,
) -> Result<BanditRecommendation, LearningError> {
    if action_count == 0 {
        return Err(LearningError::InvalidBanditConfiguration);
    }
    if !exploration.is_finite() || exploration < 0.0 {
        return Err(LearningError::NonFiniteExploration);
    }
    let matching: Vec<&BanditObservation> = observations
        .iter()
        .filter(|observation| observation.context_key == context_key)
        .collect();
    let total = u64::try_from(matching.len()).expect("usize fits u64");
    let mut counts = vec![0_u64; usize::try_from(action_count).expect("u32 fits usize")];
    let mut rewards = vec![0_i128; counts.len()];
    for observation in matching {
        if observation.action < action_count {
            let index = usize::try_from(observation.action).expect("u32 fits usize");
            counts[index] += 1;
            rewards[index] = rewards[index]
                .checked_add(i128::from(observation.reward_minor))
                .ok_or(LearningError::ArithmeticOverflow("bandit rewards"))?;
        }
    }
    if let Some(index) = counts.iter().position(|count| *count == 0) {
        return Ok(BanditRecommendation {
            action: u32::try_from(index).expect("bounded by action_count"),
            ucb_score: f64::INFINITY,
            matching_context_observations: total,
        });
    }

    let total_for_log = (total as f64 + 1.0).ln();
    let mut best_action = 0_u32;
    let mut best_score = f64::NEG_INFINITY;
    for action in 0..action_count {
        let index = usize::try_from(action).expect("u32 fits usize");
        let count = counts[index] as f64;
        let mean = rewards[index] as f64 / count;
        let bonus = exploration * (total_for_log / count).sqrt();
        let score = mean + bonus;
        if score > best_score {
            best_score = score;
            best_action = action;
        }
    }
    Ok(BanditRecommendation {
        action: best_action,
        ucb_score: best_score,
        matching_context_observations: total,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BayesianDecisionProblem {
    pub prior_probabilities_ppm: Vec<u32>,
    pub action_utilities_minor: Vec<Vec<i64>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BayesianDecision {
    pub action_index: usize,
    pub expected_utility_minor_trunc: i128,
}

impl BayesianDecisionProblem {
    pub fn validate(&self) -> Result<(), LearningError> {
        if self.prior_probabilities_ppm.is_empty() || self.action_utilities_minor.is_empty() {
            return Err(LearningError::EmptyBayesianProblem);
        }
        let actual_ppm: u64 = self
            .prior_probabilities_ppm
            .iter()
            .map(|value| u64::from(*value))
            .sum();
        if actual_ppm != u64::from(PROBABILITY_SCALE_PPM) {
            return Err(LearningError::ProbabilityMassMismatch { actual_ppm });
        }
        if self
            .action_utilities_minor
            .iter()
            .any(|utilities| utilities.len() != self.prior_probabilities_ppm.len())
        {
            return Err(LearningError::UtilityWidthMismatch);
        }
        Ok(())
    }

    pub fn best_action(&self) -> Result<BayesianDecision, LearningError> {
        self.validate()?;
        let mut best: Option<(usize, i128)> = None;
        for (action_index, utilities) in self.action_utilities_minor.iter().enumerate() {
            let weighted = utilities
                .iter()
                .zip(&self.prior_probabilities_ppm)
                .try_fold(0_i128, |sum, (utility, probability)| {
                    let term = i128::from(*utility)
                        .checked_mul(i128::from(*probability))
                        .ok_or(LearningError::ArithmeticOverflow("Bayesian expected utility"))?;
                    sum.checked_add(term)
                        .ok_or(LearningError::ArithmeticOverflow("Bayesian expected utility"))
                })?;
            if best.is_none_or(|(_, current)| weighted > current) {
                best = Some((action_index, weighted));
            }
        }
        let (action_index, weighted) = best.expect("validated actions are non-empty");
        Ok(BayesianDecision {
            action_index,
            expected_utility_minor_trunc: weighted / i128::from(PROBABILITY_SCALE_PPM),
        })
    }

    pub fn value_of_perfect_information_minor_trunc(&self) -> Result<i128, LearningError> {
        self.validate()?;
        let baseline = self.best_action()?.expected_utility_minor_trunc;
        let perfect_weighted = (0..self.prior_probabilities_ppm.len()).try_fold(
            0_i128,
            |sum, hypothesis| {
                let best_utility = self
                    .action_utilities_minor
                    .iter()
                    .map(|action| action[hypothesis])
                    .max()
                    .expect("validated actions are non-empty");
                let term = i128::from(best_utility)
                    .checked_mul(i128::from(self.prior_probabilities_ppm[hypothesis]))
                    .ok_or(LearningError::ArithmeticOverflow("perfect information value"))?;
                sum.checked_add(term)
                    .ok_or(LearningError::ArithmeticOverflow("perfect information value"))
            },
        )?;
        Ok(perfect_weighted / i128::from(PROBABILITY_SCALE_PPM) - baseline)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryExperiment {
    pub positive_likelihood_ppm_by_hypothesis: Vec<u32>,
    pub cost_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExperimentValue {
    pub experiment_index: usize,
    pub expected_value_of_sample_information_minor_trunc: i128,
    pub net_value_minor_trunc: i128,
}

pub fn select_best_binary_experiment(
    problem: &BayesianDecisionProblem,
    experiments: &[BinaryExperiment],
) -> Result<Option<ExperimentValue>, LearningError> {
    problem.validate()?;
    let baseline = problem.best_action()?.expected_utility_minor_trunc;
    let scale = i128::from(PROBABILITY_SCALE_PPM);
    let scale_squared = scale
        .checked_mul(scale)
        .ok_or(LearningError::ArithmeticOverflow("Bayesian scale"))?;
    let mut best: Option<ExperimentValue> = None;

    for (experiment_index, experiment) in experiments.iter().enumerate() {
        if experiment.cost_minor < 0 {
            return Err(LearningError::NegativeExperimentCost);
        }
        if experiment.positive_likelihood_ppm_by_hypothesis.len()
            != problem.prior_probabilities_ppm.len()
        {
            return Err(LearningError::ExperimentWidthMismatch);
        }
        if let Some(value) = experiment
            .positive_likelihood_ppm_by_hypothesis
            .iter()
            .copied()
            .find(|value| *value > PROBABILITY_SCALE_PPM)
        {
            return Err(LearningError::InvalidLikelihoodPpm(value));
        }

        let best_positive = best_joint_signal_utility(problem, experiment, true)?;
        let best_negative = best_joint_signal_utility(problem, experiment, false)?;
        let post_experiment_utility = best_positive
            .checked_add(best_negative)
            .ok_or(LearningError::ArithmeticOverflow("sample information utility"))?
            / scale_squared;
        let evsi = post_experiment_utility - baseline;
        let net = evsi - i128::from(experiment.cost_minor);
        let candidate = ExperimentValue {
            experiment_index,
            expected_value_of_sample_information_minor_trunc: evsi,
            net_value_minor_trunc: net,
        };
        if best.as_ref().is_none_or(|current| {
            candidate.net_value_minor_trunc > current.net_value_minor_trunc
        }) {
            best = Some(candidate);
        }
    }
    Ok(best)
}

fn best_joint_signal_utility(
    problem: &BayesianDecisionProblem,
    experiment: &BinaryExperiment,
    positive_signal: bool,
) -> Result<i128, LearningError> {
    let mut best = i128::MIN;
    for utilities in &problem.action_utilities_minor {
        let weighted = utilities
            .iter()
            .enumerate()
            .try_fold(0_i128, |sum, (hypothesis, utility)| {
                let likelihood = if positive_signal {
                    experiment.positive_likelihood_ppm_by_hypothesis[hypothesis]
                } else {
                    PROBABILITY_SCALE_PPM
                        - experiment.positive_likelihood_ppm_by_hypothesis[hypothesis]
                };
                let joint_weight = i128::from(problem.prior_probabilities_ppm[hypothesis])
                    .checked_mul(i128::from(likelihood))
                    .ok_or(LearningError::ArithmeticOverflow("signal probability"))?;
                let term = i128::from(*utility)
                    .checked_mul(joint_weight)
                    .ok_or(LearningError::ArithmeticOverflow("signal utility"))?;
                sum.checked_add(term)
                    .ok_or(LearningError::ArithmeticOverflow("signal utility"))
            })?;
        best = best.max(weighted);
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_forecasts_are_deterministic() {
        let ses = SimpleExponentialSmoothing::new(500_000).expect("valid alpha");
        assert_eq!(ses.forecast(&[100, 200, 300], 2).expect("forecast"), vec![225, 225]);
        let holt = HoltLinearTrend::new(1_000_000, 1_000_000).expect("valid coefficients");
        assert_eq!(holt.forecast(&[100, 200, 300], 2).expect("forecast"), vec![400, 500]);
    }

    #[test]
    fn doubly_robust_estimator_recovers_modeled_uplift_with_zero_residuals() {
        let records = [
            DoublyRobustRecord {
                segment_id: 1,
                treated: true,
                outcome_minor: 120,
                treatment_propensity_ppm: 500_000,
                modeled_control_outcome_minor: 100,
                modeled_treated_outcome_minor: 120,
            },
            DoublyRobustRecord {
                segment_id: 1,
                treated: false,
                outcome_minor: 100,
                treatment_propensity_ppm: 500_000,
                modeled_control_outcome_minor: 100,
                modeled_treated_outcome_minor: 120,
            },
        ];
        assert_eq!(doubly_robust_average_treatment_effect(&records).expect("ATE"), 20);
        assert_eq!(doubly_robust_segment_uplift(&records).expect("segments")[0].doubly_robust_uplift_minor_trunc, 20);
    }

    #[test]
    fn policy_value_uses_observed_action_correction_only_when_policy_matches() {
        let records = [PolicyLearningRecord {
            observed_treatment: true,
            policy_treatment: true,
            outcome_minor: 130,
            treatment_propensity_ppm: 500_000,
            modeled_control_outcome_minor: 100,
            modeled_treated_outcome_minor: 120,
        }];
        assert_eq!(doubly_robust_policy_value(&records).expect("policy value"), 140);
    }

    #[test]
    fn contextual_ucb_explores_unseen_actions_first() {
        let recommendation = contextual_ucb_recommend(
            &[BanditObservation { context_key: 7, action: 0, reward_minor: 10 }],
            7,
            2,
            1.0,
        )
        .expect("recommendation");
        assert_eq!(recommendation.action, 1);
        assert!(recommendation.ucb_score.is_infinite());
    }

    #[test]
    fn Bayesian_decision_and_information_value_are_explicit() {
        let problem = BayesianDecisionProblem {
            prior_probabilities_ppm: vec![500_000, 500_000],
            action_utilities_minor: vec![vec![100, 0], vec![0, 80]],
        };
        let best = problem.best_action().expect("best action");
        assert_eq!(best.action_index, 0);
        assert_eq!(best.expected_utility_minor_trunc, 50);
        assert_eq!(problem.value_of_perfect_information_minor_trunc().expect("EVPI"), 40);
    }

    #[test]
    fn experiment_selection_accounts_for_information_and_cost() {
        let problem = BayesianDecisionProblem {
            prior_probabilities_ppm: vec![500_000, 500_000],
            action_utilities_minor: vec![vec![100, 0], vec![0, 100]],
        };
        let experiment = BinaryExperiment {
            positive_likelihood_ppm_by_hypothesis: vec![900_000, 100_000],
            cost_minor: 10,
        };
        let selected = select_best_binary_experiment(&problem, &[experiment])
            .expect("selection")
            .expect("one experiment");
        assert_eq!(selected.expected_value_of_sample_information_minor_trunc, 40);
        assert_eq!(selected.net_value_minor_trunc, 30);
    }
}
