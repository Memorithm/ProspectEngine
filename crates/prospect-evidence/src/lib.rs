#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use prospect_core::ScenarioId;
use prospect_scenario::ScenarioScore;

pub const DECISION_EVIDENCE_SCHEMA_V1: &str = "prospect.decision-evidence/v1";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceSource {
    component: String,
    revision: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceError {
    EmptyRunId,
    EmptyComponent,
    EmptyRevision,
    EmptySources,
    DuplicateScenario,
    UnknownSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateEvidence<Score> {
    scenario_id: ScenarioId,
    score: Score,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionEvidence<Score> {
    schema: &'static str,
    run_id: RunId,
    sources: Vec<EvidenceSource>,
    candidates: Vec<CandidateEvidence<Score>>,
    selected: Option<ScenarioId>,
}

impl RunId {
    pub fn new(value: impl Into<String>) -> Result<Self, EvidenceError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(EvidenceError::EmptyRunId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl EvidenceSource {
    pub fn new(
        component: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, EvidenceError> {
        let component = component.into();
        if component.trim().is_empty() {
            return Err(EvidenceError::EmptyComponent);
        }

        let revision = revision.into();
        if revision.trim().is_empty() {
            return Err(EvidenceError::EmptyRevision);
        }

        Ok(Self {
            component,
            revision,
        })
    }

    #[must_use]
    pub fn component(&self) -> &str {
        &self.component
    }

    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }
}

impl<Score> CandidateEvidence<Score> {
    #[must_use]
    pub const fn scenario_id(&self) -> &ScenarioId {
        &self.scenario_id
    }

    #[must_use]
    pub const fn score(&self) -> &Score {
        &self.score
    }
}

impl<Score> DecisionEvidence<Score> {
    pub fn from_scores(
        run_id: RunId,
        sources: Vec<EvidenceSource>,
        scores: Vec<ScenarioScore<Score>>,
        selected: Option<ScenarioId>,
    ) -> Result<Self, EvidenceError> {
        if sources.is_empty() {
            return Err(EvidenceError::EmptySources);
        }

        let mut seen = BTreeSet::new();
        let mut candidates = Vec::with_capacity(scores.len());
        for score in scores {
            if !seen.insert(score.scenario_id.clone()) {
                return Err(EvidenceError::DuplicateScenario);
            }
            candidates.push(CandidateEvidence {
                scenario_id: score.scenario_id,
                score: score.score,
            });
        }

        if let Some(selected_id) = &selected
            && !seen.contains(selected_id)
        {
            return Err(EvidenceError::UnknownSelection);
        }

        Ok(Self {
            schema: DECISION_EVIDENCE_SCHEMA_V1,
            run_id,
            sources,
            candidates,
            selected,
        })
    }

    #[must_use]
    pub const fn schema(&self) -> &'static str {
        self.schema
    }

    #[must_use]
    pub const fn run_id(&self) -> &RunId {
        &self.run_id
    }

    #[must_use]
    pub fn sources(&self) -> &[EvidenceSource] {
        &self.sources
    }

    #[must_use]
    pub fn candidates(&self) -> &[CandidateEvidence<Score>] {
        &self.candidates
    }

    #[must_use]
    pub const fn selected(&self) -> Option<&ScenarioId> {
        self.selected.as_ref()
    }
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyRunId => "run id must not be empty",
            Self::EmptyComponent => "evidence source component must not be empty",
            Self::EmptyRevision => "evidence source revision must not be empty",
            Self::EmptySources => "at least one evidence source is required",
            Self::DuplicateScenario => "decision evidence contains a duplicate scenario id",
            Self::UnknownSelection => "selected scenario is not present in candidate evidence",
        })
    }
}

impl std::error::Error for EvidenceError {}

#[cfg(test)]
mod tests {
    use prospect_core::ScenarioId;
    use prospect_scenario::ScenarioScore;

    use super::{DecisionEvidence, EvidenceError, EvidenceSource, RunId};

    #[test]
    fn records_sources_scores_and_selection() {
        let selected = ScenarioId::new("reduce-concurrency").expect("id");
        let evidence = DecisionEvidence::from_scores(
            RunId::new("run-0001").expect("run id"),
            vec![EvidenceSource::new("ElasticXxx", "50bb85e").expect("source")],
            vec![
                ScenarioScore {
                    scenario_id: ScenarioId::new("evict-kv").expect("id"),
                    score: 4_i32,
                },
                ScenarioScore {
                    scenario_id: selected.clone(),
                    score: 9_i32,
                },
            ],
            Some(selected.clone()),
        )
        .expect("valid evidence");

        assert_eq!(evidence.selected(), Some(&selected));
        assert_eq!(evidence.sources()[0].component(), "ElasticXxx");
        assert_eq!(evidence.candidates().len(), 2);
    }

    #[test]
    fn rejects_a_selection_missing_from_candidates() {
        let result = DecisionEvidence::<i32>::from_scores(
            RunId::new("run-0002").expect("run id"),
            vec![EvidenceSource::new("ProspectEngine", "abc").expect("source")],
            vec![],
            Some(ScenarioId::new("missing").expect("id")),
        );

        assert_eq!(result, Err(EvidenceError::UnknownSelection));
    }

    #[test]
    fn rejects_duplicate_scenario_ids() {
        let id = ScenarioId::new("same").expect("id");
        let result = DecisionEvidence::from_scores(
            RunId::new("run-0003").expect("run id"),
            vec![EvidenceSource::new("ProspectEngine", "abc").expect("source")],
            vec![
                ScenarioScore {
                    scenario_id: id.clone(),
                    score: 1_i32,
                },
                ScenarioScore {
                    scenario_id: id,
                    score: 2_i32,
                },
            ],
            None,
        );

        assert_eq!(result, Err(EvidenceError::DuplicateScenario));
    }
}
