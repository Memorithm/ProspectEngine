use crate::bounded_linear_program::{
    solve_bounded_linear_program, BoundedLinearError, BoundedLinearProgram, LinearVariable,
};
use crate::general_linear_program::GeneralLinearConstraint;
use crate::optimization::ConstraintRelation;
use core::fmt;

const MAX_EXACT_F64_INTEGER: f64 = 9_007_199_254_740_992.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixedVariableKind {
    Continuous,
    Integer,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixedVariable {
    pub lower: f64,
    pub upper: f64,
    pub objective_coefficient: f64,
    pub kind: MixedVariableKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MixedIntegerProblem {
    pub variables: Vec<MixedVariable>,
    pub constraints: Vec<GeneralLinearConstraint>,
    pub lp_tolerance: f64,
    pub integrality_tolerance: f64,
    pub maximum_nodes: u64,
    pub maximum_lp_iterations_per_node: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MixedIntegerSolution {
    pub values: Vec<f64>,
    pub objective_value: f64,
    pub explored_nodes: u64,
    pub pruned_nodes: u64,
    pub infeasible_nodes: u64,
    pub relaxation_solves: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MixedIntegerError {
    EmptyProblem,
    NoIntegerVariable,
    InvalidBounds { variable: usize },
    ConstraintWidthMismatch,
    NonFiniteInput,
    InvalidTolerance,
    InvalidNodeBudget,
    InvalidLpIterationBudget,
    NodeBudgetExceeded { explored_nodes: u64 },
    NoFeasibleSolution,
    Relaxation(BoundedLinearError),
    NumericalBreakdown,
}

impl fmt::Display for MixedIntegerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProblem => formatter.write_str("mixed-integer problem must not be empty"),
            Self::NoIntegerVariable => formatter.write_str(
                "mixed-integer branch-and-bound requires at least one integer variable",
            ),
            Self::InvalidBounds { variable } => write!(
                formatter,
                "mixed-integer variable {variable} has invalid finite bounds"
            ),
            Self::ConstraintWidthMismatch => {
                formatter.write_str("mixed-integer constraint width must match variable width")
            }
            Self::NonFiniteInput => formatter.write_str("mixed-integer inputs must be finite"),
            Self::InvalidTolerance => formatter.write_str(
                "mixed-integer LP tolerance must be positive and integrality tolerance must lie in (0, 0.5)",
            ),
            Self::InvalidNodeBudget => {
                formatter.write_str("mixed-integer node budget must be non-zero")
            }
            Self::InvalidLpIterationBudget => formatter.write_str(
                "mixed-integer LP iteration budget per node must be non-zero",
            ),
            Self::NodeBudgetExceeded { explored_nodes } => write!(
                formatter,
                "mixed-integer node budget exceeded after {explored_nodes} nodes"
            ),
            Self::NoFeasibleSolution => {
                formatter.write_str("mixed-integer problem has no feasible integer solution")
            }
            Self::Relaxation(error) => {
                write!(formatter, "mixed-integer LP relaxation failed: {error}")
            }
            Self::NumericalBreakdown => {
                formatter.write_str("mixed-integer search encountered numerical breakdown")
            }
        }
    }
}

impl std::error::Error for MixedIntegerError {}

#[derive(Clone, Copy, Debug, PartialEq)]
struct NodeBound {
    lower: f64,
    upper: f64,
}

struct SearchState<'a> {
    problem: &'a MixedIntegerProblem,
    best_values: Option<Vec<f64>>,
    best_objective: f64,
    explored_nodes: u64,
    pruned_nodes: u64,
    infeasible_nodes: u64,
    relaxation_solves: u64,
}

/// Solve a bounded mixed continuous/integer maximization problem using LP
/// relaxation branch-and-bound.
///
/// Every variable must have finite caller-supplied bounds. Integer bounds must
/// themselves be exact integers in the exact-`f64` range. Each node is solved
/// by the bounded/free-variable LP front-end backed by the deterministic
/// two-phase simplex. Integer branching fixes `x_i <= floor(x*)` versus
/// `x_i >= ceil(x*)`. A relaxation that is integral within tolerance is snapped
/// to exact integer coordinates and revalidated in the original problem before
/// it may become an incumbent. If that near-integral snap is not primal-feasible,
/// the solver branches on a still non-exact integer coordinate rather than
/// misclassifying the node as a numerical failure. The solver fails closed on
/// node-budget exhaustion rather than returning an incumbent as if optimality
/// had been proven.
///
/// This is a real LP-relaxation branch-and-bound core, but it deliberately does
/// not claim presolve, cuts, pseudocosts, strong branching, incumbent
/// heuristics, parallel trees, or industrial MIP numerical robustness.
pub fn solve_mixed_integer_branch_and_bound(
    problem: &MixedIntegerProblem,
) -> Result<MixedIntegerSolution, MixedIntegerError> {
    validate(problem)?;
    let bounds = problem
        .variables
        .iter()
        .map(|variable| NodeBound {
            lower: variable.lower,
            upper: variable.upper,
        })
        .collect();
    let mut state = SearchState {
        problem,
        best_values: None,
        best_objective: f64::NEG_INFINITY,
        explored_nodes: 0,
        pruned_nodes: 0,
        infeasible_nodes: 0,
        relaxation_solves: 0,
    };
    search_node(&mut state, bounds)?;
    let values = state
        .best_values
        .ok_or(MixedIntegerError::NoFeasibleSolution)?;
    Ok(MixedIntegerSolution {
        values,
        objective_value: state.best_objective,
        explored_nodes: state.explored_nodes,
        pruned_nodes: state.pruned_nodes,
        infeasible_nodes: state.infeasible_nodes,
        relaxation_solves: state.relaxation_solves,
    })
}

fn search_node(
    state: &mut SearchState<'_>,
    bounds: Vec<NodeBound>,
) -> Result<(), MixedIntegerError> {
    if state.explored_nodes >= state.problem.maximum_nodes {
        return Err(MixedIntegerError::NodeBudgetExceeded {
            explored_nodes: state.explored_nodes,
        });
    }
    state.explored_nodes = state.explored_nodes.saturating_add(1);

    if bounds
        .iter()
        .any(|bound| bound.lower > bound.upper + state.problem.lp_tolerance)
    {
        state.infeasible_nodes = state.infeasible_nodes.saturating_add(1);
        return Ok(());
    }

    let relaxation_problem = BoundedLinearProgram {
        variables: state
            .problem
            .variables
            .iter()
            .zip(&bounds)
            .map(|(variable, bound)| LinearVariable {
                lower: Some(bound.lower),
                upper: Some(bound.upper),
                objective_coefficient: variable.objective_coefficient,
            })
            .collect(),
        constraints: state.problem.constraints.clone(),
        tolerance: state.problem.lp_tolerance,
        maximum_iterations: state.problem.maximum_lp_iterations_per_node,
    };
    state.relaxation_solves = state.relaxation_solves.saturating_add(1);
    let relaxation = match solve_bounded_linear_program(&relaxation_problem) {
        Ok(solution) => solution,
        Err(BoundedLinearError::General(
            crate::general_linear_program::GeneralLinearError::Infeasible,
        )) => {
            state.infeasible_nodes = state.infeasible_nodes.saturating_add(1);
            return Ok(());
        }
        Err(error) => return Err(MixedIntegerError::Relaxation(error)),
    };

    if relaxation.maximum_primal_violation > state.problem.lp_tolerance * 10.0 {
        return Err(MixedIntegerError::NumericalBreakdown);
    }
    if state.best_values.is_some()
        && relaxation.objective_value <= state.best_objective + state.problem.lp_tolerance
    {
        state.pruned_nodes = state.pruned_nodes.saturating_add(1);
        return Ok(());
    }

    if let Some((branch_variable, branch_value, _)) = choose_fractional_variable(
        state.problem,
        &relaxation.values,
        state.problem.integrality_tolerance,
    ) {
        return branch_on_variable(state, bounds, branch_variable, branch_value);
    }

    let candidate = snap_integral_values(state.problem, &relaxation.values)?;
    let violation = maximum_primal_violation(state.problem, &candidate);
    if !violation.is_finite() {
        return Err(MixedIntegerError::NumericalBreakdown);
    }
    if violation > state.problem.lp_tolerance * 10.0 {
        // A user may deliberately choose an integrality tolerance looser than
        // primal feasibility. In that case a near-integer LP point can snap to
        // an infeasible integer. It is still a legitimate branch point: search
        // the exact floor/ceil children instead of aborting a feasible MIP.
        let (branch_variable, branch_value, _) = choose_fractional_variable(
            state.problem,
            &relaxation.values,
            0.0,
        )
        .ok_or(MixedIntegerError::NumericalBreakdown)?;
        return branch_on_variable(state, bounds, branch_variable, branch_value);
    }

    let objective = original_objective(state.problem, &candidate);
    if !objective.is_finite() {
        return Err(MixedIntegerError::NumericalBreakdown);
    }
    if objective > state.best_objective + state.problem.lp_tolerance
        || ((objective - state.best_objective).abs() <= state.problem.lp_tolerance
            && state
                .best_values
                .as_ref()
                .is_none_or(|existing| lexicographically_less(&candidate, existing)))
    {
        state.best_objective = objective;
        state.best_values = Some(candidate);
    }
    Ok(())
}

fn branch_on_variable(
    state: &mut SearchState<'_>,
    bounds: Vec<NodeBound>,
    branch_variable: usize,
    branch_value: f64,
) -> Result<(), MixedIntegerError> {
    let floor = branch_value.floor();
    let ceil = branch_value.ceil();
    if floor == ceil {
        return Err(MixedIntegerError::NumericalBreakdown);
    }
    let mut lower_child = bounds.clone();
    lower_child[branch_variable].upper = lower_child[branch_variable].upper.min(floor);
    let mut upper_child = bounds;
    upper_child[branch_variable].lower = upper_child[branch_variable].lower.max(ceil);

    // Explore the child nearer the LP value first, deterministically breaking
    // ties toward the lower branch. This often establishes an incumbent early.
    let lower_distance = branch_value - floor;
    let upper_distance = ceil - branch_value;
    if lower_distance <= upper_distance {
        search_node(state, lower_child)?;
        search_node(state, upper_child)?;
    } else {
        search_node(state, upper_child)?;
        search_node(state, lower_child)?;
    }
    Ok(())
}

fn choose_fractional_variable(
    problem: &MixedIntegerProblem,
    values: &[f64],
    minimum_distance: f64,
) -> Option<(usize, f64, f64)> {
    problem
        .variables
        .iter()
        .zip(values)
        .enumerate()
        .filter(|(_, (variable, _))| variable.kind == MixedVariableKind::Integer)
        .filter_map(|(index, (_, value))| {
            let distance = (*value - value.round()).abs();
            (distance > minimum_distance).then_some((index, *value, distance))
        })
        .max_by(|left, right| {
            left.2
                .total_cmp(&right.2)
                .then_with(|| right.0.cmp(&left.0))
        })
}

fn snap_integral_values(
    problem: &MixedIntegerProblem,
    values: &[f64],
) -> Result<Vec<f64>, MixedIntegerError> {
    if values.len() != problem.variables.len() {
        return Err(MixedIntegerError::NumericalBreakdown);
    }
    let snapped: Vec<f64> = problem
        .variables
        .iter()
        .zip(values)
        .map(|(variable, value)| match variable.kind {
            MixedVariableKind::Continuous => *value,
            MixedVariableKind::Integer => value.round(),
        })
        .collect();
    if snapped.iter().any(|value| !value.is_finite()) {
        return Err(MixedIntegerError::NumericalBreakdown);
    }
    for (variable, value) in problem.variables.iter().zip(&snapped) {
        if *value < variable.lower - problem.lp_tolerance
            || *value > variable.upper + problem.lp_tolerance
        {
            return Err(MixedIntegerError::NumericalBreakdown);
        }
    }
    Ok(snapped)
}

fn maximum_primal_violation(problem: &MixedIntegerProblem, values: &[f64]) -> f64 {
    let mut maximum = 0.0_f64;
    for (variable, value) in problem.variables.iter().zip(values) {
        maximum = maximum.max((variable.lower - *value).max(0.0));
        maximum = maximum.max((*value - variable.upper).max(0.0));
    }
    for constraint in &problem.constraints {
        let lhs = constraint
            .coefficients
            .iter()
            .zip(values)
            .map(|(coefficient, value)| coefficient * value)
            .sum::<f64>();
        let violation = match constraint.relation {
            ConstraintRelation::LessOrEqual => (lhs - constraint.rhs).max(0.0),
            ConstraintRelation::GreaterOrEqual => (constraint.rhs - lhs).max(0.0),
            ConstraintRelation::Equal => (lhs - constraint.rhs).abs(),
        };
        maximum = maximum.max(violation);
    }
    maximum
}

fn original_objective(problem: &MixedIntegerProblem, values: &[f64]) -> f64 {
    problem
        .variables
        .iter()
        .zip(values)
        .map(|(variable, value)| variable.objective_coefficient * value)
        .sum()
}

fn lexicographically_less(left: &[f64], right: &[f64]) -> bool {
    for (left_value, right_value) in left.iter().zip(right) {
        match left_value.total_cmp(right_value) {
            core::cmp::Ordering::Less => return true,
            core::cmp::Ordering::Greater => return false,
            core::cmp::Ordering::Equal => {}
        }
    }
    left.len() < right.len()
}

fn validate(problem: &MixedIntegerProblem) -> Result<(), MixedIntegerError> {
    if problem.variables.is_empty() {
        return Err(MixedIntegerError::EmptyProblem);
    }
    if !problem
        .variables
        .iter()
        .any(|variable| variable.kind == MixedVariableKind::Integer)
    {
        return Err(MixedIntegerError::NoIntegerVariable);
    }
    if !problem.lp_tolerance.is_finite()
        || problem.lp_tolerance <= 0.0
        || !problem.integrality_tolerance.is_finite()
        || problem.integrality_tolerance <= 0.0
        || problem.integrality_tolerance >= 0.5
    {
        return Err(MixedIntegerError::InvalidTolerance);
    }
    if problem.maximum_nodes == 0 {
        return Err(MixedIntegerError::InvalidNodeBudget);
    }
    if problem.maximum_lp_iterations_per_node == 0 {
        return Err(MixedIntegerError::InvalidLpIterationBudget);
    }
    for (index, variable) in problem.variables.iter().enumerate() {
        if !variable.lower.is_finite()
            || !variable.upper.is_finite()
            || !variable.objective_coefficient.is_finite()
        {
            return Err(MixedIntegerError::NonFiniteInput);
        }
        if variable.lower > variable.upper {
            return Err(MixedIntegerError::InvalidBounds { variable: index });
        }
        if variable.kind == MixedVariableKind::Integer
            && (variable.lower != variable.lower.round()
                || variable.upper != variable.upper.round()
                || variable.lower.abs() > MAX_EXACT_F64_INTEGER
                || variable.upper.abs() > MAX_EXACT_F64_INTEGER)
        {
            return Err(MixedIntegerError::InvalidBounds { variable: index });
        }
    }
    for constraint in &problem.constraints {
        if constraint.coefficients.len() != problem.variables.len() {
            return Err(MixedIntegerError::ConstraintWidthMismatch);
        }
        if !constraint.rhs.is_finite()
            || constraint
                .coefficients
                .iter()
                .any(|value| !value.is_finite())
        {
            return Err(MixedIntegerError::NonFiniteInput);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_branch_and_bound_finds_integer_continuous_optimum() {
        let problem = MixedIntegerProblem {
            variables: vec![
                MixedVariable {
                    lower: 0.0,
                    upper: 4.0,
                    objective_coefficient: 5.0,
                    kind: MixedVariableKind::Integer,
                },
                MixedVariable {
                    lower: 0.0,
                    upper: 4.0,
                    objective_coefficient: 4.0,
                    kind: MixedVariableKind::Continuous,
                },
            ],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![3.0, 2.0],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 7.0,
            }],
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 100,
            maximum_lp_iterations_per_node: 200,
        };
        let solution = solve_mixed_integer_branch_and_bound(&problem).expect("mixed optimum");
        assert_eq!(solution.values[0], 0.0);
        assert!((solution.values[1] - 3.5).abs() < 1e-8);
        assert!((solution.objective_value - 14.0).abs() < 1e-8);
        assert!(solution.relaxation_solves >= 1);
    }

    #[test]
    fn all_integer_problem_uses_lp_relaxation_and_branches() {
        let problem = MixedIntegerProblem {
            variables: vec![
                MixedVariable {
                    lower: 0.0,
                    upper: 4.0,
                    objective_coefficient: 5.0,
                    kind: MixedVariableKind::Integer,
                },
                MixedVariable {
                    lower: 0.0,
                    upper: 4.0,
                    objective_coefficient: 4.0,
                    kind: MixedVariableKind::Integer,
                },
            ],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![3.0, 2.0],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 7.0,
            }],
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 100,
            maximum_lp_iterations_per_node: 200,
        };
        let solution = solve_mixed_integer_branch_and_bound(&problem).expect("integer optimum");
        assert_eq!(solution.values, vec![1.0, 2.0]);
        assert!((solution.objective_value - 13.0).abs() < 1e-8);
    }

    #[test]
    fn near_integral_values_snap_to_exact_integer_coordinates() {
        let problem = MixedIntegerProblem {
            variables: vec![
                MixedVariable {
                    lower: 0.0,
                    upper: 4.0,
                    objective_coefficient: 1.0,
                    kind: MixedVariableKind::Integer,
                },
                MixedVariable {
                    lower: 0.0,
                    upper: 4.0,
                    objective_coefficient: 1.0,
                    kind: MixedVariableKind::Continuous,
                },
            ],
            constraints: Vec::new(),
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 10,
            maximum_lp_iterations_per_node: 20,
        };
        let snapped = snap_integral_values(&problem, &[0.999_999_999, 1.25]).expect("snap");
        assert_eq!(snapped, vec![1.0, 1.25]);
        assert_eq!(maximum_primal_violation(&problem, &snapped), 0.0);
    }

    #[test]
    fn infeasible_near_integral_snap_branches_to_an_exact_feasible_integer() {
        let problem = MixedIntegerProblem {
            variables: vec![MixedVariable {
                lower: 0.0,
                upper: 1.0,
                objective_coefficient: 1.0,
                kind: MixedVariableKind::Integer,
            }],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![1.0],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 0.95,
            }],
            lp_tolerance: 1e-9,
            integrality_tolerance: 0.1,
            maximum_nodes: 10,
            maximum_lp_iterations_per_node: 50,
        };
        let solution = solve_mixed_integer_branch_and_bound(&problem).expect("exact integer search");
        assert_eq!(solution.values, vec![0.0]);
        assert_eq!(solution.objective_value, 0.0);
        assert!(solution.explored_nodes >= 2);
    }

    #[test]
    fn integer_domain_and_tolerance_validation_fail_closed() {
        let mut problem = MixedIntegerProblem {
            variables: vec![MixedVariable {
                lower: 0.25,
                upper: 4.0,
                objective_coefficient: 1.0,
                kind: MixedVariableKind::Integer,
            }],
            constraints: Vec::new(),
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 10,
            maximum_lp_iterations_per_node: 20,
        };
        assert_eq!(
            solve_mixed_integer_branch_and_bound(&problem),
            Err(MixedIntegerError::InvalidBounds { variable: 0 })
        );

        problem.variables[0].lower = 0.0;
        problem.integrality_tolerance = 0.5;
        assert_eq!(
            solve_mixed_integer_branch_and_bound(&problem),
            Err(MixedIntegerError::InvalidTolerance)
        );

        problem.integrality_tolerance = 1e-8;
        problem.variables[0].upper = MAX_EXACT_F64_INTEGER + 2.0;
        assert_eq!(
            solve_mixed_integer_branch_and_bound(&problem),
            Err(MixedIntegerError::InvalidBounds { variable: 0 })
        );
    }

    #[test]
    fn node_budget_exhaustion_never_claims_optimality() {
        let problem = MixedIntegerProblem {
            variables: vec![MixedVariable {
                lower: 0.0,
                upper: 10.0,
                objective_coefficient: 1.0,
                kind: MixedVariableKind::Integer,
            }],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![2.0],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 9.0,
            }],
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 1,
            maximum_lp_iterations_per_node: 100,
        };
        assert!(matches!(
            solve_mixed_integer_branch_and_bound(&problem),
            Err(MixedIntegerError::NodeBudgetExceeded { .. })
        ));
    }
}
