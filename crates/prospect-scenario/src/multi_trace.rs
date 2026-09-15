//! Strict multi-trace grouping without implicit aggregation or winner selection.
//!
//! A matrix requires the same ordered scenario identities and interventions on every
//! trace while allowing each trace to have a different baseline/signatures. Metrics
//! are evaluated per trace; this module never invents a cross-trace statistic.

use core::fmt;
use std::collections::BTreeSet;

use prospect_core::{ScenarioId, SignatureMetric};

use crate::{BatchResult, ScenarioScore, score_against_baseline};

/// Stable identity of one independent evaluation trace.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TraceId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTraceId;

impl TraceId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidTraceId> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(InvalidTraceId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for InvalidTraceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("trace id must not be empty")
    }
}
impl std::error::Error for InvalidTraceId {}

/// One completed prospective batch associated with one trace identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceBatch<I, S> {
    trace_id: TraceId,
    batch: BatchResult<I, S>,
}

impl<I, S> TraceBatch<I, S> {
    #[must_use]
    pub const fn new(trace_id: TraceId, batch: BatchResult<I, S>) -> Self {
        Self { trace_id, batch }
    }

    #[must_use]
    pub const fn trace_id(&self) -> &TraceId {
        &self.trace_id
    }

    #[must_use]
    pub const fn batch(&self) -> &BatchResult<I, S> {
        &self.batch
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultiTraceError {
    EmptyTraceSet,
    EmptyScenarioSet {
        trace_id: TraceId,
    },
    DuplicateTraceId(TraceId),
    DuplicateScenarioId {
        trace_id: TraceId,
        scenario_id: ScenarioId,
    },
    ScenarioCountMismatch {
        trace_id: TraceId,
    },
    ScenarioIdMismatch {
        trace_id: TraceId,
        index: usize,
        expected: ScenarioId,
        actual: ScenarioId,
    },
    InterventionMismatch {
        trace_id: TraceId,
        index: usize,
        scenario_id: ScenarioId,
    },
}

impl fmt::Display for MultiTraceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTraceSet => {
                formatter.write_str("multi-trace evaluation requires at least one trace")
            }
            Self::EmptyScenarioSet { trace_id } => write!(
                formatter,
                "trace {trace_id} contains no candidate scenarios"
            ),
            Self::DuplicateTraceId(trace_id) => {
                write!(formatter, "duplicate trace id: {trace_id}")
            }
            Self::DuplicateScenarioId {
                trace_id,
                scenario_id,
            } => write!(
                formatter,
                "trace {trace_id} contains duplicate scenario id {scenario_id}"
            ),
            Self::ScenarioCountMismatch { trace_id } => {
                write!(formatter, "trace {trace_id} has a different scenario count")
            }
            Self::ScenarioIdMismatch {
                trace_id,
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "trace {trace_id} scenario {index} has id {actual}, expected {expected}"
            ),
            Self::InterventionMismatch {
                trace_id,
                index,
                scenario_id,
            } => write!(
                formatter,
                "trace {trace_id} scenario {index} ({scenario_id}) has a different intervention"
            ),
        }
    }
}
impl std::error::Error for MultiTraceError {}

/// Structurally comparable completed batches across independent traces.
///
/// Construction validates trace/scenario uniqueness plus exact ordered scenario and
/// intervention equality. Baseline and candidate signatures intentionally remain
/// trace-specific. No cross-trace averaging, interpolation, weighting, confidence
/// estimate, statistical test, policy choice, or quality claim is performed here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiTraceBatch<I, S> {
    traces: Vec<TraceBatch<I, S>>,
    scenario_ids: Vec<ScenarioId>,
}

impl<I: PartialEq, S> MultiTraceBatch<I, S> {
    pub fn new(traces: Vec<TraceBatch<I, S>>) -> Result<Self, MultiTraceError> {
        let first = traces.first().ok_or(MultiTraceError::EmptyTraceSet)?;
        if first.batch().outcomes().is_empty() {
            return Err(MultiTraceError::EmptyScenarioSet {
                trace_id: first.trace_id().clone(),
            });
        }

        let mut trace_ids = BTreeSet::new();
        for trace in &traces {
            if !trace_ids.insert(trace.trace_id().clone()) {
                return Err(MultiTraceError::DuplicateTraceId(trace.trace_id().clone()));
            }
            validate_unique_scenarios(trace)?;
        }

        let scenario_ids = first
            .batch()
            .outcomes()
            .iter()
            .map(|outcome| outcome.scenario().id().clone())
            .collect::<Vec<_>>();
        let expected_interventions = first
            .batch()
            .outcomes()
            .iter()
            .map(|outcome| outcome.scenario().intervention())
            .collect::<Vec<_>>();

        for trace in &traces[1..] {
            let outcomes = trace.batch().outcomes();
            if outcomes.is_empty() {
                return Err(MultiTraceError::EmptyScenarioSet {
                    trace_id: trace.trace_id().clone(),
                });
            }
            if outcomes.len() != scenario_ids.len() {
                return Err(MultiTraceError::ScenarioCountMismatch {
                    trace_id: trace.trace_id().clone(),
                });
            }
            for (index, outcome) in outcomes.iter().enumerate() {
                let actual = outcome.scenario().id();
                if actual != &scenario_ids[index] {
                    return Err(MultiTraceError::ScenarioIdMismatch {
                        trace_id: trace.trace_id().clone(),
                        index,
                        expected: scenario_ids[index].clone(),
                        actual: actual.clone(),
                    });
                }
                if outcome.scenario().intervention() != expected_interventions[index] {
                    return Err(MultiTraceError::InterventionMismatch {
                        trace_id: trace.trace_id().clone(),
                        index,
                        scenario_id: actual.clone(),
                    });
                }
            }
        }

        Ok(Self {
            traces,
            scenario_ids,
        })
    }
}

impl<I, S> MultiTraceBatch<I, S> {
    #[must_use]
    pub fn traces(&self) -> &[TraceBatch<I, S>] {
        &self.traces
    }

    #[must_use]
    pub fn scenario_ids(&self) -> &[ScenarioId] {
        &self.scenario_ids
    }

    #[must_use]
    pub fn trace_count(&self) -> usize {
        self.traces.len()
    }

    #[must_use]
    pub fn scenario_count(&self) -> usize {
        self.scenario_ids.len()
    }

    /// Evaluate one existing signature metric independently on every trace.
    ///
    /// The output preserves exact trace and scenario ordering. There is deliberately
    /// no aggregate score because mean/median/quantile/weighting semantics belong to
    /// a separately specified experimental protocol.
    #[must_use]
    pub fn score_matrix<M>(&self, metric: &M) -> MultiTraceScoreMatrix<M::Score>
    where
        M: SignatureMetric<S> + ?Sized,
    {
        let traces = self
            .traces
            .iter()
            .map(|trace| TraceScores {
                trace_id: trace.trace_id().clone(),
                scores: score_against_baseline(trace.batch(), metric),
            })
            .collect();
        MultiTraceScoreMatrix {
            scenario_ids: self.scenario_ids.clone(),
            traces,
        }
    }
}

fn validate_unique_scenarios<I, S>(trace: &TraceBatch<I, S>) -> Result<(), MultiTraceError> {
    let mut seen = BTreeSet::new();
    for outcome in trace.batch().outcomes() {
        if !seen.insert(outcome.scenario().id()) {
            return Err(MultiTraceError::DuplicateScenarioId {
                trace_id: trace.trace_id().clone(),
                scenario_id: outcome.scenario().id().clone(),
            });
        }
    }
    Ok(())
}

/// Per-trace metric values; no aggregate statistic is embedded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceScores<Score> {
    trace_id: TraceId,
    scores: Vec<ScenarioScore<Score>>,
}
impl<Score> TraceScores<Score> {
    #[must_use]
    pub const fn trace_id(&self) -> &TraceId {
        &self.trace_id
    }
    #[must_use]
    pub fn scores(&self) -> &[ScenarioScore<Score>] {
        &self.scores
    }
}

/// Rectangular trace × scenario score matrix with exact identities retained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiTraceScoreMatrix<Score> {
    scenario_ids: Vec<ScenarioId>,
    traces: Vec<TraceScores<Score>>,
}
impl<Score> MultiTraceScoreMatrix<Score> {
    #[must_use]
    pub fn scenario_ids(&self) -> &[ScenarioId] {
        &self.scenario_ids
    }
    #[must_use]
    pub fn traces(&self) -> &[TraceScores<Score>] {
        &self.traces
    }
}

#[cfg(test)]
mod tests {
    use prospect_core::{ProspectiveEngine, Scenario};

    use super::*;
    use crate::{ScenarioOutcome, evaluate_batch};

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

    struct Delta;
    impl SignatureMetric<i32> for Delta {
        type Score = i32;
        fn compare(&self, reference: &i32, candidate: &i32) -> i32 {
            candidate - reference
        }
    }

    fn batch(state: i32) -> BatchResult<i32, i32> {
        evaluate_batch(
            &Add,
            &state,
            vec![
                Scenario::new(ScenarioId::new("small").unwrap(), 1),
                Scenario::new(ScenarioId::new("large").unwrap(), 3),
            ],
        )
        .unwrap()
    }

    fn traces() -> Vec<TraceBatch<i32, i32>> {
        vec![
            TraceBatch::new(TraceId::new("trace-a").unwrap(), batch(10)),
            TraceBatch::new(TraceId::new("trace-b").unwrap(), batch(100)),
        ]
    }

    #[test]
    fn matrix_accepts_different_baselines_with_exact_same_scenario_contract() {
        let matrix = MultiTraceBatch::new(traces()).unwrap();
        assert_eq!(matrix.trace_count(), 2);
        assert_eq!(matrix.scenario_count(), 2);
        assert_eq!(
            matrix
                .scenario_ids()
                .iter()
                .map(ScenarioId::as_str)
                .collect::<Vec<_>>(),
            ["small", "large"]
        );
        assert_eq!(*matrix.traces()[0].batch().baseline(), 10);
        assert_eq!(*matrix.traces()[1].batch().baseline(), 100);
    }

    #[test]
    fn metric_matrix_preserves_every_trace_without_aggregation() {
        let scores = MultiTraceBatch::new(traces()).unwrap().score_matrix(&Delta);
        assert_eq!(scores.traces().len(), 2);
        for trace in scores.traces() {
            assert_eq!(
                trace
                    .scores()
                    .iter()
                    .map(|score| score.score)
                    .collect::<Vec<_>>(),
                [1, 3]
            );
        }
        assert_eq!(scores.traces()[0].trace_id().as_str(), "trace-a");
        assert_eq!(scores.traces()[1].trace_id().as_str(), "trace-b");
    }

    #[test]
    fn duplicate_trace_ids_are_rejected() {
        let mut input = traces();
        input[1].trace_id = input[0].trace_id.clone();
        assert!(matches!(
            MultiTraceBatch::new(input),
            Err(MultiTraceError::DuplicateTraceId(_))
        ));
    }

    #[test]
    fn scenario_order_drift_is_rejected() {
        let mut second = batch(100);
        second.outcomes.swap(0, 1);
        let input = vec![
            TraceBatch::new(TraceId::new("a").unwrap(), batch(10)),
            TraceBatch::new(TraceId::new("b").unwrap(), second),
        ];
        assert!(matches!(
            MultiTraceBatch::new(input),
            Err(MultiTraceError::ScenarioIdMismatch { index: 0, .. })
        ));
    }

    #[test]
    fn scenario_count_drift_is_rejected() {
        let mut second = batch(100);
        second.outcomes.pop();
        let input = vec![
            TraceBatch::new(TraceId::new("a").unwrap(), batch(10)),
            TraceBatch::new(TraceId::new("b").unwrap(), second),
        ];
        assert!(matches!(
            MultiTraceBatch::new(input),
            Err(MultiTraceError::ScenarioCountMismatch { .. })
        ));
    }

    #[test]
    fn same_id_with_different_intervention_is_rejected() {
        let mut second = batch(100);
        let id = second.outcomes[0].scenario().id().clone();
        let signature = *second.outcomes[0].signature();
        second.outcomes[0] = ScenarioOutcome {
            scenario: Scenario::new(id, 999),
            signature,
        };
        let input = vec![
            TraceBatch::new(TraceId::new("a").unwrap(), batch(10)),
            TraceBatch::new(TraceId::new("b").unwrap(), second),
        ];
        assert!(matches!(
            MultiTraceBatch::new(input),
            Err(MultiTraceError::InterventionMismatch { index: 0, .. })
        ));
    }

    #[test]
    fn duplicate_scenario_id_is_rejected_on_any_trace() {
        let mut first = batch(10);
        let duplicate_id = first.outcomes[0].scenario().id().clone();
        let intervention = *first.outcomes[1].scenario().intervention();
        let signature = *first.outcomes[1].signature();
        first.outcomes[1] = ScenarioOutcome {
            scenario: Scenario::new(duplicate_id, intervention),
            signature,
        };
        assert!(matches!(
            MultiTraceBatch::new(vec![TraceBatch::new(TraceId::new("a").unwrap(), first)]),
            Err(MultiTraceError::DuplicateScenarioId { .. })
        ));
    }

    #[test]
    fn empty_trace_and_empty_scenario_sets_fail_closed() {
        assert_eq!(
            MultiTraceBatch::<i32, i32>::new(vec![]),
            Err(MultiTraceError::EmptyTraceSet)
        );
        let empty: BatchResult<i32, i32> = BatchResult {
            baseline: 10,
            outcomes: vec![],
        };
        assert!(matches!(
            MultiTraceBatch::new(vec![TraceBatch::new(TraceId::new("a").unwrap(), empty)]),
            Err(MultiTraceError::EmptyScenarioSet { .. })
        ));
    }

    #[test]
    fn trace_ids_must_be_nonempty() {
        assert_eq!(TraceId::new("  "), Err(InvalidTraceId));
        assert_eq!(TraceId::new("trace-01").unwrap().as_str(), "trace-01");
    }
}
