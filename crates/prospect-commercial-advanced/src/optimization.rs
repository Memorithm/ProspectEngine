use core::fmt;
use prospect_commercial::PROBABILITY_SCALE_PPM;
use std::cmp::Ordering;

const MAX_BINARY_VARIABLES: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintRelation {
    LessOrEqual,
    Equal,
    GreaterOrEqual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryLinearConstraint {
    pub coefficients: Vec<i64>,
    pub relation: ConstraintRelation,
    pub rhs: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryLinearProblem {
    pub objective: Vec<i64>,
    pub constraints: Vec<BinaryLinearConstraint>,
    pub maximum_assignments: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryLinearSolution {
    pub assignment: Vec<bool>,
    pub objective_value: i128,
    pub enumerated_assignments: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptimizationError {
    EmptyProblem,
    TooManyBinaryVariables { actual: usize, maximum: usize },
    ConstraintWidthMismatch,
    EnumerationBudgetTooSmall { required: u64, maximum: u64 },
    NoFeasibleSolution,
    ArithmeticOverflow(&'static str),
    EmptyPopulation,
    ObjectiveWidthMismatch,
    InvalidSelectionLimit,
    ProbabilityMassMismatch { actual_ppm: u64 },
    InvalidTailPpm(u32),
    InvalidProbabilityInterval,
    AmbiguityMassInfeasible,
    PortfolioWidthMismatch,
    InvalidBudget,
}

impl fmt::Display for OptimizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProblem => formatter.write_str("optimization problem must not be empty"),
            Self::TooManyBinaryVariables { actual, maximum } => write!(
                formatter,
                "binary optimization has {actual} variables; bounded exact solver supports at most {maximum}"
            ),
            Self::ConstraintWidthMismatch => {
                formatter.write_str("constraint width must match objective width")
            }
            Self::EnumerationBudgetTooSmall { required, maximum } => write!(
                formatter,
                "exact enumeration requires {required} assignments but budget is {maximum}"
            ),
            Self::NoFeasibleSolution => formatter.write_str("no feasible solution exists"),
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while computing {operation}")
            }
            Self::EmptyPopulation => formatter.write_str("Pareto population must not be empty"),
            Self::ObjectiveWidthMismatch => {
                formatter.write_str("all Pareto candidates must have the same objective width")
            }
            Self::InvalidSelectionLimit => formatter.write_str("selection limit must be non-zero"),
            Self::ProbabilityMassMismatch { actual_ppm } => write!(
                formatter,
                "scenario probabilities must sum to {PROBABILITY_SCALE_PPM} ppm, got {actual_ppm} ppm"
            ),
            Self::InvalidTailPpm(value) => write!(
                formatter,
                "tail probability must be in 1..={PROBABILITY_SCALE_PPM} ppm, got {value}"
            ),
            Self::InvalidProbabilityInterval => {
                formatter.write_str("probability interval bounds are invalid")
            }
            Self::AmbiguityMassInfeasible => formatter.write_str(
                "probability intervals cannot contain a distribution with unit probability mass",
            ),
            Self::PortfolioWidthMismatch => {
                formatter.write_str("portfolio scenario width must match asset count")
            }
            Self::InvalidBudget => formatter.write_str("portfolio budget must be non-negative"),
        }
    }
}

impl std::error::Error for OptimizationError {}

impl BinaryLinearProblem {
    pub fn validate(&self) -> Result<(), OptimizationError> {
        if self.objective.is_empty() {
            return Err(OptimizationError::EmptyProblem);
        }
        if self.objective.len() > MAX_BINARY_VARIABLES {
            return Err(OptimizationError::TooManyBinaryVariables {
                actual: self.objective.len(),
                maximum: MAX_BINARY_VARIABLES,
            });
        }
        if self
            .constraints
            .iter()
            .any(|constraint| constraint.coefficients.len() != self.objective.len())
        {
            return Err(OptimizationError::ConstraintWidthMismatch);
        }
        let required = 1_u64
            .checked_shl(u32::try_from(self.objective.len()).expect("bounded width fits u32"))
            .ok_or(OptimizationError::ArithmeticOverflow("binary assignment count"))?;
        if required > self.maximum_assignments {
            return Err(OptimizationError::EnumerationBudgetTooSmall {
                required,
                maximum: self.maximum_assignments,
            });
        }
        Ok(())
    }
}

pub fn solve_binary_linear_exact(
    problem: &BinaryLinearProblem,
) -> Result<BinaryLinearSolution, OptimizationError> {
    problem.validate()?;
    let assignment_count = 1_u64 << problem.objective.len();
    let mut best: Option<BinaryLinearSolution> = None;

    for mask in 0..assignment_count {
        let assignment: Vec<bool> = (0..problem.objective.len())
            .map(|index| mask & (1_u64 << index) != 0)
            .collect();
        if !is_feasible(problem, &assignment)? {
            continue;
        }
        let objective_value = dot_binary(&problem.objective, &assignment, "objective")?;
        let candidate = BinaryLinearSolution {
            assignment,
            objective_value,
            enumerated_assignments: assignment_count,
        };
        if best.as_ref().is_none_or(|current| {
            candidate.objective_value > current.objective_value
                || (candidate.objective_value == current.objective_value
                    && candidate.assignment < current.assignment)
        }) {
            best = Some(candidate);
        }
    }

    best.ok_or(OptimizationError::NoFeasibleSolution)
}

fn is_feasible(
    problem: &BinaryLinearProblem,
    assignment: &[bool],
) -> Result<bool, OptimizationError> {
    for constraint in &problem.constraints {
        let lhs = dot_binary(&constraint.coefficients, assignment, "constraint")?;
        let rhs = i128::from(constraint.rhs);
        let satisfied = match constraint.relation {
            ConstraintRelation::LessOrEqual => lhs <= rhs,
            ConstraintRelation::Equal => lhs == rhs,
            ConstraintRelation::GreaterOrEqual => lhs >= rhs,
        };
        if !satisfied {
            return Ok(false);
        }
    }
    Ok(true)
}

fn dot_binary(
    coefficients: &[i64],
    assignment: &[bool],
    operation: &'static str,
) -> Result<i128, OptimizationError> {
    coefficients
        .iter()
        .zip(assignment)
        .filter(|(_, selected)| **selected)
        .try_fold(0_i128, |sum, (coefficient, _)| {
            sum.checked_add(i128::from(*coefficient))
                .ok_or(OptimizationError::ArithmeticOverflow(operation))
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveDirection {
    Maximize,
    Minimize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParetoCandidate {
    pub id: String,
    pub objectives: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Nsga2SelectedCandidate {
    pub index: usize,
    pub rank: usize,
    pub crowding_distance: f64,
}

pub fn non_dominated_fronts(
    population: &[ParetoCandidate],
    directions: &[ObjectiveDirection],
) -> Result<Vec<Vec<usize>>, OptimizationError> {
    validate_population(population, directions)?;
    let n = population.len();
    let mut domination_count = vec![0_usize; n];
    let mut dominates_set = vec![Vec::<usize>::new(); n];
    let mut first_front = Vec::new();

    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            if dominates(&population[i], &population[j], directions) {
                dominates_set[i].push(j);
            } else if dominates(&population[j], &population[i], directions) {
                domination_count[i] += 1;
            }
        }
        if domination_count[i] == 0 {
            first_front.push(i);
        }
    }

    let mut fronts = Vec::new();
    let mut current = first_front;
    while !current.is_empty() {
        current.sort_unstable();
        let mut next = Vec::new();
        for &i in &current {
            for &j in &dominates_set[i] {
                domination_count[j] -= 1;
                if domination_count[j] == 0 {
                    next.push(j);
                }
            }
        }
        next.sort_unstable();
        next.dedup();
        fronts.push(current);
        current = next;
    }
    Ok(fronts)
}

pub fn nsga2_environmental_select(
    population: &[ParetoCandidate],
    directions: &[ObjectiveDirection],
    limit: usize,
) -> Result<Vec<Nsga2SelectedCandidate>, OptimizationError> {
    if limit == 0 {
        return Err(OptimizationError::InvalidSelectionLimit);
    }
    let fronts = non_dominated_fronts(population, directions)?;
    let mut selected = Vec::new();

    for (rank, front) in fronts.iter().enumerate() {
        let distances = crowding_distances(population, front, directions.len());
        let mut candidates: Vec<Nsga2SelectedCandidate> = front
            .iter()
            .copied()
            .map(|index| Nsga2SelectedCandidate {
                index,
                rank,
                crowding_distance: distances[index],
            })
            .collect();
        candidates.sort_by(|left, right| {
            right
                .crowding_distance
                .partial_cmp(&left.crowding_distance)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.index.cmp(&right.index))
        });
        let remaining = limit.saturating_sub(selected.len());
        selected.extend(candidates.into_iter().take(remaining));
        if selected.len() == limit || selected.len() == population.len() {
            break;
        }
    }
    Ok(selected)
}

fn validate_population(
    population: &[ParetoCandidate],
    directions: &[ObjectiveDirection],
) -> Result<(), OptimizationError> {
    if population.is_empty() || directions.is_empty() {
        return Err(OptimizationError::EmptyPopulation);
    }
    if population
        .iter()
        .any(|candidate| candidate.objectives.len() != directions.len())
    {
        return Err(OptimizationError::ObjectiveWidthMismatch);
    }
    Ok(())
}

fn dominates(
    left: &ParetoCandidate,
    right: &ParetoCandidate,
    directions: &[ObjectiveDirection],
) -> bool {
    let mut strictly_better = false;
    for ((left_value, right_value), direction) in left
        .objectives
        .iter()
        .zip(&right.objectives)
        .zip(directions)
    {
        let ordering = match direction {
            ObjectiveDirection::Maximize => left_value.cmp(right_value),
            ObjectiveDirection::Minimize => right_value.cmp(left_value),
        };
        if ordering == Ordering::Less {
            return false;
        }
        if ordering == Ordering::Greater {
            strictly_better = true;
        }
    }
    strictly_better
}

fn crowding_distances(
    population: &[ParetoCandidate],
    front: &[usize],
    objective_count: usize,
) -> Vec<f64> {
    let mut distances = vec![0.0_f64; population.len()];
    if front.len() <= 2 {
        for &index in front {
            distances[index] = f64::INFINITY;
        }
        return distances;
    }

    for objective in 0..objective_count {
        let mut ordered = front.to_vec();
        ordered.sort_by_key(|index| population[*index].objectives[objective]);
        let first = ordered[0];
        let last = *ordered.last().expect("non-empty front");
        distances[first] = f64::INFINITY;
        distances[last] = f64::INFINITY;
        let minimum = population[first].objectives[objective] as f64;
        let maximum = population[last].objectives[objective] as f64;
        let range = maximum - minimum;
        if range == 0.0 {
            continue;
        }
        for window in ordered.windows(3) {
            let middle = window[1];
            if distances[middle].is_infinite() {
                continue;
            }
            let previous = population[window[0]].objectives[objective] as f64;
            let next = population[window[2]].objectives[objective] as f64;
            distances[middle] += (next - previous).abs() / range.abs();
        }
    }
    distances
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioOutcome {
    pub probability_ppm: u32,
    pub payoff_minor: i64,
    pub constraint_satisfied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioDecisionSummary {
    pub expected_payoff_weighted_minor_ppm: i128,
    pub expected_payoff_minor_trunc: i128,
    pub violation_probability_ppm: u32,
    pub downside_tail_mean_minor_trunc: i128,
}

pub fn summarize_scenarios(
    outcomes: &[ScenarioOutcome],
    downside_tail_ppm: u32,
) -> Result<ScenarioDecisionSummary, OptimizationError> {
    if outcomes.is_empty() {
        return Err(OptimizationError::EmptyProblem);
    }
    if downside_tail_ppm == 0 || downside_tail_ppm > PROBABILITY_SCALE_PPM {
        return Err(OptimizationError::InvalidTailPpm(downside_tail_ppm));
    }
    let actual_ppm: u64 = outcomes
        .iter()
        .map(|outcome| u64::from(outcome.probability_ppm))
        .sum();
    if actual_ppm != u64::from(PROBABILITY_SCALE_PPM) {
        return Err(OptimizationError::ProbabilityMassMismatch { actual_ppm });
    }
    let expected_payoff_weighted_minor_ppm = outcomes.iter().try_fold(0_i128, |sum, outcome| {
        let weighted = i128::from(outcome.payoff_minor)
            .checked_mul(i128::from(outcome.probability_ppm))
            .ok_or(OptimizationError::ArithmeticOverflow("weighted scenario payoff"))?;
        sum.checked_add(weighted)
            .ok_or(OptimizationError::ArithmeticOverflow("expected scenario payoff"))
    })?;
    let violation_probability_ppm = outcomes
        .iter()
        .filter(|outcome| !outcome.constraint_satisfied)
        .try_fold(0_u32, |sum, outcome| {
            sum.checked_add(outcome.probability_ppm)
                .ok_or(OptimizationError::ArithmeticOverflow("violation probability"))
        })?;

    let mut ordered = outcomes.to_vec();
    ordered.sort_by_key(|outcome| outcome.payoff_minor);
    let mut remaining = downside_tail_ppm;
    let mut weighted_tail = 0_i128;
    for outcome in ordered {
        if remaining == 0 {
            break;
        }
        let consumed = remaining.min(outcome.probability_ppm);
        weighted_tail = weighted_tail
            .checked_add(
                i128::from(outcome.payoff_minor)
                    .checked_mul(i128::from(consumed))
                    .ok_or(OptimizationError::ArithmeticOverflow("downside tail"))?,
            )
            .ok_or(OptimizationError::ArithmeticOverflow("downside tail"))?;
        remaining -= consumed;
    }

    Ok(ScenarioDecisionSummary {
        expected_payoff_weighted_minor_ppm,
        expected_payoff_minor_trunc: expected_payoff_weighted_minor_ppm
            / i128::from(PROBABILITY_SCALE_PPM),
        violation_probability_ppm,
        downside_tail_mean_minor_trunc: weighted_tail / i128::from(downside_tail_ppm),
    })
}

#[must_use]
pub fn chance_constraint_satisfied(
    summary: &ScenarioDecisionSummary,
    maximum_violation_probability_ppm: u32,
) -> bool {
    summary.violation_probability_ppm <= maximum_violation_probability_ppm
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbabilityIntervalScenario {
    pub lower_probability_ppm: u32,
    pub upper_probability_ppm: u32,
    pub payoff_minor: i64,
}

pub fn worst_case_expected_value_under_probability_intervals(
    scenarios: &[ProbabilityIntervalScenario],
) -> Result<i128, OptimizationError> {
    if scenarios.is_empty() {
        return Err(OptimizationError::EmptyProblem);
    }
    if scenarios.iter().any(|scenario| {
        scenario.lower_probability_ppm > scenario.upper_probability_ppm
            || scenario.upper_probability_ppm > PROBABILITY_SCALE_PPM
    }) {
        return Err(OptimizationError::InvalidProbabilityInterval);
    }
    let lower_sum: u64 = scenarios
        .iter()
        .map(|scenario| u64::from(scenario.lower_probability_ppm))
        .sum();
    let upper_sum: u64 = scenarios
        .iter()
        .map(|scenario| u64::from(scenario.upper_probability_ppm))
        .sum();
    let scale = u64::from(PROBABILITY_SCALE_PPM);
    if lower_sum > scale || upper_sum < scale {
        return Err(OptimizationError::AmbiguityMassInfeasible);
    }

    let mut probabilities: Vec<u32> = scenarios
        .iter()
        .map(|scenario| scenario.lower_probability_ppm)
        .collect();
    let mut remaining = u32::try_from(scale - lower_sum).expect("remaining mass fits u32");
    let mut order: Vec<usize> = (0..scenarios.len()).collect();
    order.sort_by_key(|index| (scenarios[*index].payoff_minor, *index));
    for index in order {
        if remaining == 0 {
            break;
        }
        let capacity = scenarios[index].upper_probability_ppm - probabilities[index];
        let allocated = remaining.min(capacity);
        probabilities[index] += allocated;
        remaining -= allocated;
    }
    if remaining != 0 {
        return Err(OptimizationError::AmbiguityMassInfeasible);
    }

    scenarios
        .iter()
        .zip(probabilities)
        .try_fold(0_i128, |sum, (scenario, probability)| {
            let weighted = i128::from(scenario.payoff_minor)
                .checked_mul(i128::from(probability))
                .ok_or(OptimizationError::ArithmeticOverflow("DRO payoff"))?;
            sum.checked_add(weighted)
                .ok_or(OptimizationError::ArithmeticOverflow("DRO expectation"))
        })
        .map(|weighted| weighted / i128::from(PROBABILITY_SCALE_PPM))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortfolioProblem {
    pub asset_costs_minor: Vec<i64>,
    pub scenario_probabilities_ppm: Vec<u32>,
    pub scenario_asset_payoffs_minor: Vec<Vec<i64>>,
    pub budget_minor: i64,
    pub maximum_assets: Option<usize>,
    pub maximum_assignments: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortfolioSolution {
    pub selected_assets: Vec<bool>,
    pub invested_minor: i128,
    pub expected_payoff_minor_trunc: i128,
    pub worst_scenario_payoff_minor: i128,
}

pub fn select_portfolio_exact(
    problem: &PortfolioProblem,
) -> Result<PortfolioSolution, OptimizationError> {
    if problem.asset_costs_minor.is_empty() {
        return Err(OptimizationError::EmptyProblem);
    }
    if problem.budget_minor < 0 || problem.asset_costs_minor.iter().any(|cost| *cost < 0) {
        return Err(OptimizationError::InvalidBudget);
    }
    if problem.asset_costs_minor.len() > MAX_BINARY_VARIABLES {
        return Err(OptimizationError::TooManyBinaryVariables {
            actual: problem.asset_costs_minor.len(),
            maximum: MAX_BINARY_VARIABLES,
        });
    }
    if problem
        .scenario_asset_payoffs_minor
        .iter()
        .any(|row| row.len() != problem.asset_costs_minor.len())
        || problem.scenario_asset_payoffs_minor.len() != problem.scenario_probabilities_ppm.len()
    {
        return Err(OptimizationError::PortfolioWidthMismatch);
    }
    let actual_probability: u64 = problem
        .scenario_probabilities_ppm
        .iter()
        .map(|value| u64::from(*value))
        .sum();
    if actual_probability != u64::from(PROBABILITY_SCALE_PPM) {
        return Err(OptimizationError::ProbabilityMassMismatch {
            actual_ppm: actual_probability,
        });
    }
    let assignment_count = 1_u64 << problem.asset_costs_minor.len();
    if assignment_count > problem.maximum_assignments {
        return Err(OptimizationError::EnumerationBudgetTooSmall {
            required: assignment_count,
            maximum: problem.maximum_assignments,
        });
    }

    let mut best: Option<PortfolioSolution> = None;
    for mask in 0..assignment_count {
        let selected: Vec<bool> = (0..problem.asset_costs_minor.len())
            .map(|index| mask & (1_u64 << index) != 0)
            .collect();
        if problem
            .maximum_assets
            .is_some_and(|maximum| selected.iter().filter(|value| **value).count() > maximum)
        {
            continue;
        }
        let invested = dot_binary(&problem.asset_costs_minor, &selected, "portfolio cost")?;
        if invested > i128::from(problem.budget_minor) {
            continue;
        }
        let mut expected_weighted = 0_i128;
        let mut worst = i128::MAX;
        for (scenario_index, row) in problem.scenario_asset_payoffs_minor.iter().enumerate() {
            let payoff = dot_binary(row, &selected, "portfolio payoff")?;
            worst = worst.min(payoff);
            let weighted = payoff
                .checked_mul(i128::from(problem.scenario_probabilities_ppm[scenario_index]))
                .ok_or(OptimizationError::ArithmeticOverflow("portfolio expectation"))?;
            expected_weighted = expected_weighted
                .checked_add(weighted)
                .ok_or(OptimizationError::ArithmeticOverflow("portfolio expectation"))?;
        }
        let candidate = PortfolioSolution {
            selected_assets: selected,
            invested_minor: invested,
            expected_payoff_minor_trunc: expected_weighted
                / i128::from(PROBABILITY_SCALE_PPM),
            worst_scenario_payoff_minor: if worst == i128::MAX { 0 } else { worst },
        };
        if best.as_ref().is_none_or(|current| {
            candidate.expected_payoff_minor_trunc > current.expected_payoff_minor_trunc
                || (candidate.expected_payoff_minor_trunc == current.expected_payoff_minor_trunc
                    && candidate.worst_scenario_payoff_minor > current.worst_scenario_payoff_minor)
        }) {
            best = Some(candidate);
        }
    }
    best.ok_or(OptimizationError::NoFeasibleSolution)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_binary_solver_obeys_constraints() {
        let problem = BinaryLinearProblem {
            objective: vec![8, 7, 6],
            constraints: vec![BinaryLinearConstraint {
                coefficients: vec![5, 4, 3],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 7,
            }],
            maximum_assignments: 8,
        };
        let solution = solve_binary_linear_exact(&problem).expect("bounded problem is solvable");
        assert_eq!(solution.assignment, vec![false, true, true]);
        assert_eq!(solution.objective_value, 13);
    }

    #[test]
    fn pareto_front_and_nsga_selection_preserve_extremes() {
        let population = vec![
            ParetoCandidate { id: "a".into(), objectives: vec![10, 1] },
            ParetoCandidate { id: "b".into(), objectives: vec![7, 7] },
            ParetoCandidate { id: "c".into(), objectives: vec![1, 10] },
            ParetoCandidate { id: "d".into(), objectives: vec![5, 5] },
        ];
        let directions = [ObjectiveDirection::Maximize, ObjectiveDirection::Maximize];
        let fronts = non_dominated_fronts(&population, &directions).expect("valid population");
        assert_eq!(fronts[0], vec![0, 1, 2]);
        let selected = nsga2_environmental_select(&population, &directions, 2)
            .expect("selection succeeds");
        assert!(selected.iter().all(|entry| entry.rank == 0));
        assert!(selected.iter().any(|entry| entry.index == 0));
        assert!(selected.iter().any(|entry| entry.index == 2));
    }

    #[test]
    fn chance_constraints_and_downside_tail_are_explicit() {
        let summary = summarize_scenarios(
            &[
                ScenarioOutcome { probability_ppm: 200_000, payoff_minor: -100, constraint_satisfied: false },
                ScenarioOutcome { probability_ppm: 800_000, payoff_minor: 100, constraint_satisfied: true },
            ],
            200_000,
        )
        .expect("valid scenarios");
        assert_eq!(summary.expected_payoff_minor_trunc, 60);
        assert_eq!(summary.downside_tail_mean_minor_trunc, -100);
        assert!(chance_constraint_satisfied(&summary, 200_000));
        assert!(!chance_constraint_satisfied(&summary, 199_999));
    }

    #[test]
    fn dro_moves_mass_toward_worst_payoff() {
        let worst = worst_case_expected_value_under_probability_intervals(&[
            ProbabilityIntervalScenario { lower_probability_ppm: 200_000, upper_probability_ppm: 800_000, payoff_minor: -100 },
            ProbabilityIntervalScenario { lower_probability_ppm: 200_000, upper_probability_ppm: 800_000, payoff_minor: 100 },
        ])
        .expect("ambiguity set is feasible");
        assert_eq!(worst, -60);
    }

    #[test]
    fn portfolio_selection_respects_budget_and_scenarios() {
        let solution = select_portfolio_exact(&PortfolioProblem {
            asset_costs_minor: vec![60, 50, 40],
            scenario_probabilities_ppm: vec![500_000, 500_000],
            scenario_asset_payoffs_minor: vec![vec![100, 50, 20], vec![-20, 60, 30]],
            budget_minor: 100,
            maximum_assets: Some(2),
            maximum_assignments: 8,
        })
        .expect("portfolio is solvable");
        assert_eq!(solution.selected_assets, vec![false, true, true]);
        assert_eq!(solution.invested_minor, 90);
        assert_eq!(solution.expected_payoff_minor_trunc, 80);
    }
}
