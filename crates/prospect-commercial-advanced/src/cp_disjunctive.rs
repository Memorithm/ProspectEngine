use core::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisjunctiveTask {
    pub start_domain: Vec<i64>,
    pub duration: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForcedPrecedence {
    pub before: usize,
    pub after: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisjunctivePropagationReport {
    pub tasks: Vec<DisjunctiveTask>,
    pub removed_starts: u64,
    pub passes: u64,
    pub forced_precedences: Vec<ForcedPrecedence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisjunctivePropagationError {
    EmptyTasks,
    EmptyDomain { task: usize },
    InvalidDuration { task: usize },
    ArithmeticOverflow,
    Infeasible,
}

impl fmt::Display for DisjunctivePropagationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTasks => formatter.write_str("disjunctive propagation requires tasks"),
            Self::EmptyDomain { task } => write!(formatter, "task {task} has an empty domain"),
            Self::InvalidDuration { task } => write!(formatter, "task {task} has invalid duration"),
            Self::ArithmeticOverflow => {
                formatter.write_str("disjunctive timing arithmetic overflow")
            }
            Self::Infeasible => {
                formatter.write_str("pairwise disjunctive propagation proved infeasibility")
            }
        }
    }
}

impl std::error::Error for DisjunctivePropagationError {}

/// Enforce pairwise arc consistency for a unary `NoOverlap` resource.
///
/// A start remains in task `i` only when every other task `j` has at least one
/// remaining start that does not overlap it. Propagation repeats to a fixed
/// point. After convergence, a precedence `i -> j` is reported only when every
/// supported pair of remaining starts places `i` completely before `j`.
///
/// This is exact pairwise finite-domain disjunctive propagation. It is stronger
/// than singleton-only pruning but deliberately is **not** labeled full
/// multi-task edge-finding.
pub fn propagate_pairwise_disjunctive(
    tasks: &[DisjunctiveTask],
) -> Result<DisjunctivePropagationReport, DisjunctivePropagationError> {
    if tasks.is_empty() {
        return Err(DisjunctivePropagationError::EmptyTasks);
    }
    let mut current = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| normalize_task(index, task))
        .collect::<Result<Vec<_>, _>>()?;
    let initial_count: usize = current.iter().map(|task| task.start_domain.len()).sum();
    let mut passes = 0_u64;

    loop {
        passes = passes.saturating_add(1);
        let before = current.clone();
        for left in 0..current.len() {
            let candidates = current[left].start_domain.clone();
            let mut supported = Vec::with_capacity(candidates.len());
            'candidate: for candidate in candidates {
                for right in 0..current.len() {
                    if left == right {
                        continue;
                    }
                    let has_support = current[right].start_domain.iter().copied().any(|other| {
                        non_overlapping(
                            candidate,
                            current[left].duration,
                            other,
                            current[right].duration,
                        )
                        .unwrap_or(false)
                    });
                    if !has_support {
                        continue 'candidate;
                    }
                }
                supported.push(candidate);
            }
            if supported.is_empty() {
                return Err(DisjunctivePropagationError::Infeasible);
            }
            current[left].start_domain = supported;
        }
        if current == before {
            break;
        }
    }

    let mut forced_precedences = Vec::new();
    for left in 0..current.len() {
        for right in (left + 1)..current.len() {
            let left_before = all_pairs_force_before(&current[left], &current[right])?;
            let right_before = all_pairs_force_before(&current[right], &current[left])?;
            match (left_before, right_before) {
                (true, false) => forced_precedences.push(ForcedPrecedence {
                    before: left,
                    after: right,
                }),
                (false, true) => forced_precedences.push(ForcedPrecedence {
                    before: right,
                    after: left,
                }),
                _ => {}
            }
        }
    }
    forced_precedences.sort_by_key(|precedence| (precedence.before, precedence.after));

    let final_count: usize = current.iter().map(|task| task.start_domain.len()).sum();
    Ok(DisjunctivePropagationReport {
        tasks: current,
        removed_starts: u64::try_from(initial_count.saturating_sub(final_count))
            .expect("usize fits u64"),
        passes,
        forced_precedences,
    })
}

fn normalize_task(
    index: usize,
    task: &DisjunctiveTask,
) -> Result<DisjunctiveTask, DisjunctivePropagationError> {
    if task.duration <= 0 {
        return Err(DisjunctivePropagationError::InvalidDuration { task: index });
    }
    let mut domain = task.start_domain.clone();
    domain.sort_unstable();
    domain.dedup();
    if domain.is_empty() {
        return Err(DisjunctivePropagationError::EmptyDomain { task: index });
    }
    for start in &domain {
        start
            .checked_add(task.duration)
            .ok_or(DisjunctivePropagationError::ArithmeticOverflow)?;
    }
    Ok(DisjunctiveTask {
        start_domain: domain,
        duration: task.duration,
    })
}

fn non_overlapping(
    left_start: i64,
    left_duration: i64,
    right_start: i64,
    right_duration: i64,
) -> Result<bool, DisjunctivePropagationError> {
    let left_end = left_start
        .checked_add(left_duration)
        .ok_or(DisjunctivePropagationError::ArithmeticOverflow)?;
    let right_end = right_start
        .checked_add(right_duration)
        .ok_or(DisjunctivePropagationError::ArithmeticOverflow)?;
    Ok(left_end <= right_start || right_end <= left_start)
}

fn all_pairs_force_before(
    before: &DisjunctiveTask,
    after: &DisjunctiveTask,
) -> Result<bool, DisjunctivePropagationError> {
    let mut saw_supported_pair = false;
    for before_start in &before.start_domain {
        let before_end = before_start
            .checked_add(before.duration)
            .ok_or(DisjunctivePropagationError::ArithmeticOverflow)?;
        for after_start in &after.start_domain {
            if non_overlapping(*before_start, before.duration, *after_start, after.duration)? {
                saw_supported_pair = true;
                if before_end > *after_start {
                    return Ok(false);
                }
            }
        }
    }
    Ok(saw_supported_pair)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairwise_arc_consistency_removes_unsupported_starts() {
        let tasks = vec![
            DisjunctiveTask {
                start_domain: vec![0],
                duration: 2,
            },
            DisjunctiveTask {
                start_domain: vec![0, 1, 2],
                duration: 2,
            },
        ];
        let report = propagate_pairwise_disjunctive(&tasks).expect("disjunctive propagation");
        assert_eq!(report.tasks[1].start_domain, vec![2]);
        assert_eq!(report.removed_starts, 2);
        assert_eq!(
            report.forced_precedences,
            vec![ForcedPrecedence {
                before: 0,
                after: 1
            }]
        );
    }

    #[test]
    fn incompatible_singletons_are_infeasible() {
        let tasks = vec![
            DisjunctiveTask {
                start_domain: vec![0],
                duration: 3,
            },
            DisjunctiveTask {
                start_domain: vec![1],
                duration: 3,
            },
        ];
        assert_eq!(
            propagate_pairwise_disjunctive(&tasks),
            Err(DisjunctivePropagationError::Infeasible)
        );
    }
}
