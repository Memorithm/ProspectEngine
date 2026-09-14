use std::collections::BTreeSet;
use std::fmt;

use elastic_runtime::ValidatedPlan;
use prospect_core::{Scenario, ScenarioId};

use crate::{ElasticBridgeError, ElasticIntervention};

pub const ELASTIC_PROBE_SCHEMA_V1: &str = "prospect.elastic-probe/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedPlanIntentV1 {
    candidate: String,
    dimension: String,
    magnitude: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElasticProbeV1 {
    id: ScenarioId,
    plan: ValidatedPlanIntentV1,
    forward: ElasticIntervention,
    rollback: ElasticIntervention,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElasticProbeSetV1 {
    schema: &'static str,
    baseline_id: ScenarioId,
    probes: Vec<ElasticProbeV1>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElasticProbeError {
    PlanNotValidated,
    MissingPlanCandidate,
    CandidateNotDeclared,
    DuplicateProbeId { id: String },
    ProbeConflictsWithBaseline { id: String },
    Bridge(ElasticBridgeError),
}

impl ValidatedPlanIntentV1 {
    pub fn from_validated_plan(validated: &ValidatedPlan) -> Result<Self, ElasticProbeError> {
        if !validated.validated {
            return Err(ElasticProbeError::PlanNotValidated);
        }

        let candidate = validated
            .plan
            .candidate()
            .ok_or(ElasticProbeError::MissingPlanCandidate)?;

        if !candidate.is_declared_in(&validated.plan.resource) {
            return Err(ElasticProbeError::CandidateNotDeclared);
        }

        Ok(Self {
            candidate: candidate.to_string(),
            dimension: candidate.dimension().as_str().to_owned(),
            magnitude: candidate.magnitude(),
        })
    }

    #[must_use]
    pub fn candidate(&self) -> &str {
        &self.candidate
    }

    #[must_use]
    pub fn dimension(&self) -> &str {
        &self.dimension
    }

    #[must_use]
    pub const fn magnitude(&self) -> Option<u64> {
        self.magnitude
    }
}

impl ElasticProbeV1 {
    #[must_use]
    pub fn new(
        id: ScenarioId,
        plan: ValidatedPlanIntentV1,
        forward: ElasticIntervention,
        rollback: ElasticIntervention,
    ) -> Self {
        Self {
            id,
            plan,
            forward,
            rollback,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &ScenarioId {
        &self.id
    }

    #[must_use]
    pub const fn plan(&self) -> &ValidatedPlanIntentV1 {
        &self.plan
    }

    #[must_use]
    pub const fn forward(&self) -> &ElasticIntervention {
        &self.forward
    }

    /// Rollback intent supplied by the trusted domain adapter.
    ///
    /// ProspectEngine records and preserves this paired intent but does not
    /// claim that the physical operation is reversible. The ElasticXxx
    /// `TransactionalActuator` remains responsible for action-time validation,
    /// verification, and rollback.
    #[must_use]
    pub const fn rollback(&self) -> &ElasticIntervention {
        &self.rollback
    }

    #[must_use]
    pub fn scenario(&self) -> Scenario<ElasticIntervention> {
        Scenario::new(self.id.clone(), self.forward.clone())
    }
}

impl ElasticProbeSetV1 {
    pub fn new(
        baseline_id: ScenarioId,
        mut probes: Vec<ElasticProbeV1>,
    ) -> Result<Self, ElasticProbeError> {
        probes.sort_by(|left, right| left.id.cmp(&right.id));

        let mut seen = BTreeSet::new();
        for probe in &probes {
            if probe.id == baseline_id {
                return Err(ElasticProbeError::ProbeConflictsWithBaseline {
                    id: probe.id.as_str().to_owned(),
                });
            }
            if !seen.insert(probe.id.clone()) {
                return Err(ElasticProbeError::DuplicateProbeId {
                    id: probe.id.as_str().to_owned(),
                });
            }
        }

        Ok(Self {
            schema: ELASTIC_PROBE_SCHEMA_V1,
            baseline_id,
            probes,
        })
    }

    #[must_use]
    pub const fn schema(&self) -> &'static str {
        self.schema
    }

    /// Identifies the explicit no-op reference evaluated through
    /// `ProspectiveEngine::baseline`, not through a fabricated intervention.
    #[must_use]
    pub const fn baseline_id(&self) -> &ScenarioId {
        &self.baseline_id
    }

    #[must_use]
    pub fn probes(&self) -> &[ElasticProbeV1] {
        &self.probes
    }

    #[must_use]
    pub fn scenarios(&self) -> Vec<Scenario<ElasticIntervention>> {
        self.probes.iter().map(ElasticProbeV1::scenario).collect()
    }
}

impl fmt::Display for ElasticProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanNotValidated => formatter.write_str("ElasticXxx plan is not validated"),
            Self::MissingPlanCandidate => {
                formatter.write_str("validated ElasticXxx plan has no transition candidate")
            }
            Self::CandidateNotDeclared => {
                formatter.write_str("ElasticXxx plan candidate is not declared by its resource")
            }
            Self::DuplicateProbeId { id } => write!(formatter, "duplicate Elastic probe id: {id}"),
            Self::ProbeConflictsWithBaseline { id } => {
                write!(formatter, "Elastic probe id conflicts with no-op baseline: {id}")
            }
            Self::Bridge(error) => write!(formatter, "invalid Elastic intervention: {error}"),
        }
    }
}

impl std::error::Error for ElasticProbeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bridge(error) => Some(error),
            Self::PlanNotValidated
            | Self::MissingPlanCandidate
            | Self::CandidateNotDeclared
            | Self::DuplicateProbeId { .. }
            | Self::ProbeConflictsWithBaseline { .. } => None,
        }
    }
}

impl From<ElasticBridgeError> for ElasticProbeError {
    fn from(error: ElasticBridgeError) -> Self {
        Self::Bridge(error)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use elastic_eir::{FirstGroundedPlanner, PlanningContext};
    use elastic_runtime::{
        InvariantCheck, RuntimeConfig, ValidatedPlan, plan::plan_with_context,
        plan::validate_with_checks,
    };
    use prospect_core::ScenarioId;

    use super::{ElasticProbeError, ElasticProbeSetV1, ElasticProbeV1, ValidatedPlanIntentV1};
    use crate::ElasticIntervention;

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

    fn intervention(kind: &str, target: f64) -> ElasticIntervention {
        ElasticIntervention::new(
            kind,
            BTreeMap::from([("target".to_owned(), target)]),
        )
        .expect("valid intervention")
    }

    #[test]
    fn extracts_only_validated_declared_plan_intent() {
        let validated = validated_plan();
        let intent = ValidatedPlanIntentV1::from_validated_plan(&validated).expect("intent");

        assert!(!intent.candidate().is_empty());
        assert!(!intent.dimension().is_empty());
    }

    #[test]
    fn rejects_unvalidated_plan() {
        let mut validated = validated_plan();
        validated.validated = false;

        assert_eq!(
            ValidatedPlanIntentV1::from_validated_plan(&validated),
            Err(ElasticProbeError::PlanNotValidated)
        );
    }

    #[test]
    fn probe_set_preserves_noop_baseline_and_rollback_pair() {
        let plan = ValidatedPlanIntentV1::from_validated_plan(&validated_plan()).expect("intent");
        let probe = ElasticProbeV1::new(
            ScenarioId::new("reduce-concurrency").expect("id"),
            plan,
            intervention("set-concurrency", 4.0),
            intervention("restore-concurrency", 8.0),
        );
        let probes = ElasticProbeSetV1::new(
            ScenarioId::new("noop").expect("baseline"),
            vec![probe],
        )
        .expect("probe set");

        assert_eq!(probes.baseline_id().as_str(), "noop");
        assert_eq!(probes.probes().len(), 1);
        assert_eq!(probes.probes()[0].rollback().kind(), "restore-concurrency");
        assert_eq!(probes.scenarios()[0].id().as_str(), "reduce-concurrency");
    }

    #[test]
    fn rejects_probe_id_equal_to_baseline() {
        let plan = ValidatedPlanIntentV1::from_validated_plan(&validated_plan()).expect("intent");
        let probe = ElasticProbeV1::new(
            ScenarioId::new("noop").expect("id"),
            plan,
            intervention("forward", 1.0),
            intervention("rollback", 0.0),
        );
        let result = ElasticProbeSetV1::new(ScenarioId::new("noop").expect("baseline"), vec![probe]);

        assert!(matches!(
            result,
            Err(ElasticProbeError::ProbeConflictsWithBaseline { .. })
        ));
    }
}
