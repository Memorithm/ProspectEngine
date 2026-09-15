from pathlib import Path

p = Path("crates/prospect-scenario/src/decision.rs")
s = p.read_text()

struct_anchor = '''#[derive(Clone, Debug, PartialEq, Eq)]
struct ObjectiveSpec {
    id: String,
    direction: ObjectiveDirection,
}

'''
struct_insert = '''#[derive(Clone, Debug, PartialEq, Eq)]
struct ObjectiveSpec {
    id: String,
    direction: ObjectiveDirection,
}

/// One explicit proof that an admissible alternative is Pareto-dominated.
///
/// The objective IDs list contains exactly the objectives on which `dominator` is
/// strictly preferred to `dominated`; all omitted objectives are equal. A witness
/// is emitted only when the dominator is no worse on every objective.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParetoDominanceWitness {
    dominator: AlternativeId,
    dominated: AlternativeId,
    strictly_better_objective_ids: Vec<String>,
}

impl ParetoDominanceWitness {
    #[must_use]
    pub const fn dominator(&self) -> &AlternativeId {
        &self.dominator
    }

    #[must_use]
    pub const fn dominated(&self) -> &AlternativeId {
        &self.dominated
    }

    #[must_use]
    pub fn strictly_better_objective_ids(&self) -> &[String] {
        &self.strictly_better_objective_ids
    }
}

'''
if s.count(struct_anchor) != 1:
    raise SystemExit("ObjectiveSpec anchor drift")
s = s.replace(struct_anchor, struct_insert, 1)

method_anchor = '''    pub fn pareto_front(&self) -> Vec<&AssessedAlternative<R, T>> {
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
                    !core::ptr::eq(*candidate, other)
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
'''
method_replace = method_anchor + '''

    /// Return every Pareto-domination relation with an explicit structural witness.
    ///
    /// Output order is deterministic: dominated alternative order first, then
    /// dominator order. Rejected alternatives never participate because constraint
    /// rejection is semantically distinct from Pareto domination. Equal vectors do
    /// not produce witnesses. No single "best" dominator is invented.
    #[must_use]
    pub fn pareto_dominance_witnesses(&self) -> Vec<ParetoDominanceWitness> {
        let admissible = self
            .alternatives
            .iter()
            .filter(|alternative| alternative.is_admissible())
            .collect::<Vec<_>>();
        let mut witnesses = Vec::new();
        for dominated in &admissible {
            let dominated_objectives = dominated
                .objectives()
                .expect("filtered alternative is admissible");
            for dominator in &admissible {
                if core::ptr::eq(*dominated, *dominator) {
                    continue;
                }
                let dominator_objectives = dominator
                    .objectives()
                    .expect("filtered alternative is admissible");
                if let Some(strictly_better_objective_ids) =
                    dominance_strict_objective_ids(dominator_objectives, dominated_objectives)
                {
                    witnesses.push(ParetoDominanceWitness {
                        dominator: dominator.id().clone(),
                        dominated: dominated.id().clone(),
                        strictly_better_objective_ids,
                    });
                }
            }
        }
        witnesses
    }
'''
if s.count(method_anchor) != 1:
    raise SystemExit("pareto_front anchor drift")
s = s.replace(method_anchor, method_replace, 1)

helper_anchor = '''fn dominates<T: Ord>(left: &[Objective<T>], right: &[Objective<T>]) -> bool {
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

'''
helper_replace = helper_anchor + '''fn dominance_strict_objective_ids<T: Ord>(
    left: &[Objective<T>],
    right: &[Objective<T>],
) -> Option<Vec<String>> {
    debug_assert_eq!(left.len(), right.len());
    let mut strictly_better = Vec::new();
    for (left, right) in left.iter().zip(right) {
        debug_assert_eq!(left.id, right.id);
        debug_assert_eq!(left.direction, right.direction);
        match preferred_cmp(left.value(), right.value(), left.direction) {
            Ordering::Less => return None,
            Ordering::Greater => strictly_better.push(left.id.clone()),
            Ordering::Equal => {}
        }
    }
    (!strictly_better.is_empty()).then_some(strictly_better)
}

'''
if s.count(helper_anchor) != 1:
    raise SystemExit("dominates anchor drift")
s = s.replace(helper_anchor, helper_replace, 1)

test_anchor = '''    #[test]
    fn equal_vectors_coexist_on_pareto_front() {'''
tests = '''    #[test]
    fn pareto_witnesses_are_complete_deterministic_and_objective_explicit() {
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
        let observed = decision
            .pareto_dominance_witnesses()
            .into_iter()
            .map(|witness| {
                (
                    witness.dominator().to_string(),
                    witness.dominated().to_string(),
                    witness.strictly_better_objective_ids().to_vec(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            observed,
            [
                (
                    "s1".to_owned(),
                    "baseline".to_owned(),
                    vec!["quality".to_owned(), "cost".to_owned()],
                ),
                (
                    "baseline".to_owned(),
                    "s3".to_owned(),
                    vec!["quality".to_owned(), "cost".to_owned()],
                ),
                (
                    "s1".to_owned(),
                    "s3".to_owned(),
                    vec!["quality".to_owned(), "cost".to_owned()],
                ),
                (
                    "s2".to_owned(),
                    "s3".to_owned(),
                    vec!["quality".to_owned(), "cost".to_owned()],
                ),
            ]
        );
    }

    #[test]
    fn pareto_witness_records_only_strict_objectives_and_respects_direction() {
        let decision = assess_decision_set(&batch(), |id, _, _| {
            let (quality, cost) = match id {
                AlternativeId::Baseline => (5, 5),
                AlternativeId::Scenario(id) if id.as_str() == "s1" => (6, 5),
                _ => (5, 4),
            };
            Ok::<_, ()>(vec![
                Objective::maximize("quality", quality),
                Objective::minimize("cost", cost),
            ])
        })
        .unwrap();
        let witnesses = decision.pareto_dominance_witnesses();
        assert!(witnesses.iter().any(|witness| {
            witness.dominator().to_string() == "s1"
                && witness.dominated().is_baseline()
                && witness.strictly_better_objective_ids() == ["quality"]
        }));
        assert!(witnesses.iter().any(|witness| {
            witness.dominator().to_string() == "s2"
                && witness.dominated().is_baseline()
                && witness.strictly_better_objective_ids() == ["cost"]
        }));
    }

    #[test]
    fn rejected_and_equal_alternatives_never_create_pareto_witnesses() {
        let equal = assess_decision_set(&batch(), |_id, _, _| {
            Ok::<_, ()>(vec![Objective::maximize("quality", 1)])
        })
        .unwrap();
        assert!(equal.pareto_dominance_witnesses().is_empty());

        let rejected = assess_decision_set(&batch(), |id, _, _| {
            if matches!(id, AlternativeId::Scenario(candidate) if candidate.as_str() == "s3") {
                Err("hard constraint")
            } else {
                Ok(vec![Objective::maximize("quality", 1)])
            }
        })
        .unwrap();
        assert!(rejected.pareto_dominance_witnesses().is_empty());
    }

    #[test]
    fn equal_vectors_coexist_on_pareto_front() {'''
if s.count(test_anchor) != 1:
    raise SystemExit("test anchor drift")
s = s.replace(test_anchor, tests, 1)

p.write_text(s)
