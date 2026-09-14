use std::fmt;

use elastic_eir::{EirResource, PlanOutcome, TransitionCandidate, TransitionPlanner};
use elastic_runtime::{
    CycleResult, Observer, Runtime, RuntimeError, TransactionalActuator, ValidatedPlan,
};
use prospect_core::ScenarioId;

use crate::{
    ElasticIntervention, ElasticPrecommitChoice, ElasticPrecommitComparison, ElasticProbeError,
    ElasticProbeSetV1, ValidatedPlanIntentV1,
};

#[derive(Debug)]
pub enum ElasticTransactionGateError {
    BaselineMismatch,
    SelectedProbeMissing { id: String },
    PlanIntentMismatch { id: String },
    RollbackIntentMismatch { id: String },
    Probe(ElasticProbeError),
    Runtime(RuntimeError),
}

#[derive(Debug)]
pub enum ElasticTransactionOutcome {
    NoOp {
        scenario_id: ScenarioId,
    },
    Cycle {
        scenario_id: ScenarioId,
        declared_rollback: ElasticIntervention,
        cycle: Box<CycleResult>,
    },
}

#[derive(Clone, Debug)]
struct LockedCandidatePlanner {
    candidate: TransitionCandidate,
}

impl TransitionPlanner for LockedCandidatePlanner {
    fn propose_transition(&self, resource: &EirResource) -> PlanOutcome {
        if self.candidate.is_declared_in(resource) {
            PlanOutcome::Candidate(self.candidate.clone())
        } else {
            PlanOutcome::InsufficientEvidence {
                detail:
                    "ProspectEngine-selected candidate is not declared by the action-time resource"
                        .to_owned(),
            }
        }
    }
}

impl ElasticTransactionOutcome {
    #[must_use]
    pub const fn scenario_id(&self) -> &ScenarioId {
        match self {
            Self::NoOp { scenario_id } | Self::Cycle { scenario_id, .. } => scenario_id,
        }
    }

    #[must_use]
    pub const fn declared_rollback(&self) -> Option<&ElasticIntervention> {
        match self {
            Self::NoOp { .. } => None,
            Self::Cycle {
                declared_rollback, ..
            } => Some(declared_rollback),
        }
    }

    #[must_use]
    pub fn cycle(&self) -> Option<&CycleResult> {
        match self {
            Self::NoOp { .. } => None,
            Self::Cycle { cycle, .. } => Some(cycle.as_ref()),
        }
    }

    #[must_use]
    pub const fn is_noop(&self) -> bool {
        matches!(self, Self::NoOp { .. })
    }
}

/// Sends a prospectively selected ElasticXxx candidate through the existing
/// trusted ElasticXxx runtime transaction boundary.
///
/// A no-op choice returns before touching the actuator. A selected probe is
/// first rebound to the exact validated plan intent recorded by the probe,
/// including its normalized EIR resource identity and structural fingerprint.
/// The candidate is then exposed through a locked planner and executed via
/// [`Runtime::cycle`]. This deliberately re-runs action-time actuator
/// validation and delegates prepare/actuate/verify/commit/rollback semantics to
/// ElasticXxx rather than copying them into ProspectEngine.
///
/// The probe's declared rollback is preserved as prospective evidence only.
/// Physical rollback authority remains exclusively with
/// [`TransactionalActuator`].
pub fn execute_selected_probe<Signature, MetricScore, PolicyScore, O, A>(
    runtime: &Runtime,
    comparison: &ElasticPrecommitComparison<Signature, MetricScore, PolicyScore>,
    probes: &ElasticProbeSetV1,
    selected_plan: &ValidatedPlan,
    observer: &O,
    actuator: &mut A,
) -> Result<ElasticTransactionOutcome, ElasticTransactionGateError>
where
    O: Observer,
    A: TransactionalActuator,
{
    match comparison.choice() {
        ElasticPrecommitChoice::NoOp { scenario_id, .. } => {
            if scenario_id != probes.baseline_id() {
                return Err(ElasticTransactionGateError::BaselineMismatch);
            }
            Ok(ElasticTransactionOutcome::NoOp {
                scenario_id: scenario_id.clone(),
            })
        }
        ElasticPrecommitChoice::Probe {
            scenario_id,
            rollback,
            ..
        } => {
            let probe = probes
                .probes()
                .iter()
                .find(|probe| probe.id() == scenario_id)
                .ok_or_else(|| ElasticTransactionGateError::SelectedProbeMissing {
                    id: scenario_id.as_str().to_owned(),
                })?;

            if probe.rollback() != rollback {
                return Err(ElasticTransactionGateError::RollbackIntentMismatch {
                    id: scenario_id.as_str().to_owned(),
                });
            }

            let action_time_intent = ValidatedPlanIntentV1::from_validated_plan(selected_plan)
                .map_err(ElasticTransactionGateError::Probe)?;
            if &action_time_intent != probe.plan() {
                return Err(ElasticTransactionGateError::PlanIntentMismatch {
                    id: scenario_id.as_str().to_owned(),
                });
            }

            let candidate = selected_plan
                .plan
                .candidate()
                .ok_or(ElasticTransactionGateError::Probe(
                    ElasticProbeError::MissingPlanCandidate,
                ))?
                .clone();
            let planner = LockedCandidatePlanner { candidate };
            let cycle = runtime
                .cycle(&selected_plan.plan.resource, &planner, observer, actuator)
                .map_err(ElasticTransactionGateError::Runtime)?;

            Ok(ElasticTransactionOutcome::Cycle {
                scenario_id: scenario_id.clone(),
                declared_rollback: rollback.clone(),
                cycle: Box::new(cycle),
            })
        }
    }
}

impl fmt::Display for ElasticTransactionGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BaselineMismatch => formatter.write_str(
                "precommit no-op choice does not match the supplied Elastic probe set baseline",
            ),
            Self::SelectedProbeMissing { id } => {
                write!(
                    formatter,
                    "selected Elastic probe is absent from probe set: {id}"
                )
            }
            Self::PlanIntentMismatch { id } => write!(
                formatter,
                "action-time Elastic plan does not match prospectively selected probe: {id}"
            ),
            Self::RollbackIntentMismatch { id } => write!(
                formatter,
                "selected Elastic rollback intent differs from supplied probe set: {id}"
            ),
            Self::Probe(error) => write!(formatter, "invalid Elastic probe plan: {error}"),
            Self::Runtime(error) => {
                write!(formatter, "Elastic runtime transaction failed: {error}")
            }
        }
    }
}

impl std::error::Error for ElasticTransactionGateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Probe(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::BaselineMismatch
            | Self::SelectedProbeMissing { .. }
            | Self::PlanIntentMismatch { .. }
            | Self::RollbackIntentMismatch { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::convert::Infallible;
    use std::time::Instant;

    use elastic_eir::{FirstGroundedPlanner, PlanningContext};
    use elastic_runtime::{
        Actuation, CommitRecord, InvariantCheck, ObservationSnapshot, Plan, RollbackRecord,
        Runtime, RuntimeConfig, RuntimeMode, TransactionalActuator, ValidatedPlan,
        VerificationResult, plan::plan_with_context, plan::validate_with_checks,
    };
    use prospect_core::{DecisionPolicy, ScenarioId, SignatureMetric};

    use super::{ElasticTransactionOutcome, execute_selected_probe};
    use crate::{
        ElasticEngine, ElasticIntervention, ElasticObservationState, ElasticProbeSetV1,
        ElasticProbeV1, ElasticProspectiveModel, ValidatedPlanIntentV1, compare_before_commit,
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
            let delta = intervention
                .parameters()
                .get("delta")
                .copied()
                .unwrap_or(0.0);
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

    #[derive(Default)]
    struct CountingActuator {
        validate_calls: Cell<usize>,
        prepare_calls: usize,
        actuate_calls: usize,
        verify_calls: Cell<usize>,
        commit_calls: usize,
        rollback_calls: usize,
    }

    impl TransactionalActuator for CountingActuator {
        fn name(&self) -> &str {
            "prospect-test-actuator"
        }

        fn validate(
            &self,
            plan: &Plan,
        ) -> Result<Vec<InvariantCheck>, elastic_runtime::RuntimeError> {
            self.validate_calls.set(self.validate_calls.get() + 1);
            Ok(plan
                .resource
                .invariants()
                .iter()
                .cloned()
                .map(|invariant| InvariantCheck::new(invariant, true, None))
                .collect())
        }

        fn prepare(
            &mut self,
            plan: &ValidatedPlan,
        ) -> Result<Actuation, elastic_runtime::RuntimeError> {
            self.prepare_calls += 1;
            Ok(Actuation::new(
                plan.clone(),
                plan.plan
                    .candidate()
                    .and_then(|candidate| candidate.magnitude()),
                self.name(),
            ))
        }

        fn actuate(&mut self, _actuation: &Actuation) -> Result<(), elastic_runtime::RuntimeError> {
            self.actuate_calls += 1;
            Ok(())
        }

        fn verify(
            &self,
            _actuation: &Actuation,
        ) -> Result<VerificationResult, elastic_runtime::RuntimeError> {
            self.verify_calls.set(self.verify_calls.get() + 1);
            Ok(VerificationResult::Pass)
        }

        fn commit(
            &mut self,
            _actuation: &Actuation,
        ) -> Result<CommitRecord, elastic_runtime::RuntimeError> {
            self.commit_calls += 1;
            Ok(CommitRecord::new("test-transition", "test commit"))
        }

        fn rollback(
            &mut self,
            _actuation: &Actuation,
            _verification: &VerificationResult,
        ) -> Result<RollbackRecord, elastic_runtime::RuntimeError> {
            self.rollback_calls += 1;
            Ok(RollbackRecord::new(
                "test-transition",
                "test rollback",
                true,
            ))
        }
    }

    fn validated_plan() -> ValidatedPlan {
        let resource = RuntimeConfig::default().ir_resource;
        let plan = plan_with_context(&FirstGroundedPlanner, &resource, &PlanningContext::new());
        let checks = plan
            .resource
            .invariants()
            .iter()
            .cloned()
            .map(|invariant| InvariantCheck::new(invariant, true, None))
            .collect();
        validate_with_checks(plan, checks)
    }

    fn state() -> ElasticObservationState {
        ElasticObservationState::from_snapshot(&ObservationSnapshot::new(
            Instant::now(),
            Vec::new(),
        ))
        .expect("state")
    }

    fn intervention(kind: &str, delta: f64) -> ElasticIntervention {
        ElasticIntervention::new(kind, BTreeMap::from([("delta".to_owned(), delta)]))
            .expect("intervention")
    }

    fn probes(plan: &ValidatedPlan, delta: f64) -> ElasticProbeSetV1 {
        let intent = ValidatedPlanIntentV1::from_validated_plan(plan).expect("intent");
        let probe = ElasticProbeV1::new(
            ScenarioId::new("candidate").expect("id"),
            intent,
            intervention("forward", delta),
            intervention("declared-rollback", -delta),
        );
        ElasticProbeSetV1::new(ScenarioId::new("noop").expect("baseline"), vec![probe])
            .expect("probes")
    }

    fn runtime() -> Runtime {
        Runtime::new(RuntimeConfig {
            mode: RuntimeMode::Apply,
            dry_run: false,
            ..RuntimeConfig::default()
        })
    }

    #[test]
    fn noop_never_touches_transactional_actuator() {
        let plan = validated_plan();
        let probes = probes(&plan, 0.0);
        let comparison = compare_before_commit(
            &ElasticEngine::new(DeltaModel),
            &state(),
            &probes,
            &AbsoluteDistance,
            &PreferHigher,
        )
        .expect("comparison");
        let mut actuator = CountingActuator::default();

        let outcome =
            execute_selected_probe(&runtime(), &comparison, &probes, &plan, &(), &mut actuator)
                .expect("no-op gate");

        assert!(outcome.is_noop());
        assert_eq!(actuator.validate_calls.get(), 0);
        assert_eq!(actuator.prepare_calls, 0);
        assert_eq!(actuator.actuate_calls, 0);
        assert_eq!(actuator.commit_calls, 0);
    }

    #[test]
    fn selected_probe_reenters_elastic_trusted_transaction_boundary() {
        let plan = validated_plan();
        let probes = probes(&plan, 5.0);
        let comparison = compare_before_commit(
            &ElasticEngine::new(DeltaModel),
            &state(),
            &probes,
            &AbsoluteDistance,
            &PreferHigher,
        )
        .expect("comparison");
        let mut actuator = CountingActuator::default();

        let outcome =
            execute_selected_probe(&runtime(), &comparison, &probes, &plan, &(), &mut actuator)
                .expect("transaction gate");

        match outcome {
            ElasticTransactionOutcome::Cycle {
                declared_rollback,
                cycle,
                ..
            } => {
                assert_eq!(declared_rollback.kind(), "declared-rollback");
                assert!(cycle.commit.is_some());
                assert!(cycle.rollback.is_none());
            }
            ElasticTransactionOutcome::NoOp { .. } => panic!("probe should have executed"),
        }
        assert_eq!(actuator.validate_calls.get(), 1);
        assert_eq!(actuator.prepare_calls, 1);
        assert_eq!(actuator.actuate_calls, 1);
        assert_eq!(actuator.verify_calls.get(), 1);
        assert_eq!(actuator.commit_calls, 1);
        assert_eq!(actuator.rollback_calls, 0);
    }
}
