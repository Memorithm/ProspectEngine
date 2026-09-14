use prospect_core::{DecisionPolicy, ScenarioId, SignatureMetric};
use prospect_scenario::{
    BatchResult, ScenarioScore, best_by_policy, evaluate_batch, score_against_baseline,
};

use crate::{
    ElasticEngine, ElasticIntervention, ElasticObservationState, ElasticProbeSetV1,
    ElasticProspectiveModel,
};

#[derive(Clone, Debug, PartialEq)]
pub enum ElasticPrecommitChoice<PolicyScore> {
    NoOp {
        scenario_id: ScenarioId,
        utility: PolicyScore,
    },
    Probe {
        scenario_id: ScenarioId,
        utility: PolicyScore,
        rollback: ElasticIntervention,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElasticPrecommitComparison<Signature, MetricScore, PolicyScore> {
    batch: BatchResult<ElasticIntervention, Signature>,
    metric_scores: Vec<ScenarioScore<MetricScore>>,
    choice: ElasticPrecommitChoice<PolicyScore>,
}

impl<PolicyScore> ElasticPrecommitChoice<PolicyScore> {
    #[must_use]
    pub const fn scenario_id(&self) -> &ScenarioId {
        match self {
            Self::NoOp { scenario_id, .. } | Self::Probe { scenario_id, .. } => scenario_id,
        }
    }

    #[must_use]
    pub const fn utility(&self) -> &PolicyScore {
        match self {
            Self::NoOp { utility, .. } | Self::Probe { utility, .. } => utility,
        }
    }

    #[must_use]
    pub const fn rollback(&self) -> Option<&ElasticIntervention> {
        match self {
            Self::NoOp { .. } => None,
            Self::Probe { rollback, .. } => Some(rollback),
        }
    }

    #[must_use]
    pub const fn is_noop(&self) -> bool {
        matches!(self, Self::NoOp { .. })
    }
}

impl<Signature, MetricScore, PolicyScore>
    ElasticPrecommitComparison<Signature, MetricScore, PolicyScore>
{
    #[must_use]
    pub const fn batch(&self) -> &BatchResult<ElasticIntervention, Signature> {
        &self.batch
    }

    #[must_use]
    pub fn metric_scores(&self) -> &[ScenarioScore<MetricScore>] {
        &self.metric_scores
    }

    #[must_use]
    pub const fn choice(&self) -> &ElasticPrecommitChoice<PolicyScore> {
        &self.choice
    }
}

/// Evaluates all validated probe candidates prospectively before any physical
/// ElasticXxx actuation is authorized.
///
/// The explicit no-op baseline wins ties and all cases where the best candidate
/// does not improve the domain policy's utility. This function never calls a
/// `TransactionalActuator`; it only compares prospective signatures.
pub fn compare_before_commit<M, Metric, Policy>(
    engine: &ElasticEngine<M>,
    state: &ElasticObservationState,
    probes: &ElasticProbeSetV1,
    metric: &Metric,
    policy: &Policy,
) -> Result<ElasticPrecommitComparison<M::Signature, Metric::Score, Policy::Score>, M::Error>
where
    M: ElasticProspectiveModel,
    Metric: SignatureMetric<M::Signature>,
    Policy: DecisionPolicy<M::Signature>,
{
    let batch = evaluate_batch(engine, state, probes.scenarios())?;
    let metric_scores = score_against_baseline(&batch, metric);
    let baseline_utility = policy.utility(batch.baseline());

    let choice = if let Some((best, candidate_utility)) = best_by_policy(&batch, policy) {
        if candidate_utility > baseline_utility {
            let selected_id = best.scenario().id();
            if let Some(probe) = probes.probes().iter().find(|probe| probe.id() == selected_id) {
                ElasticPrecommitChoice::Probe {
                    scenario_id: selected_id.clone(),
                    utility: candidate_utility,
                    rollback: probe.rollback().clone(),
                }
            } else {
                ElasticPrecommitChoice::NoOp {
                    scenario_id: probes.baseline_id().clone(),
                    utility: baseline_utility,
                }
            }
        } else {
            ElasticPrecommitChoice::NoOp {
                scenario_id: probes.baseline_id().clone(),
                utility: baseline_utility,
            }
        }
    } else {
        ElasticPrecommitChoice::NoOp {
            scenario_id: probes.baseline_id().clone(),
            utility: baseline_utility,
        }
    };

    Ok(ElasticPrecommitComparison {
        batch,
        metric_scores,
        choice,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::convert::Infallible;
    use std::time::Instant;

    use elastic_eir::{FirstGroundedPlanner, PlanningContext};
    use elastic_runtime::{
        ObservationSnapshot, RuntimeConfig, plan::plan_with_context, plan::validate_with_checks,
    };
    use prospect_core::{DecisionPolicy, ScenarioId, SignatureMetric};

    use super::{ElasticPrecommitChoice, compare_before_commit};
    use crate::{
        ElasticEngine, ElasticIntervention, ElasticObservationState, ElasticProbeSetV1,
        ElasticProbeV1, ElasticProspectiveModel, ValidatedPlanIntentV1,
    };

    struct DeltaModel;

    impl ElasticProspectiveModel for DeltaModel {
        type Signature = i32;
        type Error = Infallible;

        fn baseline(
            &self,
            _state: &ElasticObservationState,
        ) -> Result<Self::Signature, Self::Error> {
            Ok(10)
        }

        fn evaluate(
            &self,
            _state: &ElasticObservationState,
            intervention: &ElasticIntervention,
        ) -> Result<Self::Signature, Self::Error> {
            let delta = intervention.parameters().get("delta").copied().unwrap_or(0.0);
            Ok(10 + delta as i32)
        }
    }

    struct AbsoluteDistance;

    impl SignatureMetric<i32> for AbsoluteDistance {
        type Score = i32;

        fn compare(&self, reference: &i32, candidate: &i32) -> Self::Score {
            (candidate - reference).abs()
        }
    }

    struct PreferHigher;

    impl DecisionPolicy<i32> for PreferHigher {
        type Score = i32;

        fn utility(&self, signature: &i32) -> Self::Score {
            *signature
        }
    }

    fn state() -> ElasticObservationState {
        ElasticObservationState::from_snapshot(&ObservationSnapshot::new(
            Instant::now(),
            Vec::new(),
        ))
        .expect("state")
    }

    fn plan_intent() -> ValidatedPlanIntentV1 {
        let resource = RuntimeConfig::default().ir_resource;
        let plan = plan_with_context(&FirstGroundedPlanner, &resource, &PlanningContext::new());
        let checks = plan
            .resource
            .invariants()
            .iter()
            .cloned()
            .map(|invariant| elastic_runtime::InvariantCheck::new(invariant, true, None))
            .collect();
        let validated = validate_with_checks(plan, checks);
        ValidatedPlanIntentV1::from_validated_plan(&validated).expect("validated intent")
    }

    fn intervention(kind: &str, delta: f64) -> ElasticIntervention {
        ElasticIntervention::new(kind, BTreeMap::from([("delta".to_owned(), delta)]))
            .expect("intervention")
    }

    fn probe(id: &str, delta: f64) -> ElasticProbeV1 {
        ElasticProbeV1::new(
            ScenarioId::new(id).expect("id"),
            plan_intent(),
            intervention("forward", delta),
            intervention("rollback", -delta),
        )
    }

    #[test]
    fn selects_only_a_probe_that_beats_noop() {
        let probes = ElasticProbeSetV1::new(
            ScenarioId::new("noop").expect("baseline"),
            vec![probe("increase", 5.0), probe("decrease", -2.0)],
        )
        .expect("probes");
        let result = compare_before_commit(
            &ElasticEngine::new(DeltaModel),
            &state(),
            &probes,
            &AbsoluteDistance,
            &PreferHigher,
        )
        .expect("comparison");

        assert_eq!(result.metric_scores().len(), 2);
        match result.choice() {
            ElasticPrecommitChoice::Probe {
                scenario_id,
                utility,
                rollback,
            } => {
                assert_eq!(scenario_id.as_str(), "increase");
                assert_eq!(*utility, 15);
                assert_eq!(rollback.parameters().get("delta"), Some(&-5.0));
            }
            ElasticPrecommitChoice::NoOp { .. } => panic!("improving probe should be selected"),
        }
    }

    #[test]
    fn noop_wins_when_all_candidates_are_worse() {
        let probes = ElasticProbeSetV1::new(
            ScenarioId::new("noop").expect("baseline"),
            vec![probe("decrease", -2.0)],
        )
        .expect("probes");
        let result = compare_before_commit(
            &ElasticEngine::new(DeltaModel),
            &state(),
            &probes,
            &AbsoluteDistance,
            &PreferHigher,
        )
        .expect("comparison");

        assert!(result.choice().is_noop());
        assert_eq!(result.choice().scenario_id().as_str(), "noop");
        assert_eq!(*result.choice().utility(), 10);
    }

    #[test]
    fn noop_wins_policy_ties() {
        let probes = ElasticProbeSetV1::new(
            ScenarioId::new("noop").expect("baseline"),
            vec![probe("tie", 0.0)],
        )
        .expect("probes");
        let result = compare_before_commit(
            &ElasticEngine::new(DeltaModel),
            &state(),
            &probes,
            &AbsoluteDistance,
            &PreferHigher,
        )
        .expect("comparison");

        assert!(result.choice().is_noop());
    }
}
