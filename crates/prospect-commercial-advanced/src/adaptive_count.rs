use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum CountBanditError {
    InvalidActionCount,
    InvalidPrior,
    InvalidAction { action: u32 },
    InvalidExposure,
    NumericalBreakdown,
}

impl fmt::Display for CountBanditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidActionCount => {
                formatter.write_str("count bandit requires at least one action")
            }
            Self::InvalidPrior => formatter
                .write_str("Gamma-Poisson prior shape and rate must be finite and positive"),
            Self::InvalidAction { action } => {
                write!(
                    formatter,
                    "count bandit observation references invalid action {action}"
                )
            }
            Self::InvalidExposure => {
                formatter.write_str("count bandit exposure must be finite and strictly positive")
            }
            Self::NumericalBreakdown => {
                formatter.write_str("Gamma-Poisson sampling encountered numerical breakdown")
            }
        }
    }
}

impl std::error::Error for CountBanditError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GammaRatePrior {
    /// Gamma shape parameter alpha.
    pub shape: f64,
    /// Gamma rate parameter beta (inverse scale).
    pub rate: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CountObservation {
    pub action: u32,
    pub count: u64,
    /// Exposure measured in caller-defined units: visits, machine-hours, days, etc.
    pub exposure: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CountThompsonRecommendation {
    pub action: u32,
    pub posterior_shape: f64,
    pub posterior_rate: f64,
    pub posterior_mean_rate: f64,
    pub sampled_rate: f64,
    pub observed_count: u64,
    pub observed_exposure: f64,
}

/// Thompson Sampling for Poisson event counts under a Gamma prior.
///
/// The model is conjugate and therefore keeps the decision evidence explicit:
/// `lambda_a ~ Gamma(alpha, beta)` and `count ~ Poisson(lambda_a * exposure)`.
/// Rates are only comparable when callers use the same exposure unit across
/// actions.
pub fn gamma_poisson_thompson_recommend(
    observations: &[CountObservation],
    action_count: u32,
    prior: GammaRatePrior,
    seed: u64,
) -> Result<CountThompsonRecommendation, CountBanditError> {
    if action_count == 0 {
        return Err(CountBanditError::InvalidActionCount);
    }
    if !prior.shape.is_finite()
        || !prior.rate.is_finite()
        || prior.shape <= 0.0
        || prior.rate <= 0.0
    {
        return Err(CountBanditError::InvalidPrior);
    }

    let width = usize::try_from(action_count).expect("u32 fits usize");
    let mut counts = vec![0_u64; width];
    let mut exposures = vec![0.0_f64; width];
    for observation in observations {
        if observation.action >= action_count {
            return Err(CountBanditError::InvalidAction {
                action: observation.action,
            });
        }
        if !observation.exposure.is_finite() || observation.exposure <= 0.0 {
            return Err(CountBanditError::InvalidExposure);
        }
        let index = usize::try_from(observation.action).expect("bounded action fits usize");
        counts[index] = counts[index].saturating_add(observation.count);
        exposures[index] += observation.exposure;
        if !exposures[index].is_finite() {
            return Err(CountBanditError::NumericalBreakdown);
        }
    }

    let mut rng = DeterministicRng::new(seed);
    let mut best: Option<CountThompsonRecommendation> = None;
    for action in 0..action_count {
        let index = usize::try_from(action).expect("bounded action fits usize");
        let posterior_shape = prior.shape + counts[index] as f64;
        let posterior_rate = prior.rate + exposures[index];
        let posterior_mean_rate = posterior_shape / posterior_rate;
        let sampled_rate = sample_gamma_rate(posterior_shape, posterior_rate, &mut rng)?;
        if !posterior_mean_rate.is_finite() || !sampled_rate.is_finite() || sampled_rate < 0.0 {
            return Err(CountBanditError::NumericalBreakdown);
        }
        let candidate = CountThompsonRecommendation {
            action,
            posterior_shape,
            posterior_rate,
            posterior_mean_rate,
            sampled_rate,
            observed_count: counts[index],
            observed_exposure: exposures[index],
        };
        if best.as_ref().is_none_or(|current| {
            candidate.sampled_rate > current.sampled_rate
                || (candidate.sampled_rate == current.sampled_rate && action < current.action)
        }) {
            best = Some(candidate);
        }
    }
    Ok(best.expect("positive action count yields recommendation"))
}

fn sample_gamma_rate(
    shape: f64,
    rate: f64,
    rng: &mut DeterministicRng,
) -> Result<f64, CountBanditError> {
    if shape < 1.0 {
        let augmented = sample_gamma_rate(shape + 1.0, rate, rng)?;
        let uniform = rng.open_unit();
        let sample = augmented * uniform.powf(1.0 / shape);
        return sample
            .is_finite()
            .then_some(sample)
            .ok_or(CountBanditError::NumericalBreakdown);
    }

    let d = shape - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    for _ in 0..10_000 {
        let normal = rng.standard_normal();
        let one_plus = 1.0 + c * normal;
        if one_plus <= 0.0 {
            continue;
        }
        let v = one_plus * one_plus * one_plus;
        let uniform = rng.open_unit();
        let normal_squared = normal * normal;
        let squeeze = 1.0 - 0.0331 * normal_squared * normal_squared;
        let accepted =
            uniform < squeeze || uniform.ln() < 0.5 * normal_squared + d * (1.0 - v + v.ln());
        if accepted {
            let sample = d * v / rate;
            if sample.is_finite() && sample >= 0.0 {
                return Ok(sample);
            }
            return Err(CountBanditError::NumericalBreakdown);
        }
    }
    Err(CountBanditError::NumericalBreakdown)
}

#[derive(Clone, Copy, Debug)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    const fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn open_unit(&mut self) -> f64 {
        // 53 random mantissa bits, shifted away from both exact endpoints.
        let mantissa = self.next_u64() >> 11;
        (mantissa as f64 + 0.5) / ((1_u64 << 53) as f64)
    }

    fn standard_normal(&mut self) -> f64 {
        let left = self.open_unit();
        let right = self.open_unit();
        (-2.0 * left.ln()).sqrt() * (2.0 * core::f64::consts::PI * right).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_poisson_policy_is_seed_replayable() {
        let observations = [
            CountObservation {
                action: 0,
                count: 5,
                exposure: 10.0,
            },
            CountObservation {
                action: 1,
                count: 40,
                exposure: 10.0,
            },
        ];
        let prior = GammaRatePrior {
            shape: 1.0,
            rate: 1.0,
        };
        let first =
            gamma_poisson_thompson_recommend(&observations, 2, prior, 42).expect("count policy");
        let second =
            gamma_poisson_thompson_recommend(&observations, 2, prior, 42).expect("count policy");
        assert_eq!(first, second);
        assert_eq!(first.action, 1);
        assert!(first.posterior_mean_rate > 3.0);
    }

    #[test]
    fn exposure_changes_posterior_rate_not_just_raw_count() {
        let observations = [
            CountObservation {
                action: 0,
                count: 10,
                exposure: 100.0,
            },
            CountObservation {
                action: 1,
                count: 8,
                exposure: 10.0,
            },
        ];
        let recommendation = gamma_poisson_thompson_recommend(
            &observations,
            2,
            GammaRatePrior {
                shape: 2.0,
                rate: 2.0,
            },
            7,
        )
        .expect("count policy");
        assert_eq!(recommendation.action, 1);
    }

    #[test]
    fn invalid_exposure_fails_closed() {
        let observations = [CountObservation {
            action: 0,
            count: 1,
            exposure: 0.0,
        }];
        assert_eq!(
            gamma_poisson_thompson_recommend(
                &observations,
                1,
                GammaRatePrior {
                    shape: 1.0,
                    rate: 1.0,
                },
                1,
            ),
            Err(CountBanditError::InvalidExposure)
        );
    }
}
