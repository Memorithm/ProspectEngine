use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum TimeSeriesError {
    EmptySeries,
    NonFiniteInput,
    InvalidOrder,
    SeriesTooShort { got: usize, need: usize },
    SingularSystem,
    InvalidSmoothing,
    InvalidSeasonLength,
}

impl fmt::Display for TimeSeriesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySeries => formatter.write_str("time series must not be empty"),
            Self::NonFiniteInput => formatter.write_str("time series inputs must be finite"),
            Self::InvalidOrder => formatter.write_str("time series order is invalid"),
            Self::SeriesTooShort { got, need } => {
                write!(formatter, "time series is too short: got {got}, need at least {need}")
            }
            Self::SingularSystem => formatter.write_str("time series regression system is singular"),
            Self::InvalidSmoothing => formatter.write_str("smoothing coefficients must be finite and in [0, 1]"),
            Self::InvalidSeasonLength => formatter.write_str("season length must be at least two"),
        }
    }
}

impl std::error::Error for TimeSeriesError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArimaOrder {
    pub p: usize,
    pub d: usize,
    pub q: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArimaModel {
    order: ArimaOrder,
    core: ArmaCore,
    regular_tails: Vec<f64>,
}

impl ArimaModel {
    pub fn fit(series: &[f64], order: ArimaOrder) -> Result<Self, TimeSeriesError> {
        validate_series(series)?;
        let mut transformed = series.to_vec();
        let mut regular_tails = Vec::with_capacity(order.d);
        for _ in 0..order.d {
            if transformed.len() < 2 {
                return Err(TimeSeriesError::SeriesTooShort {
                    got: series.len(),
                    need: series.len() + 1,
                });
            }
            regular_tails.push(*transformed.last().expect("validated non-empty series"));
            transformed = difference(&transformed, 1);
        }
        let ar_lags: Vec<usize> = (1..=order.p).collect();
        let ma_lags: Vec<usize> = (1..=order.q).collect();
        let core = fit_arma_hannan_rissanen(&transformed, ar_lags, ma_lags)?;
        Ok(Self {
            order,
            core,
            regular_tails,
        })
    }

    #[must_use]
    pub const fn order(&self) -> ArimaOrder {
        self.order
    }

    pub fn forecast(&self, horizon: usize) -> Result<Vec<f64>, TimeSeriesError> {
        let mut values = self.core.forecast(horizon)?;
        for tail in self.regular_tails.iter().rev() {
            let mut accumulator = *tail;
            for value in &mut values {
                accumulator += *value;
                *value = accumulator;
            }
        }
        Ok(values)
    }

    #[must_use]
    pub fn ar_coefficients(&self) -> &[f64] {
        &self.core.ar_coefficients
    }

    #[must_use]
    pub fn ma_coefficients(&self) -> &[f64] {
        &self.core.ma_coefficients
    }

    #[must_use]
    pub fn intercept(&self) -> f64 {
        self.core.intercept
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeasonalArimaOrder {
    pub p: usize,
    pub d: usize,
    pub q: usize,
    pub seasonal_p: usize,
    pub seasonal_d: usize,
    pub seasonal_q: usize,
    pub season_length: usize,
}

/// Seasonal ARIMA-style model with explicit non-seasonal and seasonal AR/MA lags.
///
/// This is an additive-lag Hannan-Rissanen fit, not a full multiplicative
/// maximum-likelihood SARIMA implementation. The distinction is deliberate and
/// remains visible in product evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct SeasonalArimaModel {
    order: SeasonalArimaOrder,
    core: ArmaCore,
    regular_tails: Vec<f64>,
    seasonal_tails: Vec<Vec<f64>>,
}

impl SeasonalArimaModel {
    pub fn fit(series: &[f64], order: SeasonalArimaOrder) -> Result<Self, TimeSeriesError> {
        validate_series(series)?;
        if order.season_length < 2 && (order.seasonal_p + order.seasonal_d + order.seasonal_q > 0) {
            return Err(TimeSeriesError::InvalidSeasonLength);
        }
        let mut transformed = series.to_vec();
        let mut seasonal_tails = Vec::with_capacity(order.seasonal_d);
        for _ in 0..order.seasonal_d {
            if transformed.len() <= order.season_length {
                return Err(TimeSeriesError::SeriesTooShort {
                    got: series.len(),
                    need: series.len() + order.season_length + 1 - transformed.len(),
                });
            }
            seasonal_tails.push(
                transformed[transformed.len() - order.season_length..]
                    .to_vec(),
            );
            transformed = difference(&transformed, order.season_length);
        }
        let mut regular_tails = Vec::with_capacity(order.d);
        for _ in 0..order.d {
            if transformed.len() < 2 {
                return Err(TimeSeriesError::SeriesTooShort {
                    got: series.len(),
                    need: series.len() + 1,
                });
            }
            regular_tails.push(*transformed.last().expect("validated non-empty transformed series"));
            transformed = difference(&transformed, 1);
        }

        let mut ar_lags: Vec<usize> = (1..=order.p).collect();
        ar_lags.extend((1..=order.seasonal_p).map(|lag| lag * order.season_length));
        ar_lags.sort_unstable();
        ar_lags.dedup();
        let mut ma_lags: Vec<usize> = (1..=order.q).collect();
        ma_lags.extend((1..=order.seasonal_q).map(|lag| lag * order.season_length));
        ma_lags.sort_unstable();
        ma_lags.dedup();
        let core = fit_arma_hannan_rissanen(&transformed, ar_lags, ma_lags)?;
        Ok(Self {
            order,
            core,
            regular_tails,
            seasonal_tails,
        })
    }

    pub fn forecast(&self, horizon: usize) -> Result<Vec<f64>, TimeSeriesError> {
        let mut values = self.core.forecast(horizon)?;
        for tail in self.regular_tails.iter().rev() {
            let mut accumulator = *tail;
            for value in &mut values {
                accumulator += *value;
                *value = accumulator;
            }
        }
        for tail in self.seasonal_tails.iter().rev() {
            values = undo_seasonal_difference(&values, tail)?;
        }
        Ok(values)
    }

    #[must_use]
    pub const fn order(&self) -> SeasonalArimaOrder {
        self.order
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ArmaCore {
    ar_lags: Vec<usize>,
    ma_lags: Vec<usize>,
    ar_coefficients: Vec<f64>,
    ma_coefficients: Vec<f64>,
    intercept: f64,
    history: Vec<f64>,
    residual_history: Vec<f64>,
}

impl ArmaCore {
    fn forecast(&self, horizon: usize) -> Result<Vec<f64>, TimeSeriesError> {
        let max_ar = self.ar_lags.iter().copied().max().unwrap_or(0);
        let max_ma = self.ma_lags.iter().copied().max().unwrap_or(0);
        let mut series_history = self.history.clone();
        let mut residual_history = self.residual_history.clone();
        let mut output = Vec::with_capacity(horizon);
        for _ in 0..horizon {
            let mut prediction = self.intercept;
            for (lag, coefficient) in self.ar_lags.iter().zip(&self.ar_coefficients) {
                let value = series_history[series_history.len() - *lag];
                prediction += coefficient * value;
            }
            for (lag, coefficient) in self.ma_lags.iter().zip(&self.ma_coefficients) {
                let residual = residual_history[residual_history.len() - *lag];
                prediction += coefficient * residual;
            }
            if !prediction.is_finite() {
                return Err(TimeSeriesError::NonFiniteInput);
            }
            output.push(prediction);
            if max_ar > 0 {
                series_history.push(prediction);
                if series_history.len() > max_ar {
                    series_history.remove(0);
                }
            }
            if max_ma > 0 {
                residual_history.push(0.0);
                if residual_history.len() > max_ma {
                    residual_history.remove(0);
                }
            }
        }
        Ok(output)
    }
}

fn fit_arma_hannan_rissanen(
    series: &[f64],
    ar_lags: Vec<usize>,
    ma_lags: Vec<usize>,
) -> Result<ArmaCore, TimeSeriesError> {
    if series.is_empty() {
        return Err(TimeSeriesError::EmptySeries);
    }
    let max_requested_lag = ar_lags
        .iter()
        .chain(&ma_lags)
        .copied()
        .max()
        .unwrap_or(0);
    if ar_lags.iter().any(|lag| *lag == 0) || ma_lags.iter().any(|lag| *lag == 0) {
        return Err(TimeSeriesError::InvalidOrder);
    }

    if ar_lags.is_empty() && ma_lags.is_empty() {
        let intercept = series.iter().sum::<f64>() / series.len() as f64;
        return Ok(ArmaCore {
            ar_lags,
            ma_lags,
            ar_coefficients: Vec::new(),
            ma_coefficients: Vec::new(),
            intercept,
            history: Vec::new(),
            residual_history: Vec::new(),
        });
    }

    let n = series.len();
    let adaptive = (n as f64).sqrt().ceil() as usize;
    let long_order = (ar_lags.len() + ma_lags.len() + 5)
        .max(adaptive)
        .min(n.saturating_sub(1) / 3)
        .max(1);
    let minimum = (long_order + max_requested_lag + ar_lags.len() + ma_lags.len() + 3)
        .max(max_requested_lag + 3);
    if n < minimum {
        return Err(TimeSeriesError::SeriesTooShort { got: n, need: minimum });
    }

    let long_lags: Vec<usize> = (1..=long_order).collect();
    let long_model = fit_lag_regression(series, &long_lags, None, long_order)?;
    let mut preliminary_residuals = vec![0.0; n];
    for time in long_order..n {
        let mut prediction = long_model.intercept;
        for (lag, coefficient) in long_lags.iter().zip(&long_model.coefficients) {
            prediction += coefficient * series[time - *lag];
        }
        preliminary_residuals[time] = series[time] - prediction;
    }

    let start = max_requested_lag.max(long_order + ma_lags.iter().copied().max().unwrap_or(0));
    let regression = fit_joint_regression(
        series,
        &preliminary_residuals,
        &ar_lags,
        &ma_lags,
        start,
    )?;
    let ar_count = ar_lags.len();
    let ar_coefficients = regression.coefficients[..ar_count].to_vec();
    let ma_coefficients = regression.coefficients[ar_count..].to_vec();

    let mut final_residuals = vec![0.0; n];
    for time in 0..n {
        let mut prediction = regression.intercept;
        for (lag, coefficient) in ar_lags.iter().zip(&ar_coefficients) {
            if time >= *lag {
                prediction += coefficient * series[time - *lag];
            }
        }
        for (lag, coefficient) in ma_lags.iter().zip(&ma_coefficients) {
            if time >= *lag {
                prediction += coefficient * final_residuals[time - *lag];
            }
        }
        final_residuals[time] = series[time] - prediction;
    }

    let max_ar = ar_lags.iter().copied().max().unwrap_or(0);
    let max_ma = ma_lags.iter().copied().max().unwrap_or(0);
    let history = if max_ar == 0 {
        Vec::new()
    } else {
        series[n - max_ar..].to_vec()
    };
    let residual_history = if max_ma == 0 {
        Vec::new()
    } else {
        final_residuals[n - max_ma..].to_vec()
    };

    Ok(ArmaCore {
        ar_lags,
        ma_lags,
        ar_coefficients,
        ma_coefficients,
        intercept: regression.intercept,
        history,
        residual_history,
    })
}

#[derive(Clone, Debug)]
struct RegressionFit {
    intercept: f64,
    coefficients: Vec<f64>,
}

fn fit_lag_regression(
    series: &[f64],
    lags: &[usize],
    external: Option<&[f64]>,
    start: usize,
) -> Result<RegressionFit, TimeSeriesError> {
    let width = 1 + lags.len();
    let rows = series.len().saturating_sub(start);
    if rows < width {
        return Err(TimeSeriesError::SeriesTooShort {
            got: series.len(),
            need: series.len() + width - rows,
        });
    }
    let mut ata = vec![vec![0.0; width]; width];
    let mut atb = vec![0.0; width];
    for time in start..series.len() {
        let mut row = vec![1.0; width];
        for (index, lag) in lags.iter().enumerate() {
            row[index + 1] = external.map_or(series[time - *lag], |values| values[time - *lag]);
        }
        accumulate_normal_equations(&mut ata, &mut atb, &row, series[time]);
    }
    let beta = solve_linear_system(ata, atb).ok_or(TimeSeriesError::SingularSystem)?;
    Ok(RegressionFit {
        intercept: beta[0],
        coefficients: beta[1..].to_vec(),
    })
}

fn fit_joint_regression(
    series: &[f64],
    residuals: &[f64],
    ar_lags: &[usize],
    ma_lags: &[usize],
    start: usize,
) -> Result<RegressionFit, TimeSeriesError> {
    let width = 1 + ar_lags.len() + ma_lags.len();
    let rows = series.len().saturating_sub(start);
    if rows < width {
        return Err(TimeSeriesError::SeriesTooShort {
            got: series.len(),
            need: series.len() + width - rows,
        });
    }
    let mut ata = vec![vec![0.0; width]; width];
    let mut atb = vec![0.0; width];
    for time in start..series.len() {
        let mut row = vec![1.0; width];
        for (index, lag) in ar_lags.iter().enumerate() {
            row[1 + index] = series[time - *lag];
        }
        for (index, lag) in ma_lags.iter().enumerate() {
            row[1 + ar_lags.len() + index] = residuals[time - *lag];
        }
        accumulate_normal_equations(&mut ata, &mut atb, &row, series[time]);
    }
    let beta = solve_linear_system(ata, atb).ok_or(TimeSeriesError::SingularSystem)?;
    Ok(RegressionFit {
        intercept: beta[0],
        coefficients: beta[1..].to_vec(),
    })
}

fn accumulate_normal_equations(ata: &mut [Vec<f64>], atb: &mut [f64], row: &[f64], target: f64) {
    for left in 0..row.len() {
        atb[left] += row[left] * target;
        for right in 0..row.len() {
            ata[left][right] += row[left] * row[right];
        }
    }
}

fn solve_linear_system(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Option<Vec<f64>> {
    let width = rhs.len();
    for column in 0..width {
        let mut pivot_row = column;
        let mut pivot_value = matrix[column][column].abs();
        for row in (column + 1)..width {
            let candidate = matrix[row][column].abs();
            if candidate > pivot_value {
                pivot_value = candidate;
                pivot_row = row;
            }
        }
        if !pivot_value.is_finite() || pivot_value < 1e-12 {
            return None;
        }
        matrix.swap(column, pivot_row);
        rhs.swap(column, pivot_row);
        let pivot = matrix[column][column];
        for row in (column + 1)..width {
            let factor = matrix[row][column] / pivot;
            if factor == 0.0 {
                continue;
            }
            for index in column..width {
                matrix[row][index] -= factor * matrix[column][index];
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    let mut solution = vec![0.0; width];
    for row in (0..width).rev() {
        let mut value = rhs[row];
        for column in (row + 1)..width {
            value -= matrix[row][column] * solution[column];
        }
        solution[row] = value / matrix[row][row];
        if !solution[row].is_finite() {
            return None;
        }
    }
    Some(solution)
}

fn difference(series: &[f64], lag: usize) -> Vec<f64> {
    (lag..series.len())
        .map(|index| series[index] - series[index - lag])
        .collect()
}

fn undo_seasonal_difference(values: &[f64], tail: &[f64]) -> Result<Vec<f64>, TimeSeriesError> {
    if tail.len() < 2 {
        return Err(TimeSeriesError::InvalidSeasonLength);
    }
    let season = tail.len();
    let mut history = tail.to_vec();
    let mut output = Vec::with_capacity(values.len());
    for value in values {
        let restored = *value + history[history.len() - season];
        history.push(restored);
        output.push(restored);
    }
    Ok(output)
}

fn validate_series(series: &[f64]) -> Result<(), TimeSeriesError> {
    if series.is_empty() {
        return Err(TimeSeriesError::EmptySeries);
    }
    if series.iter().any(|value| !value.is_finite()) {
        return Err(TimeSeriesError::NonFiniteInput);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoltWintersConfig {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub season_length: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HoltWintersAdditiveModel {
    config: HoltWintersConfig,
    level: f64,
    trend: f64,
    seasonal: Vec<f64>,
    next_season_index: usize,
}

impl HoltWintersAdditiveModel {
    pub fn fit(series: &[f64], config: HoltWintersConfig) -> Result<Self, TimeSeriesError> {
        validate_series(series)?;
        if config.season_length < 2 {
            return Err(TimeSeriesError::InvalidSeasonLength);
        }
        if series.len() < config.season_length * 2 {
            return Err(TimeSeriesError::SeriesTooShort {
                got: series.len(),
                need: config.season_length * 2,
            });
        }
        for coefficient in [config.alpha, config.beta, config.gamma] {
            if !coefficient.is_finite() || !(0.0..=1.0).contains(&coefficient) {
                return Err(TimeSeriesError::InvalidSmoothing);
            }
        }

        let season = config.season_length;
        let first_level = series[..season].iter().sum::<f64>() / season as f64;
        let second_level = series[season..season * 2].iter().sum::<f64>() / season as f64;
        let mut level = first_level;
        let mut trend = (second_level - first_level) / season as f64;
        let mut seasonal: Vec<f64> = series[..season]
            .iter()
            .map(|value| *value - first_level)
            .collect();

        for (index, observation) in series.iter().copied().enumerate() {
            let seasonal_index = index % season;
            let old_level = level;
            let old_season = seasonal[seasonal_index];
            level = config.alpha * (observation - old_season)
                + (1.0 - config.alpha) * (level + trend);
            trend = config.beta * (level - old_level) + (1.0 - config.beta) * trend;
            seasonal[seasonal_index] = config.gamma * (observation - level)
                + (1.0 - config.gamma) * old_season;
        }
        if !level.is_finite() || !trend.is_finite() || seasonal.iter().any(|value| !value.is_finite()) {
            return Err(TimeSeriesError::NonFiniteInput);
        }
        Ok(Self {
            config,
            level,
            trend,
            seasonal,
            next_season_index: series.len() % season,
        })
    }

    pub fn forecast(&self, horizon: usize) -> Result<Vec<f64>, TimeSeriesError> {
        (0..horizon)
            .map(|offset| {
                let seasonal = self.seasonal[(self.next_season_index + offset) % self.config.season_length];
                let step = (offset + 1) as f64;
                let value = self.level + step * self.trend + seasonal;
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(TimeSeriesError::NonFiniteInput)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arima_random_walk_with_drift_forecasts_original_scale() {
        let series: Vec<f64> = (0..80).map(|index| 10.0 + index as f64 * 2.0).collect();
        let model = ArimaModel::fit(&series, ArimaOrder { p: 0, d: 1, q: 0 })
            .expect("fit ARIMA drift model");
        let forecast = model.forecast(3).expect("forecast");
        assert!((forecast[0] - 170.0).abs() < 1e-8);
        assert!((forecast[2] - 174.0).abs() < 1e-8);
    }

    #[test]
    fn seasonal_arima_restores_seasonal_difference() {
        let pattern = [10.0, 20.0, 30.0, 40.0];
        let series: Vec<f64> = (0..20).map(|index| pattern[index % 4]).collect();
        let model = SeasonalArimaModel::fit(
            &series,
            SeasonalArimaOrder {
                p: 0,
                d: 0,
                q: 0,
                seasonal_p: 0,
                seasonal_d: 1,
                seasonal_q: 0,
                season_length: 4,
            },
        )
        .expect("fit seasonal differenced model");
        let forecast = model.forecast(4).expect("seasonal forecast");
        for (actual, expected) in forecast.iter().zip(pattern) {
            assert!((*actual - expected).abs() < 1e-8);
        }
    }

    #[test]
    fn holt_winters_additive_tracks_repeating_seasonality() {
        let pattern = [100.0, 120.0, 90.0, 110.0];
        let series: Vec<f64> = (0..12).map(|index| pattern[index % 4]).collect();
        let model = HoltWintersAdditiveModel::fit(
            &series,
            HoltWintersConfig {
                alpha: 0.4,
                beta: 0.1,
                gamma: 0.3,
                season_length: 4,
            },
        )
        .expect("fit additive ETS model");
        let forecast = model.forecast(4).expect("ETS forecast");
        assert_eq!(forecast.len(), 4);
        assert!(forecast.iter().all(|value| value.is_finite()));
        assert!(forecast[1] > forecast[2]);
    }
}
