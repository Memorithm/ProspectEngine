use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;

const MAX_WORKFORCE_TASKS: usize = 16;
const MAX_ROUTE_NODES: usize = 11;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationsError {
    EmptyProblem,
    ProbabilityMassMismatch { actual_ppm: u64 },
    InvalidServiceLevel(u32),
    NegativeField(&'static str),
    ArithmeticOverflow(&'static str),
    TooManyTasks { actual: usize, maximum: usize },
    WorkerWidthMismatch,
    NoFeasibleAssignment,
    InvalidDistanceMatrix,
    TooManyRouteNodes { actual: usize, maximum: usize },
}

impl fmt::Display for OperationsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProblem => formatter.write_str("operations problem must not be empty"),
            Self::ProbabilityMassMismatch { actual_ppm } => write!(
                formatter,
                "probabilities must sum to {PROBABILITY_SCALE_PPM} ppm, got {actual_ppm} ppm"
            ),
            Self::InvalidServiceLevel(value) => write!(
                formatter,
                "service level must be in 1..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::NegativeField(field) => write!(formatter, "{field} must be non-negative"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
            Self::TooManyTasks { actual, maximum } => write!(
                formatter,
                "workforce problem has {actual} tasks; bounded exact solver supports at most {maximum}"
            ),
            Self::WorkerWidthMismatch => formatter.write_str(
                "every task must define one optional assignment cost per worker",
            ),
            Self::NoFeasibleAssignment => formatter.write_str("no feasible workforce assignment exists"),
            Self::InvalidDistanceMatrix => {
                formatter.write_str("routing distance matrix must be square with non-negative costs")
            }
            Self::TooManyRouteNodes { actual, maximum } => write!(
                formatter,
                "routing problem has {actual} nodes; bounded exact solver supports at most {maximum}"
            ),
        }
    }
}

impl std::error::Error for OperationsError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeadTimeDemandOutcome {
    pub probability_ppm: u32,
    pub demand_units: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplenishmentPolicy {
    pub reorder_point_units: u64,
    pub safety_stock_units: i128,
    pub expected_lead_time_demand_units_trunc: i128,
    pub achieved_service_probability_ppm: u32,
}

pub fn replenishment_policy_for_service_level(
    outcomes: &[LeadTimeDemandOutcome],
    target_service_probability_ppm: u32,
) -> Result<ReplenishmentPolicy, OperationsError> {
    if outcomes.is_empty() {
        return Err(OperationsError::EmptyProblem);
    }
    if target_service_probability_ppm == 0
        || target_service_probability_ppm > PROBABILITY_SCALE_PPM
    {
        return Err(OperationsError::InvalidServiceLevel(
            target_service_probability_ppm,
        ));
    }
    let actual_ppm: u64 = outcomes
        .iter()
        .map(|outcome| u64::from(outcome.probability_ppm))
        .sum();
    if actual_ppm != u64::from(PROBABILITY_SCALE_PPM) {
        return Err(OperationsError::ProbabilityMassMismatch { actual_ppm });
    }
    let expected_weighted = outcomes.iter().try_fold(0_i128, |sum, outcome| {
        let term = i128::from(outcome.demand_units)
            .checked_mul(i128::from(outcome.probability_ppm))
            .ok_or(OperationsError::ArithmeticOverflow("lead-time demand expectation"))?;
        sum.checked_add(term)
            .ok_or(OperationsError::ArithmeticOverflow("lead-time demand expectation"))
    })?;
    let expected = expected_weighted / i128::from(PROBABILITY_SCALE_PPM);
    let mut ordered = outcomes.to_vec();
    ordered.sort_by_key(|outcome| outcome.demand_units);
    let mut cumulative = 0_u32;
    let mut reorder_point = 0_u64;
    for outcome in ordered {
        cumulative = cumulative
            .checked_add(outcome.probability_ppm)
            .ok_or(OperationsError::ArithmeticOverflow("service probability"))?;
        reorder_point = outcome.demand_units;
        if cumulative >= target_service_probability_ppm {
            break;
        }
    }
    Ok(ReplenishmentPolicy {
        reorder_point_units: reorder_point,
        safety_stock_units: i128::from(reorder_point) - expected,
        expected_lead_time_demand_units_trunc: expected,
        achieved_service_probability_ppm: cumulative,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkforceTask {
    /// `None` means the worker is ineligible for the task.
    pub worker_cost_minor: Vec<Option<i64>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkforceProblem {
    pub tasks: Vec<WorkforceTask>,
    pub worker_task_capacities: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkforceSolution {
    pub worker_by_task: Vec<usize>,
    pub total_cost_minor: i128,
}

pub fn solve_workforce_assignment_exact(
    problem: &WorkforceProblem,
) -> Result<WorkforceSolution, OperationsError> {
    if problem.tasks.is_empty() || problem.worker_task_capacities.is_empty() {
        return Err(OperationsError::EmptyProblem);
    }
    if problem.tasks.len() > MAX_WORKFORCE_TASKS {
        return Err(OperationsError::TooManyTasks {
            actual: problem.tasks.len(),
            maximum: MAX_WORKFORCE_TASKS,
        });
    }
    let worker_count = problem.worker_task_capacities.len();
    if problem
        .tasks
        .iter()
        .any(|task| task.worker_cost_minor.len() != worker_count)
    {
        return Err(OperationsError::WorkerWidthMismatch);
    }
    if problem.tasks.iter().flat_map(|task| &task.worker_cost_minor).flatten().any(|cost| *cost < 0) {
        return Err(OperationsError::NegativeField("workforce assignment cost"));
    }
    let mut capacities = problem.worker_task_capacities.clone();
    let mut current = vec![0_usize; problem.tasks.len()];
    let mut best: Option<WorkforceSolution> = None;
    workforce_dfs(problem, 0, &mut capacities, &mut current, 0, &mut best)?;
    best.ok_or(OperationsError::NoFeasibleAssignment)
}

fn workforce_dfs(
    problem: &WorkforceProblem,
    task_index: usize,
    capacities: &mut [u32],
    current: &mut [usize],
    current_cost: i128,
    best: &mut Option<WorkforceSolution>,
) -> Result<(), OperationsError> {
    if best
        .as_ref()
        .is_some_and(|solution| current_cost >= solution.total_cost_minor)
    {
        return Ok(());
    }
    if task_index == problem.tasks.len() {
        *best = Some(WorkforceSolution {
            worker_by_task: current.to_vec(),
            total_cost_minor: current_cost,
        });
        return Ok(());
    }
    for worker in 0..capacities.len() {
        if capacities[worker] == 0 {
            continue;
        }
        let Some(cost) = problem.tasks[task_index].worker_cost_minor[worker] else {
            continue;
        };
        capacities[worker] -= 1;
        current[task_index] = worker;
        let next_cost = current_cost
            .checked_add(i128::from(cost))
            .ok_or(OperationsError::ArithmeticOverflow("workforce cost"))?;
        workforce_dfs(problem, task_index + 1, capacities, current, next_cost, best)?;
        capacities[worker] += 1;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutingProblem {
    pub distance_minor: Vec<Vec<i64>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutingSolution {
    /// Closed route starting and ending at node 0.
    pub route: Vec<usize>,
    pub total_distance_minor: i128,
}

pub fn solve_route_exact(problem: &RoutingProblem) -> Result<RoutingSolution, OperationsError> {
    let n = problem.distance_minor.len();
    if n == 0 {
        return Err(OperationsError::EmptyProblem);
    }
    if n > MAX_ROUTE_NODES {
        return Err(OperationsError::TooManyRouteNodes {
            actual: n,
            maximum: MAX_ROUTE_NODES,
        });
    }
    if problem
        .distance_minor
        .iter()
        .any(|row| row.len() != n || row.iter().any(|distance| *distance < 0))
    {
        return Err(OperationsError::InvalidDistanceMatrix);
    }
    if n == 1 {
        return Ok(RoutingSolution {
            route: vec![0, 0],
            total_distance_minor: 0,
        });
    }
    let mut visited = vec![false; n];
    visited[0] = true;
    let mut route = vec![0_usize];
    let mut best: Option<RoutingSolution> = None;
    route_dfs(problem, 0, &mut visited, &mut route, 0, &mut best)?;
    best.ok_or(OperationsError::EmptyProblem)
}

fn route_dfs(
    problem: &RoutingProblem,
    current: usize,
    visited: &mut [bool],
    route: &mut Vec<usize>,
    cost: i128,
    best: &mut Option<RoutingSolution>,
) -> Result<(), OperationsError> {
    if best
        .as_ref()
        .is_some_and(|solution| cost >= solution.total_distance_minor)
    {
        return Ok(());
    }
    if route.len() == visited.len() {
        let total = cost
            .checked_add(i128::from(problem.distance_minor[current][0]))
            .ok_or(OperationsError::ArithmeticOverflow("route total"))?;
        if best
            .as_ref()
            .is_none_or(|solution| total < solution.total_distance_minor)
        {
            let mut closed = route.clone();
            closed.push(0);
            *best = Some(RoutingSolution {
                route: closed,
                total_distance_minor: total,
            });
        }
        return Ok(());
    }
    for next in 1..visited.len() {
        if visited[next] {
            continue;
        }
        visited[next] = true;
        route.push(next);
        let next_cost = cost
            .checked_add(i128::from(problem.distance_minor[current][next]))
            .ok_or(OperationsError::ArithmeticOverflow("route cost"))?;
        route_dfs(problem, next, visited, route, next_cost, best)?;
        route.pop();
        visited[next] = false;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealOptionAction {
    ExerciseNow,
    Defer,
    Expand,
    Abandon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealOptionScenario {
    pub probability_ppm: u32,
    pub exercise_now_value_minor: i64,
    pub defer_value_minor: i64,
    pub expand_value_minor: i64,
    pub abandon_value_minor: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealOptionDecision {
    pub action: RealOptionAction,
    pub expected_value_minor_trunc: i128,
}

pub fn select_real_option_action(
    scenarios: &[RealOptionScenario],
) -> Result<RealOptionDecision, OperationsError> {
    if scenarios.is_empty() {
        return Err(OperationsError::EmptyProblem);
    }
    let actual_ppm: u64 = scenarios
        .iter()
        .map(|scenario| u64::from(scenario.probability_ppm))
        .sum();
    if actual_ppm != u64::from(PROBABILITY_SCALE_PPM) {
        return Err(OperationsError::ProbabilityMassMismatch { actual_ppm });
    }
    let actions = [
        RealOptionAction::ExerciseNow,
        RealOptionAction::Defer,
        RealOptionAction::Expand,
        RealOptionAction::Abandon,
    ];
    let mut best: Option<RealOptionDecision> = None;
    for action in actions {
        let weighted = scenarios.iter().try_fold(0_i128, |sum, scenario| {
            let value = match action {
                RealOptionAction::ExerciseNow => scenario.exercise_now_value_minor,
                RealOptionAction::Defer => scenario.defer_value_minor,
                RealOptionAction::Expand => scenario.expand_value_minor,
                RealOptionAction::Abandon => scenario.abandon_value_minor,
            };
            let term = i128::from(value)
                .checked_mul(i128::from(scenario.probability_ppm))
                .ok_or(OperationsError::ArithmeticOverflow("real option value"))?;
            sum.checked_add(term)
                .ok_or(OperationsError::ArithmeticOverflow("real option value"))
        })?;
        let candidate = RealOptionDecision {
            action,
            expected_value_minor_trunc: weighted / i128::from(PROBABILITY_SCALE_PPM),
        };
        if best.as_ref().is_none_or(|current| {
            candidate.expected_value_minor_trunc > current.expected_value_minor_trunc
        }) {
            best = Some(candidate);
        }
    }
    Ok(best.expect("four actions are always evaluated"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replenishment_uses_requested_quantile() {
        let policy = replenishment_policy_for_service_level(
            &[
                LeadTimeDemandOutcome { probability_ppm: 500_000, demand_units: 10 },
                LeadTimeDemandOutcome { probability_ppm: 300_000, demand_units: 20 },
                LeadTimeDemandOutcome { probability_ppm: 200_000, demand_units: 30 },
            ],
            800_000,
        )
        .expect("valid distribution");
        assert_eq!(policy.reorder_point_units, 20);
        assert_eq!(policy.expected_lead_time_demand_units_trunc, 17);
        assert_eq!(policy.safety_stock_units, 3);
    }

    #[test]
    fn workforce_solver_finds_minimum_cost_feasible_assignment() {
        let solution = solve_workforce_assignment_exact(&WorkforceProblem {
            tasks: vec![
                WorkforceTask { worker_cost_minor: vec![Some(10), Some(30)] },
                WorkforceTask { worker_cost_minor: vec![Some(20), Some(5)] },
            ],
            worker_task_capacities: vec![1, 1],
        })
        .expect("feasible assignment");
        assert_eq!(solution.worker_by_task, vec![0, 1]);
        assert_eq!(solution.total_cost_minor, 15);
    }

    #[test]
    fn route_solver_returns_closed_exact_route() {
        let solution = solve_route_exact(&RoutingProblem {
            distance_minor: vec![
                vec![0, 10, 15, 20],
                vec![10, 0, 35, 25],
                vec![15, 35, 0, 30],
                vec![20, 25, 30, 0],
            ],
        })
        .expect("route exists");
        assert_eq!(solution.total_distance_minor, 80);
        assert_eq!(solution.route.first(), Some(&0));
        assert_eq!(solution.route.last(), Some(&0));
    }

    #[test]
    fn real_option_selects_best_expected_action() {
        let decision = select_real_option_action(&[
            RealOptionScenario {
                probability_ppm: 500_000,
                exercise_now_value_minor: 100,
                defer_value_minor: 150,
                expand_value_minor: 200,
                abandon_value_minor: 20,
            },
            RealOptionScenario {
                probability_ppm: 500_000,
                exercise_now_value_minor: -50,
                defer_value_minor: 20,
                expand_value_minor: -100,
                abandon_value_minor: 20,
            },
        ])
        .expect("valid scenarios");
        assert_eq!(decision.action, RealOptionAction::Defer);
        assert_eq!(decision.expected_value_minor_trunc, 85);
    }
}
