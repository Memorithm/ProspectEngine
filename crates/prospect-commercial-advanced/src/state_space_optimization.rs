use crate::state_space::{LocalLinearTrendConfig, LocalLinearTrendModel};
use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VarianceBounds {
    pub minimum: f64,
    pub maximum: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateSpaceOptimizationConfig {
    pub level_process: VarianceBounds,
    pub trend_process: VarianceBounds,
    pub measurement: VarianceBounds,
    pub initial_variance: f64,
    pub maximum_evaluations: u64,
    pub minimum_step_fraction: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateSpaceOptimizationResult {
    pub config: LocalLinearTrendConfig,
    pub log_likelihood: f64,
    pub evaluations: u64,
    pub accepted_moves: u64,
    pub final_step_fraction: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StateSpaceOptimizationError {
    InvalidBounds,
    InvalidInitialVariance,
    InvalidEvaluationBudget,
    InvalidStepFraction,
    NoUsableCandidate,
}

impl fmt::Display for StateSpaceOptimizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBounds => formatter.write_str(
                "state-space variance bounds must be finite, ordered, process bounds non-negative and measurement bounds positive",
            ),
            Self::InvalidInitialVariance => formatter.write_str(
                "state-space initial variance must be finite and strictly positive",
            ),
            Self::InvalidEvaluationBudget => formatter.write_str(
                "state-space optimization evaluation budget must be non-zero",
            ),
            Self::InvalidStepFraction => formatter.write_str(
                "state-space minimum step fraction must be finite and in (0, 1]",
            ),
            Self::NoUsableCandidate => formatter.write_str(
                "state-space bounded optimization found no usable likelihood candidate",
            ),
        }
    }
}

impl std::error::Error for StateSpaceOptimizationError {}

/// Deterministic bounded pattern search over the three variance components of
/// the local-linear-trend Kalman model.
///
/// The search begins at the midpoint of each caller-supplied interval, tests
/// positive and negative coordinate moves, accepts strict likelihood
/// improvements, and halves the normalized step when no coordinate improves.
/// It is continuous within the supplied box but remains a local derivative-free
/// optimizer; it does not claim a global MLE or Hessian-based uncertainty.
pub fn optimize_local_linear_trend_likelihood(
    series: &[f64],
    config: StateSpaceOptimizationConfig,
) -> Result<StateSpaceOptimizationResult, StateSpaceOptimizationError> {
    validate(config)?;

    let mut point = [
        midpoint(config.level_process),
        midpoint(config.trend_process),
        midpoint(config.measurement),
    ];
    let bounds = [config.level_process, config.trend_process, config.measurement];
    let mut evaluations = 0_u64;
    let mut accepted_moves = 0_u64;
    let mut step_fraction = 0.5_f64;
    let mut best = evaluate(series, point, config.initial_variance)?;
    evaluations = evaluations.saturating_add(1);

    while evaluations < config.maximum_evaluations && step_fraction >= config.minimum_step_fraction {
        let mut improved = false;
        for dimension in 0..3 {
            if evaluations >= config.maximum_evaluations {
                break;
            }
            let span = bounds[dimension].maximum - bounds[dimension].minimum;
            if span == 0.0 {
                continue;
            }
            let step = span * step_fraction;
            let original = point[dimension];
            let candidates = [
                (original + step).min(bounds[dimension].maximum),
                (original - step).max(bounds[dimension].minimum),
            ];
            let mut dimension_best = best;
            let mut dimension_value = original;
            for candidate in candidates {
                if evaluations >= config.maximum_evaluations || candidate == original {
                    continue;
                }
                let mut trial = point;
                trial[dimension] = candidate;
                if let Ok(score) = evaluate(series, trial, config.initial_variance) {
                    evaluations = evaluations.saturating_add(1);
                    if score.1 > dimension_best.1 {
                        dimension_best = score;
                        dimension_value = candidate;
                    }
                } else {
                    evaluations = evaluations.saturating_add(1);
                }
            }
            if dimension_value != original {
                point[dimension] = dimension_value;
                best = dimension_best;
                accepted_moves = accepted_moves.saturating_add(1);
                improved = true;
            }
        }
        if !improved {
            step_fraction *= 0.5;
        }
    }

    Ok(StateSpaceOptimizationResult {
        config: best.0,
        log_likelihood: best.1,
        evaluations,
        accepted_moves,
        final_step_fraction: step_fraction,
    })
}

fn evaluate(
    series: &[f64],
    point: [f64; 3],
    initial_variance: f64,
) -> Result<(LocalLinearTrendConfig, f64), StateSpaceOptimizationError> {
    let config = LocalLinearTrendConfig {
        level_process_variance: point[0],
        trend_process_variance: point[1],
        measurement_variance: point[2],
        initial_variance,
    };
    let model = LocalLinearTrendModel::fit(series, config)
        .map_err(|_| StateSpaceOptimizationError::NoUsableCandidate)?;
    let likelihood = model.log_likelihood();
    if !likelihood.is_finite() {
        return Err(StateSpaceOptimizationError::NoUsableCandidate);
    }
    Ok((config, likelihood))
}

fn validate(config: StateSpaceOptimizationConfig) -> Result<(), StateSpaceOptimizationError> {
    for (index, bounds) in [config.level_process, config.trend_process, config.measurement]
        .into_iter()
        .enumerate()
    {
        if !bounds.minimum.is_finite()
            || !bounds.maximum.is_finite()
            || bounds.minimum > bounds.maximum
            || bounds.minimum < 0.0
            || (index == 2 && bounds.minimum <= 0.0)
        {
            return Err(StateSpaceOptimizationError::InvalidBounds);
        }
    }
    if !config.initial_variance.is_finite() || config.initial_variance <= 0.0 {
        return Err(StateSpaceOptimizationError::InvalidInitialVariance);
    }
    if config.maximum_evaluations == 0 {
        return Err(StateSpaceOptimizationError::InvalidEvaluationBudget);
    }
    if !config.minimum_step_fraction.is_finite()
        || config.minimum_step_fraction <= 0.0
        || config.minimum_step_fraction > 1.0
    {
        return Err(StateSpaceOptimizationError::InvalidStepFraction);
    }
    Ok(())
}

fn midpoint(bounds: VarianceBounds) -> f64 {
    bounds.minimum + 0.5 * (bounds.maximum - bounds.minimum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_pattern_search_is_deterministic_and_improves_or_keeps_midpoint() {
        let series = [10.0, 10.8, 12.1, 12.9, 14.2, 14.8, 16.1, 17.0];
        let config = StateSpaceOptimizationConfig {
            level_process: VarianceBounds {
                minimum: 0.0,
                maximum: 0.5,
            },
            trend_process: VarianceBounds {
                minimum: 0.0,
                maximum: 0.1,
            },
            measurement: VarianceBounds {
                minimum: 0.01,
                maximum: 1.0,
            },
            initial_variance: 1.0,
            maximum_evaluations: 80,
            minimum_step_fraction: 1.0 / 128.0,
        };
        let midpoint_config = LocalLinearTrendConfig {
            level_process_variance: 0.25,
            trend_process_variance: 0.05,
            measurement_variance: 0.505,
            initial_variance: 1.0,
        };
        let midpoint_likelihood = LocalLinearTrendModel::fit(&series, midpoint_config)
            .expect("midpoint model")
            .log_likelihood();
        let first = optimize_local_linear_trend_likelihood(&series, config).expect("optimization");
        let second = optimize_local_linear_trend_likelihood(&series, config).expect("optimization");
        assert_eq!(first, second);
        assert!(first.log_likelihood >= midpoint_likelihood);
        assert!(first.evaluations <= config.maximum_evaluations);
    }

    #[test]
    fn invalid_measurement_lower_bound_fails_closed() {
        let config = StateSpaceOptimizationConfig {
            level_process: VarianceBounds {
                minimum: 0.0,
                maximum: 1.0,
            },
            trend_process: VarianceBounds {
                minimum: 0.0,
                maximum: 1.0,
            },
            measurement: VarianceBounds {
                minimum: 0.0,
                maximum: 1.0,
            },
            initial_variance: 1.0,
            maximum_evaluations: 10,
            minimum_step_fraction: 0.1,
        };
        assert_eq!(
            optimize_local_linear_trend_likelihood(&[1.0, 2.0], config),
            Err(StateSpaceOptimizationError::InvalidBounds)
        );
    }
}
