use crate::general_linear_program::GeneralLinearConstraint;
use crate::mixed_integer::{MixedIntegerProblem, MixedVariable, MixedVariableKind};
use crate::optimization::ConstraintRelation;
use core::fmt;

const MAX_EXACT_F64_INTEGER: f64 = 9_007_199_254_740_992.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixedIntegerPresolveConfig {
    pub maximum_passes: u64,
    pub tolerance: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MixedIntegerPresolveReport {
    pub problem: MixedIntegerProblem,
    pub passes: u64,
    pub tightened_bounds: u64,
    pub fixed_variables: Vec<usize>,
    pub redundant_constraints: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MixedIntegerPresolveError {
    EmptyProblem,
    ConstraintWidthMismatch,
    NonFiniteInput,
    InvalidBounds { variable: usize },
    InvalidTolerance,
    InvalidPassBudget,
    ArithmeticBreakdown,
    Infeasible,
}

impl fmt::Display for MixedIntegerPresolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProblem => formatter.write_str("MIP presolve requires variables"),
            Self::ConstraintWidthMismatch => {
                formatter.write_str("MIP presolve constraint width must match variable width")
            }
            Self::NonFiniteInput => formatter.write_str("MIP presolve inputs must be finite"),
            Self::InvalidBounds { variable } => {
                write!(formatter, "MIP presolve variable {variable} has invalid bounds")
            }
            Self::InvalidTolerance => {
                formatter.write_str("MIP presolve tolerance must be finite and positive")
            }
            Self::InvalidPassBudget => {
                formatter.write_str("MIP presolve pass budget must be non-zero")
            }
            Self::ArithmeticBreakdown => {
                formatter.write_str("MIP presolve interval arithmetic broke down")
            }
            Self::Infeasible => formatter.write_str("MIP presolve proved infeasibility"),
        }
    }
}

impl std::error::Error for MixedIntegerPresolveError {}

/// Conservative interval presolve for a finitely bounded mixed-integer model.
///
/// Each pass computes exact interval bounds for every linear row using the
/// current variable box, proves row infeasibility when the whole attainable
/// interval lies on the wrong side of the relation, and tightens one variable
/// at a time from the most favorable attainable contribution of all others.
/// The tolerance is applied in row units before division by a target
/// coefficient, so implied bounds remain invariant to coefficient scaling.
/// Integer bounds are rounded inward after a safe linear bound is derived and
/// remain restricted to the exact-`f64` integer range.
///
/// The routine deliberately keeps the original dimensionality and every row in
/// the returned problem. `redundant_constraints` is evidence only; rows are not
/// deleted so presolve cannot silently change downstream provenance.
pub fn presolve_mixed_integer_bounds(
    problem: &MixedIntegerProblem,
    config: MixedIntegerPresolveConfig,
) -> Result<MixedIntegerPresolveReport, MixedIntegerPresolveError> {
    validate(problem, config)?;
    let mut reduced = problem.clone();
    let mut tightened_bounds = 0_u64;
    let mut passes = 0_u64;

    loop {
        if passes >= config.maximum_passes {
            break;
        }
        passes = passes.saturating_add(1);
        let before: Vec<(f64, f64)> = reduced
            .variables
            .iter()
            .map(|variable| (variable.lower, variable.upper))
            .collect();

        for constraint in &reduced.constraints {
            let (minimum, maximum) = row_interval(constraint, &reduced.variables)?;
            if row_infeasible(
                constraint.relation,
                minimum,
                maximum,
                constraint.rhs,
                config.tolerance,
            ) {
                return Err(MixedIntegerPresolveError::Infeasible);
            }
            tighten_from_constraint(
                constraint,
                &mut reduced.variables,
                config.tolerance,
                &mut tightened_bounds,
            )?;
        }

        for (index, variable) in reduced.variables.iter().enumerate() {
            if variable.lower > variable.upper + config.tolerance {
                return Err(MixedIntegerPresolveError::Infeasible);
            }
            if !variable.lower.is_finite() || !variable.upper.is_finite() {
                return Err(MixedIntegerPresolveError::ArithmeticBreakdown);
            }
            if variable.kind == MixedVariableKind::Integer
                && (variable.lower != variable.lower.round()
                    || variable.upper != variable.upper.round()
                    || variable.lower.abs() > MAX_EXACT_F64_INTEGER
                    || variable.upper.abs() > MAX_EXACT_F64_INTEGER)
            {
                return Err(MixedIntegerPresolveError::InvalidBounds { variable: index });
            }
        }

        let after: Vec<(f64, f64)> = reduced
            .variables
            .iter()
            .map(|variable| (variable.lower, variable.upper))
            .collect();
        if before == after {
            break;
        }
    }

    // Recheck the final box and report rows that are implied throughout it.
    let mut redundant_constraints = Vec::new();
    for (index, constraint) in reduced.constraints.iter().enumerate() {
        let (minimum, maximum) = row_interval(constraint, &reduced.variables)?;
        if row_infeasible(
            constraint.relation,
            minimum,
            maximum,
            constraint.rhs,
            config.tolerance,
        ) {
            return Err(MixedIntegerPresolveError::Infeasible);
        }
        if row_redundant(
            constraint.relation,
            minimum,
            maximum,
            constraint.rhs,
            config.tolerance,
        ) {
            redundant_constraints.push(index);
        }
    }
    let fixed_variables = reduced
        .variables
        .iter()
        .enumerate()
        .filter_map(|(index, variable)| {
            let fixed = match variable.kind {
                MixedVariableKind::Integer => variable.lower == variable.upper,
                MixedVariableKind::Continuous => {
                    (variable.upper - variable.lower).abs() <= config.tolerance
                }
            };
            fixed.then_some(index)
        })
        .collect();

    Ok(MixedIntegerPresolveReport {
        problem: reduced,
        passes,
        tightened_bounds,
        fixed_variables,
        redundant_constraints,
    })
}

fn tighten_from_constraint(
    constraint: &GeneralLinearConstraint,
    variables: &mut [MixedVariable],
    tolerance: f64,
    tightened_bounds: &mut u64,
) -> Result<(), MixedIntegerPresolveError> {
    match constraint.relation {
        ConstraintRelation::LessOrEqual => {
            tighten_less_or_equal(constraint, variables, tolerance, tightened_bounds)
        }
        ConstraintRelation::GreaterOrEqual => {
            // a.x >= b  <=>  (-a).x <= -b
            let negated = GeneralLinearConstraint {
                coefficients: constraint.coefficients.iter().map(|value| -*value).collect(),
                relation: ConstraintRelation::LessOrEqual,
                rhs: -constraint.rhs,
            };
            tighten_less_or_equal(&negated, variables, tolerance, tightened_bounds)
        }
        ConstraintRelation::Equal => {
            tighten_less_or_equal(constraint, variables, tolerance, tightened_bounds)?;
            let negated = GeneralLinearConstraint {
                coefficients: constraint.coefficients.iter().map(|value| -*value).collect(),
                relation: ConstraintRelation::LessOrEqual,
                rhs: -constraint.rhs,
            };
            tighten_less_or_equal(&negated, variables, tolerance, tightened_bounds)
        }
    }
}

fn tighten_less_or_equal(
    constraint: &GeneralLinearConstraint,
    variables: &mut [MixedVariable],
    tolerance: f64,
    tightened_bounds: &mut u64,
) -> Result<(), MixedIntegerPresolveError> {
    for target in 0..variables.len() {
        let coefficient = constraint.coefficients[target];
        if coefficient == 0.0 {
            continue;
        }
        let mut minimum_other = 0.0_f64;
        for (index, (other_coefficient, variable)) in constraint
            .coefficients
            .iter()
            .zip(variables.iter())
            .enumerate()
        {
            if index == target {
                continue;
            }
            let contribution = if *other_coefficient >= 0.0 {
                *other_coefficient * variable.lower
            } else {
                *other_coefficient * variable.upper
            };
            minimum_other += contribution;
            if !minimum_other.is_finite() {
                return Err(MixedIntegerPresolveError::ArithmeticBreakdown);
            }
        }
        // For a row a.x <= b accepted within row tolerance t, the conservative
        // bound is derived from a_i x_i + min(other) <= b + t. Applying t here,
        // before division, keeps the semantics independent of coefficient scale.
        let relaxed_residual = constraint.rhs + tolerance - minimum_other;
        if !relaxed_residual.is_finite() {
            return Err(MixedIntegerPresolveError::ArithmeticBreakdown);
        }
        let implied = relaxed_residual / coefficient;
        if !implied.is_finite() {
            return Err(MixedIntegerPresolveError::ArithmeticBreakdown);
        }
        if coefficient > 0.0 {
            let candidate = if variables[target].kind == MixedVariableKind::Integer {
                implied.floor()
            } else {
                implied
            };
            if candidate < variables[target].upper {
                variables[target].upper = candidate;
                *tightened_bounds = tightened_bounds.saturating_add(1);
            }
        } else {
            let candidate = if variables[target].kind == MixedVariableKind::Integer {
                implied.ceil()
            } else {
                implied
            };
            if candidate > variables[target].lower {
                variables[target].lower = candidate;
                *tightened_bounds = tightened_bounds.saturating_add(1);
            }
        }
    }
    Ok(())
}

fn row_interval(
    constraint: &GeneralLinearConstraint,
    variables: &[MixedVariable],
) -> Result<(f64, f64), MixedIntegerPresolveError> {
    let mut minimum = 0.0_f64;
    let mut maximum = 0.0_f64;
    for (coefficient, variable) in constraint.coefficients.iter().zip(variables) {
        let (low, high) = if *coefficient >= 0.0 {
            (
                *coefficient * variable.lower,
                *coefficient * variable.upper,
            )
        } else {
            (
                *coefficient * variable.upper,
                *coefficient * variable.lower,
            )
        };
        minimum += low;
        maximum += high;
        if !minimum.is_finite() || !maximum.is_finite() {
            return Err(MixedIntegerPresolveError::ArithmeticBreakdown);
        }
    }
    Ok((minimum, maximum))
}

fn row_infeasible(
    relation: ConstraintRelation,
    minimum: f64,
    maximum: f64,
    rhs: f64,
    tolerance: f64,
) -> bool {
    match relation {
        ConstraintRelation::LessOrEqual => minimum > rhs + tolerance,
        ConstraintRelation::GreaterOrEqual => maximum < rhs - tolerance,
        ConstraintRelation::Equal => minimum > rhs + tolerance || maximum < rhs - tolerance,
    }
}

fn row_redundant(
    relation: ConstraintRelation,
    minimum: f64,
    maximum: f64,
    rhs: f64,
    tolerance: f64,
) -> bool {
    match relation {
        ConstraintRelation::LessOrEqual => maximum <= rhs + tolerance,
        ConstraintRelation::GreaterOrEqual => minimum >= rhs - tolerance,
        ConstraintRelation::Equal => {
            (minimum - rhs).abs() <= tolerance && (maximum - rhs).abs() <= tolerance
        }
    }
}

fn validate(
    problem: &MixedIntegerProblem,
    config: MixedIntegerPresolveConfig,
) -> Result<(), MixedIntegerPresolveError> {
    if problem.variables.is_empty() {
        return Err(MixedIntegerPresolveError::EmptyProblem);
    }
    if config.maximum_passes == 0 {
        return Err(MixedIntegerPresolveError::InvalidPassBudget);
    }
    if !config.tolerance.is_finite() || config.tolerance <= 0.0 {
        return Err(MixedIntegerPresolveError::InvalidTolerance);
    }
    for (index, variable) in problem.variables.iter().enumerate() {
        if !variable.lower.is_finite()
            || !variable.upper.is_finite()
            || !variable.objective_coefficient.is_finite()
        {
            return Err(MixedIntegerPresolveError::NonFiniteInput);
        }
        if variable.lower > variable.upper {
            return Err(MixedIntegerPresolveError::InvalidBounds { variable: index });
        }
        if variable.kind == MixedVariableKind::Integer
            && (variable.lower != variable.lower.round()
                || variable.upper != variable.upper.round()
                || variable.lower.abs() > MAX_EXACT_F64_INTEGER
                || variable.upper.abs() > MAX_EXACT_F64_INTEGER)
        {
            return Err(MixedIntegerPresolveError::InvalidBounds { variable: index });
        }
    }
    for constraint in &problem.constraints {
        if constraint.coefficients.len() != problem.variables.len() {
            return Err(MixedIntegerPresolveError::ConstraintWidthMismatch);
        }
        if !constraint.rhs.is_finite()
            || constraint
                .coefficients
                .iter()
                .any(|value| !value.is_finite())
        {
            return Err(MixedIntegerPresolveError::NonFiniteInput);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presolve_tightens_continuous_and_integer_bounds() {
        let problem = MixedIntegerProblem {
            variables: vec![
                MixedVariable {
                    lower: 0.0,
                    upper: 10.0,
                    objective_coefficient: 1.0,
                    kind: MixedVariableKind::Integer,
                },
                MixedVariable {
                    lower: 0.0,
                    upper: 10.0,
                    objective_coefficient: 1.0,
                    kind: MixedVariableKind::Continuous,
                },
            ],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![2.0, 1.0],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 7.0,
            }],
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 100,
            maximum_lp_iterations_per_node: 100,
        };
        let report = presolve_mixed_integer_bounds(
            &problem,
            MixedIntegerPresolveConfig {
                maximum_passes: 8,
                tolerance: 1e-9,
            },
        )
        .expect("presolve");
        assert_eq!(report.problem.variables[0].upper, 3.0);
        assert!((report.problem.variables[1].upper - (7.0 + 1e-9)).abs() < 1e-12);
        assert!(report.tightened_bounds >= 2);
    }

    #[test]
    fn row_tolerance_is_scaled_before_coefficient_division() {
        let problem = MixedIntegerProblem {
            variables: vec![MixedVariable {
                lower: 0.0,
                upper: 100.0,
                objective_coefficient: 0.0,
                kind: MixedVariableKind::Continuous,
            }],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![0.1],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 1.0,
            }],
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 10,
            maximum_lp_iterations_per_node: 10,
        };
        let report = presolve_mixed_integer_bounds(
            &problem,
            MixedIntegerPresolveConfig {
                maximum_passes: 2,
                tolerance: 0.01,
            },
        )
        .expect("scaled tolerance presolve");
        assert!((report.problem.variables[0].upper - 10.1).abs() < 1e-12);
    }

    #[test]
    fn presolve_proves_box_infeasibility() {
        let problem = MixedIntegerProblem {
            variables: vec![MixedVariable {
                lower: 0.0,
                upper: 2.0,
                objective_coefficient: 0.0,
                kind: MixedVariableKind::Integer,
            }],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![1.0],
                relation: ConstraintRelation::GreaterOrEqual,
                rhs: 3.0,
            }],
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 10,
            maximum_lp_iterations_per_node: 10,
        };
        assert_eq!(
            presolve_mixed_integer_bounds(
                &problem,
                MixedIntegerPresolveConfig {
                    maximum_passes: 4,
                    tolerance: 1e-9,
                }
            ),
            Err(MixedIntegerPresolveError::Infeasible)
        );
    }

    #[test]
    fn redundant_rows_are_reported_but_not_deleted() {
        let problem = MixedIntegerProblem {
            variables: vec![MixedVariable {
                lower: 0.0,
                upper: 2.0,
                objective_coefficient: 0.0,
                kind: MixedVariableKind::Continuous,
            }],
            constraints: vec![GeneralLinearConstraint {
                coefficients: vec![1.0],
                relation: ConstraintRelation::LessOrEqual,
                rhs: 5.0,
            }],
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 10,
            maximum_lp_iterations_per_node: 10,
        };
        let report = presolve_mixed_integer_bounds(
            &problem,
            MixedIntegerPresolveConfig {
                maximum_passes: 4,
                tolerance: 1e-9,
            },
        )
        .expect("presolve");
        assert_eq!(report.redundant_constraints, vec![0]);
        assert_eq!(report.problem.constraints.len(), 1);
    }

    #[test]
    fn integer_bounds_outside_exact_f64_range_fail_closed() {
        let problem = MixedIntegerProblem {
            variables: vec![MixedVariable {
                lower: 0.0,
                upper: MAX_EXACT_F64_INTEGER + 2.0,
                objective_coefficient: 0.0,
                kind: MixedVariableKind::Integer,
            }],
            constraints: Vec::new(),
            lp_tolerance: 1e-9,
            integrality_tolerance: 1e-8,
            maximum_nodes: 10,
            maximum_lp_iterations_per_node: 10,
        };
        assert_eq!(
            presolve_mixed_integer_bounds(
                &problem,
                MixedIntegerPresolveConfig {
                    maximum_passes: 2,
                    tolerance: 1e-9,
                }
            ),
            Err(MixedIntegerPresolveError::InvalidBounds { variable: 0 })
        );
    }
}
