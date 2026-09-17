use core::fmt;

const NUMERICAL_TOLERANCE: f64 = 1e-12;
const EXACT_FIT_TOLERANCE: f64 = 1e-14;

#[derive(Clone, Debug, PartialEq)]
pub struct InstrumentBlockDiagnostic {
    pub observations: usize,
    pub controls: usize,
    pub instruments: usize,
    pub restricted_residual_sum_of_squares: f64,
    pub full_residual_sum_of_squares: f64,
    pub partial_r_squared: f64,
    /// Joint homoskedastic F statistic for the instrument block. `None` means
    /// the full regression is numerically exact after a strictly positive SSE
    /// reduction, so the finite F ratio diverges.
    pub homoskedastic_f_statistic: Option<f64>,
    /// HC0 sandwich Wald statistic for the joint instrument block. `None`
    /// means the estimated robust covariance block is singular; no finite
    /// robust-Wald claim is then made.
    pub robust_wald_chi_square: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MultiInstrumentAndersonRubinDiagnostic {
    pub null_effect: f64,
    pub block: InstrumentBlockDiagnostic,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MultiInstrumentIvError {
    LengthMismatch,
    InstrumentRowMismatch,
    ControlRowMismatch,
    EmptyInstrumentBlock,
    TooFewObservations,
    NonFiniteInput,
    SingularDesign,
    NumericalBreakdown,
}

impl fmt::Display for MultiInstrumentIvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthMismatch => formatter.write_str("multi-IV row counts must match"),
            Self::InstrumentRowMismatch => {
                formatter.write_str("all multi-IV instrument rows must have the same width")
            }
            Self::ControlRowMismatch => {
                formatter.write_str("all multi-IV control rows must have the same width")
            }
            Self::EmptyInstrumentBlock => {
                formatter.write_str("multi-IV diagnostics require at least one instrument")
            }
            Self::TooFewObservations => formatter.write_str(
                "multi-IV diagnostics require more observations than full regression parameters",
            ),
            Self::NonFiniteInput => formatter.write_str("multi-IV inputs must be finite"),
            Self::SingularDesign => formatter.write_str("multi-IV regression design is singular"),
            Self::NumericalBreakdown => {
                formatter.write_str("multi-IV diagnostic encountered numerical breakdown")
            }
        }
    }
}

impl std::error::Error for MultiInstrumentIvError {}

#[derive(Clone, Debug)]
struct OlsFit {
    coefficients: Vec<f64>,
    residuals: Vec<f64>,
    sse: f64,
    gram_inverse: Vec<Vec<f64>>,
}

/// Diagnose the joint conditional relevance of multiple instruments for one
/// endogenous treatment.
///
/// Restricted model: `treatment ~ 1 + controls`.
/// Full model: `treatment ~ 1 + controls + instruments`.
///
/// The function reports the partial R² of the complete instrument block, the
/// ordinary homoskedastic block F statistic, and a heteroskedasticity-robust HC0
/// Wald statistic. No p-value or reference distribution is attached because
/// finite-sample interpretation depends on the caller's inferential design.
/// Relevance diagnostics do not establish instrument validity or causality.
pub fn multi_instrument_first_stage(
    instruments: &[Vec<f64>],
    treatment: &[f64],
    controls: &[Vec<f64>],
) -> Result<InstrumentBlockDiagnostic, MultiInstrumentIvError> {
    let dimensions = validate_design(instruments, treatment, controls)?;
    instrument_block_diagnostic(instruments, treatment, controls, dimensions)
}

/// Anderson-Rubin-style conditional instrument-block diagnostic for a supplied
/// null effect `beta0`.
///
/// The transformed outcome `y - beta0 * treatment` is regressed on controls
/// only and controls plus the complete instrument block. The same ordinary F
/// and HC0 robust Wald diagnostics are returned. This is evidence about the
/// reduced-form null relation under the supplied instrument design; it does not
/// establish exclusion, exogeneity, or causal identification.
pub fn multi_instrument_anderson_rubin(
    instruments: &[Vec<f64>],
    treatment: &[f64],
    outcome: &[f64],
    controls: &[Vec<f64>],
    null_effect: f64,
) -> Result<MultiInstrumentAndersonRubinDiagnostic, MultiInstrumentIvError> {
    if treatment.len() != outcome.len() {
        return Err(MultiInstrumentIvError::LengthMismatch);
    }
    if !null_effect.is_finite() || outcome.iter().any(|value| !value.is_finite()) {
        return Err(MultiInstrumentIvError::NonFiniteInput);
    }
    let dimensions = validate_design(instruments, treatment, controls)?;
    let transformed: Vec<f64> = outcome
        .iter()
        .zip(treatment)
        .map(|(y, treatment)| y - null_effect * treatment)
        .collect();
    if transformed.iter().any(|value| !value.is_finite()) {
        return Err(MultiInstrumentIvError::NumericalBreakdown);
    }
    let block = instrument_block_diagnostic(instruments, &transformed, controls, dimensions)?;
    Ok(MultiInstrumentAndersonRubinDiagnostic { null_effect, block })
}

fn instrument_block_diagnostic(
    instruments: &[Vec<f64>],
    response: &[f64],
    controls: &[Vec<f64>],
    dimensions: (usize, usize),
) -> Result<InstrumentBlockDiagnostic, MultiInstrumentIvError> {
    let (control_width, instrument_width) = dimensions;
    let restricted_design = design_matrix(controls, None);
    let full_design = design_matrix(controls, Some(instruments));
    let restricted = fit_ols(&restricted_design, response)?;
    let full = fit_ols(&full_design, response)?;

    let reduction = (restricted.sse - full.sse).max(0.0);
    let partial_r_squared = if restricted.sse <= EXACT_FIT_TOLERANCE {
        0.0
    } else {
        (reduction / restricted.sse).clamp(0.0, 1.0)
    };
    let full_parameters = 1 + control_width + instrument_width;
    let residual_degrees = response
        .len()
        .checked_sub(full_parameters)
        .ok_or(MultiInstrumentIvError::TooFewObservations)?;
    if residual_degrees == 0 {
        return Err(MultiInstrumentIvError::TooFewObservations);
    }
    let homoskedastic_f_statistic = if full.sse <= EXACT_FIT_TOLERANCE {
        if reduction <= EXACT_FIT_TOLERANCE {
            Some(0.0)
        } else {
            None
        }
    } else {
        let numerator = reduction / instrument_width as f64;
        let denominator = full.sse / residual_degrees as f64;
        let statistic = numerator / denominator;
        if !statistic.is_finite() || statistic < 0.0 {
            return Err(MultiInstrumentIvError::NumericalBreakdown);
        }
        Some(statistic)
    };

    let robust_covariance = hc0_covariance(&full_design, &full)?;
    let instrument_offset = 1 + control_width;
    let robust_wald_chi_square = block_wald(
        &full.coefficients[instrument_offset..instrument_offset + instrument_width],
        &robust_covariance,
        instrument_offset,
        instrument_width,
    )?;

    Ok(InstrumentBlockDiagnostic {
        observations: response.len(),
        controls: control_width,
        instruments: instrument_width,
        restricted_residual_sum_of_squares: restricted.sse,
        full_residual_sum_of_squares: full.sse,
        partial_r_squared,
        homoskedastic_f_statistic,
        robust_wald_chi_square,
    })
}

fn validate_design(
    instruments: &[Vec<f64>],
    response: &[f64],
    controls: &[Vec<f64>],
) -> Result<(usize, usize), MultiInstrumentIvError> {
    if instruments.len() != response.len() || controls.len() != response.len() {
        return Err(MultiInstrumentIvError::LengthMismatch);
    }
    let instrument_width = instruments.first().map_or(0, Vec::len);
    if instrument_width == 0 {
        return Err(MultiInstrumentIvError::EmptyInstrumentBlock);
    }
    if instruments.iter().any(|row| row.len() != instrument_width) {
        return Err(MultiInstrumentIvError::InstrumentRowMismatch);
    }
    let control_width = controls.first().map_or(0, Vec::len);
    if controls.iter().any(|row| row.len() != control_width) {
        return Err(MultiInstrumentIvError::ControlRowMismatch);
    }
    let full_parameters = 1 + control_width + instrument_width;
    if response.len() <= full_parameters {
        return Err(MultiInstrumentIvError::TooFewObservations);
    }
    if response.iter().any(|value| !value.is_finite())
        || instruments.iter().flatten().any(|value| !value.is_finite())
        || controls.iter().flatten().any(|value| !value.is_finite())
    {
        return Err(MultiInstrumentIvError::NonFiniteInput);
    }
    Ok((control_width, instrument_width))
}

fn design_matrix(controls: &[Vec<f64>], instruments: Option<&[Vec<f64>]>) -> Vec<Vec<f64>> {
    controls
        .iter()
        .enumerate()
        .map(|(row_index, controls_row)| {
            let instrument_width = instruments
                .and_then(|rows| rows.first())
                .map_or(0, Vec::len);
            let mut row = Vec::with_capacity(1 + controls_row.len() + instrument_width);
            row.push(1.0);
            row.extend_from_slice(controls_row);
            if let Some(instruments) = instruments {
                row.extend_from_slice(&instruments[row_index]);
            }
            row
        })
        .collect()
}

fn fit_ols(design: &[Vec<f64>], response: &[f64]) -> Result<OlsFit, MultiInstrumentIvError> {
    let width = design.first().map_or(0, Vec::len);
    if width == 0 {
        return Err(MultiInstrumentIvError::SingularDesign);
    }
    let mut gram = vec![vec![0.0_f64; width]; width];
    let mut rhs = vec![0.0_f64; width];
    for (row, y) in design.iter().zip(response) {
        for left in 0..width {
            rhs[left] += row[left] * y;
            for right in 0..width {
                gram[left][right] += row[left] * row[right];
            }
        }
    }
    let gram_inverse = invert_matrix(gram)?;
    let coefficients = matrix_vector_product(&gram_inverse, &rhs);
    if coefficients.iter().any(|value| !value.is_finite()) {
        return Err(MultiInstrumentIvError::NumericalBreakdown);
    }
    let residuals: Vec<f64> = design
        .iter()
        .zip(response)
        .map(|(row, y)| {
            let fitted = row
                .iter()
                .zip(&coefficients)
                .map(|(x, coefficient)| x * coefficient)
                .sum::<f64>();
            y - fitted
        })
        .collect();
    if residuals.iter().any(|value| !value.is_finite()) {
        return Err(MultiInstrumentIvError::NumericalBreakdown);
    }
    let sse = residuals.iter().map(|value| value * value).sum::<f64>();
    if !sse.is_finite() || sse < 0.0 {
        return Err(MultiInstrumentIvError::NumericalBreakdown);
    }
    Ok(OlsFit {
        coefficients,
        residuals,
        sse,
        gram_inverse,
    })
}

fn hc0_covariance(
    design: &[Vec<f64>],
    fit: &OlsFit,
) -> Result<Vec<Vec<f64>>, MultiInstrumentIvError> {
    let width = fit.coefficients.len();
    let mut meat = vec![vec![0.0_f64; width]; width];
    for (row, residual) in design.iter().zip(&fit.residuals) {
        let squared = residual * residual;
        for left in 0..width {
            for right in 0..width {
                meat[left][right] += squared * row[left] * row[right];
            }
        }
    }
    let left = matrix_product(&fit.gram_inverse, &meat)?;
    matrix_product(&left, &fit.gram_inverse)
}

fn block_wald(
    coefficients: &[f64],
    covariance: &[Vec<f64>],
    offset: usize,
    width: usize,
) -> Result<Option<f64>, MultiInstrumentIvError> {
    let block: Vec<Vec<f64>> = (0..width)
        .map(|row| {
            (0..width)
                .map(|column| covariance[offset + row][offset + column])
                .collect()
        })
        .collect();
    let inverse = match invert_matrix(block) {
        Ok(inverse) => inverse,
        Err(MultiInstrumentIvError::SingularDesign) => return Ok(None),
        Err(error) => return Err(error),
    };
    let weighted = matrix_vector_product(&inverse, coefficients);
    let statistic = coefficients
        .iter()
        .zip(weighted)
        .map(|(left, right)| left * right)
        .sum::<f64>();
    if !statistic.is_finite() || statistic < -NUMERICAL_TOLERANCE {
        return Err(MultiInstrumentIvError::NumericalBreakdown);
    }
    Ok(Some(statistic.max(0.0)))
}

fn invert_matrix(mut matrix: Vec<Vec<f64>>) -> Result<Vec<Vec<f64>>, MultiInstrumentIvError> {
    let width = matrix.len();
    if width == 0 || matrix.iter().any(|row| row.len() != width) {
        return Err(MultiInstrumentIvError::SingularDesign);
    }
    let mut inverse = vec![vec![0.0_f64; width]; width];
    for (index, row) in inverse.iter_mut().enumerate() {
        row[index] = 1.0;
    }
    for column in 0..width {
        let pivot_row = matrix
            .iter()
            .enumerate()
            .skip(column)
            .max_by(|left, right| left.1[column].abs().total_cmp(&right.1[column].abs()))
            .map(|(row, _)| row)
            .ok_or(MultiInstrumentIvError::SingularDesign)?;
        let pivot_value = matrix[pivot_row][column];
        if !pivot_value.is_finite() || pivot_value.abs() <= NUMERICAL_TOLERANCE {
            return Err(MultiInstrumentIvError::SingularDesign);
        }
        matrix.swap(column, pivot_row);
        inverse.swap(column, pivot_row);
        let pivot = matrix[column][column];
        for value in &mut matrix[column] {
            *value /= pivot;
        }
        for value in &mut inverse[column] {
            *value /= pivot;
        }
        let pivot_matrix_row = matrix[column].clone();
        let pivot_inverse_row = inverse[column].clone();
        for row in 0..width {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            for target in 0..width {
                matrix[row][target] -= factor * pivot_matrix_row[target];
                inverse[row][target] -= factor * pivot_inverse_row[target];
            }
        }
    }
    if inverse.iter().flatten().any(|value| !value.is_finite()) {
        return Err(MultiInstrumentIvError::NumericalBreakdown);
    }
    Ok(inverse)
}

fn matrix_vector_product(matrix: &[Vec<f64>], vector: &[f64]) -> Vec<f64> {
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

fn matrix_product(
    left: &[Vec<f64>],
    right: &[Vec<f64>],
) -> Result<Vec<Vec<f64>>, MultiInstrumentIvError> {
    let left_rows = left.len();
    let inner = left.first().map_or(0, Vec::len);
    let right_rows = right.len();
    let right_columns = right.first().map_or(0, Vec::len);
    if left_rows == 0
        || inner == 0
        || right_rows != inner
        || right.iter().any(|row| row.len() != right_columns)
        || left.iter().any(|row| row.len() != inner)
    {
        return Err(MultiInstrumentIvError::NumericalBreakdown);
    }
    let mut output = vec![vec![0.0_f64; right_columns]; left_rows];
    for row in 0..left_rows {
        for column in 0..right_columns {
            output[row][column] = (0..inner)
                .map(|index| left[row][index] * right[index][column])
                .sum();
            if !output[row][column].is_finite() {
                return Err(MultiInstrumentIvError::NumericalBreakdown);
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<Vec<f64>>, Vec<f64>, Vec<Vec<f64>>) {
        let instruments: Vec<Vec<f64>> = (0..16)
            .map(|index| vec![(index % 2) as f64, ((index / 2) % 2) as f64])
            .collect();
        let controls: Vec<Vec<f64>> = (0..16).map(|index| vec![index as f64]).collect();
        let treatment = (0..16)
            .map(|index| {
                let z1 = instruments[index][0];
                let z2 = instruments[index][1];
                let noise = match index % 3 {
                    0 => -0.04,
                    1 => 0.01,
                    _ => 0.03,
                };
                1.0 + 0.08 * index as f64 + 1.8 * z1 - 0.9 * z2 + noise
            })
            .collect();
        (instruments, treatment, controls)
    }

    #[test]
    fn multi_instrument_first_stage_reports_joint_relevance_and_robust_wald() {
        let (instruments, treatment, controls) = fixture();
        let diagnostic = multi_instrument_first_stage(&instruments, &treatment, &controls)
            .expect("multi-instrument first stage");
        assert_eq!(diagnostic.instruments, 2);
        assert!(diagnostic.partial_r_squared > 0.95);
        assert!(
            diagnostic
                .homoskedastic_f_statistic
                .is_some_and(|statistic| statistic > 1.0)
        );
        assert!(
            diagnostic
                .robust_wald_chi_square
                .is_some_and(|statistic| statistic > 1.0)
        );
    }

    #[test]
    fn zero_incremental_instrument_block_has_zero_ordinary_f() {
        let (instruments, _, controls) = fixture();
        let treatment: Vec<f64> = controls.iter().map(|row| 2.0 + 0.5 * row[0]).collect();
        let diagnostic = multi_instrument_first_stage(&instruments, &treatment, &controls)
            .expect("zero incremental relevance");
        assert!(diagnostic.partial_r_squared <= 1e-12);
        assert_eq!(diagnostic.homoskedastic_f_statistic, Some(0.0));
        assert_eq!(diagnostic.robust_wald_chi_square, None);
    }

    #[test]
    fn true_ar_null_has_less_instrument_signal_than_wrong_null() {
        let (instruments, treatment, controls) = fixture();
        let outcome: Vec<f64> = treatment
            .iter()
            .enumerate()
            .map(|(index, treatment)| {
                let noise = match index % 5 {
                    0 => -0.03,
                    1 => 0.02,
                    2 => 0.01,
                    3 => -0.01,
                    _ => 0.01,
                };
                4.0 + 0.2 * index as f64 + 3.0 * treatment + noise
            })
            .collect();
        let true_null =
            multi_instrument_anderson_rubin(&instruments, &treatment, &outcome, &controls, 3.0)
                .expect("true-null diagnostic");
        let wrong_null =
            multi_instrument_anderson_rubin(&instruments, &treatment, &outcome, &controls, 0.0)
                .expect("wrong-null diagnostic");
        assert!(true_null.block.partial_r_squared < wrong_null.block.partial_r_squared);
    }

    #[test]
    fn collinear_instrument_block_fails_closed() {
        let instruments: Vec<Vec<f64>> = (0..8)
            .map(|index| {
                let value = (index % 2) as f64;
                vec![value, value]
            })
            .collect();
        let controls: Vec<Vec<f64>> = (0..8).map(|index| vec![index as f64]).collect();
        let treatment: Vec<f64> = (0..8)
            .map(|index| index as f64 + instruments[index][0])
            .collect();
        assert_eq!(
            multi_instrument_first_stage(&instruments, &treatment, &controls),
            Err(MultiInstrumentIvError::SingularDesign)
        );
    }
}
