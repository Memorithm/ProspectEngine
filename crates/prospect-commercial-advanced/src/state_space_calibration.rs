use crate::state_space::{
    LocalLinearTrendConfig, LocalLinearTrendModel, StateSpaceError, StateSpaceForecastPoint,
};
use core::cmp::Ordering;
use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum StateSpaceCalibrationError {
    EmptyGrid,
    InvalidGridValue,
    InvalidInitialVariance,
    InvalidCandidateBudget,
    CandidateBudgetExceeded { needed: u64, maximum: u64 },
    NoUsableCandidate,
    InvalidIntervalMultiplier,
}

impl fmt::Display for StateSpaceCalibrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGrid => formatter.write_str("state-space calibration grids must not be empty"),
            Self::InvalidGridValue => formatter.write_str(
                "state-space process grids must be finite and non-negative and measurement grid strictly positive",
            ),
            Self::InvalidInitialVariance => {
                formatter.write_str("state-space calibration initial variance must be finite and positive")
            }
            Self::InvalidCandidateBudget => {
                formatter.write_str("state-space candidate budget must be non-zero")
            }
            Self::CandidateBudgetExceeded { needed, maximum } => write!(
                formatter,
                "state-space calibration candidate budget exceeded: need {needed}, maximum {maximum}"
            ),
            Self::NoUsableCandidate => {
                formatter.write_str("no state-space variance candidate could be fit")
            }
            Self::InvalidIntervalMultiplier => formatter.write_str(
                "predictive interval multiplier must be finite and non-negative",
            ),
        }
    }
}

impl std::error::Error for StateSpaceCalibrationError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VarianceCalibrationScore {
    pub config: LocalLinearTrendConfig,
    pub log_likelihood: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VarianceCalibrationReport {
    pub selected: VarianceCalibrationScore,
    pub ranked: Vec<VarianceCalibrationScore>,
    pub attempted_candidates: u64,
    pub failed_candidates: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PredictiveInterval {
    pub horizon: usize,
    pub mean: f64,
    pub lower: f64,
    pub upper: f64,
    pub variance: f64,
    pub standard_deviation: f64,
    pub standard_deviation_multiplier: f64,
}

/// Calibrate local-linear-trend variance components by deterministic grid
/// likelihood maximization.
///
/// The search space is caller supplied and finite. This makes the calibration
/// replayable and auditable while avoiding a false claim of continuous global
/// maximum-likelihood optimization. Ties use lexicographic variance ordering.
pub fn calibrate_local_linear_trend_grid(
    series: &[f64],
    level_process_variances: &[f64],
    trend_process_variances: &[f64],
    measurement_variances: &[f64],
    initial_variance: f64,
    maximum_candidates: u64,
) -> Result<VarianceCalibrationReport, StateSpaceCalibrationError> {
    validate_grid(
        level_process_variances,
        trend_process_variances,
        measurement_variances,
        initial_variance,
        maximum_candidates,
    )?;

    let needed_usize = level_process_variances
        .len()
        .saturating_mul(trend_process_variances.len())
        .saturating_mul(measurement_variances.len());
    let needed = u64::try_from(needed_usize).unwrap_or(u64::MAX);
    if needed > maximum_candidates {
        return Err(StateSpaceCalibrationError::CandidateBudgetExceeded {
            needed,
            maximum: maximum_candidates,
        });
    }

    let mut ranked = Vec::with_capacity(needed_usize);
    let mut failed_candidates = 0_u64;
    for level_process_variance in level_process_variances {
        for trend_process_variance in trend_process_variances {
            for measurement_variance in measurement_variances {
                let config = LocalLinearTrendConfig {
                    level_process_variance: *level_process_variance,
                    trend_process_variance: *trend_process_variance,
                    measurement_variance: *measurement_variance,
                    initial_variance,
                };
                match LocalLinearTrendModel::fit(series, config) {
                    Ok(model) if model.log_likelihood().is_finite() => {
                        ranked.push(VarianceCalibrationScore {
                            config,
                            log_likelihood: model.log_likelihood(),
                        });
                    }
                    Ok(_) | Err(_) => {
                        failed_candidates = failed_candidates.saturating_add(1);
                    }
                }
            }
        }
    }
    if ranked.is_empty() {
        return Err(StateSpaceCalibrationError::NoUsableCandidate);
    }
    ranked.sort_by(compare_scores);
    Ok(VarianceCalibrationReport {
        selected: ranked[0],
        ranked,
        attempted_candidates: needed,
        failed_candidates,
    })
}

/// Convert state-space forecast variances into symmetric Gaussian intervals
/// using an explicit caller-supplied standard-deviation multiplier.
///
/// A multiplier of about 1.96 is often used for a nominal 95% Gaussian
/// interval, but this function deliberately does not claim empirical coverage.
pub fn gaussian_predictive_intervals(
    forecast: &[StateSpaceForecastPoint],
    standard_deviation_multiplier: f64,
) -> Result<Vec<PredictiveInterval>, StateSpaceCalibrationError> {
    if !standard_deviation_multiplier.is_finite() || standard_deviation_multiplier < 0.0 {
        return Err(StateSpaceCalibrationError::InvalidIntervalMultiplier);
    }
    forecast
        .iter()
        .map(|point| {
            if !point.mean.is_finite() || !point.variance.is_finite() || point.variance < 0.0 {
                return Err(StateSpaceCalibrationError::NoUsableCandidate);
            }
            let standard_deviation = point.variance.sqrt();
            let radius = standard_deviation_multiplier * standard_deviation;
            Ok(PredictiveInterval {
                horizon: point.horizon,
                mean: point.mean,
                lower: point.mean - radius,
                upper: point.mean + radius,
                variance: point.variance,
                standard_deviation,
                standard_deviation_multiplier,
            })
        })
        .collect()
}

fn validate_grid(
    level_process_variances: &[f64],
    trend_process_variances: &[f64],
    measurement_variances: &[f64],
    initial_variance: f64,
    maximum_candidates: u64,
) -> Result<(), StateSpaceCalibrationError> {
    if level_process_variances.is_empty()
        || trend_process_variances.is_empty()
        || measurement_variances.is_empty()
    {
        return Err(StateSpaceCalibrationError::EmptyGrid);
    }
    if level_process_variances
        .iter()
        .chain(trend_process_variances)
        .any(|value| !value.is_finite() || *value < 0.0)
        || measurement_variances
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err(StateSpaceCalibrationError::InvalidGridValue);
    }
    if !initial_variance.is_finite() || initial_variance <= 0.0 {
        return Err(StateSpaceCalibrationError::InvalidInitialVariance);
    }
    if maximum_candidates == 0 {
        return Err(StateSpaceCalibrationError::InvalidCandidateBudget);
    }
    Ok(())
}

fn compare_scores(left: &VarianceCalibrationScore, right: &VarianceCalibrationScore) -> Ordering {
    right
        .log_likelihood
        .total_cmp(&left.log_likelihood)
        .then_with(|| {
            left.config
                .level_process_variance
                .total_cmp(&right.config.level_process_variance)
        })
        .then_with(|| {
            left.config
                .trend_process_variance
                .total_cmp(&right.config.trend_process_variance)
        })
        .then_with(|| {
            left.config
                .measurement_variance
                .total_cmp(&right.config.measurement_variance)
        })
}

impl From<StateSpaceError> for StateSpaceCalibrationError {
    fn from(_: StateSpaceError) -> Self {
        Self::NoUsableCandidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn likelihood_grid_calibration_is_deterministic_and_bounded() {
        let series = [10.0, 11.1, 11.9, 13.2, 14.0, 15.1, 15.9, 17.2];
        let first = calibrate_local_linear_trend_grid(
            &series,
            &[0.0, 0.01, 0.1],
            &[0.0, 0.001, 0.01],
            &[0.01, 0.1, 1.0],
            1.0,
            27,
        )
        .expect("calibration");
        let second = calibrate_local_linear_trend_grid(
            &series,
            &[0.0, 0.01, 0.1],
            &[0.0, 0.001, 0.01],
            &[0.01, 0.1, 1.0],
            1.0,
            27,
        )
        .expect("calibration");
        assert_eq!(first, second);
        assert_eq!(first.attempted_candidates, 27);
        assert!(first.selected.log_likelihood.is_finite());
        assert!(!first.ranked.is_empty());
    }

    #[test]
    fn candidate_budget_fails_closed_before_grid_execution() {
        let result = calibrate_local_linear_trend_grid(
            &[1.0, 2.0],
            &[0.0, 0.1],
            &[0.0, 0.1],
            &[0.1, 1.0],
            1.0,
            7,
        );
        assert_eq!(
            result,
            Err(StateSpaceCalibrationError::CandidateBudgetExceeded {
                needed: 8,
                maximum: 7,
            })
        );
    }

    #[test]
    fn predictive_intervals_preserve_variance_and_multiplier() {
        let forecast = [StateSpaceForecastPoint {
            horizon: 1,
            mean: 10.0,
            variance: 4.0,
        }];
        let interval = gaussian_predictive_intervals(&forecast, 2.0)
            .expect("predictive interval")[0];
        assert_eq!(interval.standard_deviation, 2.0);
        assert_eq!(interval.lower, 6.0);
        assert_eq!(interval.upper, 14.0);
    }
}
