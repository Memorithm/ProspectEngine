use crate::timeseries::{SeasonalArimaModel, SeasonalArimaOrder};
use core::cmp::Ordering;
use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum StateSpaceError {
    SeriesTooShort,
    NonFiniteInput,
    InvalidVariance,
    InvalidInitialVariance,
    InvalidHoldout,
    EmptyCandidateSet,
    NoUsableCandidate,
    NumericalBreakdown,
}

impl fmt::Display for StateSpaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SeriesTooShort => {
                formatter.write_str("state-space series must contain at least two observations")
            }
            Self::NonFiniteInput => formatter.write_str("state-space inputs must be finite"),
            Self::InvalidVariance => formatter.write_str(
                "state-space process variances must be non-negative and measurement variance positive",
            ),
            Self::InvalidInitialVariance => {
                formatter.write_str("state-space initial variance must be finite and positive")
            }
            Self::InvalidHoldout => {
                formatter.write_str("forecast holdout must leave at least two training observations")
            }
            Self::EmptyCandidateSet => {
                formatter.write_str("SARIMA selection requires at least one candidate")
            }
            Self::NoUsableCandidate => {
                formatter.write_str("no SARIMA candidate could be fit and scored")
            }
            Self::NumericalBreakdown => {
                formatter.write_str("state-space recursion encountered numerical breakdown")
            }
        }
    }
}

impl std::error::Error for StateSpaceError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalLinearTrendConfig {
    pub level_process_variance: f64,
    pub trend_process_variance: f64,
    pub measurement_variance: f64,
    pub initial_variance: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateSpaceForecastPoint {
    pub horizon: usize,
    pub mean: f64,
    /// Predictive observation variance, including measurement noise.
    pub variance: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalLinearTrendModel {
    config: LocalLinearTrendConfig,
    level: f64,
    trend: f64,
    covariance: [[f64; 2]; 2],
    log_likelihood: f64,
    innovations: Vec<f64>,
}

impl LocalLinearTrendModel {
    pub fn fit(
        series: &[f64],
        config: LocalLinearTrendConfig,
    ) -> Result<Self, StateSpaceError> {
        validate_series(series)?;
        validate_config(config)?;

        let mut level = series[0];
        let mut trend = series[1] - series[0];
        let mut covariance = [
            [config.initial_variance, 0.0],
            [0.0, config.initial_variance],
        ];
        let mut log_likelihood = 0.0_f64;
        let mut innovations = Vec::with_capacity(series.len().saturating_sub(1));

        for observation in &series[1..] {
            let predicted_level = level + trend;
            let predicted_trend = trend;

            let p00 = covariance[0][0]
                + covariance[0][1]
                + covariance[1][0]
                + covariance[1][1]
                + config.level_process_variance;
            let p01 = covariance[0][1] + covariance[1][1];
            let p10 = covariance[1][0] + covariance[1][1];
            let p11 = covariance[1][1] + config.trend_process_variance;

            let innovation = *observation - predicted_level;
            let innovation_variance = p00 + config.measurement_variance;
            if !innovation_variance.is_finite() || innovation_variance <= 0.0 {
                return Err(StateSpaceError::NumericalBreakdown);
            }
            let gain_level = p00 / innovation_variance;
            let gain_trend = p10 / innovation_variance;

            level = predicted_level + gain_level * innovation;
            trend = predicted_trend + gain_trend * innovation;

            let updated00 = (1.0 - gain_level) * p00;
            let updated01 = (1.0 - gain_level) * p01;
            let updated10 = p10 - gain_trend * p00;
            let updated11 = p11 - gain_trend * p01;
            let symmetric_off_diagonal = 0.5 * (updated01 + updated10);
            covariance = [
                [updated00.max(0.0), symmetric_off_diagonal],
                [symmetric_off_diagonal, updated11.max(0.0)],
            ];

            let contribution = -0.5
                * ((2.0 * core::f64::consts::PI).ln()
                    + innovation_variance.ln()
                    + innovation * innovation / innovation_variance);
            if !level.is_finite()
                || !trend.is_finite()
                || covariance
                    .iter()
                    .flatten()
                    .any(|value| !value.is_finite())
                || !contribution.is_finite()
            {
                return Err(StateSpaceError::NumericalBreakdown);
            }
            log_likelihood += contribution;
            innovations.push(innovation);
        }

        Ok(Self {
            config,
            level,
            trend,
            covariance,
            log_likelihood,
            innovations,
        })
    }

    pub fn forecast(
        &self,
        horizon: usize,
    ) -> Result<Vec<StateSpaceForecastPoint>, StateSpaceError> {
        let mut level = self.level;
        let mut trend = self.trend;
        let mut covariance = self.covariance;
        let mut output = Vec::with_capacity(horizon);

        for step in 1..=horizon {
            level += trend;
            let p00 = covariance[0][0]
                + covariance[0][1]
                + covariance[1][0]
                + covariance[1][1]
                + self.config.level_process_variance;
            let p01 = covariance[0][1] + covariance[1][1];
            let p10 = covariance[1][0] + covariance[1][1];
            let p11 = covariance[1][1] + self.config.trend_process_variance;
            covariance = [[p00, p01], [p10, p11]];
            let variance = p00 + self.config.measurement_variance;
            if !level.is_finite() || !trend.is_finite() || !variance.is_finite() || variance < 0.0 {
                return Err(StateSpaceError::NumericalBreakdown);
            }
            output.push(StateSpaceForecastPoint {
                horizon: step,
                mean: level,
                variance,
            });
        }
        Ok(output)
    }

    #[must_use]
    pub fn log_likelihood(&self) -> f64 {
        self.log_likelihood
    }

    #[must_use]
    pub fn final_level(&self) -> f64 {
        self.level
    }

    #[must_use]
    pub fn final_trend(&self) -> f64 {
        self.trend
    }

    #[must_use]
    pub fn innovations(&self) -> &[f64] {
        &self.innovations
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SarimaHoldoutScore {
    pub order: SeasonalArimaOrder,
    pub mae: f64,
    pub rmse: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SarimaSelectionReport {
    pub selected: SarimaHoldoutScore,
    pub scored_candidates: Vec<SarimaHoldoutScore>,
    pub holdout_len: usize,
}

/// Select among caller-supplied SARIMA-style orders using an untouched holdout.
///
/// This is model-family validation, not automatic statistical order discovery:
/// the candidate set remains explicit and auditable, and the underlying model
/// remains the additive-lag Hannan-Rissanen implementation documented in
/// `timeseries` rather than a full multiplicative maximum-likelihood SARIMA.
pub fn select_sarima_by_holdout(
    series: &[f64],
    holdout_len: usize,
    candidates: &[SeasonalArimaOrder],
) -> Result<SarimaSelectionReport, StateSpaceError> {
    validate_series(series)?;
    if holdout_len == 0 || series.len().saturating_sub(holdout_len) < 2 {
        return Err(StateSpaceError::InvalidHoldout);
    }
    if candidates.is_empty() {
        return Err(StateSpaceError::EmptyCandidateSet);
    }

    let split = series.len() - holdout_len;
    let train = &series[..split];
    let holdout = &series[split..];
    let mut scored = Vec::new();
    for order in candidates {
        let Ok(model) = SeasonalArimaModel::fit(train, *order) else {
            continue;
        };
        let Ok(forecast) = model.forecast(holdout_len) else {
            continue;
        };
        if forecast.len() != holdout.len() || forecast.iter().any(|value| !value.is_finite()) {
            continue;
        }
        let mut absolute = 0.0_f64;
        let mut squared = 0.0_f64;
        for (prediction, actual) in forecast.iter().zip(holdout) {
            let error = prediction - actual;
            absolute += error.abs();
            squared += error * error;
        }
        let count = holdout.len() as f64;
        let mae = absolute / count;
        let rmse = (squared / count).sqrt();
        if mae.is_finite() && rmse.is_finite() {
            scored.push(SarimaHoldoutScore {
                order: *order,
                mae,
                rmse,
            });
        }
    }
    if scored.is_empty() {
        return Err(StateSpaceError::NoUsableCandidate);
    }
    scored.sort_by(compare_scores);
    Ok(SarimaSelectionReport {
        selected: scored[0].clone(),
        scored_candidates: scored,
        holdout_len,
    })
}

fn validate_series(series: &[f64]) -> Result<(), StateSpaceError> {
    if series.len() < 2 {
        return Err(StateSpaceError::SeriesTooShort);
    }
    if series.iter().any(|value| !value.is_finite()) {
        return Err(StateSpaceError::NonFiniteInput);
    }
    Ok(())
}

fn validate_config(config: LocalLinearTrendConfig) -> Result<(), StateSpaceError> {
    if !config.level_process_variance.is_finite()
        || !config.trend_process_variance.is_finite()
        || !config.measurement_variance.is_finite()
        || config.level_process_variance < 0.0
        || config.trend_process_variance < 0.0
        || config.measurement_variance <= 0.0
    {
        return Err(StateSpaceError::InvalidVariance);
    }
    if !config.initial_variance.is_finite() || config.initial_variance <= 0.0 {
        return Err(StateSpaceError::InvalidInitialVariance);
    }
    Ok(())
}

fn compare_scores(left: &SarimaHoldoutScore, right: &SarimaHoldoutScore) -> Ordering {
    left.mae
        .total_cmp(&right.mae)
        .then_with(|| left.rmse.total_cmp(&right.rmse))
        .then_with(|| order_key(left.order).cmp(&order_key(right.order)))
}

fn order_key(order: SeasonalArimaOrder) -> (usize, usize, usize, usize, usize, usize, usize) {
    (
        order.p,
        order.d,
        order.q,
        order.seasonal_p,
        order.seasonal_d,
        order.seasonal_q,
        order.season_length,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_linear_trend_forecast_preserves_trend_and_uncertainty() {
        let series = [10.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let model = LocalLinearTrendModel::fit(
            &series,
            LocalLinearTrendConfig {
                level_process_variance: 0.01,
                trend_process_variance: 0.001,
                measurement_variance: 0.05,
                initial_variance: 1.0,
            },
        )
        .expect("state-space fit");
        let forecast = model.forecast(3).expect("state-space forecast");
        assert_eq!(forecast.len(), 3);
        assert!(forecast[0].mean > 15.0);
        assert!(forecast[2].mean > forecast[0].mean);
        assert!(forecast.iter().all(|point| point.variance > 0.0));
        assert!(model.log_likelihood().is_finite());
    }

    #[test]
    fn sarima_holdout_selection_is_deterministic() {
        let series = [
            10.0, 20.0, 11.0, 21.0, 12.0, 22.0, 13.0, 23.0, 14.0, 24.0, 15.0, 25.0,
            16.0, 26.0, 17.0, 27.0, 18.0, 28.0, 19.0, 29.0,
        ];
        let candidates = [
            SeasonalArimaOrder {
                p: 1,
                d: 0,
                q: 0,
                seasonal_p: 0,
                seasonal_d: 0,
                seasonal_q: 0,
                season_length: 2,
            },
            SeasonalArimaOrder {
                p: 0,
                d: 0,
                q: 0,
                seasonal_p: 1,
                seasonal_d: 1,
                seasonal_q: 0,
                season_length: 2,
            },
        ];
        let first = select_sarima_by_holdout(&series, 4, &candidates).expect("selection");
        let second = select_sarima_by_holdout(&series, 4, &candidates).expect("selection");
        assert_eq!(first, second);
        assert!(first.selected.mae.is_finite());
        assert!(!first.scored_candidates.is_empty());
    }
}
