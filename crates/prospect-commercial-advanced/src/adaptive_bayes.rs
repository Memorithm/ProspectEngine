use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum BayesianBanditError {
    InvalidActionCount,
    InvalidPrior,
    InvalidNoise,
    InvalidFeatureWidth,
    NonFiniteInput,
    SingularSystem,
}

impl fmt::Display for BayesianBanditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidActionCount => {
                formatter.write_str("Bayesian bandit requires at least one action")
            }
            Self::InvalidPrior => formatter.write_str("Bayesian bandit prior is invalid"),
            Self::InvalidNoise => {
                formatter.write_str("Bayesian bandit noise configuration is invalid")
            }
            Self::InvalidFeatureWidth => {
                formatter.write_str("Bayesian linear bandit feature widths must match")
            }
            Self::NonFiniteInput => formatter.write_str("Bayesian bandit inputs must be finite"),
            Self::SingularSystem => {
                formatter.write_str("Bayesian linear bandit posterior system is singular")
            }
        }
    }
}

impl std::error::Error for BayesianBanditError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GaussianRewardPrior {
    pub mean: f64,
    pub precision: f64,
    pub observation_precision: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GaussianRewardObservation {
    pub action: u32,
    pub reward: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GaussianThompsonRecommendation {
    pub action: u32,
    pub posterior_mean: f64,
    pub posterior_variance: f64,
    pub sampled_reward: f64,
    pub observations: u64,
}

pub fn gaussian_thompson_recommend(
    observations: &[GaussianRewardObservation],
    action_count: u32,
    prior: GaussianRewardPrior,
    seed: u64,
) -> Result<GaussianThompsonRecommendation, BayesianBanditError> {
    if action_count == 0 {
        return Err(BayesianBanditError::InvalidActionCount);
    }
    if !prior.mean.is_finite()
        || !prior.precision.is_finite()
        || prior.precision <= 0.0
        || !prior.observation_precision.is_finite()
        || prior.observation_precision <= 0.0
    {
        return Err(BayesianBanditError::InvalidPrior);
    }
    if observations
        .iter()
        .any(|observation| !observation.reward.is_finite())
    {
        return Err(BayesianBanditError::NonFiniteInput);
    }

    let width = usize::try_from(action_count).expect("u32 fits usize");
    let mut count = vec![0_u64; width];
    let mut sum = vec![0.0_f64; width];
    for observation in observations {
        if observation.action >= action_count {
            continue;
        }
        let index = usize::try_from(observation.action).expect("bounded action fits usize");
        count[index] = count[index].saturating_add(1);
        sum[index] += observation.reward;
    }

    let mut rng = DeterministicRng::new(seed);
    let mut best: Option<GaussianThompsonRecommendation> = None;
    for action in 0..action_count {
        let index = usize::try_from(action).expect("bounded action fits usize");
        let posterior_precision =
            prior.precision + count[index] as f64 * prior.observation_precision;
        let posterior_variance = 1.0 / posterior_precision;
        let posterior_mean = (prior.precision * prior.mean
            + prior.observation_precision * sum[index])
            / posterior_precision;
        let sampled_reward = posterior_mean + posterior_variance.sqrt() * rng.standard_normal();
        if !sampled_reward.is_finite() {
            return Err(BayesianBanditError::NonFiniteInput);
        }
        let candidate = GaussianThompsonRecommendation {
            action,
            posterior_mean,
            posterior_variance,
            sampled_reward,
            observations: count[index],
        };
        if best.as_ref().is_none_or(|current| {
            sampled_reward > current.sampled_reward
                || (sampled_reward == current.sampled_reward && action < current.action)
        }) {
            best = Some(candidate);
        }
    }
    Ok(best.expect("positive action count yields a recommendation"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinearThompsonObservation {
    pub action: u32,
    pub features: Vec<f64>,
    pub reward: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearThompsonConfig {
    pub action_count: u32,
    pub prior_precision: f64,
    pub noise_variance: f64,
    pub posterior_sample_scale: f64,
    pub seed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearThompsonRecommendation {
    pub action: u32,
    pub posterior_mean_reward: f64,
    pub sampled_reward: f64,
    pub observations: u64,
}

pub fn linear_thompson_recommend(
    observations: &[LinearThompsonObservation],
    current_features: &[f64],
    config: LinearThompsonConfig,
) -> Result<LinearThompsonRecommendation, BayesianBanditError> {
    if config.action_count == 0 || current_features.is_empty() {
        return Err(BayesianBanditError::InvalidActionCount);
    }
    if !config.prior_precision.is_finite() || config.prior_precision <= 0.0 {
        return Err(BayesianBanditError::InvalidPrior);
    }
    if !config.noise_variance.is_finite()
        || config.noise_variance <= 0.0
        || !config.posterior_sample_scale.is_finite()
        || config.posterior_sample_scale < 0.0
    {
        return Err(BayesianBanditError::InvalidNoise);
    }
    if current_features.iter().any(|value| !value.is_finite()) {
        return Err(BayesianBanditError::NonFiniteInput);
    }
    if observations.iter().any(|observation| {
        observation.features.len() != current_features.len()
            || observation.features.iter().any(|value| !value.is_finite())
            || !observation.reward.is_finite()
    }) {
        return Err(BayesianBanditError::InvalidFeatureWidth);
    }

    let dimensions = current_features.len();
    let noise_precision = 1.0 / config.noise_variance;
    let mut rng = DeterministicRng::new(config.seed);
    let mut best: Option<LinearThompsonRecommendation> = None;
    for action in 0..config.action_count {
        let mut precision = vec![vec![0.0; dimensions]; dimensions];
        for (index, row) in precision.iter_mut().enumerate() {
            row[index] = config.prior_precision;
        }
        let mut response = vec![0.0; dimensions];
        let mut count = 0_u64;
        for observation in observations
            .iter()
            .filter(|observation| observation.action == action)
        {
            count = count.saturating_add(1);
            for left in 0..dimensions {
                response[left] += noise_precision * observation.reward * observation.features[left];
                for right in 0..dimensions {
                    precision[left][right] +=
                        noise_precision * observation.features[left] * observation.features[right];
                }
            }
        }
        let covariance = invert_matrix(&precision).ok_or(BayesianBanditError::SingularSystem)?;
        let posterior_mean = matrix_vector(&covariance, &response);
        let cholesky = cholesky_lower(&covariance).ok_or(BayesianBanditError::SingularSystem)?;
        let standard_normal: Vec<f64> = (0..dimensions).map(|_| rng.standard_normal()).collect();
        let perturbation = matrix_vector(&cholesky, &standard_normal);
        let sampled_theta: Vec<f64> = posterior_mean
            .iter()
            .zip(perturbation)
            .map(|(mean, noise)| mean + config.posterior_sample_scale * noise)
            .collect();
        let posterior_mean_reward = dot(&posterior_mean, current_features);
        let sampled_reward = dot(&sampled_theta, current_features);
        if !posterior_mean_reward.is_finite() || !sampled_reward.is_finite() {
            return Err(BayesianBanditError::NonFiniteInput);
        }
        let candidate = LinearThompsonRecommendation {
            action,
            posterior_mean_reward,
            sampled_reward,
            observations: count,
        };
        if best.as_ref().is_none_or(|current| {
            sampled_reward > current.sampled_reward
                || (sampled_reward == current.sampled_reward && action < current.action)
        }) {
            best = Some(candidate);
        }
    }
    Ok(best.expect("positive action count yields recommendation"))
}

fn cholesky_lower(matrix: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let width = matrix.len();
    if width == 0 || matrix.iter().any(|row| row.len() != width) {
        return None;
    }
    let mut lower = vec![vec![0.0; width]; width];
    for row in 0..width {
        for column in 0..=row {
            let mut sum = matrix[row][column];
            for index in 0..column {
                sum -= lower[row][index] * lower[column][index];
            }
            if row == column {
                if !sum.is_finite() || sum <= 1e-14 {
                    return None;
                }
                lower[row][column] = sum.sqrt();
            } else {
                lower[row][column] = sum / lower[column][column];
            }
        }
    }
    Some(lower)
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
                0xbb67_ae85_84ca_a73b
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
    fn gaussian_thompson_is_replayable_and_prefers_better_arm() {
        let observations: Vec<GaussianRewardObservation> = (0..30)
            .flat_map(|_| {
                [
                    GaussianRewardObservation {
                        action: 0,
                        reward: 10.0,
                    },
                    GaussianRewardObservation {
                        action: 1,
                        reward: 2.0,
                    },
                ]
            })
            .collect();
        let prior = GaussianRewardPrior {
            mean: 0.0,
            precision: 1.0,
            observation_precision: 1.0,
        };
        let first = gaussian_thompson_recommend(&observations, 2, prior, 77)
            .expect("Gaussian Thompson recommendation");
        let second = gaussian_thompson_recommend(&observations, 2, prior, 77)
            .expect("replayed recommendation");
        assert_eq!(first, second);
        assert_eq!(first.action, 0);
        assert_eq!(first.observations, 30);
    }

    #[test]
    fn linear_thompson_uses_continuous_context_and_is_replayable() {
        let observations = vec![
            LinearThompsonObservation {
                action: 0,
                features: vec![1.0, 0.0],
                reward: 8.0,
            },
            LinearThompsonObservation {
                action: 0,
                features: vec![1.0, 1.0],
                reward: 10.0,
            },
            LinearThompsonObservation {
                action: 1,
                features: vec![1.0, 0.0],
                reward: 1.0,
            },
            LinearThompsonObservation {
                action: 1,
                features: vec![1.0, 1.0],
                reward: 2.0,
            },
        ];
        let config = LinearThompsonConfig {
            action_count: 2,
            prior_precision: 1.0,
            noise_variance: 1.0,
            posterior_sample_scale: 0.05,
            seed: 99,
        };
        let first = linear_thompson_recommend(&observations, &[1.0, 0.5], config)
            .expect("linear Thompson recommendation");
        let second = linear_thompson_recommend(&observations, &[1.0, 0.5], config)
            .expect("replayed linear Thompson recommendation");
        assert_eq!(first, second);
        assert_eq!(first.action, 0);
        assert!(first.posterior_mean_reward > 4.0);
    }
}
