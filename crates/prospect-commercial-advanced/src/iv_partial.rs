use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartialFirstStageDiagnostic {
    pub observations: usize,
    pub controls: usize,
    pub restricted_residual_sum_of_squares: f64,
    pub full_residual_sum_of_squares: f64,
    pub partial_r_squared: f64,
    /// None denotes a full-model exact fit reached through a strictly positive
    /// incremental reduction in SSE, for which the finite F statistic diverges.
    pub partial_f_statistic: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AndersonRubinDiagnostic {
    pub null_effect: f64,
    pub observations: usize,
    pub controls: usize,
    /// F statistic for adding the instrument to the residualized null outcome
    /// `y - beta0 * treatment` after controlling for the supplied covariates.
    /// None denotes a divergent finite F statistic after a strictly positive
    /// incremental reduction drives the full-model SSE to numerical zero.
    pub f_statistic: Option<f64>,
    pub partial_r_squared: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PartialIvError {
    LengthMismatch,
    ControlRowMismatch,
    TooFewObservations,
    NonFiniteInput,
    SingularDesign,
    NumericalBreakdown,
}

impl fmt::Display for PartialIvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthMismatch => formatter.write_str("IV vectors must have matching lengths"),
            Self::ControlRowMismatch => {
                formatter.write_str("all IV control rows must have the same width")
            }
            Self::TooFewObservations => formatter.write_str(
                "IV partial diagnostic needs more observations than full regression parameters",
            ),
            Self::NonFiniteInput => {
                formatter.write_str("IV partial diagnostic inputs must be finite")
            }
            Self::SingularDesign => {
                formatter.write_str("IV partial diagnostic design matrix is singular")
            }
            Self::NumericalBreakdown => {
                formatter.write_str("IV partial diagnostic encountered numerical breakdown")
            }
        }
    }
}

impl std::error::Error for PartialIvError {}

/// Compute the one-instrument partial first-stage F statistic conditional on
/// caller-supplied controls.
///
/// Restricted model: `treatment ~ 1 + controls`.
/// Full model: `treatment ~ 1 + controls + instrument`.
///
/// The statistic measures conditional relevance only; it does not establish
/// exclusion, exogeneity, monotonicity, or valid causal identification.
pub fn partial_first_stage(
    instrument: &[f64],
    treatment: &[f64],
    controls: &[Vec<f64>],
) -> Result<PartialFirstStageDiagnostic, PartialIvError> {
    validate_rows(instrument, treatment, controls)?;
    let controls_width = controls.first().map_or(0, Vec::len);
    let restricted = design_matrix(controls, None);
    let full = design_matrix(controls, Some(instrument));
    let restricted_sse = regression_sse(&restricted, treatment)?;
    let full_sse = regression_sse(&full, treatment)?;
    let (partial_r_squared, partial_f_statistic) = partial_statistics(
        restricted_sse,
        full_sse,
        treatment.len(),
        controls_width + 2,
    )?;
    Ok(PartialFirstStageDiagnostic {
        observations: treatment.len(),
        controls: controls_width,
        restricted_residual_sum_of_squares: restricted_sse,
        full_residual_sum_of_squares: full_sse,
        partial_r_squared,
        partial_f_statistic,
    })
}

/// Compute an Anderson-Rubin-style one-instrument F diagnostic for a supplied
/// null treatment effect `beta0`.
///
/// The null outcome `y - beta0 * treatment` is regressed on controls only and
/// then controls plus the instrument. The returned statistic is robust to weak
/// first-stage approximation in the sense that it tests the null through the
/// instrument's reduced-form association, but this function deliberately does
/// not compute finite-sample critical values or p-values and still requires a
/// valid instrument design.
pub fn anderson_rubin_diagnostic(
    instrument: &[f64],
    treatment: &[f64],
    outcome: &[f64],
    controls: &[Vec<f64>],
    null_effect: f64,
) -> Result<AndersonRubinDiagnostic, PartialIvError> {
    if treatment.len() != outcome.len() || !null_effect.is_finite() {
        return Err(if !null_effect.is_finite() {
            PartialIvError::NonFiniteInput
        } else {
            PartialIvError::LengthMismatch
        });
    }
    validate_rows(instrument, treatment, controls)?;
    if outcome.iter().any(|value| !value.is_finite()) {
        return Err(PartialIvError::NonFiniteInput);
    }
    let null_outcome: Vec<f64> = outcome
        .iter()
        .zip(treatment)
        .map(|(y, x)| y - null_effect * x)
        .collect();
    if null_outcome.iter().any(|value| !value.is_finite()) {
        return Err(PartialIvError::NumericalBreakdown);
    }
    let controls_width = controls.first().map_or(0, Vec::len);
    let restricted_sse = regression_sse(&design_matrix(controls, None), &null_outcome)?;
    let full_sse = regression_sse(&design_matrix(controls, Some(instrument)), &null_outcome)?;
    let (partial_r_squared, f_statistic) =
        partial_statistics(restricted_sse, full_sse, outcome.len(), controls_width + 2)?;
    Ok(AndersonRubinDiagnostic {
        null_effect,
        observations: outcome.len(),
        controls: controls_width,
        f_statistic,
        partial_r_squared,
    })
}

fn validate_rows(
    instrument: &[f64],
    treatment: &[f64],
    controls: &[Vec<f64>],
) -> Result<(), PartialIvError> {
    if instrument.len() != treatment.len() || controls.len() != treatment.len() {
        return Err(PartialIvError::LengthMismatch);
    }
    let width = controls.first().map_or(0, Vec::len);
    if controls.iter().any(|row| row.len() != width) {
        return Err(PartialIvError::ControlRowMismatch);
    }
    let full_parameters = width + 2;
    if treatment.len() <= full_parameters {
        return Err(PartialIvError::TooFewObservations);
    }
    if instrument
        .iter()
        .chain(treatment)
        .any(|value| !value.is_finite())
        || controls.iter().flatten().any(|value| !value.is_finite())
    {
        return Err(PartialIvError::NonFiniteInput);
    }
    Ok(())
}

fn design_matrix(controls: &[Vec<f64>], instrument: Option<&[f64]>) -> Vec<Vec<f64>> {
    controls
        .iter()
        .enumerate()
        .map(|(row, control_values)| {
            let mut values =
                Vec::with_capacity(1 + control_values.len() + usize::from(instrument.is_some()));
            values.push(1.0);
            values.extend_from_slice(control_values);
            if let Some(instrument) = instrument {
                values.push(instrument[row]);
            }
            values
        })
        .collect()
}

fn regression_sse(design: &[Vec<f64>], outcome: &[f64]) -> Result<f64, PartialIvError> {
    let parameters = design.first().map_or(0, Vec::len);
    if parameters == 0 {
        return Err(PartialIvError::SingularDesign);
    }
    let mut gram = vec![vec![0.0_f64; parameters]; parameters];
    let mut rhs = vec![0.0_f64; parameters];
    for (row, y) in design.iter().zip(outcome) {
        for left in 0..parameters {
            rhs[left] += row[left] * y;
            for right in 0..parameters {
                gram[left][right] += row[left] * row[right];
            }
        }
    }
    let coefficients = solve_linear_system(gram, rhs)?;
    let sse = design
        .iter()
        .zip(outcome)
        .map(|(row, y)| {
            let prediction = row
                .iter()
                .zip(&coefficients)
                .map(|(value, coefficient)| value * coefficient)
                .sum::<f64>();
            let residual = y - prediction;
            residual * residual
        })
        .sum::<f64>();
    if !sse.is_finite() || sse < 0.0 {
        return Err(PartialIvError::NumericalBreakdown);
    }
    Ok(sse)
}

fn partial_statistics(
    restricted_sse: f64,
    full_sse: f64,
    observations: usize,
    full_parameters: usize,
) -> Result<(f64, Option<f64>), PartialIvError> {
    const EXACT_FIT_TOLERANCE: f64 = 1e-14;

    let reduction = (restricted_sse - full_sse).max(0.0);
    let partial_r_squared = if restricted_sse <= EXACT_FIT_TOLERANCE {
        0.0
    } else {
        (reduction / restricted_sse).clamp(0.0, 1.0)
    };
    let residual_degrees = observations
        .checked_sub(full_parameters)
        .ok_or(PartialIvError::TooFewObservations)?;
    if residual_degrees == 0 {
        return Err(PartialIvError::TooFewObservations);
    }
    let f_statistic = if full_sse <= EXACT_FIT_TOLERANCE {
        if reduction <= EXACT_FIT_TOLERANCE {
            Some(0.0)
        } else {
            None
        }
    } else {
        let statistic = reduction / (full_sse / residual_degrees as f64);
        if !statistic.is_finite() || statistic < 0.0 {
            return Err(PartialIvError::NumericalBreakdown);
        }
        Some(statistic)
    };
    Ok((partial_r_squared, f_statistic))
}

fn solve_linear_system(
    mut matrix: Vec<Vec<f64>>,
    mut rhs: Vec<f64>,
) -> Result<Vec<f64>, PartialIvError> {
    let width = rhs.len();
    for column in 0..width {
        let pivot_row = matrix
            .iter()
            .enumerate()
            .skip(column)
            .max_by(|left, right| left.1[column].abs().total_cmp(&right.1[column].abs()))
            .map(|(row, _)| row)
            .ok_or(PartialIvError::SingularDesign)?;
        let pivot = matrix[pivot_row][column];
        if !pivot.is_finite() || pivot.abs() <= 1e-12 {
            return Err(PartialIvError::SingularDesign);
        }
        matrix.swap(column, pivot_row);
        rhs.swap(column, pivot_row);
        let pivot = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= pivot;
        }
        rhs[column] /= pivot;
        let pivot_row_values = matrix[column].clone();
        let pivot_rhs = rhs[column];
        for row in 0..width {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            for target in column..width {
                matrix[row][target] -= factor * pivot_row_values[target];
            }
            rhs[row] -= factor * pivot_rhs;
        }
    }
    if rhs.iter().any(|value| !value.is_finite()) {
        return Err(PartialIvError::NumericalBreakdown);
    }
    Ok(rhs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_first_stage_detects_instrument_beyond_control() {
        let instrument = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
        let controls: Vec<Vec<f64>> = (0..8).map(|index| vec![index as f64]).collect();
        let treatment: Vec<f64> = controls
            .iter()
            .zip(instrument)
            .map(|(control, z)| 1.0 + 0.2 * control[0] + 2.0 * z)
            .collect();
        let diagnostic =
            partial_first_stage(&instrument, &treatment, &controls).expect("partial first stage");
        assert!(diagnostic.partial_r_squared > 0.99);
        assert_eq!(diagnostic.partial_f_statistic, None);
    }

    #[test]
    fn perfectly_explained_restricted_stage_has_zero_incremental_f() {
        let instrument = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
        let controls: Vec<Vec<f64>> = (0..8).map(|index| vec![index as f64]).collect();
        let treatment: Vec<f64> = controls
            .iter()
            .map(|control| 1.0 + 0.5 * control[0])
            .collect();
        let diagnostic =
            partial_first_stage(&instrument, &treatment, &controls).expect("partial first stage");
        assert!(diagnostic.partial_r_squared <= 1e-12);
        assert_eq!(diagnostic.partial_f_statistic, Some(0.0));
    }

    #[test]
    fn anderson_rubin_null_at_true_effect_removes_instrument_signal() {
        let instrument = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
        let controls: Vec<Vec<f64>> = (0..8).map(|index| vec![index as f64]).collect();
        let treatment: Vec<f64> = controls
            .iter()
            .zip(instrument)
            .map(|(control, z)| 1.0 + 0.1 * control[0] + 2.0 * z)
            .collect();
        let outcome: Vec<f64> = controls
            .iter()
            .zip(&treatment)
            .map(|(control, x)| 4.0 + 0.3 * control[0] + 3.0 * x)
            .collect();
        let diagnostic =
            anderson_rubin_diagnostic(&instrument, &treatment, &outcome, &controls, 3.0)
                .expect("Anderson-Rubin diagnostic");
        assert!(diagnostic.partial_r_squared < 1e-10);
        assert_eq!(diagnostic.f_statistic, Some(0.0));
    }

    #[test]
    fn singular_control_design_fails_closed() {
        let instrument = [0.0, 1.0, 0.0, 1.0, 0.0];
        let treatment = [0.0, 1.0, 0.2, 1.2, 0.4];
        let controls = vec![vec![1.0]; 5];
        assert_eq!(
            partial_first_stage(&instrument, &treatment, &controls),
            Err(PartialIvError::SingularDesign)
        );
    }
}
