use crate::mip_presolve::{
    presolve_mixed_integer_bounds, MixedIntegerPresolveConfig, MixedIntegerPresolveError,
    MixedIntegerPresolveReport,
};
use crate::mixed_integer::{
    solve_mixed_integer_branch_and_bound, MixedIntegerError, MixedIntegerProblem, MixedIntegerSolution,
};
use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct PresolvedMixedIntegerSolution {
    pub presolve: MixedIntegerPresolveReport,
    pub solution: MixedIntegerSolution,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PresolvedMixedIntegerError {
    Presolve(MixedIntegerPresolveError),
    Solve(MixedIntegerError),
}

impl fmt::Display for PresolvedMixedIntegerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Presolve(error) => write!(formatter, "mixed-integer presolve failed: {error}"),
            Self::Solve(error) => write!(formatter, "presolved mixed-integer search failed: {error}"),
        }
    }
}

impl std::error::Error for PresolvedMixedIntegerError {}

impl From<MixedIntegerPresolveError> for PresolvedMixedIntegerError {
    fn from(error: MixedIntegerPresolveError) -> Self {
        Self::Presolve(error)
    }
}

/// Run conservative bound presolve and then solve the tightened model by the
/// Wave-8 LP-relaxation branch-and-bound engine.
///
/// The complete presolve report is returned beside the final solution. Nothing
/// is hidden as an implementation detail: callers can bind evidence to the
/// original model, tightened bounds, pass count and search result separately.
pub fn solve_mixed_integer_with_presolve(
    problem: &MixedIntegerProblem,
    config: MixedIntegerPresolveConfig,
) -> Result<PresolvedMixedIntegerSolution, PresolvedMixedIntegerError> {
    let presolve = presolve_mixed_integer_bounds(problem, config)?;
    let solution = solve_mixed_integer_branch_and_bound(&presolve.problem)
        .map_err(PresolvedMixedIntegerError::Solve)?;
    Ok(PresolvedMixedIntegerSolution { presolve, solution })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::general_linear_program::GeneralLinearConstraint;
    use crate::mixed_integer::{MixedVariable, MixedVariableKind};
    use crate::optimization::ConstraintRelation;

    #[test]
    fn presolve_pipeline_tightens_before_solving_and_preserves_optimum() {
        let problem = MixedIntegerProblem {
            variables: vec![
                MixedVariable {
                    lower: 0.0,
                    upper: 10.0,
                    objective_coefficient: 5.0,
                    kind: MixedVariableKind::Integer,
                },
                MixedVariable {
                    lower: 0.0,
                    upper: 10.0,
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
        let result = solve_mixed_integer_with_presolve(
            &problem,
            MixedIntegerPresolveConfig {
                maximum_passes: 8,
                tolerance: 1e-9,
            },
        )
        .expect("presolved MIP");
        assert_eq!(result.presolve.problem.variables[0].upper, 2.0);
        assert!((result.presolve.problem.variables[1].upper - 3.5).abs() < 1e-8);
        assert_eq!(result.solution.values[0], 0.0);
        assert!((result.solution.values[1] - 3.5).abs() < 1e-8);
        assert!((result.solution.objective_value - 14.0).abs() < 1e-8);
    }
}
