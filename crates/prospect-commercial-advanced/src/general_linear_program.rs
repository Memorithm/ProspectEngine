use crate::optimization::ConstraintRelation;
use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralLinearConstraint {
    pub coefficients: Vec<f64>,
    pub relation: ConstraintRelation,
    pub rhs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralLinearProgram {
    /// Objective coefficients for `max c^T x`.
    pub objective: Vec<f64>,
    /// Mixed `<=`, `>=`, or equality constraints. Decision variables remain
    /// non-negative in this module.
    pub constraints: Vec<GeneralLinearConstraint>,
    pub tolerance: f64,
    /// Shared pivot budget across phase I and phase II.
    pub maximum_iterations: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralLinearSolution {
    pub values: Vec<f64>,
    pub objective_value: f64,
    pub phase_one_artificial_sum: f64,
    pub iterations: u64,
    pub basis: Vec<usize>,
    pub maximum_primal_violation: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GeneralLinearError {
    EmptyObjective,
    ConstraintWidthMismatch,
    NonFiniteInput,
    InvalidTolerance,
    InvalidIterationLimit,
    Infeasible,
    Unbounded,
    IterationLimitExceeded { iterations: u64 },
    NumericalBreakdown,
}

impl fmt::Display for GeneralLinearError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyObjective => formatter.write_str("general LP objective must not be empty"),
            Self::ConstraintWidthMismatch => {
                formatter.write_str("general LP constraint width must match objective width")
            }
            Self::NonFiniteInput => formatter.write_str("general LP inputs must be finite"),
            Self::InvalidTolerance => {
                formatter.write_str("general LP tolerance must be finite and positive")
            }
            Self::InvalidIterationLimit => {
                formatter.write_str("general LP iteration limit must be non-zero")
            }
            Self::Infeasible => formatter.write_str("general LP is infeasible"),
            Self::Unbounded => formatter.write_str("general LP is unbounded"),
            Self::IterationLimitExceeded { iterations } => write!(
                formatter,
                "general LP pivot budget exceeded after {iterations} iterations"
            ),
            Self::NumericalBreakdown => {
                formatter.write_str("general LP encountered numerical breakdown")
            }
        }
    }
}

impl std::error::Error for GeneralLinearError {}

#[derive(Clone, Debug)]
struct NormalizedConstraint {
    coefficients: Vec<f64>,
    relation: ConstraintRelation,
    rhs: f64,
}

/// Solve `max c^T x` with `x >= 0` and mixed linear constraint relations by a
/// deterministic two-phase primal simplex.
///
/// Phase I introduces artificial variables only where a slack basis is not
/// immediately available, maximizes the negative artificial-variable sum, and
/// refuses to enter phase II unless that sum is numerically zero. Artificial
/// columns are then removed before the original objective is restored.
pub fn solve_general_two_phase_simplex(
    problem: &GeneralLinearProgram,
) -> Result<GeneralLinearSolution, GeneralLinearError> {
    validate(problem)?;
    let normalized = normalize_constraints(problem);
    let decision_width = problem.objective.len();
    let rows = normalized.len();

    let auxiliary_count: usize = normalized
        .iter()
        .map(|constraint| match constraint.relation {
            ConstraintRelation::LessOrEqual => 1,
            ConstraintRelation::GreaterOrEqual => 2,
            ConstraintRelation::Equal => 1,
        })
        .sum();
    let total_width = decision_width + auxiliary_count;
    let rhs_column = total_width;
    let mut tableau = vec![vec![0.0_f64; total_width + 1]; rows + 1];
    let mut basis = Vec::with_capacity(rows);
    let mut artificial = vec![false; total_width];
    let mut next_column = decision_width;

    for (row, constraint) in normalized.iter().enumerate() {
        tableau[row][..decision_width].copy_from_slice(&constraint.coefficients);
        match constraint.relation {
            ConstraintRelation::LessOrEqual => {
                tableau[row][next_column] = 1.0;
                basis.push(next_column);
                next_column += 1;
            }
            ConstraintRelation::GreaterOrEqual => {
                tableau[row][next_column] = -1.0;
                next_column += 1;
                tableau[row][next_column] = 1.0;
                artificial[next_column] = true;
                basis.push(next_column);
                next_column += 1;
            }
            ConstraintRelation::Equal => {
                tableau[row][next_column] = 1.0;
                artificial[next_column] = true;
                basis.push(next_column);
                next_column += 1;
            }
        }
        tableau[row][rhs_column] = constraint.rhs;
    }
    debug_assert_eq!(next_column, total_width);

    let mut phase_one_costs = vec![0.0_f64; total_width];
    for (column, is_artificial) in artificial.iter().copied().enumerate() {
        if is_artificial {
            phase_one_costs[column] = -1.0;
        }
    }
    initialize_objective_row(&mut tableau, &basis, &phase_one_costs, rows, rhs_column)?;

    let mut iterations = 0_u64;
    run_simplex(
        &mut tableau,
        &mut basis,
        rows,
        total_width,
        problem.tolerance,
        problem.maximum_iterations,
        &mut iterations,
    )?;

    let phase_one_objective = tableau[rows][rhs_column];
    if !phase_one_objective.is_finite() {
        return Err(GeneralLinearError::NumericalBreakdown);
    }
    let phase_one_artificial_sum = (-phase_one_objective).max(0.0);
    if phase_one_artificial_sum > problem.tolerance {
        return Err(GeneralLinearError::Infeasible);
    }

    let redundant_rows = remove_artificial_basics(
        &mut tableau,
        &mut basis,
        &artificial,
        rows,
        total_width,
        rhs_column,
        problem.tolerance,
    )?;

    let kept_columns: Vec<usize> = (0..total_width)
        .filter(|column| !artificial[*column])
        .collect();
    let mut old_to_new = vec![None; total_width];
    for (new_column, old_column) in kept_columns.iter().copied().enumerate() {
        old_to_new[old_column] = Some(new_column);
    }
    let kept_rows: Vec<usize> = (0..rows)
        .filter(|row| !redundant_rows[*row])
        .collect();
    let phase_two_rows = kept_rows.len();
    let phase_two_width = kept_columns.len();
    let phase_two_rhs = phase_two_width;
    let mut phase_two = vec![vec![0.0_f64; phase_two_width + 1]; phase_two_rows + 1];
    let mut phase_two_basis = Vec::with_capacity(phase_two_rows);

    for (new_row, old_row) in kept_rows.iter().copied().enumerate() {
        for (new_column, old_column) in kept_columns.iter().copied().enumerate() {
            phase_two[new_row][new_column] = tableau[old_row][old_column];
        }
        phase_two[new_row][phase_two_rhs] = tableau[old_row][rhs_column];
        let mapped_basis = old_to_new[basis[old_row]].ok_or(GeneralLinearError::NumericalBreakdown)?;
        phase_two_basis.push(mapped_basis);
    }

    let mut phase_two_costs = vec![0.0_f64; phase_two_width];
    for decision in 0..decision_width {
        let mapped = old_to_new[decision].expect("decision columns are never artificial");
        phase_two_costs[mapped] = problem.objective[decision];
    }
    initialize_objective_row(
        &mut phase_two,
        &phase_two_basis,
        &phase_two_costs,
        phase_two_rows,
        phase_two_rhs,
    )?;
    run_simplex(
        &mut phase_two,
        &mut phase_two_basis,
        phase_two_rows,
        phase_two_width,
        problem.tolerance,
        problem.maximum_iterations,
        &mut iterations,
    )?;

    let mut values = vec![0.0_f64; decision_width];
    for (row, basic_column) in phase_two_basis.iter().copied().enumerate() {
        let old_column = kept_columns[basic_column];
        if old_column < decision_width {
            let value = phase_two[row][phase_two_rhs];
            if !value.is_finite() || value < -problem.tolerance {
                return Err(GeneralLinearError::NumericalBreakdown);
            }
            values[old_column] = if value.abs() <= problem.tolerance {
                0.0
            } else {
                value
            };
        }
    }
    let objective_value = dot(&problem.objective, &values);
    if !objective_value.is_finite() {
        return Err(GeneralLinearError::NumericalBreakdown);
    }
    let maximum_primal_violation = maximum_violation(problem, &values);
    if !maximum_primal_violation.is_finite() {
        return Err(GeneralLinearError::NumericalBreakdown);
    }

    Ok(GeneralLinearSolution {
        values,
        objective_value,
        phase_one_artificial_sum,
        iterations,
        basis: phase_two_basis,
        maximum_primal_violation,
    })
}

fn validate(problem: &GeneralLinearProgram) -> Result<(), GeneralLinearError> {
    if problem.objective.is_empty() {
        return Err(GeneralLinearError::EmptyObjective);
    }
    if !problem.tolerance.is_finite() || problem.tolerance <= 0.0 {
        return Err(GeneralLinearError::InvalidTolerance);
    }
    if problem.maximum_iterations == 0 {
        return Err(GeneralLinearError::InvalidIterationLimit);
    }
    if problem.objective.iter().any(|value| !value.is_finite()) {
        return Err(GeneralLinearError::NonFiniteInput);
    }
    for constraint in &problem.constraints {
        if constraint.coefficients.len() != problem.objective.len() {
            return Err(GeneralLinearError::ConstraintWidthMismatch);
        }
        if !constraint.rhs.is_finite()
            || constraint.coefficients.iter().any(|value| !value.is_finite())
        {
            return Err(GeneralLinearError::NonFiniteInput);
        }
    }
    Ok(())
}

fn normalize_constraints(problem: &GeneralLinearProgram) -> Vec<NormalizedConstraint> {
    problem
        .constraints
        .iter()
        .map(|constraint| {
            if constraint.rhs < -problem.tolerance {
                NormalizedConstraint {
                    coefficients: constraint.coefficients.iter().map(|value| -*value).collect(),
                    relation: flip_relation(constraint.relation),
                    rhs: -constraint.rhs,
                }
            } else {
                NormalizedConstraint {
                    coefficients: constraint.coefficients.clone(),
                    relation: constraint.relation,
                    rhs: if constraint.rhs.abs() <= problem.tolerance {
                        0.0
                    } else {
                        constraint.rhs
                    },
                }
            }
        })
        .collect()
}

fn flip_relation(relation: ConstraintRelation) -> ConstraintRelation {
    match relation {
        ConstraintRelation::LessOrEqual => ConstraintRelation::GreaterOrEqual,
        ConstraintRelation::GreaterOrEqual => ConstraintRelation::LessOrEqual,
        ConstraintRelation::Equal => ConstraintRelation::Equal,
    }
}

fn initialize_objective_row(
    tableau: &mut [Vec<f64>],
    basis: &[usize],
    costs: &[f64],
    rows: usize,
    rhs_column: usize,
) -> Result<(), GeneralLinearError> {
    for (column, cost) in costs.iter().copied().enumerate() {
        tableau[rows][column] = -cost;
    }
    tableau[rows][rhs_column] = 0.0;
    for (row, basic_column) in basis.iter().copied().enumerate() {
        let basic_cost = costs[basic_column];
        if basic_cost == 0.0 {
            continue;
        }
        let row_snapshot = tableau[row].clone();
        for (target, source) in tableau[rows].iter_mut().zip(row_snapshot) {
            *target += basic_cost * source;
            if !target.is_finite() {
                return Err(GeneralLinearError::NumericalBreakdown);
            }
        }
    }
    Ok(())
}

fn run_simplex(
    tableau: &mut [Vec<f64>],
    basis: &mut [usize],
    rows: usize,
    width: usize,
    tolerance: f64,
    maximum_iterations: u64,
    iterations: &mut u64,
) -> Result<(), GeneralLinearError> {
    let rhs_column = width;
    loop {
        let Some(entering) = (0..width).find(|column| tableau[rows][*column] < -tolerance)
        else {
            return Ok(());
        };
        let leaving = choose_leaving_row(
            tableau,
            basis,
            rows,
            entering,
            rhs_column,
            tolerance,
        )?
        .ok_or(GeneralLinearError::Unbounded)?;
        if *iterations >= maximum_iterations {
            return Err(GeneralLinearError::IterationLimitExceeded {
                iterations: *iterations,
            });
        }
        pivot(tableau, leaving, entering, tolerance)?;
        basis[leaving] = entering;
        *iterations = iterations.saturating_add(1);
    }
}

fn choose_leaving_row(
    tableau: &[Vec<f64>],
    basis: &[usize],
    rows: usize,
    entering: usize,
    rhs_column: usize,
    tolerance: f64,
) -> Result<Option<usize>, GeneralLinearError> {
    let mut best: Option<(usize, f64, usize)> = None;
    for (row, values) in tableau.iter().take(rows).enumerate() {
        let coefficient = values[entering];
        if coefficient <= tolerance {
            continue;
        }
        let ratio = values[rhs_column] / coefficient;
        if !ratio.is_finite() {
            return Err(GeneralLinearError::NumericalBreakdown);
        }
        if ratio < -tolerance {
            continue;
        }
        match best {
            None => best = Some((row, ratio, basis[row])),
            Some((_, best_ratio, best_basic)) => {
                if ratio < best_ratio - tolerance
                    || ((ratio - best_ratio).abs() <= tolerance && basis[row] < best_basic)
                {
                    best = Some((row, ratio, basis[row]));
                }
            }
        }
    }
    Ok(best.map(|(row, _, _)| row))
}

fn pivot(
    tableau: &mut [Vec<f64>],
    pivot_row: usize,
    pivot_column: usize,
    tolerance: f64,
) -> Result<(), GeneralLinearError> {
    let pivot_value = tableau[pivot_row][pivot_column];
    if !pivot_value.is_finite() || pivot_value.abs() <= tolerance {
        return Err(GeneralLinearError::NumericalBreakdown);
    }
    for value in &mut tableau[pivot_row] {
        *value /= pivot_value;
        if !value.is_finite() {
            return Err(GeneralLinearError::NumericalBreakdown);
        }
        if value.abs() <= tolerance {
            *value = 0.0;
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
        for (value, pivot_entry) in values.iter_mut().zip(&pivot_snapshot) {
            *value -= factor * pivot_entry;
            if !value.is_finite() {
                return Err(GeneralLinearError::NumericalBreakdown);
            }
            if value.abs() <= tolerance {
                *value = 0.0;
            }
        }
    }
    Ok(())
}

fn remove_artificial_basics(
    tableau: &mut [Vec<f64>],
    basis: &mut [usize],
    artificial: &[bool],
    rows: usize,
    width: usize,
    rhs_column: usize,
    tolerance: f64,
) -> Result<Vec<bool>, GeneralLinearError> {
    let mut redundant = vec![false; rows];
    for row in 0..rows {
        if !artificial[basis[row]] {
            continue;
        }
        if tableau[row][rhs_column].abs() > tolerance {
            return Err(GeneralLinearError::Infeasible);
        }
        let candidate = (0..width)
            .filter(|column| !artificial[*column])
            .find(|column| tableau[row][*column].abs() > tolerance);
        if let Some(entering) = candidate {
            pivot(tableau, row, entering, tolerance)?;
            basis[row] = entering;
        } else {
            redundant[row] = true;
        }
    }
    Ok(redundant)
}

fn maximum_violation(problem: &GeneralLinearProgram, values: &[f64]) -> f64 {
    let nonnegative_violation = values
        .iter()
        .map(|value| (-*value).max(0.0))
        .fold(0.0_f64, f64::max);
    problem.constraints.iter().fold(nonnegative_violation, |current, constraint| {
        let lhs = dot(&constraint.coefficients, values);
        let violation = match constraint.relation {
            ConstraintRelation::LessOrEqual => (lhs - constraint.rhs).max(0.0),
            ConstraintRelation::GreaterOrEqual => (constraint.rhs - lhs).max(0.0),
            ConstraintRelation::Equal => (lhs - constraint.rhs).abs(),
        };
        current.max(violation)
    })
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_phase_simplex_solves_mixed_relations() {
        let problem = GeneralLinearProgram {
            objective: vec![3.0, 2.0],
            constraints: vec![
                GeneralLinearConstraint {
                    coefficients: vec![1.0, 1.0],
                    relation: ConstraintRelation::GreaterOrEqual,
                    rhs: 2.0,
                },
                GeneralLinearConstraint {
                    coefficients: vec![1.0, 1.0],
                    relation: ConstraintRelation::LessOrEqual,
                    rhs: 4.0,
                },
                GeneralLinearConstraint {
                    coefficients: vec![1.0, 0.0],
                    relation: ConstraintRelation::LessOrEqual,
                    rhs: 3.0,
                },
                GeneralLinearConstraint {
                    coefficients: vec![0.0, 1.0],
                    relation: ConstraintRelation::Equal,
                    rhs: 1.0,
                },
            ],
            tolerance: 1e-10,
            maximum_iterations: 100,
        };
        let solution = solve_general_two_phase_simplex(&problem).expect("mixed LP");
        assert!((solution.values[0] - 3.0).abs() < 1e-8);
        assert!((solution.values[1] - 1.0).abs() < 1e-8);
        assert!((solution.objective_value - 11.0).abs() < 1e-8);
        assert!(solution.phase_one_artificial_sum <= 1e-8);
        assert!(solution.maximum_primal_violation <= 1e-8);
    }

    #[test]
    fn negative_rhs_is_normalized_before_phase_one() {
        let problem = GeneralLinearProgram {
            objective: vec![1.0],
            constraints: vec![
                GeneralLinearConstraint {
                    coefficients: vec![-1.0],
                    relation: ConstraintRelation::LessOrEqual,
                    rhs: -1.0,
                },
                GeneralLinearConstraint {
                    coefficients: vec![1.0],
                    relation: ConstraintRelation::LessOrEqual,
                    rhs: 3.0,
                },
            ],
            tolerance: 1e-10,
            maximum_iterations: 100,
        };
        let solution = solve_general_two_phase_simplex(&problem).expect("normalized LP");
        assert!((solution.values[0] - 3.0).abs() < 1e-8);
    }

    #[test]
    fn phase_one_detects_infeasibility() {
        let problem = GeneralLinearProgram {
            objective: vec![1.0],
            constraints: vec![
                GeneralLinearConstraint {
                    coefficients: vec![1.0],
                    relation: ConstraintRelation::GreaterOrEqual,
                    rhs: 2.0,
                },
                GeneralLinearConstraint {
                    coefficients: vec![1.0],
                    relation: ConstraintRelation::LessOrEqual,
                    rhs: 1.0,
                },
            ],
            tolerance: 1e-10,
            maximum_iterations: 100,
        };
        assert_eq!(
            solve_general_two_phase_simplex(&problem),
            Err(GeneralLinearError::Infeasible)
        );
    }

    #[test]
    fn phase_two_detects_unbounded_objective() {
        let problem = GeneralLinearProgram {
            objective: vec![1.0],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![1.0],
                relation: ConstraintRelation::GreaterOrEqual,
                rhs: 1.0,
            }],
            tolerance: 1e-10,
            maximum_iterations: 100,
        };
        assert_eq!(
            solve_general_two_phase_simplex(&problem),
            Err(GeneralLinearError::Unbounded)
        );
    }
}
