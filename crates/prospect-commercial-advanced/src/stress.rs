use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StressError {
    EmptySample,
    ZeroDraws,
    ArithmeticOverflow(&'static str),
    InvalidProbabilityPpm(u32),
    ProbabilityMassMismatch { actual_ppm: u64 },
}

impl fmt::Display for StressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySample => formatter.write_str("stress or Monte Carlo sample must not be empty"),
            Self::ZeroDraws => formatter.write_str("Monte Carlo draw count must be non-zero"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
            Self::InvalidProbabilityPpm(value) => write!(
                formatter,
                "probability must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::ProbabilityMassMismatch { actual_ppm } => write!(
                formatter,
                "stress probabilities must sum to {PROBABILITY_SCALE_PPM} ppm, got {actual_ppm} ppm"
            ),
        }
    }
}

impl std::error::Error for StressError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonteCarloConfig {
    pub seed: u64,
    pub draws: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonteCarloSummary {
    pub seed: u64,
    pub draws: u64,
    pub mean_payoff_minor_trunc: i128,
    pub minimum_payoff_minor: i64,
    pub p05_payoff_minor: i64,
    pub median_payoff_minor: i64,
    pub p95_payoff_minor: i64,
    pub maximum_payoff_minor: i64,
    pub loss_probability_ppm: u32,
}

/// Reproducible non-parametric Monte Carlo bootstrap over caller-supplied
/// observed/simulated payoffs. It does not invent a parametric distribution.
pub fn bootstrap_monte_carlo(
    empirical_payoffs_minor: &[i64],
    config: MonteCarloConfig,
) -> Result<MonteCarloSummary, StressError> {
    if empirical_payoffs_minor.is_empty() {
        return Err(StressError::EmptySample);
    }
    if config.draws == 0 {
        return Err(StressError::ZeroDraws);
    }
    let draw_count = usize::try_from(config.draws)
        .map_err(|_| StressError::ArithmeticOverflow("Monte Carlo draw allocation"))?;
    let mut rng = Lcg64::new(config.seed);
    let mut sample = Vec::with_capacity(draw_count);
    let mut total = 0_i128;
    let mut losses = 0_u64;
    for _ in 0..draw_count {
        let index = rng.next_index(empirical_payoffs_minor.len());
        let payoff = empirical_payoffs_minor[index];
        sample.push(payoff);
        total = total
            .checked_add(i128::from(payoff))
            .ok_or(StressError::ArithmeticOverflow("Monte Carlo mean"))?;
        if payoff < 0 {
            losses += 1;
        }
    }
    sample.sort_unstable();
    let loss_probability_ppm = u32::try_from(
        u128::from(losses)
            .saturating_mul(u128::from(PROBABILITY_SCALE_PPM))
            / u128::from(config.draws),
    )
    .expect("probability is bounded by ppm scale");
    Ok(MonteCarloSummary {
        seed: config.seed,
        draws: config.draws,
        mean_payoff_minor_trunc: total / i128::try_from(draw_count).expect("usize fits i128"),
        minimum_payoff_minor: sample[0],
        p05_payoff_minor: empirical_quantile(&sample, 50_000),
        median_payoff_minor: empirical_quantile(&sample, 500_000),
        p95_payoff_minor: empirical_quantile(&sample, 950_000),
        maximum_payoff_minor: *sample.last().expect("non-empty sample"),
        loss_probability_ppm,
    })
}

fn empirical_quantile(sorted: &[i64], quantile_ppm: u32) -> i64 {
    let last = sorted.len() - 1;
    let numerator = u128::try_from(last)
        .expect("usize fits u128")
        .saturating_mul(u128::from(quantile_ppm));
    let index = usize::try_from(
        numerator
            .saturating_add(u128::from(PROBABILITY_SCALE_PPM - 1))
            / u128::from(PROBABILITY_SCALE_PPM),
    )
    .expect("quantile index fits usize")
    .min(last);
    sorted[index]
}

struct Lcg64 {
    state: u64,
}

impl Lcg64 {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn next_index(&mut self, upper: usize) -> usize {
        let upper_u64 = u64::try_from(upper).expect("usize fits u64 on supported targets");
        usize::try_from(self.next_u64() % upper_u64).expect("index fits usize")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressScenario {
    pub probability_ppm: u32,
    /// Additive impact relative to the caller's baseline economic value.
    pub impact_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressSummary {
    pub baseline_value_minor: i64,
    pub expected_stressed_value_minor_trunc: i128,
    pub worst_stressed_value_minor: i128,
    pub best_stressed_value_minor: i128,
    pub probability_below_zero_ppm: u32,
}

pub fn evaluate_weighted_stress_cases(
    baseline_value_minor: i64,
    scenarios: &[StressScenario],
) -> Result<StressSummary, StressError> {
    if scenarios.is_empty() {
        return Err(StressError::EmptySample);
    }
    if let Some(value) = scenarios
        .iter()
        .map(|scenario| scenario.probability_ppm)
        .find(|value| *value > PROBABILITY_SCALE_PPM)
    {
        return Err(StressError::InvalidProbabilityPpm(value));
    }
    let actual_ppm: u64 = scenarios
        .iter()
        .map(|scenario| u64::from(scenario.probability_ppm))
        .sum();
    if actual_ppm != u64::from(PROBABILITY_SCALE_PPM) {
        return Err(StressError::ProbabilityMassMismatch { actual_ppm });
    }
    let mut expected_weighted = 0_i128;
    let mut worst = i128::MAX;
    let mut best = i128::MIN;
    let mut below_zero = 0_u32;
    for scenario in scenarios {
        let stressed = i128::from(baseline_value_minor)
            .checked_add(i128::from(scenario.impact_minor))
            .ok_or(StressError::ArithmeticOverflow("stressed value"))?;
        worst = worst.min(stressed);
        best = best.max(stressed);
        if stressed < 0 {
            below_zero = below_zero
                .checked_add(scenario.probability_ppm)
                .ok_or(StressError::ArithmeticOverflow("stress loss probability"))?;
        }
        expected_weighted = expected_weighted
            .checked_add(
                stressed
                    .checked_mul(i128::from(scenario.probability_ppm))
                    .ok_or(StressError::ArithmeticOverflow("weighted stress value"))?,
            )
            .ok_or(StressError::ArithmeticOverflow("stress expectation"))?;
    }
    Ok(StressSummary {
        baseline_value_minor,
        expected_stressed_value_minor_trunc: expected_weighted
            / i128::from(PROBABILITY_SCALE_PPM),
        worst_stressed_value_minor: worst,
        best_stressed_value_minor: best,
        probability_below_zero_ppm: below_zero,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_is_seeded_and_replayable() {
        let config = MonteCarloConfig { seed: 7, draws: 1_000 };
        let first = bootstrap_monte_carlo(&[-100, 0, 100, 200], config).expect("simulation");
        let second = bootstrap_monte_carlo(&[-100, 0, 100, 200], config).expect("simulation");
        assert_eq!(first, second);
        assert_eq!(first.minimum_payoff_minor, -100);
        assert_eq!(first.maximum_payoff_minor, 200);
    }

    #[test]
    fn weighted_stress_reports_loss_probability() {
        let summary = evaluate_weighted_stress_cases(
            100,
            &[
                StressScenario { probability_ppm: 200_000, impact_minor: -200 },
                StressScenario { probability_ppm: 800_000, impact_minor: 50 },
            ],
        )
        .expect("stress summary");
        assert_eq!(summary.expected_stressed_value_minor_trunc, 100);
        assert_eq!(summary.worst_stressed_value_minor, -100);
        assert_eq!(summary.probability_below_zero_ppm, 200_000);
    }
}
