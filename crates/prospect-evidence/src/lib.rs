#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use prospect_core::ScenarioId;
use prospect_scenario::ScenarioScore;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const DECISION_EVIDENCE_SCHEMA_V1: &str = "prospect.decision-evidence/v1";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceNature {
    Observed,
    Simulated,
    Inferred,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceSource {
    component: String,
    revision: String,
    nature: EvidenceNature,
    content_hash: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceError {
    EmptyRunId,
    EmptyComponent,
    EmptyRevision,
    EmptyContentHash,
    EmptySources,
    DuplicateSource,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayMismatch {
    Sources,
    Candidates,
    Selection,
}

#[derive(Debug)]
pub enum EvidenceCodecError {
    Json(serde_json::Error),
    UnsupportedSchema,
    InvalidScenarioId,
    Evidence(EvidenceError),
}

#[derive(Serialize)]
struct EvidenceSourceWireRef<'a> {
    component: &'a str,
    revision: &'a str,
    nature: EvidenceNature,
    content_hash: Option<&'a str>,
}

#[derive(Serialize)]
struct CandidateEvidenceWireRef<'a, Score> {
    scenario_id: &'a str,
    score: &'a Score,
}

#[derive(Serialize)]
struct DecisionEvidenceWireRef<'a, Score> {
    schema: &'static str,
    run_id: &'a str,
    sources: Vec<EvidenceSourceWireRef<'a>>,
    candidates: Vec<CandidateEvidenceWireRef<'a, Score>>,
    selected: Option<&'a str>,
}

#[derive(Deserialize)]
struct EvidenceSourceWire {
    component: String,
    revision: String,
    nature: EvidenceNature,
    content_hash: Option<String>,
}

#[derive(Deserialize)]
struct CandidateEvidenceWire<Score> {
    scenario_id: String,
    score: Score,
}

#[derive(Deserialize)]
struct DecisionEvidenceWire<Score> {
    schema: String,
    run_id: String,
    sources: Vec<EvidenceSourceWire>,
    candidates: Vec<CandidateEvidenceWire<Score>>,
    selected: Option<String>,
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
        Self::new_with_nature(component, revision, EvidenceNature::Observed)
    }

    pub fn new_with_nature(
        component: impl Into<String>,
        revision: impl Into<String>,
        nature: EvidenceNature,
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
            nature,
            content_hash: None,
        })
    }

    pub fn with_content_hash(
        mut self,
        content_hash: impl Into<String>,
    ) -> Result<Self, EvidenceError> {
        let content_hash = content_hash.into();
        if content_hash.trim().is_empty() {
            return Err(EvidenceError::EmptyContentHash);
        }
        self.content_hash = Some(content_hash);
        Ok(self)
    }

    #[must_use]
    pub fn component(&self) -> &str {
        &self.component
    }

    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }

    #[must_use]
    pub const fn nature(&self) -> EvidenceNature {
        self.nature
    }

    #[must_use]
    pub fn content_hash(&self) -> Option<&str> {
        self.content_hash.as_deref()
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
        mut sources: Vec<EvidenceSource>,
        scores: Vec<ScenarioScore<Score>>,
        selected: Option<ScenarioId>,
    ) -> Result<Self, EvidenceError> {
        if sources.is_empty() {
            return Err(EvidenceError::EmptySources);
        }

        sources.sort();
        if sources.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(EvidenceError::DuplicateSource);
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
        candidates.sort_by(|left, right| left.scenario_id.cmp(&right.scenario_id));

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

    pub fn canonical_json(&self) -> Result<String, EvidenceCodecError>
    where
        Score: Serialize,
    {
        let wire = DecisionEvidenceWireRef {
            schema: self.schema,
            run_id: self.run_id.as_str(),
            sources: self
                .sources
                .iter()
                .map(|source| EvidenceSourceWireRef {
                    component: source.component(),
                    revision: source.revision(),
                    nature: source.nature(),
                    content_hash: source.content_hash(),
                })
                .collect(),
            candidates: self
                .candidates
                .iter()
                .map(|candidate| CandidateEvidenceWireRef {
                    scenario_id: candidate.scenario_id().as_str(),
                    score: candidate.score(),
                })
                .collect(),
            selected: self.selected.as_ref().map(ScenarioId::as_str),
        };

        serde_json::to_string(&wire).map_err(EvidenceCodecError::Json)
    }

    pub fn from_canonical_json(json: &str) -> Result<Self, EvidenceCodecError>
    where
        Score: DeserializeOwned,
    {
        let wire: DecisionEvidenceWire<Score> =
            serde_json::from_str(json).map_err(EvidenceCodecError::Json)?;
        if wire.schema != DECISION_EVIDENCE_SCHEMA_V1 {
            return Err(EvidenceCodecError::UnsupportedSchema);
        }

        let run_id = RunId::new(wire.run_id).map_err(EvidenceCodecError::Evidence)?;
        let mut sources = Vec::with_capacity(wire.sources.len());
        for source in wire.sources {
            let mut converted =
                EvidenceSource::new_with_nature(source.component, source.revision, source.nature)
                    .map_err(EvidenceCodecError::Evidence)?;
            if let Some(content_hash) = source.content_hash {
                converted = converted
                    .with_content_hash(content_hash)
                    .map_err(EvidenceCodecError::Evidence)?;
            }
            sources.push(converted);
        }

        let mut scores = Vec::with_capacity(wire.candidates.len());
        for candidate in wire.candidates {
            let scenario_id = ScenarioId::new(candidate.scenario_id)
                .map_err(|_| EvidenceCodecError::InvalidScenarioId)?;
            scores.push(ScenarioScore {
                scenario_id,
                score: candidate.score,
            });
        }

        let selected = wire
            .selected
            .map(ScenarioId::new)
            .transpose()
            .map_err(|_| EvidenceCodecError::InvalidScenarioId)?;

        Self::from_scores(run_id, sources, scores, selected).map_err(EvidenceCodecError::Evidence)
    }

    pub fn verify_replay(&self, replayed: &Self) -> Result<(), ReplayMismatch>
    where
        Score: PartialEq,
    {
        if self.sources != replayed.sources {
            return Err(ReplayMismatch::Sources);
        }
        if self.candidates != replayed.candidates {
            return Err(ReplayMismatch::Candidates);
        }
        if self.selected != replayed.selected {
            return Err(ReplayMismatch::Selection);
        }
        Ok(())
    }
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyRunId => "run id must not be empty",
            Self::EmptyComponent => "evidence source component must not be empty",
            Self::EmptyRevision => "evidence source revision must not be empty",
            Self::EmptyContentHash => "evidence source content hash must not be empty",
            Self::EmptySources => "at least one evidence source is required",
            Self::DuplicateSource => "decision evidence contains a duplicate evidence source",
            Self::DuplicateScenario => "decision evidence contains a duplicate scenario id",
            Self::UnknownSelection => "selected scenario is not present in candidate evidence",
        })
    }
}

impl std::error::Error for EvidenceError {}

impl fmt::Display for ReplayMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Sources => "replay provenance differs from recorded evidence",
            Self::Candidates => "replay candidate evidence differs from recorded evidence",
            Self::Selection => "replay selection differs from recorded evidence",
        })
    }
}

impl std::error::Error for ReplayMismatch {}

impl fmt::Display for EvidenceCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid evidence JSON: {error}"),
            Self::UnsupportedSchema => formatter.write_str("unsupported decision evidence schema"),
            Self::InvalidScenarioId => formatter.write_str("invalid scenario id in evidence JSON"),
            Self::Evidence(error) => write!(formatter, "invalid decision evidence: {error}"),
        }
    }
}

impl std::error::Error for EvidenceCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Evidence(error) => Some(error),
            Self::UnsupportedSchema | Self::InvalidScenarioId => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use prospect_core::ScenarioId;
    use prospect_scenario::ScenarioScore;

    use super::{
        DecisionEvidence, EvidenceError, EvidenceNature, EvidenceSource, ReplayMismatch, RunId,
    };

    fn source(component: &str, nature: EvidenceNature) -> EvidenceSource {
        EvidenceSource::new_with_nature(component, "revision-1", nature).expect("source")
    }

    #[test]
    fn records_sources_scores_and_selection() {
        let selected = ScenarioId::new("reduce-concurrency").expect("id");
        let evidence = DecisionEvidence::from_scores(
            RunId::new("run-0001").expect("run id"),
            vec![source("ElasticXxx", EvidenceNature::Observed)],
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
        assert_eq!(evidence.sources()[0].nature(), EvidenceNature::Observed);
        assert_eq!(evidence.candidates().len(), 2);
    }

    #[test]
    fn canonical_json_is_independent_of_input_order() {
        let first = DecisionEvidence::from_scores(
            RunId::new("run-a").expect("run id"),
            vec![
                source("TDI", EvidenceNature::Simulated),
                source("ElasticXxx", EvidenceNature::Observed),
            ],
            vec![
                ScenarioScore {
                    scenario_id: ScenarioId::new("zeta").expect("id"),
                    score: 2_i32,
                },
                ScenarioScore {
                    scenario_id: ScenarioId::new("alpha").expect("id"),
                    score: 1_i32,
                },
            ],
            None,
        )
        .expect("evidence");

        let second = DecisionEvidence::from_scores(
            RunId::new("run-a").expect("run id"),
            vec![
                source("ElasticXxx", EvidenceNature::Observed),
                source("TDI", EvidenceNature::Simulated),
            ],
            vec![
                ScenarioScore {
                    scenario_id: ScenarioId::new("alpha").expect("id"),
                    score: 1_i32,
                },
                ScenarioScore {
                    scenario_id: ScenarioId::new("zeta").expect("id"),
                    score: 2_i32,
                },
            ],
            None,
        )
        .expect("evidence");

        assert_eq!(
            first.canonical_json().expect("json"),
            second.canonical_json().expect("json")
        );
    }

    #[test]
    fn canonical_json_round_trips() {
        let evidence = DecisionEvidence::from_scores(
            RunId::new("run-roundtrip").expect("run id"),
            vec![
                source("ElasticXxx", EvidenceNature::Observed)
                    .with_content_hash("sha256:abc")
                    .expect("hash"),
            ],
            vec![ScenarioScore {
                scenario_id: ScenarioId::new("candidate").expect("id"),
                score: 42_i32,
            }],
            Some(ScenarioId::new("candidate").expect("id")),
        )
        .expect("evidence");

        let json = evidence.canonical_json().expect("json");
        let decoded = DecisionEvidence::<i32>::from_canonical_json(&json).expect("decode");
        assert_eq!(decoded, evidence);
    }

    #[test]
    fn replay_ignores_run_identity_but_checks_evidence() {
        let sources = vec![source("ElasticXxx", EvidenceNature::Observed)];
        let scores = vec![ScenarioScore {
            scenario_id: ScenarioId::new("candidate").expect("id"),
            score: 7_i32,
        }];
        let original = DecisionEvidence::from_scores(
            RunId::new("run-original").expect("run id"),
            sources.clone(),
            scores.clone(),
            None,
        )
        .expect("evidence");
        let replayed = DecisionEvidence::from_scores(
            RunId::new("run-replay").expect("run id"),
            sources,
            scores,
            None,
        )
        .expect("evidence");

        assert_eq!(original.verify_replay(&replayed), Ok(()));
    }

    #[test]
    fn replay_detects_candidate_drift() {
        let original = DecisionEvidence::from_scores(
            RunId::new("run-original").expect("run id"),
            vec![source("ElasticXxx", EvidenceNature::Observed)],
            vec![ScenarioScore {
                scenario_id: ScenarioId::new("candidate").expect("id"),
                score: 7_i32,
            }],
            None,
        )
        .expect("evidence");
        let replayed = DecisionEvidence::from_scores(
            RunId::new("run-replay").expect("run id"),
            vec![source("ElasticXxx", EvidenceNature::Observed)],
            vec![ScenarioScore {
                scenario_id: ScenarioId::new("candidate").expect("id"),
                score: 8_i32,
            }],
            None,
        )
        .expect("evidence");

        assert_eq!(
            original.verify_replay(&replayed),
            Err(ReplayMismatch::Candidates)
        );
    }

    #[test]
    fn rejects_a_selection_missing_from_candidates() {
        let result = DecisionEvidence::<i32>::from_scores(
            RunId::new("run-0002").expect("run id"),
            vec![source("ProspectEngine", EvidenceNature::Inferred)],
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
            vec![source("ProspectEngine", EvidenceNature::Inferred)],
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
