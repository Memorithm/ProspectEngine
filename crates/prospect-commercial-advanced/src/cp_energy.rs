use core::fmt;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnergyTask {
    pub start_domain: Vec<i64>,
    pub duration: i64,
    pub demand: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnergeticPropagationReport {
    pub tasks: Vec<EnergyTask>,
    pub removed_starts: u64,
    pub passes: u64,
    pub examined_intervals: u64,
    /// Minimum `capacity * interval_length - mandatory_energy` observed after
    /// convergence. Zero means at least one tested interval is energy-tight.
    pub minimum_energy_slack: i128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnergeticPropagationError {
    EmptyTasks,
    EmptyDomain { task: usize },
    InvalidTask { task: usize },
    InvalidCapacity,
    InvalidIntervalBudget,
    IntervalBudgetExceeded { needed: u64, maximum: u64 },
    ArithmeticOverflow,
    Infeasible,
}

impl fmt::Display for EnergeticPropagationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTasks => formatter.write_str("energetic propagation requires tasks"),
            Self::EmptyDomain { task } => write!(formatter, "task {task} has an empty start domain"),
            Self::InvalidTask { task } => write!(formatter, "task {task} has invalid duration or demand"),
            Self::InvalidCapacity => formatter.write_str("energetic capacity must be strictly positive"),
            Self::InvalidIntervalBudget => formatter.write_str("energetic interval budget must be non-zero"),
            Self::IntervalBudgetExceeded { needed, maximum } => write!(
                formatter,
                "energetic interval budget exceeded: need {needed}, maximum {maximum}"
            ),
            Self::ArithmeticOverflow => formatter.write_str("energetic propagation arithmetic overflow"),
            Self::Infeasible => formatter.write_str("energetic reasoning proved the schedule infeasible"),
        }
    }
}

impl std::error::Error for EnergeticPropagationError {}

/// Apply finite-domain energetic reasoning to a cumulative resource.
///
/// Candidate intervals are generated from every current start and completion
/// endpoint. For each interval, a task contributes the **minimum** overlap
/// energy achievable by any remaining start value. If that mandatory energy
/// exceeds the resource energy `capacity * interval_length`, the domain state
/// is infeasible. A candidate start is removed when fixing it makes any tested
/// interval energetically impossible.
///
/// This is exact with respect to the generated finite interval family but does
/// not claim a full CP-SAT edge-finding or lazy-clause implementation.
pub fn propagate_cumulative_energy(
    tasks: &[EnergyTask],
    capacity: i64,
    maximum_intervals: u64,
) -> Result<EnergeticPropagationReport, EnergeticPropagationError> {
    if tasks.is_empty() {
        return Err(EnergeticPropagationError::EmptyTasks);
    }
    if capacity <= 0 {
        return Err(EnergeticPropagationError::InvalidCapacity);
    }
    if maximum_intervals == 0 {
        return Err(EnergeticPropagationError::InvalidIntervalBudget);
    }

    let mut current = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| normalize_task(index, task))
        .collect::<Result<Vec<_>, _>>()?;
    let initial_starts: usize = current.iter().map(|task| task.start_domain.len()).sum();
    let mut passes = 0_u64;
    let mut examined_intervals = 0_u64;

    loop {
        passes = passes.saturating_add(1);
        let intervals = candidate_intervals(&current, maximum_intervals)?;
        examined_intervals = examined_intervals.saturating_add(
            u64::try_from(intervals.len()).expect("interval count already bounded by u64"),
        );
        ensure_global_energy_feasible(&current, capacity, &intervals)?;
        let before = current.clone();

        for task_index in 0..current.len() {
            let candidates = current[task_index].start_domain.clone();
            let mut supported = Vec::with_capacity(candidates.len());
            for candidate in candidates {
                if candidate_energy_supported(
                    &current,
                    task_index,
                    candidate,
                    capacity,
                    &intervals,
                )? {
                    supported.push(candidate);
                }
            }
            if supported.is_empty() {
                return Err(EnergeticPropagationError::Infeasible);
            }
            current[task_index].start_domain = supported;
        }
        if current == before {
            break;
        }
    }

    let intervals = candidate_intervals(&current, maximum_intervals)?;
    let minimum_energy_slack = minimum_slack(&current, capacity, &intervals)?;
    let final_starts: usize = current.iter().map(|task| task.start_domain.len()).sum();
    Ok(EnergeticPropagationReport {
        tasks: current,
        removed_starts: u64::try_from(initial_starts.saturating_sub(final_starts))
            .expect("usize fits u64"),
        passes,
        examined_intervals,
        minimum_energy_slack,
    })
}

fn normalize_task(
    index: usize,
    task: &EnergyTask,
) -> Result<EnergyTask, EnergeticPropagationError> {
    if task.duration <= 0 || task.demand <= 0 {
        return Err(EnergeticPropagationError::InvalidTask { task: index });
    }
    let mut domain = task.start_domain.clone();
    domain.sort_unstable();
    domain.dedup();
    if domain.is_empty() {
        return Err(EnergeticPropagationError::EmptyDomain { task: index });
    }
    for start in &domain {
        start
            .checked_add(task.duration)
            .ok_or(EnergeticPropagationError::ArithmeticOverflow)?;
    }
    Ok(EnergyTask {
        start_domain: domain,
        duration: task.duration,
        demand: task.demand,
    })
}

fn candidate_intervals(
    tasks: &[EnergyTask],
    maximum_intervals: u64,
) -> Result<Vec<(i64, i64)>, EnergeticPropagationError> {
    let mut endpoints = BTreeSet::new();
    for task in tasks {
        for start in &task.start_domain {
            endpoints.insert(*start);
            endpoints.insert(
                start
                    .checked_add(task.duration)
                    .ok_or(EnergeticPropagationError::ArithmeticOverflow)?,
            );
        }
    }
    let points: Vec<i64> = endpoints.into_iter().collect();
    let count = points.len();
    let needed_usize = count.saturating_mul(count.saturating_sub(1)) / 2;
    let needed = u64::try_from(needed_usize).unwrap_or(u64::MAX);
    if needed > maximum_intervals {
        return Err(EnergeticPropagationError::IntervalBudgetExceeded {
            needed,
            maximum: maximum_intervals,
        });
    }
    let mut intervals = Vec::with_capacity(needed_usize);
    for left in 0..points.len() {
        for right in (left + 1)..points.len() {
            intervals.push((points[left], points[right]));
        }
    }
    Ok(intervals)
}

fn ensure_global_energy_feasible(
    tasks: &[EnergyTask],
    capacity: i64,
    intervals: &[(i64, i64)],
) -> Result<(), EnergeticPropagationError> {
    for interval in intervals {
        let required = required_energy(tasks, None, *interval)?;
        let available = available_energy(capacity, *interval)?;
        if required > available {
            return Err(EnergeticPropagationError::Infeasible);
        }
    }
    Ok(())
}

fn candidate_energy_supported(
    tasks: &[EnergyTask],
    fixed_task: usize,
    fixed_start: i64,
    capacity: i64,
    intervals: &[(i64, i64)],
) -> Result<bool, EnergeticPropagationError> {
    for interval in intervals {
        let required = required_energy(tasks, Some((fixed_task, fixed_start)), *interval)?;
        let available = available_energy(capacity, *interval)?;
        if required > available {
            return Ok(false);
        }
    }
    Ok(true)
}

fn minimum_slack(
    tasks: &[EnergyTask],
    capacity: i64,
    intervals: &[(i64, i64)],
) -> Result<i128, EnergeticPropagationError> {
    let mut minimum = i128::MAX;
    for interval in intervals {
        let required = required_energy(tasks, None, *interval)?;
        let available = available_energy(capacity, *interval)?;
        minimum = minimum.min(available - required);
    }
    Ok(if minimum == i128::MAX { 0 } else { minimum })
}

fn required_energy(
    tasks: &[EnergyTask],
    fixed: Option<(usize, i64)>,
    interval: (i64, i64),
) -> Result<i128, EnergeticPropagationError> {
    let mut required = 0_i128;
    for (index, task) in tasks.iter().enumerate() {
        let overlap = match fixed {
            Some((fixed_index, start)) if fixed_index == index => {
                overlap_length(start, task.duration, interval)?
            }
            _ => {
                let mut minimum = i64::MAX;
                for start in &task.start_domain {
                    minimum = minimum.min(overlap_length(*start, task.duration, interval)?);
                }
                minimum
            }
        };
        let energy = i128::from(overlap)
            .checked_mul(i128::from(task.demand))
            .ok_or(EnergeticPropagationError::ArithmeticOverflow)?;
        required = required
            .checked_add(energy)
            .ok_or(EnergeticPropagationError::ArithmeticOverflow)?;
    }
    Ok(required)
}

fn overlap_length(
    start: i64,
    duration: i64,
    interval: (i64, i64),
) -> Result<i64, EnergeticPropagationError> {
    let end = start
        .checked_add(duration)
        .ok_or(EnergeticPropagationError::ArithmeticOverflow)?;
    let left = start.max(interval.0);
    let right = end.min(interval.1);
    Ok(right.saturating_sub(left).max(0))
}

fn available_energy(
    capacity: i64,
    interval: (i64, i64),
) -> Result<i128, EnergeticPropagationError> {
    let length = interval
        .1
        .checked_sub(interval.0)
        .ok_or(EnergeticPropagationError::ArithmeticOverflow)?;
    i128::from(length)
        .checked_mul(i128::from(capacity))
        .ok_or(EnergeticPropagationError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energetic_reasoning_detects_overload() {
        let tasks = vec![
            EnergyTask {
                start_domain: vec![0, 1],
                duration: 2,
                demand: 1,
            },
            EnergyTask {
                start_domain: vec![0, 1],
                duration: 2,
                demand: 1,
            },
            EnergyTask {
                start_domain: vec![0, 1],
                duration: 2,
                demand: 1,
            },
        ];
        assert_eq!(
            propagate_cumulative_energy(&tasks, 2, 100),
            Err(EnergeticPropagationError::Infeasible)
        );
    }

    #[test]
    fn energetic_reasoning_filters_fixed_overlap_candidate() {
        let tasks = vec![
            EnergyTask {
                start_domain: vec![0],
                duration: 2,
                demand: 1,
            },
            EnergyTask {
                start_domain: vec![0, 2],
                duration: 2,
                demand: 1,
            },
        ];
        let report = propagate_cumulative_energy(&tasks, 1, 100).expect("energy propagation");
        assert_eq!(report.tasks[1].start_domain, vec![2]);
        assert_eq!(report.removed_starts, 1);
        assert!(report.minimum_energy_slack >= 0);
    }

    #[test]
    fn interval_budget_fails_closed() {
        let tasks = vec![EnergyTask {
            start_domain: vec![0, 1, 2, 3],
            duration: 2,
            demand: 1,
        }];
        assert!(matches!(
            propagate_cumulative_energy(&tasks, 1, 1),
            Err(EnergeticPropagationError::IntervalBudgetExceeded { .. })
        ));
    }
}
