//! Canonical evidence for raw multi-trace score matrices.
//!
//! This module preserves every trace/scenario score cell and its identities. It
//! deliberately performs no cross-trace aggregation, statistical inference, or
//! winner selection.

use core::fmt;
use std::collections::BTreeSet;

use prospect_core::ScenarioId;
use prospect_scenario::multi_trace::{MultiTraceScoreMatrix, TraceId};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::canonical::to_canonical_json;
use super::{EvidenceError, EvidenceNature, EvidenceSource, RunId};

pub const MULTI_TRACE_SCORE_EVIDENCE_SCHEMA_V1: &str = "prospect.multi-trace-score-evidence/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioScoreEvidence<Score> {
    scenario_id: ScenarioId,
    score: Score,
}

impl<Score> ScenarioScoreEvidence<Score> {
    #[must_use]
    pub const fn scenario_id(&self) -> &ScenarioId {
        &self.scenario_id
    }

    #[must_use]
    pub const fn score(&self) -> &Score {
        &self.score
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceScoreEvidence<Score> {
    trace_id: TraceId,
    scores: Vec<ScenarioScoreEvidence<Score>>,
}

impl<Score> TraceScoreEvidence<Score> {
    #[must_use]
    pub const fn trace_id(&self) -> &TraceId {
        &self.trace_id
    }

    #[must_use]
    pub fn scores(&self) -> &[ScenarioScoreEvidence<Score>] {
        &self.scores
    }
}

/// Exact raw trace × scenario score matrix plus explicit provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiTraceScoreEvidence<Score> {
    schema: &'static str,
    run_id: RunId,
    sources: Vec<EvidenceSource>,
    metric_id: String,
    scenario_ids: Vec<ScenarioId>,
    traces: Vec<TraceScoreEvidence<Score>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultiTraceEvidenceError {
    Evidence(EvidenceError),
    EmptyMetricId,
    EmptyScenarioSet,
    EmptyTraceSet,
    DuplicateScenarioId(ScenarioId),
    DuplicateTraceId(TraceId),
    TraceScoreCountMismatch {
        trace_id: TraceId,
    },
    TraceScenarioMismatch {
        trace_id: TraceId,
        index: usize,
        expected: ScenarioId,
        actual: ScenarioId,
    },
}

#[derive(Debug)]
pub enum MultiTraceEvidenceCodecError {
    Json(serde_json::Error),
    UnsupportedSchema,
    InvalidScenarioId,
    InvalidTraceId,
    Invalid(MultiTraceEvidenceError),
    NonCanonical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiTraceReplayMismatch {
    Sources,
    Metric,
    ScenarioContract,
    Traces,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceWire {
    component: String,
    revision: String,
    nature: EvidenceNature,
    content_hash: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScoreWire<Score> {
    scenario_id: String,
    score: Score,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceWire<Score> {
    trace_id: String,
    scores: Vec<ScoreWire<Score>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceWire<Score> {
    schema: String,
    run_id: String,
    sources: Vec<SourceWire>,
    metric_id: String,
    scenario_ids: Vec<String>,
    traces: Vec<TraceWire<Score>>,
}

impl<Score: Clone> MultiTraceScoreEvidence<Score> {
    /// Capture an already validated raw matrix without aggregating across traces.
    pub fn from_matrix(
        run_id: RunId,
        sources: Vec<EvidenceSource>,
        metric_id: impl Into<String>,
        matrix: &MultiTraceScoreMatrix<Score>,
    ) -> Result<Self, MultiTraceEvidenceError> {
        let sources = normalize_sources(sources)?;
        let metric_id = metric_id.into();
        if metric_id.trim().is_empty() {
            return Err(MultiTraceEvidenceError::EmptyMetricId);
        }

        let scenario_ids = matrix.scenario_ids().to_vec();
        let traces = matrix
            .traces()
            .iter()
            .map(|trace| TraceScoreEvidence {
                trace_id: trace.trace_id().clone(),
                scores: trace
                    .scores()
                    .iter()
                    .map(|score| ScenarioScoreEvidence {
                        scenario_id: score.scenario_id.clone(),
                        score: score.score.clone(),
                    })
                    .collect(),
            })
            .collect();

        let evidence = Self {
            schema: MULTI_TRACE_SCORE_EVIDENCE_SCHEMA_V1,
            run_id,
            sources,
            metric_id,
            scenario_ids,
            traces,
        };
        evidence.validate()?;
        Ok(evidence)
    }
}

impl<Score> MultiTraceScoreEvidence<Score> {
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
    pub fn metric_id(&self) -> &str {
        &self.metric_id
    }

    #[must_use]
    pub fn scenario_ids(&self) -> &[ScenarioId] {
        &self.scenario_ids
    }

    #[must_use]
    pub fn traces(&self) -> &[TraceScoreEvidence<Score>] {
        &self.traces
    }

    #[must_use]
    pub fn trace_count(&self) -> usize {
        self.traces.len()
    }

    #[must_use]
    pub fn scenario_count(&self) -> usize {
        self.scenario_ids.len()
    }

    pub fn canonical_json(&self) -> Result<String, MultiTraceEvidenceCodecError>
    where
        Score: Serialize,
    {
        to_canonical_json(&self.to_wire()).map_err(MultiTraceEvidenceCodecError::Json)
    }

    pub fn from_canonical_json(json: &str) -> Result<Self, MultiTraceEvidenceCodecError>
    where
        Score: DeserializeOwned + Serialize,
    {
        let wire: EvidenceWire<Score> =
            serde_json::from_str(json).map_err(MultiTraceEvidenceCodecError::Json)?;
        if wire.schema != MULTI_TRACE_SCORE_EVIDENCE_SCHEMA_V1 {
            return Err(MultiTraceEvidenceCodecError::UnsupportedSchema);
        }
        let evidence = Self::from_wire(wire)?;
        evidence
            .validate()
            .map_err(MultiTraceEvidenceCodecError::Invalid)?;
        if evidence.canonical_json()? != json {
            return Err(MultiTraceEvidenceCodecError::NonCanonical);
        }
        Ok(evidence)
    }

    pub fn verify_replay(&self, replayed: &Self) -> Result<(), MultiTraceReplayMismatch>
    where
        Score: PartialEq,
    {
        if self.sources != replayed.sources {
            return Err(MultiTraceReplayMismatch::Sources);
        }
        if self.metric_id != replayed.metric_id {
            return Err(MultiTraceReplayMismatch::Metric);
        }
        if self.scenario_ids != replayed.scenario_ids {
            return Err(MultiTraceReplayMismatch::ScenarioContract);
        }
        if self.traces != replayed.traces {
            return Err(MultiTraceReplayMismatch::Traces);
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), MultiTraceEvidenceError> {
        if self.metric_id.trim().is_empty() {
            return Err(MultiTraceEvidenceError::EmptyMetricId);
        }
        if self.scenario_ids.is_empty() {
            return Err(MultiTraceEvidenceError::EmptyScenarioSet);
        }
        if self.traces.is_empty() {
            return Err(MultiTraceEvidenceError::EmptyTraceSet);
        }

        let mut scenario_ids = BTreeSet::new();
        for scenario_id in &self.scenario_ids {
            if !scenario_ids.insert(scenario_id.clone()) {
                return Err(MultiTraceEvidenceError::DuplicateScenarioId(
                    scenario_id.clone(),
                ));
            }
        }

        let mut trace_ids = BTreeSet::new();
        for trace in &self.traces {
            if !trace_ids.insert(trace.trace_id.clone()) {
                return Err(MultiTraceEvidenceError::DuplicateTraceId(
                    trace.trace_id.clone(),
                ));
            }
            if trace.scores.len() != self.scenario_ids.len() {
                return Err(MultiTraceEvidenceError::TraceScoreCountMismatch {
                    trace_id: trace.trace_id.clone(),
                });
            }
            for (index, score) in trace.scores.iter().enumerate() {
                if score.scenario_id != self.scenario_ids[index] {
                    return Err(MultiTraceEvidenceError::TraceScenarioMismatch {
                        trace_id: trace.trace_id.clone(),
                        index,
                        expected: self.scenario_ids[index].clone(),
                        actual: score.scenario_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn to_wire(&self) -> EvidenceWire<&Score> {
        EvidenceWire {
            schema: self.schema.to_owned(),
            run_id: self.run_id.as_str().to_owned(),
            sources: self
                .sources
                .iter()
                .map(|source| SourceWire {
                    component: source.component().to_owned(),
                    revision: source.revision().to_owned(),
                    nature: source.nature(),
                    content_hash: source.content_hash().map(str::to_owned),
                })
                .collect(),
            metric_id: self.metric_id.clone(),
            scenario_ids: self
                .scenario_ids
                .iter()
                .map(|scenario_id| scenario_id.as_str().to_owned())
                .collect(),
            traces: self
                .traces
                .iter()
                .map(|trace| TraceWire {
                    trace_id: trace.trace_id.as_str().to_owned(),
                    scores: trace
                        .scores
                        .iter()
                        .map(|score| ScoreWire {
                            scenario_id: score.scenario_id.as_str().to_owned(),
                            score: &score.score,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    fn from_wire(wire: EvidenceWire<Score>) -> Result<Self, MultiTraceEvidenceCodecError> {
        let run_id = RunId::new(wire.run_id).map_err(|error| {
            MultiTraceEvidenceCodecError::Invalid(MultiTraceEvidenceError::Evidence(error))
        })?;
        let sources = decode_sources(wire.sources)?;
        let scenario_ids = wire
            .scenario_ids
            .into_iter()
            .map(|scenario_id| {
                ScenarioId::new(scenario_id)
                    .map_err(|_| MultiTraceEvidenceCodecError::InvalidScenarioId)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let traces = wire
            .traces
            .into_iter()
            .map(|trace| {
                let trace_id = TraceId::new(trace.trace_id)
                    .map_err(|_| MultiTraceEvidenceCodecError::InvalidTraceId)?;
                let scores = trace
                    .scores
                    .into_iter()
                    .map(|score| {
                        let scenario_id = ScenarioId::new(score.scenario_id)
                            .map_err(|_| MultiTraceEvidenceCodecError::InvalidScenarioId)?;
                        Ok(ScenarioScoreEvidence {
                            scenario_id,
                            score: score.score,
                        })
                    })
                    .collect::<Result<Vec<_>, MultiTraceEvidenceCodecError>>()?;
                Ok(TraceScoreEvidence { trace_id, scores })
            })
            .collect::<Result<Vec<_>, MultiTraceEvidenceCodecError>>()?;

        Ok(Self {
            schema: MULTI_TRACE_SCORE_EVIDENCE_SCHEMA_V1,
            run_id,
            sources,
            metric_id: wire.metric_id,
            scenario_ids,
            traces,
        })
    }
}

fn normalize_sources(
    mut sources: Vec<EvidenceSource>,
) -> Result<Vec<EvidenceSource>, MultiTraceEvidenceError> {
    if sources.is_empty() {
        return Err(MultiTraceEvidenceError::Evidence(
            EvidenceError::EmptySources,
        ));
    }
    sources.sort();
    if sources.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(MultiTraceEvidenceError::Evidence(
            EvidenceError::DuplicateSource,
        ));
    }
    Ok(sources)
}

fn decode_sources(
    sources: Vec<SourceWire>,
) -> Result<Vec<EvidenceSource>, MultiTraceEvidenceCodecError> {
    let mut decoded = Vec::with_capacity(sources.len());
    for source in sources {
        let mut converted =
            EvidenceSource::new_with_nature(source.component, source.revision, source.nature)
                .map_err(|error| {
                    MultiTraceEvidenceCodecError::Invalid(MultiTraceEvidenceError::Evidence(error))
                })?;
        if let Some(content_hash) = source.content_hash {
            converted = converted.with_content_hash(content_hash).map_err(|error| {
                MultiTraceEvidenceCodecError::Invalid(MultiTraceEvidenceError::Evidence(error))
            })?;
        }
        decoded.push(converted);
    }
    normalize_sources(decoded).map_err(MultiTraceEvidenceCodecError::Invalid)
}

impl fmt::Display for MultiTraceEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evidence(error) => error.fmt(formatter),
            Self::EmptyMetricId => formatter.write_str("multi-trace metric id must not be empty"),
            Self::EmptyScenarioSet => {
                formatter.write_str("multi-trace evidence requires at least one scenario")
            }
            Self::EmptyTraceSet => {
                formatter.write_str("multi-trace evidence requires at least one trace")
            }
            Self::DuplicateScenarioId(id) => {
                write!(formatter, "multi-trace evidence repeats scenario id {id}")
            }
            Self::DuplicateTraceId(id) => {
                write!(formatter, "multi-trace evidence repeats trace id {id}")
            }
            Self::TraceScoreCountMismatch { trace_id } => write!(
                formatter,
                "trace {trace_id} has a score count different from the scenario contract"
            ),
            Self::TraceScenarioMismatch {
                trace_id,
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "trace {trace_id} score {index} has scenario {actual}, expected {expected}"
            ),
        }
    }
}

impl std::error::Error for MultiTraceEvidenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for MultiTraceEvidenceCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid multi-trace evidence JSON: {error}"),
            Self::UnsupportedSchema => {
                formatter.write_str("unsupported multi-trace evidence schema")
            }
            Self::InvalidScenarioId => {
                formatter.write_str("invalid scenario id in multi-trace evidence")
            }
            Self::InvalidTraceId => formatter.write_str("invalid trace id in multi-trace evidence"),
            Self::Invalid(error) => write!(formatter, "invalid multi-trace evidence: {error}"),
            Self::NonCanonical => formatter.write_str("multi-trace evidence JSON is not canonical"),
        }
    }
}

impl std::error::Error for MultiTraceEvidenceCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Invalid(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for MultiTraceReplayMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Sources => "multi-trace replay provenance differs",
            Self::Metric => "multi-trace replay metric identity differs",
            Self::ScenarioContract => "multi-trace replay scenario contract differs",
            Self::Traces => "multi-trace replay trace identities or score cells differ",
        })
    }
}
impl std::error::Error for MultiTraceReplayMismatch {}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use prospect_core::{ProspectiveEngine, Scenario, ScenarioId, SignatureMetric};
    use prospect_scenario::evaluate_batch;
    use prospect_scenario::multi_trace::{MultiTraceBatch, TraceBatch};

    use super::*;

    struct Add;
    impl ProspectiveEngine<i32, i32> for Add {
        type Signature = i32;
        type Error = core::convert::Infallible;

        fn baseline(&self, state: &i32) -> Result<i32, Self::Error> {
            Ok(*state)
        }

        fn evaluate(&self, state: &i32, intervention: &i32) -> Result<i32, Self::Error> {
            Ok(state + intervention)
        }
    }

    struct Delta;
    impl SignatureMetric<i32> for Delta {
        type Score = i32;

        fn compare(&self, reference: &i32, candidate: &i32) -> i32 {
            candidate - reference
        }
    }

    struct MapMetric;
    impl SignatureMetric<i32> for MapMetric {
        type Score = HashMap<String, i32>;

        fn compare(&self, reference: &i32, candidate: &i32) -> Self::Score {
            let mut score = HashMap::new();
            score.insert("zeta".to_owned(), candidate - reference);
            score.insert("alpha".to_owned(), *candidate);
            score
        }
    }

    fn source(component: &str) -> EvidenceSource {
        EvidenceSource::new_with_nature(component, "rev-1", EvidenceNature::Observed).unwrap()
    }

    fn trace(id: &str, state: i32) -> TraceBatch<i32, i32> {
        let batch = evaluate_batch(
            &Add,
            &state,
            vec![
                Scenario::new(ScenarioId::new("s1").unwrap(), 1),
                Scenario::new(ScenarioId::new("s2").unwrap(), 2),
            ],
        )
        .unwrap();
        TraceBatch::new(TraceId::new(id).unwrap(), batch)
    }

    fn matrix() -> MultiTraceScoreMatrix<i32> {
        MultiTraceBatch::new(vec![trace("trace-b", 20), trace("trace-a", 10)])
            .unwrap()
            .score_matrix(&Delta)
    }

    #[test]
    fn captures_raw_matrix_without_aggregation_or_reordering() {
        let evidence = MultiTraceScoreEvidence::from_matrix(
            RunId::new("multi-run").unwrap(),
            vec![source("Metric"), source("Dataset")],
            "delta-v1",
            &matrix(),
        )
        .unwrap();
        assert_eq!(evidence.trace_count(), 2);
        assert_eq!(evidence.scenario_count(), 2);
        assert_eq!(evidence.traces()[0].trace_id().as_str(), "trace-b");
        assert_eq!(evidence.traces()[1].trace_id().as_str(), "trace-a");
        assert_eq!(
            evidence
                .scenario_ids()
                .iter()
                .map(ScenarioId::as_str)
                .collect::<Vec<_>>(),
            ["s1", "s2"]
        );
        assert_eq!(*evidence.traces()[0].scores()[0].score(), 1);
        assert_eq!(*evidence.traces()[0].scores()[1].score(), 2);
        let json = evidence.canonical_json().unwrap();
        assert!(!json.contains("aggregate"));
        assert!(!json.contains("selected"));
        assert!(!json.contains("winner"));
    }

    #[test]
    fn canonical_roundtrip_preserves_trace_order_and_score_cells() {
        let evidence = MultiTraceScoreEvidence::from_matrix(
            RunId::new("roundtrip").unwrap(),
            vec![source("Dataset"), source("Metric")],
            "delta-v1",
            &matrix(),
        )
        .unwrap();
        let json = evidence.canonical_json().unwrap();
        let decoded = MultiTraceScoreEvidence::<i32>::from_canonical_json(&json).unwrap();
        assert_eq!(decoded, evidence);
        assert_eq!(decoded.canonical_json().unwrap(), json);
    }

    #[test]
    fn nested_map_scores_are_recursively_canonical() {
        let matrix = MultiTraceBatch::new(vec![trace("trace-a", 10), trace("trace-b", 20)])
            .unwrap()
            .score_matrix(&MapMetric);
        let evidence = MultiTraceScoreEvidence::from_matrix(
            RunId::new("map-score").unwrap(),
            vec![source("Metric")],
            "map-v1",
            &matrix,
        )
        .unwrap();
        let json = evidence.canonical_json().unwrap();
        assert!(json.contains("\"score\":{\"alpha\":"));
        assert!(json.contains(",\"zeta\":"));
        let decoded =
            MultiTraceScoreEvidence::<HashMap<String, i32>>::from_canonical_json(&json).unwrap();
        assert_eq!(decoded.canonical_json().unwrap(), json);
    }

    #[test]
    fn sources_are_canonicalized_but_trace_and_scenario_order_are_semantic() {
        let first = MultiTraceScoreEvidence::from_matrix(
            RunId::new("same").unwrap(),
            vec![source("Z"), source("A")],
            "delta-v1",
            &matrix(),
        )
        .unwrap();
        let second = MultiTraceScoreEvidence::from_matrix(
            RunId::new("same").unwrap(),
            vec![source("A"), source("Z")],
            "delta-v1",
            &matrix(),
        )
        .unwrap();
        assert_eq!(
            first.canonical_json().unwrap(),
            second.canonical_json().unwrap()
        );
        assert_eq!(first.traces()[0].trace_id().as_str(), "trace-b");
    }

    #[test]
    fn rejects_empty_metric_and_duplicate_sources() {
        let empty = MultiTraceScoreEvidence::from_matrix(
            RunId::new("empty-metric").unwrap(),
            vec![source("Metric")],
            "   ",
            &matrix(),
        );
        assert_eq!(empty, Err(MultiTraceEvidenceError::EmptyMetricId));

        let duplicate = MultiTraceScoreEvidence::from_matrix(
            RunId::new("duplicate-source").unwrap(),
            vec![source("Metric"), source("Metric")],
            "delta-v1",
            &matrix(),
        );
        assert!(matches!(
            duplicate,
            Err(MultiTraceEvidenceError::Evidence(
                EvidenceError::DuplicateSource
            ))
        ));
    }

    #[test]
    fn canonical_parser_rejects_structural_trace_scenario_drift() {
        let evidence = MultiTraceScoreEvidence::from_matrix(
            RunId::new("tamper").unwrap(),
            vec![source("Metric")],
            "delta-v1",
            &matrix(),
        )
        .unwrap();
        let json = evidence.canonical_json().unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["traces"][0]["scores"][0]["scenario_id"] = serde_json::Value::String("s2".to_owned());
        let tampered = to_canonical_json(&value).unwrap();
        assert!(matches!(
            MultiTraceScoreEvidence::<i32>::from_canonical_json(&tampered),
            Err(MultiTraceEvidenceCodecError::Invalid(
                MultiTraceEvidenceError::TraceScenarioMismatch { .. }
            ))
        ));
    }

    #[test]
    fn canonical_parser_rejects_unknown_fields_and_noncanonical_bytes() {
        let evidence = MultiTraceScoreEvidence::from_matrix(
            RunId::new("strict-json").unwrap(),
            vec![source("Metric")],
            "delta-v1",
            &matrix(),
        )
        .unwrap();
        let json = evidence.canonical_json().unwrap();
        let unknown = json.replacen("{", "{\"unexpected\":true,", 1);
        assert!(matches!(
            MultiTraceScoreEvidence::<i32>::from_canonical_json(&unknown),
            Err(MultiTraceEvidenceCodecError::Json(_))
        ));

        let pretty: serde_json::Value = serde_json::from_str(&json).unwrap();
        let pretty = serde_json::to_string_pretty(&pretty).unwrap();
        assert_eq!(
            MultiTraceScoreEvidence::<i32>::from_canonical_json(&pretty)
                .unwrap_err()
                .to_string(),
            "multi-trace evidence JSON is not canonical"
        );
    }

    #[test]
    fn replay_ignores_run_id_and_classifies_substantive_drift() {
        let original = MultiTraceScoreEvidence::from_matrix(
            RunId::new("original").unwrap(),
            vec![source("Metric")],
            "delta-v1",
            &matrix(),
        )
        .unwrap();
        let replay = MultiTraceScoreEvidence::from_matrix(
            RunId::new("replay").unwrap(),
            vec![source("Metric")],
            "delta-v1",
            &matrix(),
        )
        .unwrap();
        assert_eq!(original.verify_replay(&replay), Ok(()));

        let metric_drift = MultiTraceScoreEvidence::from_matrix(
            RunId::new("replay").unwrap(),
            vec![source("Metric")],
            "other-metric",
            &matrix(),
        )
        .unwrap();
        assert_eq!(
            original.verify_replay(&metric_drift),
            Err(MultiTraceReplayMismatch::Metric)
        );

        let source_drift = MultiTraceScoreEvidence::from_matrix(
            RunId::new("replay").unwrap(),
            vec![source("Other")],
            "delta-v1",
            &matrix(),
        )
        .unwrap();
        assert_eq!(
            original.verify_replay(&source_drift),
            Err(MultiTraceReplayMismatch::Sources)
        );
    }
}
