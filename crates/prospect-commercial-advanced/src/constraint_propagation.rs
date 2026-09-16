use crate::optimization::ConstraintRelation;
use crate::solver::{FiniteDomainConstraint, FiniteDomainProblem};
use core::fmt;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropagationError {
    InvalidProblem,
    Infeasible,
    InvalidNodeBudget,
    NodeBudgetExceeded { explored_nodes: u64 },
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for PropagationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProblem => formatter.write_str("constraint propagation problem is invalid"),
            Self::Infeasible => formatter.write_str("constraint propagation proved the problem infeasible"),
            Self::InvalidNodeBudget => formatter.write_str("propagated CP node budget must be non-zero"),
            Self::NodeBudgetExceeded { explored_nodes } => write!(
                formatter,
                "propagated CP node budget exceeded after {explored_nodes} nodes"
            ),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
        }
    }
}

impl std::error::Error for PropagationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropagationReport {
    pub domains: Vec<Vec<i64>>,
    pub passes: u64,
    pub removed_values: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropagatedSolution {
    pub values: Vec<i64>,
    pub explored_nodes: u64,
    pub propagation_passes: u64,
    pub removed_values: u64,
}

pub fn propagate_domains(problem: &FiniteDomainProblem) -> Result<PropagationReport, PropagationError> {
    validate(problem)?;
    let mut domains = normalize_domains(&problem.domains)?;
    let initial_values: usize = domains.iter().map(Vec::len).sum();
    let mut passes = 0_u64;
    loop {
        passes = passes.saturating_add(1);
        let before = domains.clone();
        propagate_all_different(&mut domains, &problem.constraints)?;
        propagate_linear(&mut domains, &problem.constraints)?;
        propagate_no_overlap(&mut domains, &problem.constraints)?;
        if domains.iter().any(Vec::is_empty) {
            return Err(PropagationError::Infeasible);
        }
        if domains == before {
            break;
        }
    }
    let final_values: usize = domains.iter().map(Vec::len).sum();
    Ok(PropagationReport {
        domains,
        passes,
        removed_values: u64::try_from(initial_values.saturating_sub(final_values))
            .expect("usize fits u64"),
    })
}

pub fn solve_finite_domain_propagated(
    problem: &FiniteDomainProblem,
) -> Result<PropagatedSolution, PropagationError> {
    validate(problem)?;
    if problem.maximum_nodes == 0 {
        return Err(PropagationError::InvalidNodeBudget);
    }
    let mut explored_nodes = 0_u64;
    let mut propagation_passes = 0_u64;
    let mut removed_values = 0_u64;
    let solution = search(
        problem,
        normalize_domains(&problem.domains)?,
        &mut explored_nodes,
        &mut propagation_passes,
        &mut removed_values,
    )?
    .ok_or(PropagationError::Infeasible)?;
    Ok(PropagatedSolution {
        values: solution,
        explored_nodes,
        propagation_passes,
        removed_values,
    })
}

fn search(
    problem: &FiniteDomainProblem,
    domains: Vec<Vec<i64>>,
    explored_nodes: &mut u64,
    propagation_passes: &mut u64,
    removed_values: &mut u64,
) -> Result<Option<Vec<i64>>, PropagationError> {
    *explored_nodes = explored_nodes.saturating_add(1);
    if *explored_nodes > problem.maximum_nodes {
        return Err(PropagationError::NodeBudgetExceeded {
            explored_nodes: *explored_nodes - 1,
        });
    }

    let local = FiniteDomainProblem {
        domains,
        constraints: problem.constraints.clone(),
        maximum_nodes: problem.maximum_nodes,
    };
    let report = match propagate_domains(&local) {
        Ok(report) => report,
        Err(PropagationError::Infeasible) => return Ok(None),
        Err(error) => return Err(error),
    };
    *propagation_passes = propagation_passes.saturating_add(report.passes);
    *removed_values = removed_values.saturating_add(report.removed_values);

    if report.domains.iter().all(|domain| domain.len() == 1) {
        return Ok(Some(report.domains.iter().map(|domain| domain[0]).collect()));
    }

    let variable = report
        .domains
        .iter()
        .enumerate()
        .filter(|(_, domain)| domain.len() > 1)
        .min_by_key(|(index, domain)| (domain.len(), *index))
        .map(|(index, _)| index)
        .expect("a non-singleton domain exists");

    for value in report.domains[variable].iter().copied() {
        let mut child = report.domains.clone();
        child[variable] = vec![value];
        if let Some(solution) = search(
            problem,
            child,
            explored_nodes,
            propagation_passes,
            removed_values,
        )? {
            return Ok(Some(solution));
        }
    }
    Ok(None)
}

fn validate(problem: &FiniteDomainProblem) -> Result<(), PropagationError> {
    if problem.domains.is_empty() || problem.domains.iter().any(Vec::is_empty) {
        return Err(PropagationError::InvalidProblem);
    }
    let width = problem.domains.len();
    for constraint in &problem.constraints {
        match constraint {
            FiniteDomainConstraint::AllDifferent(variables) => {
                if variables.is_empty() || variables.iter().any(|index| *index >= width) {
                    return Err(PropagationError::InvalidProblem);
                }
            }
            FiniteDomainConstraint::Linear {
                variables,
                coefficients,
                ..
            } => {
                if variables.is_empty()
                    || variables.len() != coefficients.len()
                    || variables.iter().any(|index| *index >= width)
                {
                    return Err(PropagationError::InvalidProblem);
                }
            }
            FiniteDomainConstraint::NoOverlap {
                start_variables,
                durations,
            } => {
                if start_variables.is_empty()
                    || start_variables.len() != durations.len()
                    || start_variables.iter().any(|index| *index >= width)
                    || durations.iter().any(|duration| *duration <= 0)
                {
                    return Err(PropagationError::InvalidProblem);
                }
            }
        }
    }
    Ok(())
}

fn normalize_domains(domains: &[Vec<i64>]) -> Result<Vec<Vec<i64>>, PropagationError> {
    domains
        .iter()
        .map(|domain| {
            let mut values = domain.clone();
            values.sort_unstable();
            values.dedup();
            if values.is_empty() {
                Err(PropagationError::Infeasible)
            } else {
                Ok(values)
            }
        })
        .collect()
}

fn propagate_all_different(
    domains: &mut [Vec<i64>],
    constraints: &[FiniteDomainConstraint],
) -> Result<(), PropagationError> {
    for variables in constraints.iter().filter_map(|constraint| match constraint {
        FiniteDomainConstraint::AllDifferent(variables) => Some(variables.as_slice()),
        _ => None,
    }) {
        let singleton_values: Vec<i64> = variables
            .iter()
            .filter_map(|index| (domains[*index].len() == 1).then_some(domains[*index][0]))
            .collect();
        let unique: BTreeSet<i64> = singleton_values.iter().copied().collect();
        if unique.len() != singleton_values.len() {
            return Err(PropagationError::Infeasible);
        }
        for variable in variables {
            if domains[*variable].len() == 1 {
                continue;
            }
            domains[*variable].retain(|value| !unique.contains(value));
            if domains[*variable].is_empty() {
                return Err(PropagationError::Infeasible);
            }
        }
    }
    Ok(())
}

fn propagate_linear(
    domains: &mut [Vec<i64>],
    constraints: &[FiniteDomainConstraint],
) -> Result<(), PropagationError> {
    for constraint in constraints {
        let FiniteDomainConstraint::Linear {
            variables,
            coefficients,
            relation,
            rhs,
        } = constraint
        else {
            continue;
        };
        for position in 0..variables.len() {
            let variable = variables[position];
            let coefficient = coefficients[position];
            let candidates = domains[variable].clone();
            let mut supported = Vec::new();
            for candidate in candidates {
                if linear_candidate_supported(
                    domains,
                    variables,
                    coefficients,
                    position,
                    candidate,
                    *relation,
                    *rhs,
                )? {
                    supported.push(candidate);
                }
            }
            if supported.is_empty() {
                return Err(PropagationError::Infeasible);
            }
            domains[variable] = supported;
        }
    }
    Ok(())
}

fn linear_candidate_supported(
    domains: &[Vec<i64>],
    variables: &[usize],
    coefficients: &[i64],
    fixed_position: usize,
    fixed_value: i64,
    relation: ConstraintRelation,
    rhs: i64,
) -> Result<bool, PropagationError> {
    let mut minimum = 0_i128;
    let mut maximum = 0_i128;
    for (position, (variable, coefficient)) in variables.iter().zip(coefficients).enumerate() {
        let coefficient = i128::from(*coefficient);
        let (low, high) = if position == fixed_position {
            (i128::from(fixed_value), i128::from(fixed_value))
        } else {
            (
                i128::from(*domains[*variable].first().expect("non-empty domain")),
                i128::from(*domains[*variable].last().expect("non-empty domain")),
            )
        };
        let (term_minimum, term_maximum) = if coefficient >= 0 {
            (coefficient * low, coefficient * high)
        } else {
            (coefficient * high, coefficient * low)
        };
        minimum = minimum
            .checked_add(term_minimum)
            .ok_or(PropagationError::ArithmeticOverflow("linear propagation minimum"))?;
        maximum = maximum
            .checked_add(term_maximum)
            .ok_or(PropagationError::ArithmeticOverflow("linear propagation maximum"))?;
    }
    let rhs = i128::from(rhs);
    Ok(match relation {
        ConstraintRelation::LessOrEqual => minimum <= rhs,
        ConstraintRelation::GreaterOrEqual => maximum >= rhs,
        ConstraintRelation::Equal => minimum <= rhs && rhs <= maximum,
    })
}

fn propagate_no_overlap(
    domains: &mut [Vec<i64>],
    constraints: &[FiniteDomainConstraint],
) -> Result<(), PropagationError> {
    for constraint in constraints {
        let FiniteDomainConstraint::NoOverlap {
            start_variables,
            durations,
        } = constraint
        else {
            continue;
        };
        for left in 0..start_variables.len() {
            if domains[start_variables[left]].len() != 1 {
                continue;
            }
            let fixed_start = domains[start_variables[left]][0];
            let fixed_end = fixed_start
                .checked_add(durations[left])
                .ok_or(PropagationError::ArithmeticOverflow("fixed interval end"))?;
            for right in 0..start_variables.len() {
                if left == right {
                    continue;
                }
                let variable = start_variables[right];
                let duration = durations[right];
                domains[variable].retain(|candidate| {
                    candidate
                        .checked_add(duration)
                        .is_some_and(|end| *candidate >= fixed_end || end <= fixed_start)
                });
                if domains[variable].is_empty() {
                    return Err(PropagationError::Infeasible);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn propagation_removes_values_before_search() {
        let problem = FiniteDomainProblem {
            domains: vec![vec![1], vec![1, 2, 3], vec![0, 1, 2, 3]],
            constraints: vec![
                FiniteDomainConstraint::AllDifferent(vec![0, 1]),
                FiniteDomainConstraint::Linear {
                    variables: vec![1, 2],
                    coefficients: vec![1, 1],
                    relation: ConstraintRelation::Equal,
                    rhs: 3,
                },
            ],
            maximum_nodes: 100,
        };
        let report = propagate_domains(&problem).expect("propagation succeeds");
        assert_eq!(report.domains[1], vec![2, 3]);
        assert!(report.removed_values > 0);
    }

    #[test]
    fn propagated_solver_solves_scheduling_problem() {
        let problem = FiniteDomainProblem {
            domains: vec![vec![0, 1, 2], vec![0, 1, 2], vec![0, 1, 2]],
            constraints: vec![
                FiniteDomainConstraint::AllDifferent(vec![0, 1, 2]),
                FiniteDomainConstraint::NoOverlap {
                    start_variables: vec![0, 1],
                    durations: vec![1, 1],
                },
            ],
            maximum_nodes: 100,
        };
        let solution = solve_finite_domain_propagated(&problem).expect("propagated solution");
        assert_eq!(solution.values.len(), 3);
        assert!(solution.explored_nodes <= 4);
        assert!(solution.propagation_passes > 0);
    }

    #[test]
    fn propagation_detects_duplicate_singletons_in_all_different() {
        let problem = FiniteDomainProblem {
            domains: vec![vec![1], vec![1]],
            constraints: vec![FiniteDomainConstraint::AllDifferent(vec![0, 1])],
            maximum_nodes: 10,
        };
        assert_eq!(propagate_domains(&problem), Err(PropagationError::Infeasible));
    }
}
