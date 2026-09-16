use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum SensitivityError {
    InvalidRiskRatio,
    InvalidConfidenceLimit,
    InvalidConfoundingStrength,
    NonFiniteInput,
}

impl fmt::Display for SensitivityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRiskRatio => {
                formatter.write_str("risk ratio must be finite and strictly positive")
            }
            Self::InvalidConfidenceLimit => formatter.write_str(
                "confidence limit must be finite, positive, and interpreted relative to the null",
            ),
            Self::InvalidConfoundingStrength => formatter.write_str(
                "confounder-exposure and confounder-outcome risk ratios must be at least one",
            ),
            Self::NonFiniteInput => formatter.write_str("sensitivity calculation is non-finite"),
        }
    }
}

impl std::error::Error for SensitivityError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiskRatioSensitivity {
    pub observed_risk_ratio: f64,
    /// Confidence limit on the side closest to the null, when supplied.
    pub confidence_limit_toward_null: Option<f64>,
    pub point_e_value: f64,
    pub confidence_e_value: Option<f64>,
}

/// Compute the E-value for a positive risk ratio.
///
/// Protective ratios below one are inverted before applying the standard
/// formula, so the returned strength is always expressed on a >= 1 scale.
pub fn e_value(risk_ratio: f64) -> Result<f64, SensitivityError> {
    validate_rr(risk_ratio)?;
    let magnitude = if risk_ratio >= 1.0 {
        risk_ratio
    } else {
        1.0 / risk_ratio
    };
    if magnitude <= 1.0 {
        return Ok(1.0);
    }
    let value = magnitude + (magnitude * (magnitude - 1.0)).sqrt();
    value
        .is_finite()
        .then_some(value)
        .ok_or(SensitivityError::NonFiniteInput)
}

/// Produce a point and confidence-limit E-value certificate.
///
/// If the supplied confidence limit crosses or reaches the null (`RR = 1`),
/// the confidence E-value is exactly one. The function does not turn an
/// observational association into an identified causal effect; it only
/// quantifies robustness to a stylized unmeasured-confounding mechanism.
pub fn risk_ratio_sensitivity(
    observed_risk_ratio: f64,
    confidence_limit_toward_null: Option<f64>,
) -> Result<RiskRatioSensitivity, SensitivityError> {
    validate_rr(observed_risk_ratio)?;
    let point_e_value = e_value(observed_risk_ratio)?;
    let confidence_e_value = match confidence_limit_toward_null {
        None => None,
        Some(limit) => {
            validate_rr(limit).map_err(|_| SensitivityError::InvalidConfidenceLimit)?;
            let crosses_null = (observed_risk_ratio >= 1.0 && limit <= 1.0)
                || (observed_risk_ratio < 1.0 && limit >= 1.0);
            if crosses_null {
                Some(1.0)
            } else {
                Some(e_value(limit)?)
            }
        }
    };
    Ok(RiskRatioSensitivity {
        observed_risk_ratio,
        confidence_limit_toward_null,
        point_e_value,
        confidence_e_value,
    })
}

/// Maximum multiplicative bias factor under two supplied confounding links.
///
/// `confounder_outcome_rr` is the maximum risk-ratio association between the
/// unmeasured confounder and outcome within exposure strata. `confounder_exposure_rr`
/// is the maximum imbalance of that confounder across exposure groups.
pub fn confounding_bias_factor(
    confounder_outcome_rr: f64,
    confounder_exposure_rr: f64,
) -> Result<f64, SensitivityError> {
    if !confounder_outcome_rr.is_finite()
        || !confounder_exposure_rr.is_finite()
        || confounder_outcome_rr < 1.0
        || confounder_exposure_rr < 1.0
    {
        return Err(SensitivityError::InvalidConfoundingStrength);
    }
    let denominator = confounder_outcome_rr + confounder_exposure_rr - 1.0;
    let factor = confounder_outcome_rr * confounder_exposure_rr / denominator;
    if !factor.is_finite() || factor < 1.0 {
        return Err(SensitivityError::NonFiniteInput);
    }
    Ok(factor)
}

/// Move an observed risk ratio toward the null by a supplied bias factor.
///
/// The result is capped at the null rather than being allowed to reverse the
/// sign of the association. This is a sensitivity bound, not a corrected
/// causal estimate.
pub fn attenuate_risk_ratio_toward_null(
    observed_risk_ratio: f64,
    bias_factor: f64,
) -> Result<f64, SensitivityError> {
    validate_rr(observed_risk_ratio)?;
    if !bias_factor.is_finite() || bias_factor < 1.0 {
        return Err(SensitivityError::InvalidConfoundingStrength);
    }
    let adjusted = if observed_risk_ratio >= 1.0 {
        (observed_risk_ratio / bias_factor).max(1.0)
    } else {
        (observed_risk_ratio * bias_factor).min(1.0)
    };
    adjusted
        .is_finite()
        .then_some(adjusted)
        .ok_or(SensitivityError::NonFiniteInput)
}

fn validate_rr(value: f64) -> Result<(), SensitivityError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(SensitivityError::InvalidRiskRatio);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e_value_matches_standard_rr_two_example() {
        let value = e_value(2.0).expect("valid E-value");
        let expected = 2.0 + 2.0_f64.sqrt();
        assert!((value - expected).abs() < 1e-12);
    }

    #[test]
    fn confidence_limit_crossing_null_has_unit_e_value() {
        let report = risk_ratio_sensitivity(2.0, Some(0.95)).expect("sensitivity report");
        assert_eq!(report.confidence_e_value, Some(1.0));
    }

    #[test]
    fn bias_factor_and_attenuation_are_explicit() {
        let factor = confounding_bias_factor(3.0, 3.0).expect("bias factor");
        assert!((factor - 1.8).abs() < 1e-12);
        let adjusted = attenuate_risk_ratio_toward_null(2.0, factor).expect("attenuation");
        assert!((adjusted - (2.0 / 1.8)).abs() < 1e-12);
    }

    #[test]
    fn protective_association_uses_reciprocal_strength() {
        let harmful = e_value(2.0).expect("harmful E-value");
        let protective = e_value(0.5).expect("protective E-value");
        assert!((harmful - protective).abs() < 1e-12);
    }
}
