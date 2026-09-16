use crate::timeseries::{HoltWintersAdditiveModel, HoltWintersConfig};
use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;

#[derive(Clone, Debug, PartialEq)]
pub enum AdvancedForecastError {
    SeriesTooShort,
    NonFiniteInput,
    NonPositiveInput,
    InvalidSmoothing,
    InvalidSeasonLength,
    InvalidHoldout,
    InvalidQuantile(u32),
    NoEligibleModel,
}

impl fmt::Display for AdvancedForecastError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SeriesTooShort => formatter.write_str("advanced forecast series is too short"),
            Self::NonFiniteInput => formatter.write_str("advanced forecast inputs must be finite"),
            Self::NonPositiveInput => {
                formatter.write_str("multiplicative forecast inputs must be strictly positive")
            }
            Self::InvalidSmoothing => {
                formatter.write_str("forecast smoothing parameters are invalid")
            }
            Self::InvalidSeasonLength => {
                formatter.write_str("forecast season length must be at least two")
            }
            Self::InvalidHoldout => {
                formatter.write_str("forecast holdout must leave a non-empty training sample")
            }
            Self::InvalidQuantile(value) => write!(
                formatter,
                "forecast quantile must be in 0..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::NoEligibleModel => {
                formatter.write_str("no forecast model is eligible for the supplied series")
            }
        }
    }
}

impl std::error::Error for AdvancedForecastError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DampedTrendConfig {
    pub alpha: f64,
    pub beta: f64,
    pub phi: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DampedTrendModel {
    config: DampedTrendConfig,
    level: f64,
    trend: f64,
}

impl DampedTrendModel {
    pub fn fit(series: &[f64], config: DampedTrendConfig) -> Result<Self, AdvancedForecastError> {
        validate_finite(series)?;
        if series.len() < 2 {
            return Err(AdvancedForecastError::SeriesTooShort);
        }
        if !valid_unit(config.alpha)
            || !valid_unit(config.beta)
            || !config.phi.is_finite()
            || !(0.0..=1.0).contains(&config.phi)
        {
            return Err(AdvancedForecastError::InvalidSmoothing);
        }
        let mut level = series[0];
        let mut trend = series[1] - series[0];
        for observation in &series[1..] {
            let previous_level = level;
            level =
                config.alpha * *observation + (1.0 - config.alpha) * (level + config.phi * trend);
            trend =
                config.beta * (level - previous_level) + (1.0 - config.beta) * config.phi * trend;
        }
        if !level.is_finite() || !trend.is_finite() {
            return Err(AdvancedForecastError::NonFiniteInput);
        }
        Ok(Self {
            config,
            level,
            trend,
        })
    }

    pub fn forecast(&self, horizon: usize) -> Result<Vec<f64>, AdvancedForecastError> {
        let mut damping_sum = 0.0;
        let mut power = self.config.phi;
        let mut output = Vec::with_capacity(horizon);
        for _ in 0..horizon {
            damping_sum += power;
            let value = self.level + damping_sum * self.trend;
            if !value.is_finite() {
                return Err(AdvancedForecastError::NonFiniteInput);
            }
            output.push(value);
            power *= self.config.phi;
        }
        Ok(output)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MultiplicativeHoltWintersConfig {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub phi: f64,
    pub season_length: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HoltWintersMultiplicativeModel {
    config: MultiplicativeHoltWintersConfig,
    level: f64,
    trend: f64,
    seasonal: Vec<f64>,
    next_season_index: usize,
}

impl HoltWintersMultiplicativeModel {
    pub fn fit(
        series: &[f64],
        config: MultiplicativeHoltWintersConfig,
    ) -> Result<Self, AdvancedForecastError> {
        validate_finite(series)?;
        if config.season_length < 2 {
            return Err(AdvancedForecastError::InvalidSeasonLength);
        }
        if series.len() < config.season_length * 2 {
            return Err(AdvancedForecastError::SeriesTooShort);
        }
        if series.iter().any(|value| *value <= 0.0) {
            return Err(AdvancedForecastError::NonPositiveInput);
        }
        if !valid_unit(config.alpha)
            || !valid_unit(config.beta)
            || !valid_unit(config.gamma)
            || !config.phi.is_finite()
            || !(0.0..=1.0).contains(&config.phi)
        {
            return Err(AdvancedForecastError::InvalidSmoothing);
        }

        let season = config.season_length;
        let first_average = series[..season].iter().sum::<f64>() / season as f64;
        let second_average = series[season..2 * season].iter().sum::<f64>() / season as f64;
        if first_average <= 0.0 || second_average <= 0.0 {
            return Err(AdvancedForecastError::NonPositiveInput);
        }
        let mut level = first_average;
        let mut trend = (second_average - first_average) / season as f64;
        let mut seasonal: Vec<f64> = series[..season]
            .iter()
            .map(|value| *value / first_average)
            .collect();

        for (index, observation) in series.iter().copied().enumerate() {
            let seasonal_index = index % season;
            let previous_level = level;
            let previous_seasonal = seasonal[seasonal_index];
            if previous_seasonal <= 0.0 {
                return Err(AdvancedForecastError::NonPositiveInput);
            }
            level = config.alpha * (observation / previous_seasonal)
                + (1.0 - config.alpha) * (level + config.phi * trend);
            trend =
                config.beta * (level - previous_level) + (1.0 - config.beta) * config.phi * trend;
            if level <= 0.0 {
                return Err(AdvancedForecastError::NonPositiveInput);
            }
            seasonal[seasonal_index] =
                config.gamma * (observation / level) + (1.0 - config.gamma) * previous_seasonal;
        }
        if !level.is_finite()
            || !trend.is_finite()
            || seasonal
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err(AdvancedForecastError::NonFiniteInput);
        }
        Ok(Self {
            config,
            level,
            trend,
            seasonal,
            next_season_index: series.len() % season,
        })
    }

    pub fn forecast(&self, horizon: usize) -> Result<Vec<f64>, AdvancedForecastError> {
        let mut damping_sum = 0.0;
        let mut power = self.config.phi;
        let mut output = Vec::with_capacity(horizon);
        for offset in 0..horizon {
            damping_sum += power;
            let seasonal =
                self.seasonal[(self.next_season_index + offset) % self.config.season_length];
            let value = (self.level + damping_sum * self.trend) * seasonal;
            if !value.is_finite() || value <= 0.0 {
                return Err(AdvancedForecastError::NonFiniteInput);
            }
            output.push(value);
            power *= self.config.phi;
        }
        Ok(output)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForecastFamily {
    DampedTrend,
    AdditiveSeasonal,
    MultiplicativeSeasonal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelSelectionReport {
    pub family: ForecastFamily,
    pub holdout_mae: f64,
    pub training_len: usize,
    pub holdout_len: usize,
}

pub fn select_forecast_family(
    series: &[f64],
    season_length: usize,
    holdout_len: usize,
) -> Result<ModelSelectionReport, AdvancedForecastError> {
    validate_finite(series)?;
    if holdout_len == 0 || holdout_len >= series.len() {
        return Err(AdvancedForecastError::InvalidHoldout);
    }
    let training_len = series.len() - holdout_len;
    let train = &series[..training_len];
    let holdout = &series[training_len..];
    if train.len() < 2 {
        return Err(AdvancedForecastError::SeriesTooShort);
    }

    let mut candidates = Vec::new();
    let damped = DampedTrendModel::fit(
        train,
        DampedTrendConfig {
            alpha: 0.35,
            beta: 0.15,
            phi: 0.9,
        },
    )?;
    candidates.push((ForecastFamily::DampedTrend, damped.forecast(holdout_len)?));

    if season_length >= 2 && train.len() >= season_length * 2 {
        if let Ok(additive) = HoltWintersAdditiveModel::fit(
            train,
            HoltWintersConfig {
                alpha: 0.35,
                beta: 0.1,
                gamma: 0.2,
                season_length,
            },
        ) && let Ok(forecast) = additive.forecast(holdout_len)
        {
            candidates.push((ForecastFamily::AdditiveSeasonal, forecast));
        }
        if train.iter().all(|value| *value > 0.0)
            && let Ok(multiplicative) = HoltWintersMultiplicativeModel::fit(
                train,
                MultiplicativeHoltWintersConfig {
                    alpha: 0.35,
                    beta: 0.1,
                    gamma: 0.2,
                    phi: 0.95,
                    season_length,
                },
            )
            && let Ok(forecast) = multiplicative.forecast(holdout_len)
        {
            candidates.push((ForecastFamily::MultiplicativeSeasonal, forecast));
        }
    }

    candidates
        .into_iter()
        .map(|(family, forecast)| {
            let mae = forecast
                .iter()
                .zip(holdout)
                .map(|(predicted, actual)| (predicted - actual).abs())
                .sum::<f64>()
                / holdout_len as f64;
            ModelSelectionReport {
                family,
                holdout_mae: mae,
                training_len,
                holdout_len,
            }
        })
        .filter(|report| report.holdout_mae.is_finite())
        .min_by(|left, right| {
            left.holdout_mae
                .total_cmp(&right.holdout_mae)
                .then_with(|| family_order(left.family).cmp(&family_order(right.family)))
        })
        .ok_or(AdvancedForecastError::NoEligibleModel)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForecastIntervalPoint {
    pub point: f64,
    pub lower: f64,
    pub upper: f64,
}

pub fn empirical_residual_interval(
    point_forecast: &[f64],
    residuals: &[f64],
    lower_quantile_ppm: u32,
    upper_quantile_ppm: u32,
) -> Result<Vec<ForecastIntervalPoint>, AdvancedForecastError> {
    if point_forecast
        .iter()
        .chain(residuals)
        .any(|value| !value.is_finite())
    {
        return Err(AdvancedForecastError::NonFiniteInput);
    }
    if residuals.is_empty() {
        return Err(AdvancedForecastError::SeriesTooShort);
    }
    if lower_quantile_ppm > PROBABILITY_SCALE_PPM {
        return Err(AdvancedForecastError::InvalidQuantile(lower_quantile_ppm));
    }
    if upper_quantile_ppm > PROBABILITY_SCALE_PPM || lower_quantile_ppm > upper_quantile_ppm {
        return Err(AdvancedForecastError::InvalidQuantile(upper_quantile_ppm));
    }
    let mut ordered = residuals.to_vec();
    ordered.sort_by(f64::total_cmp);
    let lower = quantile(&ordered, lower_quantile_ppm);
    let upper = quantile(&ordered, upper_quantile_ppm);
    point_forecast
        .iter()
        .map(|point| {
            let lower_bound = *point + lower;
            let upper_bound = *point + upper;
            if lower_bound.is_finite() && upper_bound.is_finite() {
                Ok(ForecastIntervalPoint {
                    point: *point,
                    lower: lower_bound,
                    upper: upper_bound,
                })
            } else {
                Err(AdvancedForecastError::NonFiniteInput)
            }
        })
        .collect()
}

fn quantile(sorted: &[f64], quantile_ppm: u32) -> f64 {
    if sorted.len() == 1 {
        return sorted[0];
    }
    let numerator =
        u128::from(quantile_ppm) * u128::try_from(sorted.len() - 1).expect("usize fits u128");
    let index = numerator / u128::from(PROBABILITY_SCALE_PPM);
    sorted[usize::try_from(index).expect("quantile index fits usize")]
}

fn family_order(family: ForecastFamily) -> u8 {
    match family {
        ForecastFamily::DampedTrend => 0,
        ForecastFamily::AdditiveSeasonal => 1,
        ForecastFamily::MultiplicativeSeasonal => 2,
    }
}

fn validate_finite(series: &[f64]) -> Result<(), AdvancedForecastError> {
    if series.is_empty() {
        return Err(AdvancedForecastError::SeriesTooShort);
    }
    if series.iter().any(|value| !value.is_finite()) {
        return Err(AdvancedForecastError::NonFiniteInput);
    }
    Ok(())
}

fn valid_unit(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damped_trend_has_finite_horizon_and_damping() {
        let series: Vec<f64> = (0..30).map(|index| 100.0 + index as f64 * 3.0).collect();
        let model = DampedTrendModel::fit(
            &series,
            DampedTrendConfig {
                alpha: 0.4,
                beta: 0.2,
                phi: 0.8,
            },
        )
        .expect("damped fit");
        let forecast = model.forecast(4).expect("damped forecast");
        assert!(forecast.windows(2).all(|pair| pair[1] > pair[0]));
        assert!((forecast[3] - forecast[2]) < (forecast[1] - forecast[0]));
    }

    #[test]
    fn multiplicative_holt_winters_preserves_relative_seasonality() {
        let pattern = [100.0, 150.0, 80.0, 120.0];
        let series: Vec<f64> = (0..20)
            .map(|index| pattern[index % 4] * (1.0 + index as f64 * 0.01))
            .collect();
        let model = HoltWintersMultiplicativeModel::fit(
            &series,
            MultiplicativeHoltWintersConfig {
                alpha: 0.4,
                beta: 0.1,
                gamma: 0.3,
                phi: 0.95,
                season_length: 4,
            },
        )
        .expect("multiplicative fit");
        let forecast = model.forecast(4).expect("multiplicative forecast");
        assert!(forecast[1] > forecast[3]);
        assert!(forecast[3] > forecast[0]);
        assert!(forecast[0] > forecast[2]);
    }

    #[test]
    fn automatic_selection_prefers_seasonal_model_for_repeated_pattern() {
        let pattern = [10.0, 30.0, 15.0, 25.0];
        let series: Vec<f64> = (0..28).map(|index| pattern[index % 4]).collect();
        let report = select_forecast_family(&series, 4, 4).expect("model selection");
        assert_ne!(report.family, ForecastFamily::DampedTrend);
        assert!(report.holdout_mae < 5.0);
    }

    #[test]
    fn empirical_intervals_use_observed_residual_quantiles() {
        let intervals = empirical_residual_interval(
            &[100.0, 110.0],
            &[-10.0, -5.0, 0.0, 5.0, 10.0],
            250_000,
            750_000,
        )
        .expect("empirical interval");
        assert_eq!(intervals[0].lower, 95.0);
        assert_eq!(intervals[0].upper, 105.0);
    }
}
