use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum InstrumentDiagnosticError {
    LengthMismatch,
    TooFewObservations,
    NonFiniteInput,
    DegenerateInstrument,
    DegenerateTreatment,
    InvalidThreshold,
    NumericalBreakdown,
}

impl fmt::Display for InstrumentDiagnosticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthMismatch => {
                formatter.write_str("instrument and treatment lengths must match")
            }
            Self::TooFewObservations => {
                formatter.write_str("first-stage diagnostic requires at least three observations")
            }
            Self::NonFiniteInput => formatter.write_str("first-stage inputs must be finite"),
            Self::DegenerateInstrument => {
                formatter.write_str("instrument has zero empirical variation")
            }
            Self::DegenerateTreatment => {
                formatter.write_str("treatment has zero empirical variation")
            }
            Self::InvalidThreshold => {
                formatter.write_str("first-stage F threshold must be finite and non-negative")
            }
            Self::NumericalBreakdown => {
                formatter.write_str("first-stage regression encountered numerical breakdown")
            }
        }
    }
}

impl std::error::Error for InstrumentDiagnosticError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FirstStageDiagnostic {
    pub observations: usize,
    pub intercept: f64,
    pub instrument_slope: f64,
    pub treatment_mean: f64,
    pub instrument_mean: f64,
    pub r_squared: f64,
    /// `None` denotes an exact first-stage fit with zero residual variance,
    /// for which the usual finite F statistic diverges.
    pub f_statistic: Option<f64>,
    pub residual_sum_of_squares: f64,
    pub treatment_total_sum_of_squares: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FirstStageThresholdAssessment {
    pub threshold: f64,
    pub passes: bool,
    pub perfect_first_stage: bool,
}

/// Fit the single-instrument first stage `treatment = intercept + slope * z + e`.
///
/// The returned F statistic is the one-regressor first-stage statistic
/// `F = (R² / (1-R²)) * (n-2)`, equivalent to the squared slope t-statistic.
/// It diagnoses statistical relevance only. A large F does **not** establish
/// IV exogeneity, exclusion, monotonicity, or causal identification.
pub fn single_instrument_first_stage(
    instrument: &[f64],
    treatment: &[f64],
) -> Result<FirstStageDiagnostic, InstrumentDiagnosticError> {
    if instrument.len() != treatment.len() {
        return Err(InstrumentDiagnosticError::LengthMismatch);
    }
    if instrument.len() < 3 {
        return Err(InstrumentDiagnosticError::TooFewObservations);
    }
    if instrument
        .iter()
        .chain(treatment)
        .any(|value| !value.is_finite())
    {
        return Err(InstrumentDiagnosticError::NonFiniteInput);
    }

    let count = instrument.len() as f64;
    let instrument_mean = instrument.iter().sum::<f64>() / count;
    let treatment_mean = treatment.iter().sum::<f64>() / count;
    let mut instrument_ss = 0.0_f64;
    let mut treatment_ss = 0.0_f64;
    let mut cross = 0.0_f64;
    for (z, x) in instrument.iter().zip(treatment) {
        let centered_z = *z - instrument_mean;
        let centered_x = *x - treatment_mean;
        instrument_ss += centered_z * centered_z;
        treatment_ss += centered_x * centered_x;
        cross += centered_z * centered_x;
    }
    if !instrument_ss.is_finite() || instrument_ss <= 0.0 {
        return Err(InstrumentDiagnosticError::DegenerateInstrument);
    }
    if !treatment_ss.is_finite() || treatment_ss <= 0.0 {
        return Err(InstrumentDiagnosticError::DegenerateTreatment);
    }

    let instrument_slope = cross / instrument_ss;
    let intercept = treatment_mean - instrument_slope * instrument_mean;
    if !instrument_slope.is_finite() || !intercept.is_finite() {
        return Err(InstrumentDiagnosticError::NumericalBreakdown);
    }

    let residual_sum_of_squares = instrument
        .iter()
        .zip(treatment)
        .map(|(z, x)| {
            let residual = *x - (intercept + instrument_slope * *z);
            residual * residual
        })
        .sum::<f64>();
    if !residual_sum_of_squares.is_finite() {
        return Err(InstrumentDiagnosticError::NumericalBreakdown);
    }
    let mut r_squared = 1.0 - residual_sum_of_squares / treatment_ss;
    if r_squared < 0.0 && r_squared > -1e-12 {
        r_squared = 0.0;
    }
    if r_squared > 1.0 && r_squared < 1.0 + 1e-12 {
        r_squared = 1.0;
    }
    if !r_squared.is_finite() || !(0.0..=1.0).contains(&r_squared) {
        return Err(InstrumentDiagnosticError::NumericalBreakdown);
    }

    let residual_fraction = 1.0 - r_squared;
    let f_statistic = if residual_fraction <= 1e-14 {
        None
    } else {
        let statistic = r_squared / residual_fraction * (count - 2.0);
        if !statistic.is_finite() || statistic < 0.0 {
            return Err(InstrumentDiagnosticError::NumericalBreakdown);
        }
        Some(statistic)
    };

    Ok(FirstStageDiagnostic {
        observations: instrument.len(),
        intercept,
        instrument_slope,
        treatment_mean,
        instrument_mean,
        r_squared,
        f_statistic,
        residual_sum_of_squares,
        treatment_total_sum_of_squares: treatment_ss,
    })
}

/// Compare a first-stage diagnostic with a caller-supplied F threshold.
///
/// This deliberately does not hard-code the common `F >= 10` rule of thumb;
/// the threshold is part of the decision/evidence contract and should be
/// chosen for the design and inferential procedure in use.
pub fn assess_first_stage_threshold(
    diagnostic: FirstStageDiagnostic,
    threshold: f64,
) -> Result<FirstStageThresholdAssessment, InstrumentDiagnosticError> {
    if !threshold.is_finite() || threshold < 0.0 {
        return Err(InstrumentDiagnosticError::InvalidThreshold);
    }
    let perfect_first_stage = diagnostic.f_statistic.is_none();
    let passes = diagnostic
        .f_statistic
        .is_none_or(|statistic| statistic >= threshold);
    Ok(FirstStageThresholdAssessment {
        threshold,
        passes,
        perfect_first_stage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_first_stage_is_reported_without_fake_finite_f() {
        let instrument = [0.0, 1.0, 2.0, 3.0, 4.0];
        let treatment = [1.0, 3.0, 5.0, 7.0, 9.0];
        let diagnostic =
            single_instrument_first_stage(&instrument, &treatment).expect("exact first stage");
        assert!((diagnostic.instrument_slope - 2.0).abs() < 1e-12);
        assert!((diagnostic.intercept - 1.0).abs() < 1e-12);
        assert!((diagnostic.r_squared - 1.0).abs() < 1e-12);
        assert_eq!(diagnostic.f_statistic, None);
        let assessment = assess_first_stage_threshold(diagnostic, 10.0).expect("assessment");
        assert!(assessment.passes);
        assert!(assessment.perfect_first_stage);
    }

    #[test]
    fn noisy_first_stage_returns_finite_f_statistic() {
        let instrument = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let treatment = [0.2, 0.9, 2.4, 2.8, 4.5, 4.7];
        let diagnostic =
            single_instrument_first_stage(&instrument, &treatment).expect("noisy first stage");
        let statistic = diagnostic.f_statistic.expect("finite F");
        assert!(statistic > 0.0);
        assert!(diagnostic.r_squared > 0.0 && diagnostic.r_squared < 1.0);
    }

    #[test]
    fn degenerate_instrument_fails_closed() {
        let instrument = [1.0, 1.0, 1.0];
        let treatment = [0.0, 1.0, 2.0];
        assert_eq!(
            single_instrument_first_stage(&instrument, &treatment),
            Err(InstrumentDiagnosticError::DegenerateInstrument)
        );
    }
}
