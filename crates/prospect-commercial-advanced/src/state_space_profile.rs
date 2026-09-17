use crate::state_space::LocalLinearTrendConfig;
use crate::state_space_optimization::{
    StateSpaceOptimizationConfig, StateSpaceOptimizationError, VarianceBounds,
    optimize_local_linear_trend_likelihood,
};
use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileVariance {
    LevelProcess,
    TrendProcess,
    Measurement,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileLikelihoodPoint {
    pub variance: f64,
    pub log_likelihood: f64,
    pub deviance_from_profile_maximum: f64,
    pub nuisance_optimum: LocalLinearTrendConfig,
    pub evaluations: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileLikelihoodReport {
    pub parameter: ProfileVariance,
    pub points: Vec<ProfileLikelihoodPoint>,
    pub maximum_log_likelihood: f64,
    pub maximizing_variance: f64,
    pub total_evaluations: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiscreteProfileRegion {
    pub lower: f64,
    pub upper: f64,
    pub cutoff: f64,
    pub included_points: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StateSpaceProfileError {
    EmptyGrid,
    NonFiniteGrid,
    DuplicateGridPoint,
    GridPointOutsideBounds,
    InvalidTotalBudget,
    InvalidCutoff,
    BudgetExceeded { used: u64 },
    NoUsablePoint,
    Optimization(StateSpaceOptimizationError),
}

impl fmt::Display for StateSpaceProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGrid => formatter.write_str("state-space profile grid must not be empty"),
            Self::NonFiniteGrid => {
                formatter.write_str("state-space profile grid values must be finite")
            }
            Self::DuplicateGridPoint => {
                formatter.write_str("state-space profile grid values must be unique")
            }
            Self::GridPointOutsideBounds => formatter
                .write_str("state-space profile grid value lies outside the optimization bounds"),
            Self::InvalidTotalBudget => {
                formatter.write_str("state-space profile total evaluation budget must be non-zero")
            }
            Self::InvalidCutoff => formatter
                .write_str("state-space profile deviance cutoff must be finite and non-negative"),
            Self::BudgetExceeded { used } => write!(
                formatter,
                "state-space profile exhausted its evaluation budget after {used} evaluations"
            ),
            Self::NoUsablePoint => {
                formatter.write_str("state-space profile contains no usable likelihood point")
            }
            Self::Optimization(error) => {
                write!(
                    formatter,
                    "state-space profile optimization failed: {error}"
                )
            }
        }
    }
}

impl std::error::Error for StateSpaceProfileError {}

impl From<StateSpaceOptimizationError> for StateSpaceProfileError {
    fn from(error: StateSpaceOptimizationError) -> Self {
        Self::Optimization(error)
    }
}

/// Profile one variance component of the local-linear-trend Kalman model.
///
/// For each caller-supplied value of the profiled variance, that coordinate is
/// fixed and the remaining variance components are reoptimized within the
/// caller-supplied box by the deterministic bounded pattern search. The report
/// then records the likelihood-ratio deviance `2 * (max_ll - ll)` for every
/// point.
///
/// The grid and evaluation budgets remain explicit evidence. This routine does
/// not infer a confidence level, asymptotic chi-square cutoff, or global MLE.
pub fn profile_local_linear_trend_variance(
    series: &[f64],
    optimization: StateSpaceOptimizationConfig,
    parameter: ProfileVariance,
    grid: &[f64],
    maximum_total_evaluations: u64,
) -> Result<ProfileLikelihoodReport, StateSpaceProfileError> {
    if grid.is_empty() {
        return Err(StateSpaceProfileError::EmptyGrid);
    }
    if maximum_total_evaluations == 0 {
        return Err(StateSpaceProfileError::InvalidTotalBudget);
    }
    if grid.iter().any(|value| !value.is_finite()) {
        return Err(StateSpaceProfileError::NonFiniteGrid);
    }

    let mut ordered = grid.to_vec();
    ordered.sort_by(f64::total_cmp);
    if ordered.windows(2).any(|window| window[0] == window[1]) {
        return Err(StateSpaceProfileError::DuplicateGridPoint);
    }
    let profiled_bounds = parameter_bounds(optimization, parameter);
    if ordered
        .iter()
        .any(|value| *value < profiled_bounds.minimum || *value > profiled_bounds.maximum)
    {
        return Err(StateSpaceProfileError::GridPointOutsideBounds);
    }

    let mut raw = Vec::with_capacity(ordered.len());
    let mut total_evaluations = 0_u64;
    for variance in ordered {
        if total_evaluations >= maximum_total_evaluations {
            return Err(StateSpaceProfileError::BudgetExceeded {
                used: total_evaluations,
            });
        }
        let remaining = maximum_total_evaluations - total_evaluations;
        let mut point_config = optimization;
        point_config.maximum_evaluations = point_config.maximum_evaluations.min(remaining).max(1);
        set_fixed_parameter(&mut point_config, parameter, variance);
        let result = optimize_local_linear_trend_likelihood(series, point_config)?;
        total_evaluations = total_evaluations.saturating_add(result.evaluations);
        raw.push((
            variance,
            result.log_likelihood,
            result.config,
            result.evaluations,
        ));
    }
    if raw.is_empty() {
        return Err(StateSpaceProfileError::NoUsablePoint);
    }

    let maximum_log_likelihood = raw
        .iter()
        .map(|(_, likelihood, _, _)| *likelihood)
        .max_by(f64::total_cmp)
        .ok_or(StateSpaceProfileError::NoUsablePoint)?;
    if !maximum_log_likelihood.is_finite() {
        return Err(StateSpaceProfileError::NoUsablePoint);
    }
    let maximizing_variance = raw
        .iter()
        .filter(|(_, likelihood, _, _)| *likelihood == maximum_log_likelihood)
        .map(|(variance, _, _, _)| *variance)
        .min_by(f64::total_cmp)
        .ok_or(StateSpaceProfileError::NoUsablePoint)?;

    let points = raw
        .into_iter()
        .map(
            |(variance, log_likelihood, nuisance_optimum, evaluations)| {
                let deviance = 2.0 * (maximum_log_likelihood - log_likelihood);
                ProfileLikelihoodPoint {
                    variance,
                    log_likelihood,
                    deviance_from_profile_maximum: deviance.max(0.0),
                    nuisance_optimum,
                    evaluations,
                }
            },
        )
        .collect();

    Ok(ProfileLikelihoodReport {
        parameter,
        points,
        maximum_log_likelihood,
        maximizing_variance,
        total_evaluations,
    })
}

/// Return the discrete envelope of profile points accepted by a caller-supplied
/// likelihood-ratio deviance cutoff.
///
/// No confidence level is attached to `cutoff`; interpreting it statistically
/// remains the caller's responsibility and depends on regularity assumptions.
pub fn discrete_profile_region(
    report: &ProfileLikelihoodReport,
    cutoff: f64,
) -> Result<Option<DiscreteProfileRegion>, StateSpaceProfileError> {
    if !cutoff.is_finite() || cutoff < 0.0 {
        return Err(StateSpaceProfileError::InvalidCutoff);
    }
    let mut included = report
        .points
        .iter()
        .filter(|point| point.deviance_from_profile_maximum <= cutoff)
        .map(|point| point.variance);
    let Some(first) = included.next() else {
        return Ok(None);
    };
    let mut lower = first;
    let mut upper = first;
    let mut count = 1_usize;
    for value in included {
        lower = lower.min(value);
        upper = upper.max(value);
        count = count.saturating_add(1);
    }
    Ok(Some(DiscreteProfileRegion {
        lower,
        upper,
        cutoff,
        included_points: count,
    }))
}

fn parameter_bounds(
    config: StateSpaceOptimizationConfig,
    parameter: ProfileVariance,
) -> VarianceBounds {
    match parameter {
        ProfileVariance::LevelProcess => config.level_process,
        ProfileVariance::TrendProcess => config.trend_process,
        ProfileVariance::Measurement => config.measurement,
    }
}

fn set_fixed_parameter(
    config: &mut StateSpaceOptimizationConfig,
    parameter: ProfileVariance,
    value: f64,
) {
    let fixed = VarianceBounds {
        minimum: value,
        maximum: value,
    };
    match parameter {
        ProfileVariance::LevelProcess => config.level_process = fixed,
        ProfileVariance::TrendProcess => config.trend_process = fixed,
        ProfileVariance::Measurement => config.measurement = fixed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn optimization_config() -> StateSpaceOptimizationConfig {
        StateSpaceOptimizationConfig {
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
            maximum_evaluations: 60,
            minimum_step_fraction: 1.0 / 64.0,
        }
    }

    #[test]
    fn likelihood_profile_is_deterministic_and_zero_at_its_maximum() {
        let series = [10.0, 10.8, 12.1, 12.9, 14.2, 14.8, 16.1, 17.0];
        let grid = [0.02, 0.10, 0.30, 0.70];
        let first = profile_local_linear_trend_variance(
            &series,
            optimization_config(),
            ProfileVariance::Measurement,
            &grid,
            400,
        )
        .expect("profile");
        let second = profile_local_linear_trend_variance(
            &series,
            optimization_config(),
            ProfileVariance::Measurement,
            &grid,
            400,
        )
        .expect("profile");
        assert_eq!(first, second);
        assert_eq!(first.points.len(), grid.len());
        assert!(
            first
                .points
                .iter()
                .any(|point| point.deviance_from_profile_maximum == 0.0)
        );
        assert!(first.total_evaluations <= 400);
    }

    #[test]
    fn discrete_region_uses_only_the_caller_cutoff() {
        let series = [10.0, 10.8, 12.1, 12.9, 14.2, 14.8, 16.1, 17.0];
        let report = profile_local_linear_trend_variance(
            &series,
            optimization_config(),
            ProfileVariance::Measurement,
            &[0.02, 0.10, 0.30, 0.70],
            400,
        )
        .expect("profile");
        let region = discrete_profile_region(&report, 2.0)
            .expect("region")
            .expect("at least profile maximum");
        assert!(region.lower <= report.maximizing_variance);
        assert!(region.upper >= report.maximizing_variance);
        assert!(region.included_points >= 1);
    }

    #[test]
    fn duplicate_and_out_of_bounds_profile_points_fail_closed() {
        let series = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(
            profile_local_linear_trend_variance(
                &series,
                optimization_config(),
                ProfileVariance::Measurement,
                &[0.1, 0.1],
                100,
            ),
            Err(StateSpaceProfileError::DuplicateGridPoint)
        );
        assert_eq!(
            profile_local_linear_trend_variance(
                &series,
                optimization_config(),
                ProfileVariance::Measurement,
                &[1.5],
                100,
            ),
            Err(StateSpaceProfileError::GridPointOutsideBounds)
        );
    }
}
