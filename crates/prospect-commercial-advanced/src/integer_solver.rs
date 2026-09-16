use crate::optimization::ConstraintRelation;
use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegerVariable {
    pub lower: i64,
    pub upper: i64,
    pub objective_coefficient: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegerLinearConstraint {
    pub coefficients: Vec<i64>,
    pub relation: ConstraintRelation,
    pub rhs: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedIntegerProblem {
    pub variables: Vec<IntegerVariable>,
    pub constraints: Vec<IntegerLinearConstraint>,
    pub maximum_nodes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedIntegerSolution {
    pub values: Vec<i64>,
    pub objective_value: i128,
    pub explored_nodes: u64,
    pub pruned_nodes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegerSolverError {
    EmptyProblem,
    InvalidBounds { variable: usize },
    ConstraintWidthMismatch,
    InvalidNodeBudget,
    NodeBudgetExceeded { explored_nodes: u64 },
    NoFeasibleSolution,
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for IntegerSolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProblem => formatter.write_str("bounded integer problem must not be empty"),
            Self::InvalidBounds { variable } => {
                write!(formatter, "invalid integer bounds for variable {variable}")
            }
            Self::ConstraintWidthMismatch => {
                formatter.write_str("integer constraint width must match variable width")
            }
            Self::InvalidNodeBudget => {
                formatter.write_str("integer solver node budget must be non-zero")
            }
            Self::NodeBudgetExceeded { explored_nodes } => write!(
                formatter,
                "integer solver node budget exceeded after {explored_nodes} nodes"
            ),
            Self::NoFeasibleSolution => {
                formatter.write_str("bounded integer problem has no feasible solution")
            }
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
        }
    }
}

impl std::error::Error for IntegerSolverError {}

pub fn solve_bounded_integer(
    problem: &BoundedIntegerProblem,
) -> Result<BoundedIntegerSolution, IntegerSolverError> {
    validate(problem)?;
    let order = variable_order(problem);
    let mut state = SearchState {
        problem,
        order,
        assignment: vec![None; problem.variables.len()],
        best_values: None,
        best_objective: i128::MIN,
        explored_nodes: 0,
        pruned_nodes: 0,
    };
    search(&mut state, 0, 0)?;
    let values = state
        .best_values
        .ok_or(IntegerSolverError::NoFeasibleSolution)?;
    Ok(BoundedIntegerSolution {
        values,
        objective_value: state.best_objective,
        explored_nodes: state.explored_nodes,
        pruned_nodes: state.pruned_nodes,
    })
}

fn validate(problem: &BoundedIntegerProblem) -> Result<(), IntegerSolverError> {
    if problem.variables.is_empty() {
        return Err(IntegerSolverError::EmptyProblem);
    }
    if problem.maximum_nodes == 0 {
        return Err(IntegerSolverError::InvalidNodeBudget);
    }
    for (index, variable) in problem.variables.iter().enumerate() {
        if variable.lower > variable.upper {
            return Err(IntegerSolverError::InvalidBounds { variable: index });
        }
    }
    if problem
        .constraints
        .iter()
        .any(|constraint| constraint.coefficients.len() != problem.variables.len())
    {
        return Err(IntegerSolverError::ConstraintWidthMismatch);
    }
    Ok(())
}

fn variable_order(problem: &BoundedIntegerProblem) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..problem.variables.len()).collect();
    indices.sort_by(|left, right| {
        let left_variable = problem.variables[*left];
        let right_variable = problem.variables[*right];
        let left_span = i128::from(left_variable.upper) - i128::from(left_variable.lower) + 1;
        let right_span = i128::from(right_variable.upper) - i128::from(right_variable.lower) + 1;
        let left_impact = i128::from(left_variable.objective_coefficient).abs() * left_span;
        let right_impact = i128::from(right_variable.objective_coefficient).abs() * right_span;
        right_impact
            .cmp(&left_impact)
            .then_with(|| left_span.cmp(&right_span))
            .then_with(|| left.cmp(right))
    });
    indices
}

struct SearchState<'a> {
    problem: &'a BoundedIntegerProblem,
    order: Vec<usize>,
    assignment: Vec<Option<i64>>,
    best_values: Option<Vec<i64>>,
    best_objective: i128,
    explored_nodes: u64,
    pruned_nodes: u64,
}

fn search(
    state: &mut SearchState<'_>,
    depth: usize,
    current_objective: i128,
) -> Result<(), IntegerSolverError> {
    state.explored_nodes = state.explored_nodes.saturating_add(1);
    if state.explored_nodes > state.problem.maximum_nodes {
        return Err(IntegerSolverError::NodeBudgetExceeded {
            explored_nodes: state.explored_nodes - 1,
        });
    }

    if !constraints_possible(state.problem, &state.assignment)? {
        state.pruned_nodes = state.pruned_nodes.saturating_add(1);
        return Ok(());
    }

    let optimistic = objective_upper_bound(state.problem, &state.assignment, current_objective)?;
    if state.best_values.is_some() && optimistic < state.best_objective {
        state.pruned_nodes = state.pruned_nodes.saturating_add(1);
        return Ok(());
    }

    if depth == state.order.len() {
        let values: Vec<i64> = state
            .assignment
            .iter()
            .map(|value| value.expect("complete assignment at leaf"))
            .collect();
        if current_objective > state.best_objective
            || (current_objective == state.best_objective
                && state
                    .best_values
                    .as_ref()
                    .is_none_or(|existing| values < *existing))
        {
            state.best_objective = current_objective;
            state.best_values = Some(values);
        }
        return Ok(());
    }

    let variable_index = state.order[depth];
    let variable = state.problem.variables[variable_index];
    let mut values: Vec<i64> = (variable.lower..=variable.upper).collect();
    if variable.objective_coefficient >= 0 {
        values.reverse();
    }
    for value in values {
        state.assignment[variable_index] = Some(value);
        let contribution = i128::from(variable.objective_coefficient)
            .checked_mul(i128::from(value))
            .ok_or(IntegerSolverError::ArithmeticOverflow(
                "integer objective term",
            ))?;
        let next_objective = current_objective
            .checked_add(contribution)
            .ok_or(IntegerSolverError::ArithmeticOverflow("integer objective"))?;
        search(state, depth + 1, next_objective)?;
        state.assignment[variable_index] = None;
    }
    Ok(())
}

fn objective_upper_bound(
    problem: &BoundedIntegerProblem,
    assignment: &[Option<i64>],
    current: i128,
) -> Result<i128, IntegerSolverError> {
    let mut upper = current;
    for (variable, value) in problem.variables.iter().zip(assignment) {
        if value.is_some() {
            continue;
        }
        let chosen = if variable.objective_coefficient >= 0 {
            variable.upper
        } else {
            variable.lower
        };
        let term = i128::from(variable.objective_coefficient)
            .checked_mul(i128::from(chosen))
            .ok_or(IntegerSolverError::ArithmeticOverflow(
                "integer objective bound",
            ))?;
        upper = upper
            .checked_add(term)
            .ok_or(IntegerSolverError::ArithmeticOverflow(
                "integer objective bound",
            ))?;
    }
    Ok(upper)
}

fn constraints_possible(
    problem: &BoundedIntegerProblem,
    assignment: &[Option<i64>],
) -> Result<bool, IntegerSolverError> {
    for constraint in &problem.constraints {
        let mut minimum = 0_i128;
        let mut maximum = 0_i128;
        for ((coefficient, variable), assigned) in constraint
            .coefficients
            .iter()
            .zip(&problem.variables)
            .zip(assignment)
        {
            let coefficient = i128::from(*coefficient);
            let (low, high) = if let Some(value) = assigned {
                (i128::from(*value), i128::from(*value))
            } else {
                (i128::from(variable.lower), i128::from(variable.upper))
            };
            let (term_minimum, term_maximum) = if coefficient >= 0 {
                (coefficient * low, coefficient * high)
            } else {
                (coefficient * high, coefficient * low)
            };
            minimum =
                minimum
                    .checked_add(term_minimum)
                    .ok_or(IntegerSolverError::ArithmeticOverflow(
                        "integer constraint minimum",
                    ))?;
            maximum =
                maximum
                    .checked_add(term_maximum)
                    .ok_or(IntegerSolverError::ArithmeticOverflow(
                        "integer constraint maximum",
                    ))?;
        }
        let rhs = i128::from(constraint.rhs);
        let possible = match constraint.relation {
            ConstraintRelation::LessOrEqual => minimum <= rhs,
            ConstraintRelation::GreaterOrEqual => maximum >= rhs,
            ConstraintRelation::Equal => minimum <= rhs && rhs <= maximum,
        };
        if !possible {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_integer_solver_finds_exact_mix() {
        let problem = BoundedIntegerProblem {
            variables: vec![
                IntegerVariable {
                    lower: 0,
                    upper: 4,
                    objective_coefficient: 7,
                },
                IntegerVariable {
                    lower: 0,
                    upper: 5,
                    objective_coefficient: 5,
                },
            ],
            constraints: vec![IntegerLinearConstraint {
                coefficients: vec![3, 2],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 10,
            }],
            maximum_nodes: 500,
        };
        let solution = solve_bounded_integer(&problem).expect("exact integer optimum");
        assert_eq!(solution.values, vec![0, 5]);
        assert_eq!(solution.objective_value, 25);
        assert!(solution.pruned_nodes > 0);
    }

    #[test]
    fn bounded_integer_solver_handles_negative_objective_coefficients() {
        let problem = BoundedIntegerProblem {
            variables: vec![IntegerVariable {
                lower: -3,
                upper: 3,
                objective_coefficient: -2,
            }],
            constraints: Vec::new(),
            maximum_nodes: 50,
        };
        let solution = solve_bounded_integer(&problem).expect("exact integer optimum");
        assert_eq!(solution.values, vec![-3]);
        assert_eq!(solution.objective_value, 6);
    }
}
