use core::fmt;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalPropagationError {
    EmptyVariables,
    EmptyDomain { variable: usize },
    DuplicateVariable,
    InvalidTask,
    InvalidCapacity,
    Infeasible,
}

impl fmt::Display for GlobalPropagationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyVariables => formatter.write_str("global propagator requires variables"),
            Self::EmptyDomain { variable } => {
                write!(formatter, "variable {variable} has an empty domain")
            }
            Self::DuplicateVariable => {
                formatter.write_str("global constraint variable list contains duplicates")
            }
            Self::InvalidTask => formatter.write_str("cumulative task configuration is invalid"),
            Self::InvalidCapacity => {
                formatter.write_str("cumulative capacity must be strictly positive")
            }
            Self::Infeasible => formatter.write_str("global propagation proved infeasibility"),
        }
    }
}

impl std::error::Error for GlobalPropagationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllDifferentGacReport {
    pub domains: Vec<Vec<i64>>,
    pub removed_values: u64,
    pub passes: u64,
}

/// Enforce generalized arc consistency for one `AllDifferent` constraint.
///
/// A value remains in a domain only if the complete variable set admits a
/// matching while that variable is fixed to the candidate. This is more
/// expensive than singleton pruning but detects Hall-set failures and removes
/// unsupported values deterministically.
pub fn propagate_all_different_gac(
    domains: &[Vec<i64>],
) -> Result<AllDifferentGacReport, GlobalPropagationError> {
    if domains.is_empty() {
        return Err(GlobalPropagationError::EmptyVariables);
    }
    let mut current = normalize_domains(domains)?;
    let initial_count: usize = current.iter().map(Vec::len).sum();
    let mut passes = 0_u64;

    loop {
        passes = passes.saturating_add(1);
        if !has_perfect_matching(&current, None) {
            return Err(GlobalPropagationError::Infeasible);
        }
        let before = current.clone();
        for variable in 0..current.len() {
            let candidates = current[variable].clone();
            let mut supported = Vec::new();
            for candidate in candidates {
                if has_perfect_matching(&current, Some((variable, candidate))) {
                    supported.push(candidate);
                }
            }
            if supported.is_empty() {
                return Err(GlobalPropagationError::Infeasible);
            }
            current[variable] = supported;
        }
        if current == before {
            break;
        }
    }

    let final_count: usize = current.iter().map(Vec::len).sum();
    Ok(AllDifferentGacReport {
        domains: current,
        removed_values: u64::try_from(initial_count.saturating_sub(final_count))
            .expect("usize fits u64"),
        passes,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CumulativeTask {
    pub start_domain: Vec<i64>,
    pub duration: i64,
    pub demand: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CumulativePropagationReport {
    pub tasks: Vec<CumulativeTask>,
    pub removed_starts: u64,
    pub passes: u64,
    pub mandatory_peak_load: i64,
}

/// Time-table propagation for a finite-domain cumulative resource constraint.
///
/// The propagator computes mandatory parts induced by current start domains,
/// rejects profiles that exceed capacity, and removes starts that would exceed
/// capacity when combined with other tasks' mandatory load. It deliberately
/// does not claim edge-finding or energetic reasoning.
pub fn propagate_cumulative_timetable(
    tasks: &[CumulativeTask],
    capacity: i64,
) -> Result<CumulativePropagationReport, GlobalPropagationError> {
    if capacity <= 0 {
        return Err(GlobalPropagationError::InvalidCapacity);
    }
    if tasks.is_empty() {
        return Err(GlobalPropagationError::EmptyVariables);
    }
    let mut current = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| normalize_task(index, task))
        .collect::<Result<Vec<_>, _>>()?;
    let initial_count: usize = current.iter().map(|task| task.start_domain.len()).sum();
    let mut passes = 0_u64;
    let mut mandatory_peak_load = 0_i64;

    loop {
        passes = passes.saturating_add(1);
        let before = current.clone();
        let profile = mandatory_profile(&current)?;
        mandatory_peak_load = profile.values().copied().max().unwrap_or(0);
        if mandatory_peak_load > capacity {
            return Err(GlobalPropagationError::Infeasible);
        }

        for task_index in 0..current.len() {
            let candidates = current[task_index].start_domain.clone();
            let mut supported = Vec::new();
            for candidate in candidates {
                if cumulative_candidate_supported(&current, task_index, candidate, capacity)? {
                    supported.push(candidate);
                }
            }
            if supported.is_empty() {
                return Err(GlobalPropagationError::Infeasible);
            }
            current[task_index].start_domain = supported;
        }

        if current == before {
            break;
        }
    }

    let final_profile = mandatory_profile(&current)?;
    mandatory_peak_load = final_profile.values().copied().max().unwrap_or(0);
    let final_count: usize = current.iter().map(|task| task.start_domain.len()).sum();
    Ok(CumulativePropagationReport {
        tasks: current,
        removed_starts: u64::try_from(initial_count.saturating_sub(final_count))
            .expect("usize fits u64"),
        passes,
        mandatory_peak_load,
    })
}

fn normalize_domains(domains: &[Vec<i64>]) -> Result<Vec<Vec<i64>>, GlobalPropagationError> {
    domains
        .iter()
        .enumerate()
        .map(|(variable, domain)| {
            let mut values = domain.clone();
            values.sort_unstable();
            values.dedup();
            if values.is_empty() {
                Err(GlobalPropagationError::EmptyDomain { variable })
            } else {
                Ok(values)
            }
        })
        .collect()
}

fn has_perfect_matching(domains: &[Vec<i64>], fixed: Option<(usize, i64)>) -> bool {
    let mut value_owner: BTreeMap<i64, usize> = BTreeMap::new();
    for variable in 0..domains.len() {
        let candidates: Vec<i64> = match fixed {
            Some((fixed_variable, fixed_value)) if fixed_variable == variable => {
                if !domains[variable].contains(&fixed_value) {
                    return false;
                }
                vec![fixed_value]
            }
            _ => domains[variable].clone(),
        };
        let mut visited = BTreeMap::new();
        if !augment(
            variable,
            &candidates,
            domains,
            fixed,
            &mut value_owner,
            &mut visited,
        ) {
            return false;
        }
    }
    true
}

fn augment(
    variable: usize,
    candidates: &[i64],
    domains: &[Vec<i64>],
    fixed: Option<(usize, i64)>,
    value_owner: &mut BTreeMap<i64, usize>,
    visited: &mut BTreeMap<i64, bool>,
) -> bool {
    for candidate in candidates {
        if visited.insert(*candidate, true).is_some() {
            continue;
        }
        let previous_owner = value_owner.get(candidate).copied();
        match previous_owner {
            None => {
                value_owner.insert(*candidate, variable);
                return true;
            }
            Some(owner) => {
                if fixed.is_some_and(|(fixed_variable, fixed_value)| {
                    owner == fixed_variable && *candidate == fixed_value
                }) {
                    continue;
                }
                let owner_candidates: Vec<i64> = match fixed {
                    Some((fixed_variable, fixed_value)) if fixed_variable == owner => {
                        vec![fixed_value]
                    }
                    _ => domains[owner].clone(),
                };
                if augment(
                    owner,
                    &owner_candidates,
                    domains,
                    fixed,
                    value_owner,
                    visited,
                ) {
                    value_owner.insert(*candidate, variable);
                    return true;
                }
            }
        }
    }
    false
}

fn normalize_task(
    index: usize,
    task: &CumulativeTask,
) -> Result<CumulativeTask, GlobalPropagationError> {
    if task.duration <= 0 || task.demand <= 0 {
        return Err(GlobalPropagationError::InvalidTask);
    }
    let mut starts = task.start_domain.clone();
    starts.sort_unstable();
    starts.dedup();
    if starts.is_empty() {
        return Err(GlobalPropagationError::EmptyDomain { variable: index });
    }
    Ok(CumulativeTask {
        start_domain: starts,
        duration: task.duration,
        demand: task.demand,
    })
}

fn mandatory_profile(
    tasks: &[CumulativeTask],
) -> Result<BTreeMap<i64, i64>, GlobalPropagationError> {
    let mut profile = BTreeMap::new();
    for task in tasks {
        let earliest_start = *task.start_domain.first().expect("validated task domain");
        let latest_start = *task.start_domain.last().expect("validated task domain");
        let earliest_end = earliest_start
            .checked_add(task.duration)
            .ok_or(GlobalPropagationError::InvalidTask)?;
        if latest_start < earliest_end {
            for instant in latest_start..earliest_end {
                let load = profile.entry(instant).or_insert(0_i64);
                *load = load
                    .checked_add(task.demand)
                    .ok_or(GlobalPropagationError::InvalidTask)?;
            }
        }
    }
    Ok(profile)
}

fn cumulative_candidate_supported(
    tasks: &[CumulativeTask],
    task_index: usize,
    candidate_start: i64,
    capacity: i64,
) -> Result<bool, GlobalPropagationError> {
    let candidate_task = &tasks[task_index];
    let candidate_end = candidate_start
        .checked_add(candidate_task.duration)
        .ok_or(GlobalPropagationError::InvalidTask)?;
    let other_profile = mandatory_profile_excluding(tasks, task_index)?;
    for instant in candidate_start..candidate_end {
        let load = other_profile.get(&instant).copied().unwrap_or(0);
        if load
            .checked_add(candidate_task.demand)
            .ok_or(GlobalPropagationError::InvalidTask)?
            > capacity
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn mandatory_profile_excluding(
    tasks: &[CumulativeTask],
    excluded: usize,
) -> Result<BTreeMap<i64, i64>, GlobalPropagationError> {
    let mut profile = BTreeMap::new();
    for (index, task) in tasks.iter().enumerate() {
        if index == excluded {
            continue;
        }
        let earliest_start = *task.start_domain.first().expect("validated task domain");
        let latest_start = *task.start_domain.last().expect("validated task domain");
        let earliest_end = earliest_start
            .checked_add(task.duration)
            .ok_or(GlobalPropagationError::InvalidTask)?;
        if latest_start < earliest_end {
            for instant in latest_start..earliest_end {
                let load = profile.entry(instant).or_insert(0_i64);
                *load = load
                    .checked_add(task.demand)
                    .ok_or(GlobalPropagationError::InvalidTask)?;
            }
        }
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_different_gac_detects_hall_set_and_removes_unsupported_value() {
        let domains = vec![vec![1, 2], vec![1, 2], vec![1, 2, 3]];
        let report = propagate_all_different_gac(&domains).expect("GAC propagation");
        assert_eq!(report.domains[2], vec![3]);
        assert_eq!(report.removed_values, 2);
    }

    #[test]
    fn all_different_gac_rejects_hall_violation() {
        let domains = vec![vec![1, 2], vec![1, 2], vec![1, 2]];
        assert_eq!(
            propagate_all_different_gac(&domains),
            Err(GlobalPropagationError::Infeasible)
        );
    }

    #[test]
    fn cumulative_timetable_removes_start_overlapping_mandatory_load() {
        let tasks = vec![
            CumulativeTask {
                start_domain: vec![0],
                duration: 3,
                demand: 2,
            },
            CumulativeTask {
                start_domain: vec![0, 1, 3],
                duration: 2,
                demand: 2,
            },
        ];
        let report = propagate_cumulative_timetable(&tasks, 3).expect("timetable propagation");
        assert_eq!(report.tasks[1].start_domain, vec![3]);
        assert_eq!(report.removed_starts, 2);
    }
}
