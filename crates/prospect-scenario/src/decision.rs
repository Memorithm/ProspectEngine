//! Constraint-first multi-objective decision support.
//!
//! This module is deliberately additive to `DecisionPolicy`: it does not reinterpret
//! existing scalar policies. Mandatory constraints are evaluated before objectives,
//! and rejected alternatives can never win lexicographic or Pareto selection.

use core::cmp::Ordering;
use core::fmt;
use std::collections::BTreeSet;

use prospect_core::{ScenarioId};

use crate::BatchResult;

/// Stable identity of a decision alternative.
///
/// Baseline is explicit so a no-intervention outcome can remain competitive instead
/// of forcing the generic layer to pick one intervention whenever candidates exist.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AlternativeId {
    Baseline,
    Scenario(ScenarioId),
}

impl AlternativeId {
    #[must_use]
    pub const fn is_baseline(&self) -> bool {
        matches!(self, Self::Baseline)
    }

    #[must_use]
    pub const fn scenario_id(&self) -> Option<&ScenarioId> {
        match self {
            Self::Baseline => None,
            Self::Scenario(id) => Some(id),
        }
    }
}

impl fmt::Display for AlternativeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Baseline => formatter.write_str("baseline"),
            Self::Scenario(id) => id.fmt(formatter),
        }
    }
}

/// Whether a larger or smaller value is preferred for one objective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveDirection {
    Maximize,
    Minimize,
}

/// One named objective value.
///
/// Objective IDs are validated by [`assess_decision_set`], where the complete
/// objective schema is available. Names must be non-empty and unique, and every
/// admissible alternative must expose the same ordered names and directions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Objective<T> {
    id: String,
    direction: ObjectiveDirection,
    value: T,
}

impl<T> Objective<T> {
    #[must_use]
    pub fn new(id: impl Into<String>, direction: ObjectiveDirection, value: T) -> Self {
        Self {
            id: id.into(),
            direction,
            value,
        }
    }

    #[must_use]
    pub fn maximize(id: impl Into<String>, value: T) -> Self {
        Self::new(id, ObjectiveDirection::Maximize, value)
    }

    #[must_use]
    pub fn minimize(id: impl Into<String>, value: T) -> Self {
        Self::new(id, ObjectiveDirection::Minimize, value)
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub const fn direction(&self) -> ObjectiveDirection {
        self.direction
    }

    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }
}

/// Constraint result and objective values for one alternative.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlternativeAssessment<R, T> {
    /// Every mandatory constraint passed. Only these alternatives are selectable.
    Admissible { objectives: Vec<Objective<T>> },
    /// At least one mandatory constraint failed. Objectives are intentionally absent.
    Rejected { reason: R },
}

/// One baseline or scenario after constraint-first assessment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssessedAlternative<R, T> {
    id: AlternativeId,
    assessment: AlternativeAssessment<R, T>,
}

impl<R, T> AssessedAlternative<R, T> {
    #[must_use]
    pub const fn id(&self) -> &AlternativeId {
        &self.id
    }

    #[must_use]
    pub const fn assessment(&self) -> &AlternativeAssessment<R, T> {
        &self.assessment
    }

    #[must_use]
    pub const fn is_admissible(&self) -> bool {
        matches!(self.assessment, AlternativeAssessment::Admissible { .. })
    }

    #[must_use]
    pub fn objectives(&self) -> Option<&[Objective<T>]> {
        match &self.assessment {
            AlternativeAssessment::Admissible { objectives } => Some(objectives),
            AlternativeAssessment::Rejected { .. } => None,
        }
    }

    #[must_use]
    pub const fn rejection_reason(&self) -> Option<&R> {
        match &self.assessment {
            AlternativeAssessment::Admissible { .. } => None,
            AlternativeAssessment::Rejected { reason } => Some(reason),
        }
    }
}

/// Structural error in the objective schema, not a domain-constraint rejection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionSetError {
    EmptyObjectiveVector { alternative: AlternativeId },
    EmptyObjectiveId { alternative: AlternativeId, index: usize },
    DuplicateObjectiveId { alternative: AlternativeId, id: String },
    ObjectiveSchemaMismatch { alternative: AlternativeId },
}

impl fmt::Display for DecisionSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyObjectiveVector { alternative } => {
                write!(formatter, "admissible alternative {alternative} has no objectives")
            }
            Self::EmptyObjectiveId { alternative, index } => write!(
                formatter,
                "admissible alternative {alternative} has an empty objective id at index {index}"
            ),
            Self::DuplicateObjectiveId { alternative, id } => write!(
                formatter,
                "admissible alternative {alternative} repeats objective id {id}"
            ),
            Self::ObjectiveSchemaMismatch { alternative } => write!(
                formatter,
                "admissible alternative {alternative} does not match the established objective schema"
            ),
        }
    }
}

impl std::error::Error for DecisionSetError {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObjectiveSpec {
    id: String,
    direction: ObjectiveDirection,
}

/// Complete constraint-first assessment of baseline plus every scenario.
///
/// Alternative order is deterministic: baseline first, then input scenario order.
/// Consequently exact lexicographic ties conservatively retain the baseline or the
/// earliest scenario instead of inventing an extra tie-breaker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstrainedDecisionSet<R, T> {
    alternatives: Vec<AssessedAlternative<R, T>>,
    schema: Option<Vec<ObjectiveSpec>>,
}

impl<R, T> ConstrainedDecisionSet<R, T> {
    #[must_use]
    pub fn alternatives(&self) -> &[AssessedAlternative<R, T>] {
        &self.alternatives
    }

    #[must_use]
    pub fn admissible_count(&self) -> usize {
        self.alternatives
            .iter()
            .filter(|alternative| alternative.is_admissible())
            .count()
    }

    #[must_use]
    pub fn rejected_count(&self) -> usize {
        self.alternatives.len() - self.admissible_count()
    }

    #[must_use]
    pub fn objective_count(&self) -> usize {
        self.schema.as_ref().map_or(0, Vec::len)
    }
}

impl<R, T: Ord> ConstrainedDecisionSet<R, T> {
    /// Select by ordered objectives after all mandatory constraints have passed.
    ///
    /// Objective zero has highest priority. Equal vectors keep the earlier
    /// alternative, making baseline win an exact tie because it is assessed first.
    #[must_use]
    pub fn lexicographic_best(&self) -> Option<&AssessedAlternative<R, T>> {
        let mut best: Option<&AssessedAlternative<R, T>> = None;
        for candidate in &self.alternatives {
            let Some(candidate_objectives) = candidate.objectives() else {
                continue;
            };
            match best {
                None => best = Some(candidate),
                Some(current) => {
                    let current_objectives = current
                        .objectives()
                        .expect("best alternative is always admissible");
                    if compare_vectors(candidate_objectives, current_objectives)
                        == Ordering::Greater
                    {
                        best = Some(candidate);
                    }
                }
            }
        }
        best
    }

    /// Return every admissible non-dominated alternative in deterministic input order.
    ///
    /// An alternative dominates another only when it is no worse on every objective
    /// and strictly better on at least one. Equal objective vectors therefore remain
    /// together on the front; no hidden tie-breaker is introduced.
    #[must_use]
    pub fn pareto_front(&self) -> Vec<&AssessedAlternative<R, T>> {
        let admissible = self
            .alternatives
            .iter()
            .filter(|alternative| alternative.is_admissible())
            .collect::<Vec<_>>();
        admissible
            .iter()
            .copied()
            .filter(|candidate| {
                let candidate_objectives = candidate
                    .objectives()
                    .expect("filtered alternative is admissible");
                !admissible.iter().copied().any(|other| {
                    !core::ptr::eq(candidate, other)
                        && dominates(
                            other
                                .objectives()
                                .expect("filtered alternative is admissible"),
                            candidate_objectives,
                        )
                })
            })
            .collect()
    }
}

/// Assess baseline and scenarios with mandatory constraints before objectives.
///
/// The callback receives `(alternative_id, baseline_signature, alternative_signature)`.
/// `Err(reason)` means the alternative violates a mandatory domain constraint; its
/// objectives are discarded and it can never be selected. `Ok(objectives)` means
/// the alternative is admissible. Every admissible alternative must expose one
/// identical, non-empty ordered objective schema. A schema error rejects the whole
/// decision set rather than comparing semantically incompatible numbers.
///
/// ```
/// use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};
/// use prospect_scenario::{evaluate_batch, decision::{assess_decision_set, Objective}};
/// struct Engine;
/// impl ProspectiveEngine<i32, i32> for Engine {
///     type Signature = i32;
///     type Error = core::convert::Infallible;
///     fn baseline(&self, state: &i32) -> Result<i32, Self::Error> { Ok(*state) }
///     fn evaluate(&self, state: &i32, intervention: &i32) -> Result<i32, Self::Error> {
///         Ok(*state + *intervention)
///     }
/// }
/// let batch = evaluate_batch(&Engine, &10, vec![
///     Scenario::new(ScenarioId::new("safe").unwrap(), 2),
///     Scenario::new(ScenarioId::new("too-high").unwrap(), 50),
/// ]).unwrap();
/// let assessed = assess_decision_set(&batch, |_id, _baseline, signature| {
///     if *signature > 20 { Err("hard safety limit") }
///     else { Ok(vec![Objective::maximize("utility", *signature)]) }
/// }).unwrap();
/// assert_eq!(assessed.lexicographic_best().unwrap().id().to_string(), "safe");
/// ```
pub fn assess_decision_set<I, S, R, T, F>(
    batch: &BatchResult<I, S>,
    mut assess: F,
) -> Result<ConstrainedDecisionSet<R, T>, DecisionSetError>
where
    F: FnMut(&AlternativeId, &S, &S) -> Result<Vec<Objective<T>>, R>,
{
    let mut alternatives = Vec::with_capacity(batch.outcomes().len() + 1);
    let mut schema = None;

    let baseline_id = AlternativeId::Baseline;
    let baseline_assessment = assess(&baseline_id, batch.baseline(), batch.baseline());
    push_assessment(&mut alternatives, &mut schema, baseline_id, baseline_assessment)?;

    for outcome in batch.outcomes() {
        let id = AlternativeId::Scenario(outcome.scenario().id().clone());
        let assessment = assess(&id, batch.baseline(), outcome.signature());
        push_assessment(&mut alternatives, &mut schema, id, assessment)?;
    }

    Ok(ConstrainedDecisionSet {
        alternatives,
        schema,
    })
}

fn push_assessment<R, T>(
    alternatives: &mut Vec<AssessedAlternative<R, T>>,
    schema: &mut Option<Vec<ObjectiveSpec>>,
    id: AlternativeId,
    assessment: Result<Vec<Objective<T>>, R>,
) -> Result<(), DecisionSetError> {
    let assessment = match assessment {
        Err(reason) => AlternativeAssessment::Rejected { reason },
        Ok(objectives) => {
            validate_objectives(&id, &objectives, schema)?;
            AlternativeAssessment::Admissible { objectives }
        }
    };
    alternatives.push(AssessedAlternative { id, assessment });
    Ok(())
}

fn validate_objectives<T>(
    alternative: &AlternativeId,
    objectives: &[Objective<T>],
    schema: &mut Option<Vec<ObjectiveSpec>>,
) -> Result<(), DecisionSetError> {
    if objectives.is_empty() {
        return Err(DecisionSetError::EmptyObjectiveVector {
            alternative: alternative.clone(),
        });
    }

    let mut ids = BTreeSet::new();
    let mut current = Vec::with_capacity(objectives.len());
    for (index, objective) in objectives.iter().enumerate() {
        if objective.id.trim().is_empty() {
            return Err(DecisionSetError::EmptyObjectiveId {
                alternative: alternative.clone(),
                index,
            });
        }
        if !ids.insert(objective.id.as_str()) {
            return Err(DecisionSetError::DuplicateObjectiveId {
                alternative: alternative.clone(),
                id: objective.id.clone(),
            });
        }
        current.push(ObjectiveSpec {
            id: objective.id.clone(),
            direction: objective.direction,
        });
    }

    match schema {
        None => *schema = Some(current),
        Some(expected) if *expected == current => {}
        Some(_) => {
            return Err(DecisionSetError::ObjectiveSchemaMismatch {
                alternative: alternative.clone(),
            });
        }
    }
    Ok(())
}

fn compare_vectors<T: Ord>(left: &[Objective<T>], right: &[Objective<T>]) -> Ordering {
    debug_assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right) {
        debug_assert_eq!(left.id, right.id);
        debug_assert_eq!(left.direction, right.direction);
        let order = preferred_cmp(left.value(), right.value(), left.direction);
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

fn dominates<T: Ord>(left: &[Objective<T>], right: &[Objective<T>]) -> bool {
    debug_assert_eq!(left.len(), right.len());
    let mut strictly_better = false;
    for (left, right) in left.iter().zip(right) {
        debug_assert_eq!(left.id, right.id);
        debug_assert_eq!(left.direction, right.direction);
        match preferred_cmp(left.value(), right.value(), left.direction) {
            Ordering::Less => return false,
            Ordering::Greater => strictly_better = true,
            Ordering::Equal => {}
        }
    }
    strictly_better
}

fn preferred_cmp<T: Ord>(left: &T, right: &T, direction: ObjectiveDirection) -> Ordering {
    match direction {
        ObjectiveDirection::Maximize => left.cmp(right),
        ObjectiveDirection::Minimize => right.cmp(left),
    }
}

#[cfg(test)]
mod tests {
    use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};

    use super::*;
    use crate::evaluate_batch;

    struct Add;
    impl ProspectiveEngine<i32, i32> for Add {
        type Signature = i32;
        type Error = core::convert::Infallible;
        fn baseline(&self, state: &i32) -> Result<i32, Self::Error> {
            Ok(*state)
        }
        fn evaluate(&self, state: &i32, intervention: &i32) -> Result<i32, Self::Error> {
            Ok(*state + *intervention)
        }
    }

    fn batch() -> BatchResult<i32, i32> {
        evaluate_batch(
            &Add,
            &10,
            vec![
                Scenario::new(ScenarioId::new("s1").unwrap(), 1),
                Scenario::new(ScenarioId::new("s2").unwrap(), 2),
                Scenario::new(ScenarioId::new("s3").unwrap(), 3),
            ],
        )
        .unwrap()
    }

    #[test]
    fn mandatory_constraint_excludes_high_scoring_candidate() {
        let decision = assess_decision_set(&batch(), |_id, _baseline, signature| {
            if *signature > 12 {
                Err("limit")
            } else {
                Ok(vec![Objective::maximize("utility", *signature)])
            }
        })
        .unwrap();
        assert_eq!(decision.admissible_count(), 3);
        assert_eq!(decision.rejected_count(), 1);
        assert_eq!(
            decision.lexicographic_best().unwrap().id(),
            &AlternativeId::Scenario(ScenarioId::new("s2").unwrap())
        );
        assert_eq!(
            decision.alternatives()[3].rejection_reason(),
            Some(&"limit")
        );
    }

    #[test]
    fn baseline_wins_exact_lexicographic_tie() {
        let decision = assess_decision_set(&batch(), |_id, _baseline, _signature| {
            Ok::<_, ()>(vec![Objective::maximize("same", 1)])
        })
        .unwrap();
        assert_eq!(decision.lexicographic_best().unwrap().id(), &AlternativeId::Baseline);
    }

    #[test]
    fn lexicographic_priority_precedes_later_objectives() {
        let decision = assess_decision_set(&batch(), |id, _, signature| {
            let priority = match id {
                AlternativeId::Scenario(id) if id.as_str() == "s1" => 2,
                _ => 1,
            };
            Ok::<_, ()>(vec![
                Objective::maximize("priority", priority),
                Objective::maximize("utility", *signature),
            ])
        })
        .unwrap();
        assert_eq!(
            decision.lexicographic_best().unwrap().id(),
            &AlternativeId::Scenario(ScenarioId::new("s1").unwrap())
        );
    }

    #[test]
    fn pareto_front_respects_mixed_minimize_and_maximize_directions() {
        let decision = assess_decision_set(&batch(), |id, _, signature| {
            let cost = match id {
                AlternativeId::Baseline => 0,
                AlternativeId::Scenario(_) => *signature - 9,
            };
            Ok::<_, ()>(vec![
                Objective::maximize("quality", *signature),
                Objective::minimize("cost", cost),
            ])
        })
        .unwrap();
        let ids = decision
            .pareto_front()
            .into_iter()
            .map(|alternative| alternative.id().to_string())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["baseline", "s1", "s2", "s3"]);
    }

    #[test]
    fn dominated_alternatives_are_removed_from_pareto_front() {
        let decision = assess_decision_set(&batch(), |id, _, _| {
            let (quality, cost) = match id {
                AlternativeId::Baseline => (5, 5),
                AlternativeId::Scenario(id) if id.as_str() == "s1" => (6, 4),
                AlternativeId::Scenario(id) if id.as_str() == "s2" => (7, 6),
                AlternativeId::Scenario(_) => (4, 7),
            };
            Ok::<_, ()>(vec![
                Objective::maximize("quality", quality),
                Objective::minimize("cost", cost),
            ])
        })
        .unwrap();
        let ids = decision
            .pareto_front()
            .into_iter()
            .map(|alternative| alternative.id().to_string())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["s1", "s2"]);
    }

    #[test]
    fn equal_vectors_coexist_on_pareto_front() {
        let decision = assess_decision_set(&batch(), |_id, _, _| {
            Ok::<_, ()>(vec![Objective::maximize("quality", 1)])
        })
        .unwrap();
        assert_eq!(decision.pareto_front().len(), 4);
    }

    #[test]
    fn no_admissible_alternative_produces_no_selection() {
        let decision = assess_decision_set(&batch(), |_id, _, _| {
            Err::<Vec<Objective<i32>>, _>("blocked")
        })
        .unwrap();
        assert_eq!(decision.admissible_count(), 0);
        assert_eq!(decision.objective_count(), 0);
        assert!(decision.lexicographic_best().is_none());
        assert!(decision.pareto_front().is_empty());
    }

    #[test]
    fn baseline_may_be_rejected_without_forcing_no_selection() {
        let decision = assess_decision_set(&batch(), |id, _, signature| {
            if id.is_baseline() {
                Err("baseline unavailable")
            } else {
                Ok(vec![Objective::maximize("utility", *signature)])
            }
        })
        .unwrap();
        assert_eq!(decision.rejected_count(), 1);
        assert_eq!(
            decision.lexicographic_best().unwrap().id().to_string(),
            "s3"
        );
    }

    #[test]
    fn empty_objectives_fail_closed() {
        let error = assess_decision_set(&batch(), |_id, _, _| {
            Ok::<Vec<Objective<i32>>, ()>(vec![])
        })
        .unwrap_err();
        assert!(matches!(
            error,
            DecisionSetError::EmptyObjectiveVector {
                alternative: AlternativeId::Baseline
            }
        ));
    }

    #[test]
    fn empty_and_duplicate_objective_ids_fail_closed() {
        let error = assess_decision_set(&batch(), |_id, _, _| {
            Ok::<_, ()>(vec![Objective::maximize("  ", 1)])
        })
        .unwrap_err();
        assert!(matches!(error, DecisionSetError::EmptyObjectiveId { .. }));

        let error = assess_decision_set(&batch(), |_id, _, _| {
            Ok::<_, ()>(vec![
                Objective::maximize("x", 1),
                Objective::minimize("x", 2),
            ])
        })
        .unwrap_err();
        assert!(matches!(error, DecisionSetError::DuplicateObjectiveId { .. }));
    }

    #[test]
    fn objective_name_order_and_direction_must_match() {
        for drift in ["name", "order", "direction"] {
            let error = assess_decision_set(&batch(), |id, _, _| {
                let normal = vec![
                    Objective::maximize("quality", 1),
                    Objective::minimize("cost", 2),
                ];
                if !matches!(id, AlternativeId::Scenario(candidate) if candidate.as_str() == "s1") {
                    return Ok::<_, ()>(normal);
                }
                Ok(match drift {
                    "name" => vec![
                        Objective::maximize("other", 1),
                        Objective::minimize("cost", 2),
                    ],
                    "order" => vec![
                        Objective::minimize("cost", 2),
                        Objective::maximize("quality", 1),
                    ],
                    _ => vec![
                        Objective::minimize("quality", 1),
                        Objective::minimize("cost", 2),
                    ],
                })
            })
            .unwrap_err();
            assert!(matches!(error, DecisionSetError::ObjectiveSchemaMismatch { .. }));
        }
    }

    #[test]
    fn assessment_order_is_baseline_then_scenarios() {
        let mut seen = Vec::new();
        let decision = assess_decision_set(&batch(), |id, _, signature| {
            seen.push(id.to_string());
            Ok::<_, ()>(vec![Objective::maximize("utility", *signature)])
        })
        .unwrap();
        assert_eq!(seen, ["baseline", "s1", "s2", "s3"]);
        assert_eq!(decision.alternatives().len(), 4);
    }
}
