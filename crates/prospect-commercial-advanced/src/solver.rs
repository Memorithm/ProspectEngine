use crate::optimization::{BinaryLinearConstraint, ConstraintRelation};
use core::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolverError {
    EmptyProblem,
    ConstraintWidthMismatch,
    InvalidNodeBudget,
    NodeBudgetExceeded { explored_nodes: u64 },
    NoFeasibleSolution,
    ArithmeticOverflow(&'static str),
    EmptyDomain { variable: usize },
    InvalidVariableIndex(usize),
    InvalidConstraint,
}

impl fmt::Display for SolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProblem => formatter.write_str("solver problem must not be empty"),
            Self::ConstraintWidthMismatch => {
                formatter.write_str("constraint width must match variable width")
            }
            Self::InvalidNodeBudget => formatter.write_str("node budget must be non-zero"),
            Self::NodeBudgetExceeded { explored_nodes } => {
                write!(formatter, "solver node budget exceeded after {explored_nodes} nodes")
            }
            Self::NoFeasibleSolution => formatter.write_str("no feasible solution exists"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
            Self::EmptyDomain { variable } => write!(formatter, "variable {variable} has an empty domain"),
            Self::InvalidVariableIndex(index) => write!(formatter, "invalid variable index {index}"),
            Self::InvalidConstraint => formatter.write_str("invalid finite-domain constraint"),
        }
    }
}

impl std::error::Error for SolverError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchBoundProblem {
    pub objective: Vec<i64>,
    pub constraints: Vec<BinaryLinearConstraint>,
    pub maximum_nodes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchBoundSolution {
    pub assignment: Vec<bool>,
    pub objective_value: i128,
    pub explored_nodes: u64,
    pub pruned_nodes: u64,
}

pub fn solve_binary_branch_and_bound(
    problem: &BranchBoundProblem,
) -> Result<BranchBoundSolution, SolverError> {
    validate_branch_bound(problem)?;
    let mut state = BranchBoundState {
        problem,
        assignment: vec![None; problem.objective.len()],
        best_assignment: None,
        best_value: i128::MIN,
        explored_nodes: 0,
        pruned_nodes: 0,
    };
    branch_binary(&mut state, 0, 0)?;
    let assignment = state.best_assignment.ok_or(SolverError::NoFeasibleSolution)?;
    Ok(BranchBoundSolution {
        assignment,
        objective_value: state.best_value,
        explored_nodes: state.explored_nodes,
        pruned_nodes: state.pruned_nodes,
    })
}

fn validate_branch_bound(problem: &BranchBoundProblem) -> Result<(), SolverError> {
    if problem.objective.is_empty() {
        return Err(SolverError::EmptyProblem);
    }
    if problem.maximum_nodes == 0 {
        return Err(SolverError::InvalidNodeBudget);
    }
    if problem
        .constraints
        .iter()
        .any(|constraint| constraint.coefficients.len() != problem.objective.len())
    {
        return Err(SolverError::ConstraintWidthMismatch);
    }
    Ok(())
}

struct BranchBoundState<'a> {
    problem: &'a BranchBoundProblem,
    assignment: Vec<Option<bool>>,
    best_assignment: Option<Vec<bool>>,
    best_value: i128,
    explored_nodes: u64,
    pruned_nodes: u64,
}

fn branch_binary(
    state: &mut BranchBoundState<'_>,
    variable: usize,
    current_objective: i128,
) -> Result<(), SolverError> {
    state.explored_nodes = state.explored_nodes.saturating_add(1);
    if state.explored_nodes > state.problem.maximum_nodes {
        return Err(SolverError::NodeBudgetExceeded {
            explored_nodes: state.explored_nodes - 1,
        });
    }

    if !partial_constraints_possible(state.problem, &state.assignment)? {
        state.pruned_nodes = state.pruned_nodes.saturating_add(1);
        return Ok(());
    }

    let optimistic = state.problem.objective[variable..]
        .iter()
        .filter(|coefficient| **coefficient > 0)
        .try_fold(current_objective, |sum, coefficient| {
            sum.checked_add(i128::from(*coefficient))
                .ok_or(SolverError::ArithmeticOverflow("branch upper bound"))
        })?;
    if state.best_assignment.is_some() && optimistic < state.best_value {
        state.pruned_nodes = state.pruned_nodes.saturating_add(1);
        return Ok(());
    }

    if variable == state.problem.objective.len() {
        let assignment: Vec<bool> = state
            .assignment
            .iter()
            .map(|value| value.expect("leaf assignments are complete"))
            .collect();
        if current_objective > state.best_value
            || (current_objective == state.best_value
                && state
                    .best_assignment
                    .as_ref()
                    .is_none_or(|current| assignment < *current))
        {
            state.best_value = current_objective;
            state.best_assignment = Some(assignment);
        }
        return Ok(());
    }

    // Explore the objective-favourable value first to obtain a strong incumbent early.
    let preferred = state.problem.objective[variable] >= 0;
    for value in [preferred, !preferred] {
        state.assignment[variable] = Some(value);
        let next_objective = if value {
            current_objective
                .checked_add(i128::from(state.problem.objective[variable]))
                .ok_or(SolverError::ArithmeticOverflow("branch objective"))?
        } else {
            current_objective
        };
        branch_binary(state, variable + 1, next_objective)?;
        state.assignment[variable] = None;
    }
    Ok(())
}

fn partial_constraints_possible(
    problem: &BranchBoundProblem,
    assignment: &[Option<bool>],
) -> Result<bool, SolverError> {
    for constraint in &problem.constraints {
        let mut fixed = 0_i128;
        let mut minimum_remaining = 0_i128;
        let mut maximum_remaining = 0_i128;
        for (coefficient, value) in constraint.coefficients.iter().zip(assignment) {
            let coefficient = i128::from(*coefficient);
            match value {
                Some(true) => {
                    fixed = fixed
                        .checked_add(coefficient)
                        .ok_or(SolverError::ArithmeticOverflow("constraint partial sum"))?;
                }
                Some(false) => {}
                None if coefficient < 0 => {
                    minimum_remaining = minimum_remaining
                        .checked_add(coefficient)
                        .ok_or(SolverError::ArithmeticOverflow("constraint lower bound"))?;
                }
                None => {
                    maximum_remaining = maximum_remaining
                        .checked_add(coefficient)
                        .ok_or(SolverError::ArithmeticOverflow("constraint upper bound"))?;
                }
            }
        }
        let minimum = fixed
            .checked_add(minimum_remaining)
            .ok_or(SolverError::ArithmeticOverflow("constraint minimum"))?;
        let maximum = fixed
            .checked_add(maximum_remaining)
            .ok_or(SolverError::ArithmeticOverflow("constraint maximum"))?;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FiniteDomainConstraint {
    AllDifferent(Vec<usize>),
    Linear {
        variables: Vec<usize>,
        coefficients: Vec<i64>,
        relation: ConstraintRelation,
        rhs: i64,
    },
    NoOverlap {
        start_variables: Vec<usize>,
        durations: Vec<i64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiniteDomainProblem {
    pub domains: Vec<Vec<i64>>,
    pub constraints: Vec<FiniteDomainConstraint>,
    pub maximum_nodes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiniteDomainSolution {
    pub values: Vec<i64>,
    pub explored_nodes: u64,
    pub backtracks: u64,
}

pub fn solve_finite_domain(problem: &FiniteDomainProblem) -> Result<FiniteDomainSolution, SolverError> {
    validate_finite_domain(problem)?;
    let mut assignment = vec![None; problem.domains.len()];
    let mut explored_nodes = 0_u64;
    let mut backtracks = 0_u64;
    let solved = search_finite_domain(
        problem,
        &mut assignment,
        &mut explored_nodes,
        &mut backtracks,
    )?;
    let values = solved.ok_or(SolverError::NoFeasibleSolution)?;
    Ok(FiniteDomainSolution {
        values,
        explored_nodes,
        backtracks,
    })
}

fn validate_finite_domain(problem: &FiniteDomainProblem) -> Result<(), SolverError> {
    if problem.domains.is_empty() {
        return Err(SolverError::EmptyProblem);
    }
    if problem.maximum_nodes == 0 {
        return Err(SolverError::InvalidNodeBudget);
    }
    for (index, domain) in problem.domains.iter().enumerate() {
        if domain.is_empty() {
            return Err(SolverError::EmptyDomain { variable: index });
        }
    }
    for constraint in &problem.constraints {
        match constraint {
            FiniteDomainConstraint::AllDifferent(variables) => {
                validate_indices(variables, problem.domains.len())?;
            }
            FiniteDomainConstraint::Linear {
                variables,
                coefficients,
                ..
            } => {
                if variables.is_empty() || variables.len() != coefficients.len() {
                    return Err(SolverError::InvalidConstraint);
                }
                validate_indices(variables, problem.domains.len())?;
            }
            FiniteDomainConstraint::NoOverlap {
                start_variables,
                durations,
            } => {
                if start_variables.is_empty() || start_variables.len() != durations.len() {
                    return Err(SolverError::InvalidConstraint);
                }
                if durations.iter().any(|duration| *duration <= 0) {
                    return Err(SolverError::InvalidConstraint);
                }
                validate_indices(start_variables, problem.domains.len())?;
            }
        }
    }
    Ok(())
}

fn validate_indices(indices: &[usize], width: usize) -> Result<(), SolverError> {
    if let Some(index) = indices.iter().copied().find(|index| *index >= width) {
        return Err(SolverError::InvalidVariableIndex(index));
    }
    Ok(())
}

fn search_finite_domain(
    problem: &FiniteDomainProblem,
    assignment: &mut [Option<i64>],
    explored_nodes: &mut u64,
    backtracks: &mut u64,
) -> Result<Option<Vec<i64>>, SolverError> {
    *explored_nodes = explored_nodes.saturating_add(1);
    if *explored_nodes > problem.maximum_nodes {
        return Err(SolverError::NodeBudgetExceeded {
            explored_nodes: *explored_nodes - 1,
        });
    }
    if !finite_constraints_possible(problem, assignment)? {
        *backtracks = backtracks.saturating_add(1);
        return Ok(None);
    }
    if assignment.iter().all(Option::is_some) {
        return Ok(Some(
            assignment
                .iter()
                .map(|value| value.expect("complete assignment"))
                .collect(),
        ));
    }

    let variable = assignment
        .iter()
        .enumerate()
        .filter(|(_, value)| value.is_none())
        .min_by_key(|(index, _)| (problem.domains[*index].len(), *index))
        .map(|(index, _)| index)
        .expect("an unassigned variable exists");

    let mut values = problem.domains[variable].clone();
    values.sort_unstable();
    values.dedup();
    for value in values {
        assignment[variable] = Some(value);
        if let Some(solution) = search_finite_domain(
            problem,
            assignment,
            explored_nodes,
            backtracks,
        )? {
            return Ok(Some(solution));
        }
        assignment[variable] = None;
    }
    *backtracks = backtracks.saturating_add(1);
    Ok(None)
}

fn finite_constraints_possible(
    problem: &FiniteDomainProblem,
    assignment: &[Option<i64>],
) -> Result<bool, SolverError> {
    for constraint in &problem.constraints {
        let possible = match constraint {
            FiniteDomainConstraint::AllDifferent(variables) => {
                let mut seen = Vec::new();
                let mut ok = true;
                for variable in variables {
                    if let Some(value) = assignment[*variable] {
                        if seen.contains(&value) {
                            ok = false;
                            break;
                        }
                        seen.push(value);
                    }
                }
                ok
            }
            FiniteDomainConstraint::Linear {
                variables,
                coefficients,
                relation,
                rhs,
            } => linear_domain_possible(
                problem,
                assignment,
                variables,
                coefficients,
                *relation,
                *rhs,
            )?,
            FiniteDomainConstraint::NoOverlap {
                start_variables,
                durations,
            } => no_overlap_possible(assignment, start_variables, durations)?,
        };
        if !possible {
            return Ok(false);
        }
    }
    Ok(true)
}

fn linear_domain_possible(
    problem: &FiniteDomainProblem,
    assignment: &[Option<i64>],
    variables: &[usize],
    coefficients: &[i64],
    relation: ConstraintRelation,
    rhs: i64,
) -> Result<bool, SolverError> {
    let mut minimum = 0_i128;
    let mut maximum = 0_i128;
    for (variable, coefficient) in variables.iter().zip(coefficients) {
        let coefficient = i128::from(*coefficient);
        if let Some(value) = assignment[*variable] {
            let term = coefficient
                .checked_mul(i128::from(value))
                .ok_or(SolverError::ArithmeticOverflow("finite linear term"))?;
            minimum = minimum
                .checked_add(term)
                .ok_or(SolverError::ArithmeticOverflow("finite linear minimum"))?;
            maximum = maximum
                .checked_add(term)
                .ok_or(SolverError::ArithmeticOverflow("finite linear maximum"))?;
        } else {
            let domain = &problem.domains[*variable];
            let low = *domain.iter().min().expect("validated non-empty domain");
            let high = *domain.iter().max().expect("validated non-empty domain");
            let (term_min, term_max) = if coefficient >= 0 {
                (
                    coefficient * i128::from(low),
                    coefficient * i128::from(high),
                )
            } else {
                (
                    coefficient * i128::from(high),
                    coefficient * i128::from(low),
                )
            };
            minimum = minimum
                .checked_add(term_min)
                .ok_or(SolverError::ArithmeticOverflow("finite linear minimum"))?;
            maximum = maximum
                .checked_add(term_max)
                .ok_or(SolverError::ArithmeticOverflow("finite linear maximum"))?;
        }
    }
    let rhs = i128::from(rhs);
    Ok(match relation {
        ConstraintRelation::LessOrEqual => minimum <= rhs,
        ConstraintRelation::GreaterOrEqual => maximum >= rhs,
        ConstraintRelation::Equal => minimum <= rhs && rhs <= maximum,
    })
}

fn no_overlap_possible(
    assignment: &[Option<i64>],
    starts: &[usize],
    durations: &[i64],
) -> Result<bool, SolverError> {
    for left in 0..starts.len() {
        let Some(left_start) = assignment[starts[left]] else {
            continue;
        };
        let left_end = left_start
            .checked_add(durations[left])
            .ok_or(SolverError::ArithmeticOverflow("interval end"))?;
        for right in (left + 1)..starts.len() {
            let Some(right_start) = assignment[starts[right]] else {
                continue;
            };
            let right_end = right_start
                .checked_add(durations[right])
                .ok_or(SolverError::ArithmeticOverflow("interval end"))?;
            if left_start < right_end && right_start < left_end {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_and_bound_finds_exact_binary_optimum_and_prunes() {
        let problem = BranchBoundProblem {
            objective: vec![10, 7, 5, 1],
            constraints: vec![BinaryLinearConstraint {
                coefficients: vec![5, 4, 3, 1],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 7,
            }],
            maximum_nodes: 100,
        };
        let solution = solve_binary_branch_and_bound(&problem).expect("exact solution");
        assert_eq!(solution.objective_value, 12);
        assert_eq!(solution.assignment, vec![false, true, true, false]);
        assert!(solution.explored_nodes < 31);
        assert!(solution.pruned_nodes > 0);
    }

    #[test]
    fn branch_and_bound_refuses_to_claim_optimality_after_budget_exhaustion() {
        let problem = BranchBoundProblem {
            objective: vec![1, 1, 1, 1],
            constraints: Vec::new(),
            maximum_nodes: 1,
        };
        assert!(matches!(
            solve_binary_branch_and_bound(&problem),
            Err(SolverError::NodeBudgetExceeded { .. })
        ));
    }

    #[test]
    fn finite_domain_solver_combines_all_different_linear_and_no_overlap() {
        let problem = FiniteDomainProblem {
            domains: vec![vec![0, 1, 2], vec![0, 1, 2], vec![0, 1, 2, 3]],
            constraints: vec![
                FiniteDomainConstraint::AllDifferent(vec![0, 1]),
                FiniteDomainConstraint::Linear {
                    variables: vec![0, 1],
                    coefficients: vec![1, 1],
                    relation: ConstraintRelation::Equal,
                    rhs: 2,
                },
                FiniteDomainConstraint::NoOverlap {
                    start_variables: vec![0, 2],
                    durations: vec![1, 1],
                },
            ],
            maximum_nodes: 100,
        };
        let solution = solve_finite_domain(&problem).expect("finite-domain solution");
        assert_ne!(solution.values[0], solution.values[1]);
        assert_eq!(solution.values[0] + solution.values[1], 2);
        assert_ne!(solution.values[0], solution.values[2]);
    }
}
