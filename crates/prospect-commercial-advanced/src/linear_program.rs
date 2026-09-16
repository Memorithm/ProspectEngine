use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct LinearInequality {
    pub coefficients: Vec<f64>,
    pub rhs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalLinearProgram {
    /// Objective coefficients for `max c^T x`.
    pub objective: Vec<f64>,
    /// Constraints in canonical form `A x <= b`.
    pub constraints: Vec<LinearInequality>,
    /// Strictly positive numerical tolerance used for pivot tests.
    pub tolerance: f64,
    /// Maximum number of pivots before the solve fails closed.
    pub maximum_iterations: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinearProgramSolution {
    pub values: Vec<f64>,
    pub objective_value: f64,
    pub iterations: u64,
    pub basis: Vec<usize>,
    pub maximum_primal_violation: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinearProgramError {
    EmptyObjective,
    ConstraintWidthMismatch,
    NonFiniteInput,
    NegativeRightHandSide { constraint: usize, rhs: f64 },
    InvalidTolerance,
    InvalidIterationLimit,
    Unbounded,
    IterationLimitExceeded { iterations: u64 },
    NumericalBreakdown,
}

impl fmt::Display for LinearProgramError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyObjective => formatter.write_str("linear program objective must not be empty"),
            Self::ConstraintWidthMismatch => formatter.write_str(
                "linear program constraint width must match objective width",
            ),
            Self::NonFiniteInput => formatter.write_str("linear program inputs must be finite"),
            Self::NegativeRightHandSide { constraint, rhs } => write!(
                formatter,
                "canonical simplex requires non-negative RHS; constraint {constraint} has {rhs}"
            ),
            Self::InvalidTolerance => {
                formatter.write_str("linear program tolerance must be finite and positive")
            }
            Self::InvalidIterationLimit => {
                formatter.write_str("linear program iteration limit must be non-zero")
            }
            Self::Unbounded => formatter.write_str("canonical linear program is unbounded"),
            Self::IterationLimitExceeded { iterations } => write!(
                formatter,
                "linear program pivot budget exceeded after {iterations} iterations"
            ),
            Self::NumericalBreakdown => {
                formatter.write_str("linear program encountered numerical breakdown")
            }
        }
    }
}

impl std::error::Error for LinearProgramError {}

/// Solve a canonical maximization LP with deterministic Bland-style pivoting.
///
/// The accepted form is deliberately narrow: `max c^T x` subject to
/// `A x <= b`, `x >= 0`, with `b >= 0`. This gives the primal simplex a valid
/// slack basis without silently inventing a phase-I procedure. Callers that
/// need free variables, negative right-hand sides, equalities, or `>=`
/// constraints must transform and validate those semantics explicitly.
pub fn solve_canonical_simplex(
    problem: &CanonicalLinearProgram,
) -> Result<LinearProgramSolution, LinearProgramError> {
    validate(problem)?;

    let rows = problem.constraints.len();
    let decision_width = problem.objective.len();
    let total_width = decision_width + rows;
    let rhs_column = total_width;
    let mut tableau = vec![vec![0.0_f64; total_width + 1]; rows + 1];
    let mut basis = Vec::with_capacity(rows);

    for (row, constraint) in problem.constraints.iter().enumerate() {
        tableau[row][..decision_width].copy_from_slice(&constraint.coefficients);
        tableau[row][decision_width + row] = 1.0;
        tableau[row][rhs_column] = constraint.rhs;
        basis.push(decision_width + row);
    }
    for (column, coefficient) in problem.objective.iter().enumerate() {
        tableau[rows][column] = -*coefficient;
    }

    let mut iterations = 0_u64;
    loop {
        let Some(entering) = (0..total_width)
            .find(|column| tableau[rows][*column] < -problem.tolerance)
        else {
            break;
        };

        let leaving = choose_leaving_row(
            &tableau,
            rows,
            entering,
            rhs_column,
            problem.tolerance,
        )?
        .ok_or(LinearProgramError::Unbounded)?;

        if iterations >= problem.maximum_iterations {
            return Err(LinearProgramError::IterationLimitExceeded { iterations });
        }
        pivot(&mut tableau, leaving, entering, problem.tolerance)?;
        basis[leaving] = entering;
        iterations = iterations.saturating_add(1);
    }

    let mut values = vec![0.0_f64; decision_width];
    for (row, basic_column) in basis.iter().copied().enumerate() {
        if basic_column < decision_width {
            let value = tableau[row][rhs_column];
            if !value.is_finite() {
                return Err(LinearProgramError::NumericalBreakdown);
            }
            values[basic_column] = if value.abs() <= problem.tolerance {
                0.0
            } else {
                value
            };
        }
    }

    let objective_value = dot(&problem.objective, &values);
    if !objective_value.is_finite() {
        return Err(LinearProgramError::NumericalBreakdown);
    }
    let maximum_primal_violation = problem
        .constraints
        .iter()
        .map(|constraint| {
            let lhs = dot(&constraint.coefficients, &values);
            (lhs - constraint.rhs).max(0.0)
        })
        .fold(0.0_f64, f64::max);

    Ok(LinearProgramSolution {
        values,
        objective_value,
        iterations,
        basis,
        maximum_primal_violation,
    })
}

fn validate(problem: &CanonicalLinearProgram) -> Result<(), LinearProgramError> {
    if problem.objective.is_empty() {
        return Err(LinearProgramError::EmptyObjective);
    }
    if !problem.tolerance.is_finite() || problem.tolerance <= 0.0 {
        return Err(LinearProgramError::InvalidTolerance);
    }
    if problem.maximum_iterations == 0 {
        return Err(LinearProgramError::InvalidIterationLimit);
    }
    if problem.objective.iter().any(|value| !value.is_finite()) {
        return Err(LinearProgramError::NonFiniteInput);
    }
    for (index, constraint) in problem.constraints.iter().enumerate() {
        if constraint.coefficients.len() != problem.objective.len() {
            return Err(LinearProgramError::ConstraintWidthMismatch);
        }
        if !constraint.rhs.is_finite()
            || constraint
                .coefficients
                .iter()
                .any(|value| !value.is_finite())
        {
            return Err(LinearProgramError::NonFiniteInput);
        }
        if constraint.rhs < -problem.tolerance {
            return Err(LinearProgramError::NegativeRightHandSide {
                constraint: index,
                rhs: constraint.rhs,
            });
        }
    }
    Ok(())
}

fn choose_leaving_row(
    tableau: &[Vec<f64>],
    rows: usize,
    entering: usize,
    rhs_column: usize,
    tolerance: f64,
) -> Result<Option<usize>, LinearProgramError> {
    let mut best: Option<(usize, f64)> = None;
    for (row, values) in tableau.iter().take(rows).enumerate() {
        let coefficient = values[entering];
        if coefficient <= tolerance {
            continue;
        }
        let ratio = values[rhs_column] / coefficient;
        if !ratio.is_finite() {
            return Err(LinearProgramError::NumericalBreakdown);
        }
        if ratio < -tolerance {
            continue;
        }
        match best {
            None => best = Some((row, ratio)),
            Some((best_row, best_ratio)) => {
                if ratio < best_ratio - tolerance
                    || ((ratio - best_ratio).abs() <= tolerance && row < best_row)
                {
                    best = Some((row, ratio));
                }
            }
        }
    }
    Ok(best.map(|(row, _)| row))
}

fn pivot(
    tableau: &mut [Vec<f64>],
    pivot_row: usize,
    pivot_column: usize,
    tolerance: f64,
) -> Result<(), LinearProgramError> {
    let pivot = tableau[pivot_row][pivot_column];
    if !pivot.is_finite() || pivot.abs() <= tolerance {
        return Err(LinearProgramError::NumericalBreakdown);
    }
    for value in &mut tableau[pivot_row] {
        *value /= pivot;
        if !value.is_finite() {
            return Err(LinearProgramError::NumericalBreakdown);
        }
    }
    let pivot_snapshot = tableau[pivot_row].clone();
    for (row, values) in tableau.iter_mut().enumerate() {
        if row == pivot_row {
            continue;
        }
        let factor = values[pivot_column];
        if factor.abs() <= tolerance {
            values[pivot_column] = 0.0;
            continue;
        }
        for (value, pivot_value) in values.iter_mut().zip(&pivot_snapshot) {
            *value -= factor * pivot_value;
            if !value.is_finite() {
                return Err(LinearProgramError::NumericalBreakdown);
            }
            if value.abs() <= tolerance {
                *value = 0.0;
            }
        }
    }
    Ok(())
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplex_solves_capacity_relaxation() {
        let problem = CanonicalLinearProgram {
            objective: vec![3.0, 2.0],
            constraints: vec![
                LinearInequality {
                    coefficients: vec![1.0, 1.0],
                    rhs: 4.0,
                },
                LinearInequality {
                    coefficients: vec![1.0, 0.0],
                    rhs: 2.0,
                },
                LinearInequality {
                    coefficients: vec![0.0, 1.0],
                    rhs: 3.0,
                },
            ],
            tolerance: 1e-10,
            maximum_iterations: 100,
        };
        let solution = solve_canonical_simplex(&problem).expect("bounded LP");
        assert!((solution.values[0] - 2.0).abs() < 1e-8);
        assert!((solution.values[1] - 2.0).abs() < 1e-8);
        assert!((solution.objective_value - 10.0).abs() < 1e-8);
        assert!(solution.maximum_primal_violation <= 1e-8);
    }

    #[test]
    fn simplex_detects_unbounded_objective() {
        let problem = CanonicalLinearProgram {
            objective: vec![1.0],
            constraints: Vec::new(),
            tolerance: 1e-10,
            maximum_iterations: 10,
        };
        assert_eq!(
            solve_canonical_simplex(&problem),
            Err(LinearProgramError::Unbounded)
        );
    }

    #[test]
    fn negative_rhs_is_rejected_instead_of_inventing_phase_one() {
        let problem = CanonicalLinearProgram {
            objective: vec![1.0],
            constraints: vec![LinearInequality {
                coefficients: vec![-1.0],
                rhs: -2.0,
            }],
            tolerance: 1e-10,
            maximum_iterations: 10,
        };
        assert!(matches!(
            solve_canonical_simplex(&problem),
            Err(LinearProgramError::NegativeRightHandSide { .. })
        ));
    }
}
