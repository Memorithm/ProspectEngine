use crate::integer_solver::BoundedIntegerProblem;
use crate::linear_program::{
    CanonicalLinearProgram, LinearInequality, LinearProgramError, solve_canonical_simplex,
};
use crate::optimization::ConstraintRelation;
use core::fmt;

const MAX_SAFE_F64_INTEGER: i128 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq)]
pub enum IntegerRelaxationError {
    EmptyProblem,
    InvalidBounds { variable: usize },
    ConstraintWidthMismatch,
    UnsupportedConstraintRelation,
    NegativeConstraintCoefficient,
    UnsafeIntegerMagnitude,
    InfeasibleAtLowerBounds,
    InvalidIntegralityTolerance,
    LinearProgram(LinearProgramError),
}

impl fmt::Display for IntegerRelaxationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProblem => formatter.write_str("integer relaxation problem is empty"),
            Self::InvalidBounds { variable } => {
                write!(formatter, "invalid bounds for integer variable {variable}")
            }
            Self::ConstraintWidthMismatch => {
                formatter.write_str("integer relaxation constraint width mismatch")
            }
            Self::UnsupportedConstraintRelation => {
                formatter.write_str("capacity relaxation currently accepts only <= constraints")
            }
            Self::NegativeConstraintCoefficient => formatter
                .write_str("capacity relaxation requires non-negative constraint coefficients"),
            Self::UnsafeIntegerMagnitude => formatter.write_str(
                "integer relaxation rejects magnitudes not exactly representable as f64 integers",
            ),
            Self::InfeasibleAtLowerBounds => formatter.write_str(
                "non-negative capacity constraint is already violated at variable lower bounds",
            ),
            Self::InvalidIntegralityTolerance => {
                formatter.write_str("integrality tolerance must be finite and strictly positive")
            }
            Self::LinearProgram(error) => write!(formatter, "LP relaxation failed: {error}"),
        }
    }
}

impl std::error::Error for IntegerRelaxationError {}

impl From<LinearProgramError> for IntegerRelaxationError {
    fn from(error: LinearProgramError) -> Self {
        Self::LinearProgram(error)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntegerRelaxationReport {
    pub values: Vec<f64>,
    /// Numerical LP objective. It is useful as a relaxation diagnostic, but it
    /// is not advertised as a formal exact-integer optimality certificate.
    pub numerical_objective_upper_bound: f64,
    pub fractional_variables: Vec<usize>,
    pub simplex_iterations: u64,
    pub maximum_primal_violation: f64,
}

/// Relax a bounded integer capacity model into canonical LP form.
///
/// Supported problems use only `<=` constraints with non-negative integer
/// coefficients. Variable lower bounds are shifted to zero and upper bounds
/// become explicit LP constraints. The restriction keeps the initial slack
/// basis feasible and makes the transformation auditable.
pub fn relax_bounded_integer_capacity(
    problem: &BoundedIntegerProblem,
    lp_tolerance: f64,
    integrality_tolerance: f64,
    maximum_iterations: u64,
) -> Result<IntegerRelaxationReport, IntegerRelaxationError> {
    validate(problem, integrality_tolerance)?;

    let mut objective = Vec::with_capacity(problem.variables.len());
    let mut constant_objective = 0.0_f64;
    for variable in &problem.variables {
        ensure_safe_i64(variable.lower)?;
        ensure_safe_i64(variable.upper)?;
        ensure_safe_i64(variable.objective_coefficient)?;
        objective.push(variable.objective_coefficient as f64);
        constant_objective += variable.objective_coefficient as f64 * variable.lower as f64;
    }
    if !constant_objective.is_finite() {
        return Err(IntegerRelaxationError::UnsafeIntegerMagnitude);
    }

    let mut constraints = Vec::with_capacity(problem.constraints.len() + problem.variables.len());
    for constraint in &problem.constraints {
        if constraint.relation != ConstraintRelation::LessOrEqual {
            return Err(IntegerRelaxationError::UnsupportedConstraintRelation);
        }
        if constraint
            .coefficients
            .iter()
            .any(|coefficient| *coefficient < 0)
        {
            return Err(IntegerRelaxationError::NegativeConstraintCoefficient);
        }
        let mut lower_contribution = 0_i128;
        let mut coefficients = Vec::with_capacity(problem.variables.len());
        for (coefficient, variable) in constraint.coefficients.iter().zip(&problem.variables) {
            ensure_safe_i64(*coefficient)?;
            lower_contribution += i128::from(*coefficient) * i128::from(variable.lower);
            ensure_safe_i128(lower_contribution)?;
            coefficients.push(*coefficient as f64);
        }
        ensure_safe_i64(constraint.rhs)?;
        let shifted_rhs = i128::from(constraint.rhs) - lower_contribution;
        ensure_safe_i128(shifted_rhs)?;
        if shifted_rhs < 0 {
            return Err(IntegerRelaxationError::InfeasibleAtLowerBounds);
        }
        constraints.push(LinearInequality {
            coefficients,
            rhs: shifted_rhs as f64,
        });
    }

    for (index, variable) in problem.variables.iter().enumerate() {
        let span = i128::from(variable.upper) - i128::from(variable.lower);
        ensure_safe_i128(span)?;
        let mut coefficients = vec![0.0_f64; problem.variables.len()];
        coefficients[index] = 1.0;
        constraints.push(LinearInequality {
            coefficients,
            rhs: span as f64,
        });
    }

    let solution = solve_canonical_simplex(&CanonicalLinearProgram {
        objective,
        constraints,
        tolerance: lp_tolerance,
        maximum_iterations,
    })?;

    let mut values = Vec::with_capacity(problem.variables.len());
    let mut fractional_variables = Vec::new();
    for (index, (relaxed, variable)) in solution.values.iter().zip(&problem.variables).enumerate() {
        let value = *relaxed + variable.lower as f64;
        if !value.is_finite() {
            return Err(IntegerRelaxationError::UnsafeIntegerMagnitude);
        }
        if (value - value.round()).abs() > integrality_tolerance {
            fractional_variables.push(index);
        }
        values.push(value);
    }

    let numerical_objective_upper_bound = constant_objective + solution.objective_value;
    if !numerical_objective_upper_bound.is_finite() {
        return Err(IntegerRelaxationError::UnsafeIntegerMagnitude);
    }
    Ok(IntegerRelaxationReport {
        values,
        numerical_objective_upper_bound,
        fractional_variables,
        simplex_iterations: solution.iterations,
        maximum_primal_violation: solution.maximum_primal_violation,
    })
}

fn validate(
    problem: &BoundedIntegerProblem,
    integrality_tolerance: f64,
) -> Result<(), IntegerRelaxationError> {
    if problem.variables.is_empty() {
        return Err(IntegerRelaxationError::EmptyProblem);
    }
    if !integrality_tolerance.is_finite() || integrality_tolerance <= 0.0 {
        return Err(IntegerRelaxationError::InvalidIntegralityTolerance);
    }
    for (index, variable) in problem.variables.iter().enumerate() {
        if variable.lower > variable.upper {
            return Err(IntegerRelaxationError::InvalidBounds { variable: index });
        }
    }
    if problem
        .constraints
        .iter()
        .any(|constraint| constraint.coefficients.len() != problem.variables.len())
    {
        return Err(IntegerRelaxationError::ConstraintWidthMismatch);
    }
    Ok(())
}

fn ensure_safe_i64(value: i64) -> Result<(), IntegerRelaxationError> {
    ensure_safe_i128(i128::from(value))
}

fn ensure_safe_i128(value: i128) -> Result<(), IntegerRelaxationError> {
    if value.abs() > MAX_SAFE_F64_INTEGER {
        return Err(IntegerRelaxationError::UnsafeIntegerMagnitude);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integer_solver::{IntegerLinearConstraint, IntegerVariable};

    #[test]
    fn capacity_relaxation_reports_fractional_variables() {
        let problem = BoundedIntegerProblem {
            variables: vec![
                IntegerVariable {
                    lower: 0,
                    upper: 4,
                    objective_coefficient: 5,
                },
                IntegerVariable {
                    lower: 0,
                    upper: 4,
                    objective_coefficient: 4,
                },
            ],
            constraints: vec![IntegerLinearConstraint {
                coefficients: vec![3, 2],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 7,
            }],
            maximum_nodes: 100,
        };
        let report = relax_bounded_integer_capacity(&problem, 1e-10, 1e-9, 100)
            .expect("capacity relaxation");
        assert!(report.numerical_objective_upper_bound >= 14.0 - 1e-8);
        assert!(!report.fractional_variables.is_empty());
        assert!(report.maximum_primal_violation <= 1e-8);
    }

    #[test]
    fn lower_bound_shift_is_reflected_in_solution() {
        let problem = BoundedIntegerProblem {
            variables: vec![IntegerVariable {
                lower: 2,
                upper: 5,
                objective_coefficient: 3,
            }],
            constraints: vec![IntegerLinearConstraint {
                coefficients: vec![1],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 4,
            }],
            maximum_nodes: 50,
        };
        let report =
            relax_bounded_integer_capacity(&problem, 1e-10, 1e-9, 50).expect("shifted relaxation");
        assert!((report.values[0] - 4.0).abs() < 1e-8);
        assert!((report.numerical_objective_upper_bound - 12.0).abs() < 1e-8);
    }
}
