use core::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalDisjunctiveTask {
    pub start_domain: Vec<i64>,
    pub duration: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalDisjunctiveConfig {
    /// Total DFS nodes permitted across all support checks in one propagation.
    pub maximum_search_nodes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalDisjunctiveReport {
    pub tasks: Vec<GlobalDisjunctiveTask>,
    pub removed_starts: u64,
    pub passes: u64,
    pub search_nodes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalDisjunctiveError {
    EmptyTasks,
    EmptyDomain { task: usize },
    InvalidDuration { task: usize },
    InvalidSearchBudget,
    ArithmeticOverflow,
    SearchBudgetExceeded { visited: u64 },
    Infeasible,
}

impl fmt::Display for GlobalDisjunctiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTasks => formatter.write_str("global disjunctive propagation requires tasks"),
            Self::EmptyDomain { task } => write!(formatter, "task {task} has an empty start domain"),
            Self::InvalidDuration { task } => write!(formatter, "task {task} has invalid duration"),
            Self::InvalidSearchBudget => {
                formatter.write_str("global disjunctive search budget must be non-zero")
            }
            Self::ArithmeticOverflow => {
                formatter.write_str("global disjunctive timing arithmetic overflow")
            }
            Self::SearchBudgetExceeded { visited } => write!(
                formatter,
                "global disjunctive support search exceeded its budget after {visited} nodes"
            ),
            Self::Infeasible => {
                formatter.write_str("global disjunctive propagation proved infeasibility")
            }
        }
    }
}

impl std::error::Error for GlobalDisjunctiveError {}

/// Enforce global domain consistency for a finite-domain unary `NoOverlap`.
///
/// A candidate start for task `i` is retained only when there exists a complete
/// assignment of one start to every task such that no two task intervals
/// overlap. Support is established by a deterministic depth-first search with
/// minimum-remaining-values variable ordering. Unsupported starts are removed
/// and the process repeats to a fixed point.
///
/// This is exact generalized arc consistency for the finite domains supplied to
/// this function. Its cost is exponential in the worst case, so callers must
/// provide a total search-node budget. Budget exhaustion is an error, never a
/// partial-consistency success claim.
pub fn propagate_global_disjunctive_gac(
    tasks: &[GlobalDisjunctiveTask],
    config: GlobalDisjunctiveConfig,
) -> Result<GlobalDisjunctiveReport, GlobalDisjunctiveError> {
    if tasks.is_empty() {
        return Err(GlobalDisjunctiveError::EmptyTasks);
    }
    if config.maximum_search_nodes == 0 {
        return Err(GlobalDisjunctiveError::InvalidSearchBudget);
    }
    let mut current = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| normalize_task(index, task))
        .collect::<Result<Vec<_>, _>>()?;
    let initial_count: usize = current.iter().map(|task| task.start_domain.len()).sum();
    let mut passes = 0_u64;
    let mut search_nodes = 0_u64;

    loop {
        passes = passes.saturating_add(1);
        let before = current.clone();
        for task_index in 0..current.len() {
            let candidates = current[task_index].start_domain.clone();
            let mut supported = Vec::with_capacity(candidates.len());
            for candidate in candidates {
                if candidate_has_global_support(
                    &current,
                    task_index,
                    candidate,
                    &mut search_nodes,
                    config.maximum_search_nodes,
                )? {
                    supported.push(candidate);
                }
            }
            if supported.is_empty() {
                return Err(GlobalDisjunctiveError::Infeasible);
            }
            current[task_index].start_domain = supported;
        }
        if current == before {
            break;
        }
    }

    let final_count: usize = current.iter().map(|task| task.start_domain.len()).sum();
    Ok(GlobalDisjunctiveReport {
        tasks: current,
        removed_starts: u64::try_from(initial_count.saturating_sub(final_count))
            .expect("usize fits u64"),
        passes,
        search_nodes,
    })
}

fn candidate_has_global_support(
    tasks: &[GlobalDisjunctiveTask],
    fixed_task: usize,
    fixed_start: i64,
    visited: &mut u64,
    maximum_search_nodes: u64,
) -> Result<bool, GlobalDisjunctiveError> {
    if !tasks[fixed_task].start_domain.contains(&fixed_start) {
        return Ok(false);
    }
    let mut assignment = vec![None; tasks.len()];
    assignment[fixed_task] = Some(fixed_start);
    search_complete_assignment(tasks, &mut assignment, visited, maximum_search_nodes)
}

fn search_complete_assignment(
    tasks: &[GlobalDisjunctiveTask],
    assignment: &mut [Option<i64>],
    visited: &mut u64,
    maximum_search_nodes: u64,
) -> Result<bool, GlobalDisjunctiveError> {
    if *visited >= maximum_search_nodes {
        return Err(GlobalDisjunctiveError::SearchBudgetExceeded { visited: *visited });
    }
    *visited = visited.saturating_add(1);

    let Some(next) = choose_next_task(tasks, assignment) else {
        return Ok(true);
    };

    for start in &tasks[next].start_domain {
        if compatible_with_assignment(tasks, assignment, next, *start)? {
            assignment[next] = Some(*start);
            if search_complete_assignment(tasks, assignment, visited, maximum_search_nodes)? {
                assignment[next] = None;
                return Ok(true);
            }
            assignment[next] = None;
        }
    }
    Ok(false)
}

fn choose_next_task(tasks: &[GlobalDisjunctiveTask], assignment: &[Option<i64>]) -> Option<usize> {
    tasks
        .iter()
        .enumerate()
        .filter(|(index, _)| assignment[*index].is_none())
        .min_by_key(|(index, task)| (task.start_domain.len(), *index))
        .map(|(index, _)| index)
}

fn compatible_with_assignment(
    tasks: &[GlobalDisjunctiveTask],
    assignment: &[Option<i64>],
    task_index: usize,
    candidate: i64,
) -> Result<bool, GlobalDisjunctiveError> {
    for (other_index, other_start) in assignment.iter().enumerate() {
        let Some(other_start) = other_start else {
            continue;
        };
        if !non_overlapping(
            candidate,
            tasks[task_index].duration,
            *other_start,
            tasks[other_index].duration,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn non_overlapping(
    left_start: i64,
    left_duration: i64,
    right_start: i64,
    right_duration: i64,
) -> Result<bool, GlobalDisjunctiveError> {
    let left_end = left_start
        .checked_add(left_duration)
        .ok_or(GlobalDisjunctiveError::ArithmeticOverflow)?;
    let right_end = right_start
        .checked_add(right_duration)
        .ok_or(GlobalDisjunctiveError::ArithmeticOverflow)?;
    Ok(left_end <= right_start || right_end <= left_start)
}

fn normalize_task(
    index: usize,
    task: &GlobalDisjunctiveTask,
) -> Result<GlobalDisjunctiveTask, GlobalDisjunctiveError> {
    if task.duration <= 0 {
        return Err(GlobalDisjunctiveError::InvalidDuration { task: index });
    }
    let mut start_domain = task.start_domain.clone();
    start_domain.sort_unstable();
    start_domain.dedup();
    if start_domain.is_empty() {
        return Err(GlobalDisjunctiveError::EmptyDomain { task: index });
    }
    for start in &start_domain {
        start
            .checked_add(task.duration)
            .ok_or(GlobalDisjunctiveError::ArithmeticOverflow)?;
    }
    Ok(GlobalDisjunctiveTask {
        start_domain,
        duration: task.duration,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_support_removes_value_that_is_pairwise_supported_but_not_jointly_supported() {
        // Three length-2 jobs on the starts {0,2}. Every pair has a
        // non-overlapping assignment, but all three cannot fit simultaneously.
        // Fixing task 0 at either value therefore has no complete support and
        // proves the whole instance infeasible.
        let tasks = vec![
            GlobalDisjunctiveTask {
                start_domain: vec![0, 2],
                duration: 2,
            },
            GlobalDisjunctiveTask {
                start_domain: vec![0, 2],
                duration: 2,
            },
            GlobalDisjunctiveTask {
                start_domain: vec![0, 2],
                duration: 2,
            },
        ];
        assert_eq!(
            propagate_global_disjunctive_gac(
                &tasks,
                GlobalDisjunctiveConfig {
                    maximum_search_nodes: 10_000,
                }
            ),
            Err(GlobalDisjunctiveError::Infeasible)
        );
    }

    #[test]
    fn global_support_prunes_a_start_using_three_task_interaction() {
        // If task 0 starts at 2, the two fixed-edge tasks cannot both be
        // scheduled. Starting task 0 at 4 has complete support.
        let tasks = vec![
            GlobalDisjunctiveTask {
                start_domain: vec![2, 4],
                duration: 2,
            },
            GlobalDisjunctiveTask {
                start_domain: vec![0, 4],
                duration: 2,
            },
            GlobalDisjunctiveTask {
                start_domain: vec![0, 2],
                duration: 2,
            },
        ];
        let report = propagate_global_disjunctive_gac(
            &tasks,
            GlobalDisjunctiveConfig {
                maximum_search_nodes: 10_000,
            },
        )
        .expect("global propagation");
        assert_eq!(report.tasks[0].start_domain, vec![4]);
        assert!(report.removed_starts >= 1);
    }

    #[test]
    fn support_search_budget_exhaustion_fails_closed() {
        let tasks = vec![
            GlobalDisjunctiveTask {
                start_domain: vec![0, 2, 4],
                duration: 2,
            },
            GlobalDisjunctiveTask {
                start_domain: vec![0, 2, 4],
                duration: 2,
            },
            GlobalDisjunctiveTask {
                start_domain: vec![0, 2, 4],
                duration: 2,
            },
        ];
        assert!(matches!(
            propagate_global_disjunctive_gac(
                &tasks,
                GlobalDisjunctiveConfig {
                    maximum_search_nodes: 1,
                }
            ),
            Err(GlobalDisjunctiveError::SearchBudgetExceeded { .. })
        ));
    }
}
