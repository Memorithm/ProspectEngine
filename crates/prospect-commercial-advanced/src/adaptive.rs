use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum AdaptiveError {
    InvalidActionCount,
    InvalidPrior,
    InvalidConfiguration,
    InvalidFeatureWidth,
    NonFiniteInput,
    SingularSystem,
}

impl fmt::Display for AdaptiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidActionCount => {
                formatter.write_str("adaptive policy requires at least one action")
            }
            Self::InvalidPrior => {
                formatter.write_str("Beta prior parameters must be finite and positive")
            }
            Self::InvalidConfiguration => {
                formatter.write_str("adaptive policy configuration is invalid")
            }
            Self::InvalidFeatureWidth => formatter.write_str("context feature widths must match"),
            Self::NonFiniteInput => formatter.write_str("adaptive policy inputs must be finite"),
            Self::SingularSystem => {
                formatter.write_str("linear contextual bandit system is singular")
            }
        }
    }
}

impl std::error::Error for AdaptiveError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BetaPrior {
    pub alpha: f64,
    pub beta: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinaryBanditObservation {
    pub context_key: u64,
    pub action: u32,
    pub success: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThompsonRecommendation {
    pub action: u32,
    pub sampled_success_probability: f64,
    pub posterior_alpha: f64,
    pub posterior_beta: f64,
    pub matching_context_observations: u64,
}

pub fn thompson_beta_bernoulli_recommend(
    observations: &[BinaryBanditObservation],
    context_key: u64,
    action_count: u32,
    prior: BetaPrior,
    seed: u64,
) -> Result<ThompsonRecommendation, AdaptiveError> {
    if action_count == 0 {
        return Err(AdaptiveError::InvalidActionCount);
    }
    if !prior.alpha.is_finite()
        || !prior.beta.is_finite()
        || prior.alpha <= 0.0
        || prior.beta <= 0.0
    {
        return Err(AdaptiveError::InvalidPrior);
    }
    let action_count_usize = usize::try_from(action_count).expect("u32 fits usize");
    let mut successes = vec![0_u64; action_count_usize];
    let mut failures = vec![0_u64; action_count_usize];
    let mut matching = 0_u64;
    for observation in observations {
        if observation.context_key != context_key || observation.action >= action_count {
            continue;
        }
        matching = matching.saturating_add(1);
        let index = usize::try_from(observation.action).expect("bounded action fits usize");
        if observation.success {
            successes[index] = successes[index].saturating_add(1);
        } else {
            failures[index] = failures[index].saturating_add(1);
        }
    }

    let mut rng = DeterministicRng::new(seed ^ context_key.rotate_left(17));
    let mut best: Option<ThompsonRecommendation> = None;
    for action in 0..action_count {
        let index = usize::try_from(action).expect("bounded action fits usize");
        let alpha = prior.alpha + successes[index] as f64;
        let beta = prior.beta + failures[index] as f64;
        let sample = sample_beta(alpha, beta, &mut rng)?;
        let candidate = ThompsonRecommendation {
            action,
            sampled_success_probability: sample,
            posterior_alpha: alpha,
            posterior_beta: beta,
            matching_context_observations: matching,
        };
        if best.as_ref().is_none_or(|current| {
            sample > current.sampled_success_probability
                || (sample == current.sampled_success_probability && action < current.action)
        }) {
            best = Some(candidate);
        }
    }
    Ok(best.expect("positive action count yields a recommendation"))
}

fn sample_beta(alpha: f64, beta: f64, rng: &mut DeterministicRng) -> Result<f64, AdaptiveError> {
    let left = sample_gamma(alpha, rng)?;
    let right = sample_gamma(beta, rng)?;
    let total = left + right;
    if !total.is_finite() || total <= 0.0 {
        return Err(AdaptiveError::NonFiniteInput);
    }
    Ok(left / total)
}

fn sample_gamma(shape: f64, rng: &mut DeterministicRng) -> Result<f64, AdaptiveError> {
    if !shape.is_finite() || shape <= 0.0 {
        return Err(AdaptiveError::InvalidPrior);
    }
    if shape < 1.0 {
        let uniform = rng.unit().max(f64::MIN_POSITIVE);
        return Ok(sample_gamma(shape + 1.0, rng)? * uniform.powf(1.0 / shape));
    }
    let d = shape - 1.0 / 3.0;
    let c = (9.0 * d).sqrt().recip();
    loop {
        let normal = rng.standard_normal();
        let base = 1.0 + c * normal;
        if base <= 0.0 {
            continue;
        }
        let value = base * base * base;
        let uniform = rng.unit().max(f64::MIN_POSITIVE);
        if uniform < 1.0 - 0.0331 * normal.powi(4)
            || uniform.ln() < 0.5 * normal * normal + d * (1.0 - value + value.ln())
        {
            return Ok(d * value);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinearBanditObservation {
    pub action: u32,
    pub features: Vec<f64>,
    pub reward: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinUcbConfig {
    pub action_count: u32,
    pub exploration_alpha: f64,
    pub ridge: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinUcbRecommendation {
    pub action: u32,
    pub predicted_reward: f64,
    pub uncertainty: f64,
    pub score: f64,
    pub action_observations: u64,
}

pub fn linucb_recommend(
    observations: &[LinearBanditObservation],
    current_features: &[f64],
    config: LinUcbConfig,
) -> Result<LinUcbRecommendation, AdaptiveError> {
    if config.action_count == 0 || current_features.is_empty() {
        return Err(AdaptiveError::InvalidActionCount);
    }
    if !config.exploration_alpha.is_finite()
        || config.exploration_alpha < 0.0
        || !config.ridge.is_finite()
        || config.ridge <= 0.0
    {
        return Err(AdaptiveError::InvalidConfiguration);
    }
    if current_features.iter().any(|value| !value.is_finite()) {
        return Err(AdaptiveError::NonFiniteInput);
    }
    if observations.iter().any(|observation| {
        observation.features.len() != current_features.len()
            || observation.features.iter().any(|value| !value.is_finite())
            || !observation.reward.is_finite()
    }) {
        return Err(AdaptiveError::InvalidFeatureWidth);
    }

    let dimensions = current_features.len();
    let mut best: Option<LinUcbRecommendation> = None;
    for action in 0..config.action_count {
        let mut matrix = vec![vec![0.0; dimensions]; dimensions];
        for (index, row) in matrix.iter_mut().enumerate() {
            row[index] = config.ridge;
        }
        let mut response = vec![0.0; dimensions];
        let mut count = 0_u64;
        for observation in observations
            .iter()
            .filter(|observation| observation.action == action)
        {
            count = count.saturating_add(1);
            for left in 0..dimensions {
                response[left] += observation.reward * observation.features[left];
                for right in 0..dimensions {
                    matrix[left][right] += observation.features[left] * observation.features[right];
                }
            }
        }
        let inverse = invert_matrix(&matrix).ok_or(AdaptiveError::SingularSystem)?;
        let theta = matrix_vector(&inverse, &response);
        let predicted_reward = dot(&theta, current_features);
        let projected = matrix_vector(&inverse, current_features);
        let variance = dot(current_features, &projected).max(0.0);
        let uncertainty = variance.sqrt();
        let score = predicted_reward + config.exploration_alpha * uncertainty;
        if !predicted_reward.is_finite() || !uncertainty.is_finite() || !score.is_finite() {
            return Err(AdaptiveError::NonFiniteInput);
        }
        let candidate = LinUcbRecommendation {
            action,
            predicted_reward,
            uncertainty,
            score,
            action_observations: count,
        };
        if best.as_ref().is_none_or(|current| {
            score > current.score || (score == current.score && action < current.action)
        }) {
            best = Some(candidate);
        }
    }
    Ok(best.expect("positive action count yields recommendation"))
}

fn invert_matrix(matrix: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let width = matrix.len();
    if width == 0 || matrix.iter().any(|row| row.len() != width) {
        return None;
    }
    let mut augmented = vec![vec![0.0; width * 2]; width];
    for row in 0..width {
        for column in 0..width {
            augmented[row][column] = matrix[row][column];
        }
        augmented[row][width + row] = 1.0;
    }
    for column in 0..width {
        let pivot_row = (column..width).max_by(|left, right| {
            augmented[*left][column]
                .abs()
                .total_cmp(&augmented[*right][column].abs())
        })?;
        if augmented[pivot_row][column].abs() < 1e-12 {
            return None;
        }
        augmented.swap(column, pivot_row);
        let pivot = augmented[column][column];
        for value in &mut augmented[column] {
            *value /= pivot;
        }
        for row in 0..width {
            if row == column {
                continue;
            }
            let factor = augmented[row][column];
            for index in 0..width * 2 {
                augmented[row][index] -= factor * augmented[column][index];
            }
        }
    }
    Some(
        augmented
            .into_iter()
            .map(|row| row[width..].to_vec())
            .collect(),
    )
}

fn matrix_vector(matrix: &[Vec<f64>], vector: &[f64]) -> Vec<f64> {
    matrix
        .iter()
        .map(|row| {
            row.iter()
                .zip(vector)
                .map(|(left, right)| left * right)
                .sum()
        })
        .collect()
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[derive(Clone, Copy, Debug)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x6a09_e667_f3bc_c909
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.state = value;
        value.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn unit(&mut self) -> f64 {
        let bits = self.next_u64() >> 11;
        (bits as f64) / ((1_u64 << 53) as f64)
    }

    fn standard_normal(&mut self) -> f64 {
        let first = self.unit().max(f64::MIN_POSITIVE);
        let second = self.unit();
        (-2.0 * first.ln()).sqrt() * (std::f64::consts::TAU * second).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thompson_sampling_is_seed_replayable_and_uses_context() {
        let mut observations = Vec::new();
        for _ in 0..40 {
            observations.push(BinaryBanditObservation {
                context_key: 7,
                action: 0,
                success: true,
            });
            observations.push(BinaryBanditObservation {
                context_key: 7,
                action: 1,
                success: false,
            });
        }
        let first = thompson_beta_bernoulli_recommend(
            &observations,
            7,
            2,
            BetaPrior {
                alpha: 1.0,
                beta: 1.0,
            },
            123,
        )
        .expect("valid Thompson recommendation");
        let second = thompson_beta_bernoulli_recommend(
            &observations,
            7,
            2,
            BetaPrior {
                alpha: 1.0,
                beta: 1.0,
            },
            123,
        )
        .expect("replayed Thompson recommendation");
        assert_eq!(first, second);
        assert_eq!(first.action, 0);
        assert_eq!(first.matching_context_observations, 80);
    }

    #[test]
    fn linucb_combines_prediction_and_uncertainty() {
        let observations = vec![
            LinearBanditObservation {
                action: 0,
                features: vec![1.0, 0.0],
                reward: 10.0,
            },
            LinearBanditObservation {
                action: 0,
                features: vec![1.0, 1.0],
                reward: 12.0,
            },
            LinearBanditObservation {
                action: 1,
                features: vec![1.0, 0.0],
                reward: 2.0,
            },
        ];
        let recommendation = linucb_recommend(
            &observations,
            &[1.0, 0.5],
            LinUcbConfig {
                action_count: 2,
                exploration_alpha: 0.1,
                ridge: 1.0,
            },
        )
        .expect("valid LinUCB recommendation");
        assert_eq!(recommendation.action, 0);
        assert!(recommendation.predicted_reward > 5.0);
        assert!(recommendation.uncertainty > 0.0);
    }
}
