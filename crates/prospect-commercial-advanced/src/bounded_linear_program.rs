use crate::general_linear_program::{
    solve_general_two_phase_simplex, GeneralLinearConstraint, GeneralLinearError,
    GeneralLinearProgram,
};
use crate::optimization::ConstraintRelation;
use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearVariable {
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub objective_coefficient: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoundedLinearProgram {
    pub variables: Vec<LinearVariable>,
    pub constraints: Vec<GeneralLinearConstraint>,
    pub tolerance: f64,
    pub maximum_iterations: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoundedLinearSolution {
    pub values: Vec<f64>,
    pub objective_value: f64,
    pub transformed_values: Vec<f64>,
    pub iterations: u64,
    pub maximum_primal_violation: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BoundedLinearError {
    EmptyProblem,
    InvalidBounds { variable: usize },
    ConstraintWidthMismatch,
    NonFiniteInput,
    InvalidTolerance,
    InvalidIterationLimit,
    General(GeneralLinearError),
    NumericalBreakdown,
}

impl fmt::Display for BoundedLinearError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProblem => formatter.write_str("bounded LP must contain variables"),
            Self::InvalidBounds { variable } => {
                write!(formatter, "bounded LP variable {variable} has invalid bounds")
            }
            Self::ConstraintWidthMismatch => {
                formatter.write_str("bounded LP constraint width must match variable width")
            }
            Self::NonFiniteInput => formatter.write_str("bounded LP inputs must be finite"),
            Self::InvalidTolerance => {
                formatter.write_str("bounded LP tolerance must be finite and positive")
            }
            Self::InvalidIterationLimit => {
                formatter.write_str("bounded LP iteration budget must be non-zero")
            }
            Self::General(error) => write!(formatter, "bounded LP transformed solve failed: {error}"),
            Self::NumericalBreakdown => {
                formatter.write_str("bounded LP reconstruction encountered numerical breakdown")
            }
        }
    }
}

impl std::error::Error for BoundedLinearError {}

impl From<GeneralLinearError> for BoundedLinearError {
    fn from(error: GeneralLinearError) -> Self {
        Self::General(error)
    }
}

#[derive(Clone, Debug)]
enum VariableTransform {
    LowerShift { lower: f64, column: usize },
    UpperShift { upper: f64, column: usize },
    FreeSplit { positive: usize, negative: usize },
}

/// Solve an LP with free, lower-bounded, upper-bounded, or doubly-bounded
/// variables by transforming it into the non-negative two-phase LP core.
///
/// Transformations are explicit:
/// - finite lower bound: `x = lower + y`, `y >= 0`;
/// - upper-only bound: `x = upper - y`, `y >= 0`;
/// - free variable: `x = y_plus - y_minus`;
/// - finite lower+upper: lower-shift plus `y <= upper-lower`.
///
/// The returned objective and constraints are checked again in original
/// coordinates, so the transform cannot silently change caller semantics.
pub fn solve_bounded_linear_program(
    problem: &BoundedLinearProgram,
) -> Result<BoundedLinearSolution, BoundedLinearError> {
    validate(problem)?;

    let mut transforms = Vec::with_capacity(problem.variables.len());
    let mut transformed_objective = Vec::new();
    let mut objective_constant = 0.0_f64;
    let mut upper_constraints: Vec<GeneralLinearConstraint> = Vec::new();

    for variable in &problem.variables {
        let coefficient = variable.objective_coefficient;
        match (variable.lower, variable.upper) {
            (Some(lower), upper) => {
                let column = transformed_objective.len();
                transformed_objective.push(coefficient);
                objective_constant += coefficient * lower;
                transforms.push(VariableTransform::LowerShift { lower, column });
                if let Some(upper) = upper {
                    let span = upper - lower;
                    let mut coefficients = vec![0.0; transformed_objective.len()];
                    coefficients[column] = 1.0;
                    upper_constraints.push(GeneralLinearConstraint {
                        coefficients,
                        relation: ConstraintRelation::LessOrEqual,
                        rhs: span,
                    });
                }
            }
            (None, Some(upper)) => {
                let column = transformed_objective.len();
                transformed_objective.push(-coefficient);
                objective_constant += coefficient * upper;
                transforms.push(VariableTransform::UpperShift { upper, column });
            }
            (None, None) => {
                let positive = transformed_objective.len();
                transformed_objective.push(coefficient);
                let negative = transformed_objective.len();
                transformed_objective.push(-coefficient);
                transforms.push(VariableTransform::FreeSplit { positive, negative });
            }
        }
    }

    // Upper-bound constraints were created while the transformed width was
    // still growing. Extend every row to the final width now.
    let transformed_width = transformed_objective.len();
    for constraint in &mut upper_constraints {
        constraint.coefficients.resize(transformed_width, 0.0);
    }

    let mut transformed_constraints = Vec::with_capacity(
        problem.constraints.len().saturating_add(upper_constraints.len()),
    );
    for constraint in &problem.constraints {
        let mut coefficients = vec![0.0_f64; transformed_width];
        let mut constant = 0.0_f64;
        for ((original_coefficient, variable), transform) in constraint
            .coefficients
            .iter()
            .zip(&problem.variables)
            .zip(&transforms)
        {
            match *transform {
                VariableTransform::LowerShift { lower, column } => {
                    coefficients[column] += *original_coefficient;
                    constant += *original_coefficient * lower;
                }
                VariableTransform::UpperShift { upper, column } => {
                    coefficients[column] -= *original_coefficient;
                    constant += *original_coefficient * upper;
                }
                VariableTransform::FreeSplit { positive, negative } => {
                    coefficients[positive] += *original_coefficient;
                    coefficients[negative] -= *original_coefficient;
                }
            }
            if !constant.is_finite() || !variable.objective_coefficient.is_finite() {
                return Err(BoundedLinearError::NumericalBreakdown);
            }
        }
        let rhs = constraint.rhs - constant;
        if !rhs.is_finite() || coefficients.iter().any(|value| !value.is_finite()) {
            return Err(BoundedLinearError::NumericalBreakdown);
        }
        transformed_constraints.push(GeneralLinearConstraint {
            coefficients,
            relation: constraint.relation,
            rhs,
        });
    }
    transformed_constraints.extend(upper_constraints);

    let transformed = solve_general_two_phase_simplex(&GeneralLinearProgram {
        objective: transformed_objective,
        constraints: transformed_constraints,
        tolerance: problem.tolerance,
        maximum_iterations: problem.maximum_iterations,
    })?;

    let mut values = Vec::with_capacity(problem.variables.len());
    for transform in &transforms {
        let value = match *transform {
            VariableTransform::LowerShift { lower, column } => lower + transformed.values[column],
            VariableTransform::UpperShift { upper, column } => upper - transformed.values[column],
            VariableTransform::FreeSplit { positive, negative } => {
                transformed.values[positive] - transformed.values[negative]
            }
        };
        if !value.is_finite() {
            return Err(BoundedLinearError::NumericalBreakdown);
        }
        values.push(value);
    }

    let objective_value = objective_constant
        + problem
            .variables
            .iter()
            .zip(&values)
            .map(|(variable, value)| {
                // Subtract the constant contribution already included above
                // only through reconstruction? No: recompute original-space
                // objective below instead, avoiding transform drift.
                variable.objective_coefficient * value
            })
            .sum::<f64>()
        - objective_constant;
    if !objective_value.is_finite() {
        return Err(BoundedLinearError::NumericalBreakdown);
    }
    let maximum_primal_violation = original_violation(problem, &values);
    if !maximum_primal_violation.is_finite() {
        return Err(BoundedLinearError::NumericalBreakdown);
    }

    Ok(BoundedLinearSolution {
        values,
        objective_value,
        transformed_values: transformed.values,
        iterations: transformed.iterations,
        maximum_primal_violation,
    })
}

fn validate(problem: &BoundedLinearProgram) -> Result<(), BoundedLinearError> {
    if problem.variables.is_empty() {
        return Err(BoundedLinearError::EmptyProblem);
    }
    if !problem.tolerance.is_finite() || problem.tolerance <= 0.0 {
        return Err(BoundedLinearError::InvalidTolerance);
    }
    if problem.maximum_iterations == 0 {
        return Err(BoundedLinearError::InvalidIterationLimit);
    }
    for (index, variable) in problem.variables.iter().enumerate() {
        if !variable.objective_coefficient.is_finite()
            || variable.lower.is_some_and(|value| !value.is_finite())
            || variable.upper.is_some_and(|value| !value.is_finite())
        {
            return Err(BoundedLinearError::NonFiniteInput);
        }
        if let (Some(lower), Some(upper)) = (variable.lower, variable.upper) {
            if lower > upper {
                return Err(BoundedLinearError::InvalidBounds { variable: index });
            }
        }
    }
    for constraint in &problem.constraints {
        if constraint.coefficients.len() != problem.variables.len() {
            return Err(BoundedLinearError::ConstraintWidthMismatch);
        }
        if !constraint.rhs.is_finite()
            || constraint.coefficients.iter().any(|value| !value.is_finite())
        {
            return Err(BoundedLinearError::NonFiniteInput);
        }
    }
    Ok(())
}

fn original_violation(problem: &BoundedLinearProgram, values: &[f64]) -> f64 {
    let mut maximum = 0.0_f64;
    for (value, variable) in values.iter().zip(&problem.variables) {
        if let Some(lower) = variable.lower {
            maximum = maximum.max((lower - *value).max(0.0));
        }
        if let Some(upper) = variable.upper {
            maximum = maximum.max((*value - upper).max(0.0));
        }
    }
    for constraint in &problem.constraints {
        let lhs = constraint
            .coefficients
            .iter()
            .zip(values)
            .map(|(coefficient, value)| coefficient * value)
            .sum::<f64>();
        let violation = match constraint.relation {
            ConstraintRelation::LessOrEqual => (lhs - constraint.rhs).max(0.0),
            ConstraintRelation::GreaterOrEqual => (constraint.rhs - lhs).max(0.0),
            ConstraintRelation::Equal => (lhs - constraint.rhs).abs(),
        };
        maximum = maximum.max(violation);
    }
    maximum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_frontend_handles_free_and_boxed_variables() {
        let problem = BoundedLinearProgram {
            variables: vec![
                LinearVariable {
                    lower: None,
                    upper: None,
                    objective_coefficient: 1.0,
                },
                LinearVariable {
                    lower: Some(-1.0),
                    upper: Some(3.0),
                    objective_coefficient: 2.0,
                },
            ],
            constraints: vec![
                GeneralLinearConstraint {
                    coefficients: vec![1.0, 1.0],
                    relation: ConstraintRelation::LessOrEqual,
                    rhs: 4.0,
                },
                GeneralLinearConstraint {
                    coefficients: vec![-1.0, 0.0],
                    relation: ConstraintRelation::LessOrEqual,
                    rhs: 2.0,
                },
            ],
            tolerance: 1e-10,
            maximum_iterations: 200,
        };
        let solution = solve_bounded_linear_program(&problem).expect("bounded LP");
        assert!((solution.values[0] - 1.0).abs() < 1e-8);
        assert!((solution.values[1] - 3.0).abs() < 1e-8);
        assert!((solution.objective_value - 7.0).abs() < 1e-8);
        assert!(solution.maximum_primal_violation <= 1e-8);
    }

    #[test]
    fn upper_only_variable_is_reconstructed() {
        let problem = BoundedLinearProgram {
            variables: vec![LinearVariable {
                lower: None,
                upper: Some(5.0),
                objective_coefficient: 1.0,
            }],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![1.0],
                relation: ConstraintRelation::GreaterOrEqual,
                rhs: 2.0,
            }],
            tolerance: 1e-10,
            maximum_iterations: 100,
        };
        let solution = solve_bounded_linear_program(&problem).expect("upper-only LP");
        assert!((solution.values[0] - 5.0).abs() < 1e-8);
    }
}
