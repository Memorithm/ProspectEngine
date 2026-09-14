use std::collections::BTreeMap;
use std::fmt;

use elastic_runtime::VerificationResult;
use prospect_core::{DecisionPolicy, ScenarioId};
use prospect_evidence::{
    DECISION_EVIDENCE_SCHEMA_V1, DecisionEvidence, EvidenceError, EvidenceNature, EvidenceSource,
    RunId,
};
use prospect_scenario::ScenarioScore;
use serde::{Deserialize, Serialize};

use crate::{
    ELASTICXXX_REVISION, ELASTIC_PROBE_SCHEMA_V1, ElasticIntervention, ElasticPrecommitComparison,
    ElasticProbeError, ElasticProbeSetV1, ElasticTransactionOutcome, ValidatedPlanIntentV1,
};

pub const ELASTIC_EXECUTION_EVIDENCE_SCHEMA_V1: &str = "prospect.elastic-execution-evidence/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElasticExecutionDisposition {
    NoOp,
    NoPhysicalActuation,
    Committed,
    RolledBack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ElasticVerificationEvidence {
    NotPerformed,
    Pass,
    Fail { detail: String },
    Inconclusive { detail: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElasticCommitEvidence {
    transition: String,
    rationale: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElasticRollbackEvidence {
    transition: String,
    rationale: String,
    invariants_restored: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ElasticInterventionEvidence {
    kind: String,
    parameters: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ElasticExecutionEvidenceV1 {
    schema: String,
    run_id: String,
    decision_schema: String,
    probe_schema: String,
    elastic_revision: String,
    resource_id: String,
    resource_fingerprint: String,
    scenario_id: String,
    disposition: ElasticExecutionDisposition,
    action_time_plan_validated: Option<bool>,
    actuation_performed: bool,
    verification: ElasticVerificationEvidence,
    commit: Option<ElasticCommitEvidence>,
    rollback: Option<ElasticRollbackEvidence>,
    declared_rollback: Option<ElasticInterventionEvidence>,
    observation_snapshot_count: usize,
    event_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElasticExecutionEvidenceBundle<PolicyScore> {
    decision: DecisionEvidence<PolicyScore>,
    execution: ElasticExecutionEvidenceV1,
}

#[derive(Debug)]
pub enum ElasticExecutionEvidenceError {
    Evidence(EvidenceError),
    Probe(ElasticProbeError),
    Json(serde_json::Error),
    UnsupportedSchema,
    UnsupportedDecisionSchema,
    UnsupportedProbeSchema,
    ElasticRevisionMismatch,
    InvalidResourceId,
    InvalidResourceFingerprint,
    ChoiceScenarioMismatch,
    ChoiceUtilityDrift,
    OutcomeScenarioMismatch,
    InconsistentCycleOutcome,
    ReplayMismatch,
}

impl ElasticCommitEvidence {
    #[must_use]
    pub fn transition(&self) -> &str {
        &self.transition
    }

    #[must_use]
    pub fn rationale(&self) -> &str {
        &self.rationale
    }
}

impl ElasticRollbackEvidence {
    #[must_use]
    pub fn transition(&self) -> &str {
        &self.transition
    }

    #[must_use]
    pub fn rationale(&self) -> &str {
        &self.rationale
    }

    #[must_use]
    pub const fn invariants_restored(&self) -> bool {
        self.invariants_restored
    }
}

impl ElasticInterventionEvidence {
    fn from_intervention(intervention: &ElasticIntervention) -> Self {
        Self {
            kind: intervention.kind().to_owned(),
            parameters: intervention.parameters().clone(),
        }
    }

    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    #[must_use]
    pub fn parameters(&self) -> &BTreeMap<String, f64> {
        &self.parameters
    }
}

impl ElasticExecutionEvidenceV1 {
    fn from_transaction_outcome(
        run_id: &RunId,
        resource: &ValidatedPlanIntentV1,
        scenario_id: &ScenarioId,
        probe_schema: &str,
        outcome: &ElasticTransactionOutcome,
    ) -> Result<Self, ElasticExecutionEvidenceError> {
        if outcome.scenario_id() != scenario_id {
            return Err(ElasticExecutionEvidenceError::OutcomeScenarioMismatch);
        }

        let resource_fingerprint = format!(
            "eir-fp:{:016x}",
            resource.resource_fingerprint()
        );

        match outcome {
            ElasticTransactionOutcome::NoOp { .. } => Ok(Self {
                schema: ELASTIC_EXECUTION_EVIDENCE_SCHEMA_V1.to_owned(),
                run_id: run_id.as_str().to_owned(),
                decision_schema: DECISION_EVIDENCE_SCHEMA_V1.to_owned(),
                probe_schema: probe_schema.to_owned(),
                elastic_revision: ELASTICXXX_REVISION.to_owned(),
                resource_id: resource.resource_id().to_owned(),
                resource_fingerprint,
                scenario_id: scenario_id.as_str().to_owned(),
                disposition: ElasticExecutionDisposition::NoOp,
                action_time_plan_validated: None,
                actuation_performed: false,
                verification: ElasticVerificationEvidence::NotPerformed,
                commit: None,
                rollback: None,
                declared_rollback: None,
                observation_snapshot_count: 0,
                event_count: 0,
            }),
            ElasticTransactionOutcome::Cycle {
                declared_rollback,
                cycle,
                ..
            } => {
                if cycle.commit.is_some() && cycle.rollback.is_some() {
                    return Err(ElasticExecutionEvidenceError::InconsistentCycleOutcome);
                }

                let actuation_performed = cycle.actuation.is_some();
                let action_time_plan_validated = cycle.plan.as_ref().map(|plan| plan.validated);
                let verification = match &cycle.verification {
                    None => ElasticVerificationEvidence::NotPerformed,
                    Some(VerificationResult::Pass) => ElasticVerificationEvidence::Pass,
                    Some(VerificationResult::Fail { detail }) => {
                        ElasticVerificationEvidence::Fail {
                            detail: detail.clone(),
                        }
                    }
                    Some(VerificationResult::Inconclusive { detail }) => {
                        ElasticVerificationEvidence::Inconclusive {
                            detail: detail.clone(),
                        }
                    }
                };
                let commit = cycle.commit.as_ref().map(|commit| ElasticCommitEvidence {
                    transition: commit.transition.clone(),
                    rationale: commit.rationale.clone(),
                });
                let rollback = cycle
                    .rollback
                    .as_ref()
                    .map(|rollback| ElasticRollbackEvidence {
                        transition: rollback.transition.clone(),
                        rationale: rollback.rationale.clone(),
                        invariants_restored: rollback.invariants_restored,
                    });

                let disposition = if commit.is_some() {
                    if !actuation_performed
                        || action_time_plan_validated != Some(true)
                        || verification != ElasticVerificationEvidence::Pass
                    {
                        return Err(ElasticExecutionEvidenceError::InconsistentCycleOutcome);
                    }
                    ElasticExecutionDisposition::Committed
                } else if rollback.is_some() {
                    if !actuation_performed || action_time_plan_validated != Some(true) {
                        return Err(ElasticExecutionEvidenceError::InconsistentCycleOutcome);
                    }
                    ElasticExecutionDisposition::RolledBack
                } else if actuation_performed {
                    return Err(ElasticExecutionEvidenceError::InconsistentCycleOutcome);
                } else {
                    ElasticExecutionDisposition::NoPhysicalActuation
                };

                Ok(Self {
                    schema: ELASTIC_EXECUTION_EVIDENCE_SCHEMA_V1.to_owned(),
                    run_id: run_id.as_str().to_owned(),
                    decision_schema: DECISION_EVIDENCE_SCHEMA_V1.to_owned(),
                    probe_schema: probe_schema.to_owned(),
                    elastic_revision: ELASTICXXX_REVISION.to_owned(),
                    resource_id: resource.resource_id().to_owned(),
                    resource_fingerprint,
                    scenario_id: scenario_id.as_str().to_owned(),
                    disposition,
                    action_time_plan_validated,
                    actuation_performed,
                    verification,
                    commit,
                    rollback,
                    declared_rollback: Some(ElasticInterventionEvidence::from_intervention(
                        declared_rollback,
                    )),
                    observation_snapshot_count: cycle.observations.len(),
                    event_count: cycle.events.len(),
                })
            }
        }
    }

    pub fn canonical_json(&self) -> Result<String, ElasticExecutionEvidenceError> {
        serde_json::to_string(self).map_err(ElasticExecutionEvidenceError::Json)
    }

    pub fn from_canonical_json(json: &str) -> Result<Self, ElasticExecutionEvidenceError> {
        let evidence: Self =
            serde_json::from_str(json).map_err(ElasticExecutionEvidenceError::Json)?;
        evidence.validate_contract()?;
        Ok(evidence)
    }

    fn validate_contract(&self) -> Result<(), ElasticExecutionEvidenceError> {
        if self.schema != ELASTIC_EXECUTION_EVIDENCE_SCHEMA_V1 {
            return Err(ElasticExecutionEvidenceError::UnsupportedSchema);
        }
        if self.decision_schema != DECISION_EVIDENCE_SCHEMA_V1 {
            return Err(ElasticExecutionEvidenceError::UnsupportedDecisionSchema);
        }
        if self.probe_schema != ELASTIC_PROBE_SCHEMA_V1 {
            return Err(ElasticExecutionEvidenceError::UnsupportedProbeSchema);
        }
        if self.elastic_revision != ELASTICXXX_REVISION {
            return Err(ElasticExecutionEvidenceError::ElasticRevisionMismatch);
        }
        RunId::new(self.run_id.clone()).map_err(ElasticExecutionEvidenceError::Evidence)?;
        ScenarioId::new(self.scenario_id.clone())
            .map_err(|_| ElasticExecutionEvidenceError::ChoiceScenarioMismatch)?;
        if self.resource_id.trim().is_empty() {
            return Err(ElasticExecutionEvidenceError::InvalidResourceId);
        }
        let Some(fingerprint) = self.resource_fingerprint.strip_prefix("eir-fp:") else {
            return Err(ElasticExecutionEvidenceError::InvalidResourceFingerprint);
        };
        if fingerprint.len() != 16 || u64::from_str_radix(fingerprint, 16).is_err() {
            return Err(ElasticExecutionEvidenceError::InvalidResourceFingerprint);
        }

        match self.disposition {
            ElasticExecutionDisposition::NoOp => {
                if self.action_time_plan_validated.is_some()
                    || self.actuation_performed
                    || self.verification != ElasticVerificationEvidence::NotPerformed
                    || self.commit.is_some()
                    || self.rollback.is_some()
                    || self.declared_rollback.is_some()
                    || self.observation_snapshot_count != 0
                    || self.event_count != 0
                {
                    return Err(ElasticExecutionEvidenceError::InconsistentCycleOutcome);
                }
            }
            ElasticExecutionDisposition::NoPhysicalActuation => {
                if self.actuation_performed || self.commit.is_some() || self.rollback.is_some() {
                    return Err(ElasticExecutionEvidenceError::InconsistentCycleOutcome);
                }
            }
            ElasticExecutionDisposition::Committed => {
                if !self.actuation_performed
                    || self.action_time_plan_validated != Some(true)
                    || self.verification != ElasticVerificationEvidence::Pass
                    || self.commit.is_none()
                    || self.rollback.is_some()
                    || self.declared_rollback.is_none()
                {
                    return Err(ElasticExecutionEvidenceError::InconsistentCycleOutcome);
                }
            }
            ElasticExecutionDisposition::RolledBack => {
                if !self.actuation_performed
                    || self.action_time_plan_validated != Some(true)
                    || self.commit.is_some()
                    || self.rollback.is_none()
                    || self.declared_rollback.is_none()
                {
                    return Err(ElasticExecutionEvidenceError::InconsistentCycleOutcome);
                }
            }
        }

        Ok(())
    }

    pub fn verify_replay(&self, replayed: &Self) -> Result<(), ElasticExecutionEvidenceError> {
        if self == replayed {
            Ok(())
        } else {
            Err(ElasticExecutionEvidenceError::ReplayMismatch)
        }
    }

    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    #[must_use]
    pub fn resource_id(&self) -> &str {
        &self.resource_id
    }

    #[must_use]
    pub fn resource_fingerprint(&self) -> &str {
        &self.resource_fingerprint
    }

    #[must_use]
    pub fn scenario_id(&self) -> &str {
        &self.scenario_id
    }

    #[must_use]
    pub const fn disposition(&self) -> ElasticExecutionDisposition {
        self.disposition
    }

    #[must_use]
    pub const fn action_time_plan_validated(&self) -> Option<bool> {
        self.action_time_plan_validated
    }

    #[must_use]
    pub const fn actuation_performed(&self) -> bool {
        self.actuation_performed
    }

    #[must_use]
    pub const fn verification(&self) -> &ElasticVerificationEvidence {
        &self.verification
    }

    #[must_use]
    pub const fn commit(&self) -> Option<&ElasticCommitEvidence> {
        self.commit.as_ref()
    }

    #[must_use]
    pub const fn rollback(&self) -> Option<&ElasticRollbackEvidence> {
        self.rollback.as_ref()
    }

    #[must_use]
    pub const fn declared_rollback(&self) -> Option<&ElasticInterventionEvidence> {
        self.declared_rollback.as_ref()
    }
}

impl<PolicyScore> ElasticExecutionEvidenceBundle<PolicyScore> {
    #[must_use]
    pub const fn decision(&self) -> &DecisionEvidence<PolicyScore> {
        &self.decision
    }

    #[must_use]
    pub const fn execution(&self) -> &ElasticExecutionEvidenceV1 {
        &self.execution
    }
}

/// Captures the prospective policy decision and the actual ElasticXxx
/// transaction result as two canonical, run-id-linked evidence records.
///
/// The generic [`DecisionEvidence`] records every policy utility, including the
/// explicit no-op baseline. The Elastic-specific execution record captures only
/// observed transaction facts. This keeps inferred/simulated decision evidence
/// separate from physical outcome evidence while retaining one stable run id.
pub fn capture_elastic_execution_evidence<Signature, MetricScore, Policy, PolicyScore>(
    run_id: RunId,
    mut sources: Vec<EvidenceSource>,
    comparison: &ElasticPrecommitComparison<Signature, MetricScore, PolicyScore>,
    probes: &ElasticProbeSetV1,
    selected_plan: &elastic_runtime::ValidatedPlan,
    outcome: &ElasticTransactionOutcome,
    policy: &Policy,
) -> Result<ElasticExecutionEvidenceBundle<PolicyScore>, ElasticExecutionEvidenceError>
where
    Policy: DecisionPolicy<Signature, Score = PolicyScore>,
    PolicyScore: Clone + PartialEq,
{
    let selected_id = comparison.choice().scenario_id().clone();
    if outcome.scenario_id() != &selected_id {
        return Err(ElasticExecutionEvidenceError::OutcomeScenarioMismatch);
    }

    let resource = ValidatedPlanIntentV1::from_validated_plan(selected_plan)
        .map_err(ElasticExecutionEvidenceError::Probe)?;

    sources.push(
        EvidenceSource::new_with_nature(
            "ElasticXxx",
            ELASTICXXX_REVISION,
            EvidenceNature::Observed,
        )
        .map_err(ElasticExecutionEvidenceError::Evidence)?,
    );

    let mut policy_scores = Vec::with_capacity(comparison.batch().outcomes().len() + 1);
    policy_scores.push(ScenarioScore {
        scenario_id: probes.baseline_id().clone(),
        score: policy.utility(comparison.batch().baseline()),
    });
    policy_scores.extend(comparison.batch().outcomes().iter().map(|candidate| {
        ScenarioScore {
            scenario_id: candidate.scenario().id().clone(),
            score: policy.utility(candidate.signature()),
        }
    }));

    let selected_score = policy_scores
        .iter()
        .find(|score| score.scenario_id == selected_id)
        .ok_or(ElasticExecutionEvidenceError::ChoiceScenarioMismatch)?;
    if selected_score.score != *comparison.choice().utility() {
        return Err(ElasticExecutionEvidenceError::ChoiceUtilityDrift);
    }

    let decision = DecisionEvidence::from_scores(
        run_id.clone(),
        sources,
        policy_scores,
        Some(selected_id.clone()),
    )
    .map_err(ElasticExecutionEvidenceError::Evidence)?;
    let execution = ElasticExecutionEvidenceV1::from_transaction_outcome(
        &run_id,
        &resource,
        &selected_id,
        probes.schema(),
        outcome,
    )?;

    Ok(ElasticExecutionEvidenceBundle {
        decision,
        execution,
    })
}

impl fmt::Display for ElasticExecutionEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evidence(error) => write!(formatter, "invalid decision evidence: {error}"),
            Self::Probe(error) => write!(formatter, "invalid Elastic probe evidence: {error}"),
            Self::Json(error) => write!(formatter, "invalid Elastic execution evidence JSON: {error}"),
            Self::UnsupportedSchema => formatter.write_str("unsupported Elastic execution evidence schema"),
            Self::UnsupportedDecisionSchema => formatter.write_str("unsupported linked decision evidence schema"),
            Self::UnsupportedProbeSchema => formatter.write_str("unsupported linked Elastic probe schema"),
            Self::ElasticRevisionMismatch => formatter.write_str("Elastic execution evidence revision does not match the pinned runtime"),
            Self::InvalidResourceId => formatter.write_str("Elastic execution evidence has an empty resource id"),
            Self::InvalidResourceFingerprint => formatter.write_str("Elastic execution evidence has an invalid EIR fingerprint"),
            Self::ChoiceScenarioMismatch => formatter.write_str("prospective choice is absent from the recorded policy utilities"),
            Self::ChoiceUtilityDrift => formatter.write_str("prospective choice utility changed while capturing evidence"),
            Self::OutcomeScenarioMismatch => formatter.write_str("physical transaction outcome does not match the prospective choice"),
            Self::InconsistentCycleOutcome => formatter.write_str("Elastic runtime cycle has an inconsistent physical outcome"),
            Self::ReplayMismatch => formatter.write_str("replayed Elastic execution evidence differs from the recorded outcome"),
        }
    }
}

impl std::error::Error for ElasticExecutionEvidenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evidence(error) => Some(error),
            Self::Probe(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::UnsupportedSchema
            | Self::UnsupportedDecisionSchema
            | Self::UnsupportedProbeSchema
            | Self::ElasticRevisionMismatch
            | Self::InvalidResourceId
            | Self::InvalidResourceFingerprint
            | Self::ChoiceScenarioMismatch
            | Self::ChoiceUtilityDrift
            | Self::OutcomeScenarioMismatch
            | Self::InconsistentCycleOutcome
            | Self::ReplayMismatch => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::convert::Infallible;
    use std::time::Instant;

    use elastic_eir::{FirstGroundedPlanner, PlanningContext};
    use elastic_runtime::{
        Actuation, CommitRecord, CycleResult, ObservationSnapshot, RuntimeConfig, ValidatedPlan,
        VerificationResult, plan::plan_with_context, plan::validate_with_checks,
    };
    use prospect_core::{DecisionPolicy, ScenarioId, SignatureMetric};
    use prospect_evidence::{EvidenceNature, EvidenceSource, RunId};

    use super::{
        ElasticExecutionDisposition, ElasticExecutionEvidenceV1, capture_elastic_execution_evidence,
    };
    use crate::{
        ElasticEngine, ElasticIntervention, ElasticObservationState, ElasticProbeSetV1,
        ElasticProbeV1, ElasticProspectiveModel, ElasticTransactionOutcome, ValidatedPlanIntentV1,
        compare_before_commit,
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

    struct Distance;

    impl SignatureMetric<i32> for Distance {
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

    fn plan() -> ValidatedPlan {
        let resource = RuntimeConfig::default().ir_resource;
        let plan = plan_with_context(&FirstGroundedPlanner, &resource, &PlanningContext::new());
        let checks = resource
            .invariants()
            .iter()
            .cloned()
            .map(|invariant| elastic_runtime::InvariantCheck::new(invariant, true, None))
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
        let probe = ElasticProbeV1::new(
            ScenarioId::new("candidate").expect("id"),
            ValidatedPlanIntentV1::from_validated_plan(plan).expect("intent"),
            intervention("forward", delta),
            intervention("declared-rollback", -delta),
        );
        ElasticProbeSetV1::new(ScenarioId::new("noop").expect("baseline"), vec![probe])
            .expect("probes")
    }

    fn source() -> EvidenceSource {
        EvidenceSource::new_with_nature("test-model", "rev-1", EvidenceNature::Inferred)
            .expect("source")
    }

    #[test]
    fn noop_is_a_first_class_selected_decision_without_physical_effects() {
        let plan = plan();
        let probes = probes(&plan, 0.0);
        let comparison = compare_before_commit(
            &ElasticEngine::new(DeltaModel),
            &state(),
            &probes,
            &Distance,
            &PreferHigher,
        )
        .expect("comparison");
        let outcome = ElasticTransactionOutcome::NoOp {
            scenario_id: ScenarioId::new("noop").expect("id"),
        };

        let bundle = capture_elastic_execution_evidence(
            RunId::new("run-noop").expect("run id"),
            vec![source()],
            &comparison,
            &probes,
            &plan,
            &outcome,
            &PreferHigher,
        )
        .expect("evidence");

        assert_eq!(bundle.decision().selected().expect("selection").as_str(), "noop");
        assert_eq!(bundle.decision().candidates().len(), 2);
        assert_eq!(bundle.execution().disposition(), ElasticExecutionDisposition::NoOp);
        assert!(!bundle.execution().actuation_performed());
    }

    #[test]
    fn committed_cycle_round_trips_as_canonical_execution_evidence() {
        let plan = plan();
        let probes = probes(&plan, 5.0);
        let comparison = compare_before_commit(
            &ElasticEngine::new(DeltaModel),
            &state(),
            &probes,
            &Distance,
            &PreferHigher,
        )
        .expect("comparison");
        let actuation = Actuation::new(plan.clone(), None, "test-adapter");
        let cycle = CycleResult {
            observations: Vec::new(),
            plan: Some(plan.clone()),
            actuation: Some(actuation),
            verification: Some(VerificationResult::Pass),
            commit: Some(CommitRecord::new("candidate", "verified")),
            rollback: None,
            events: Vec::new(),
        };
        let outcome = ElasticTransactionOutcome::Cycle {
            scenario_id: ScenarioId::new("candidate").expect("id"),
            declared_rollback: probes.probes()[0].rollback().clone(),
            cycle: Box::new(cycle),
        };

        let bundle = capture_elastic_execution_evidence(
            RunId::new("run-commit").expect("run id"),
            vec![source()],
            &comparison,
            &probes,
            &plan,
            &outcome,
            &PreferHigher,
        )
        .expect("evidence");
        let json = bundle.execution().canonical_json().expect("json");
        let decoded = ElasticExecutionEvidenceV1::from_canonical_json(&json).expect("decode");

        bundle.execution().verify_replay(&decoded).expect("replay");
        assert_eq!(decoded.disposition(), ElasticExecutionDisposition::Committed);
        assert_eq!(decoded.commit().expect("commit").transition(), "candidate");
        assert_eq!(
            decoded.declared_rollback().expect("rollback intent").kind(),
            "declared-rollback"
        );
    }
}
