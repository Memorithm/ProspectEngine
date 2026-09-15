//! Canonical evidence for constraint-first multi-objective decisions.
//!
//! This records software decision structure and provenance. Constraint names,
//! thresholds, objective units, and source trust remain application responsibilities.

use core::cmp::Ordering;
use core::fmt;
use std::collections::BTreeSet;

use prospect_core::ScenarioId;
use prospect_scenario::decision::{
    AlternativeAssessment, AlternativeId, ConstrainedDecisionSet, ObjectiveDirection,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::canonical::to_canonical_json;
use super::{EvidenceError, EvidenceNature, EvidenceSource, RunId};

pub const CONSTRAINED_DECISION_EVIDENCE_SCHEMA_V1: &str =
    "prospect.constrained-decision-evidence/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceObjectiveDirection {
    Maximize,
    Minimize,
}

impl From<ObjectiveDirection> for EvidenceObjectiveDirection {
    fn from(value: ObjectiveDirection) -> Self {
        match value {
            ObjectiveDirection::Maximize => Self::Maximize,
            ObjectiveDirection::Minimize => Self::Minimize,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceAlternativeId {
    Baseline,
    Scenario(ScenarioId),
}

impl EvidenceAlternativeId {
    #[must_use]
    pub const fn scenario_id(&self) -> Option<&ScenarioId> {
        match self {
            Self::Baseline => None,
            Self::Scenario(id) => Some(id),
        }
    }
}

impl From<&AlternativeId> for EvidenceAlternativeId {
    fn from(value: &AlternativeId) -> Self {
        match value {
            AlternativeId::Baseline => Self::Baseline,
            AlternativeId::Scenario(id) => Self::Scenario(id.clone()),
        }
    }
}

impl fmt::Display for EvidenceAlternativeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Baseline => formatter.write_str("baseline"),
            Self::Scenario(id) => id.fmt(formatter),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveEvidence<T> {
    id: String,
    direction: EvidenceObjectiveDirection,
    value: T,
}

impl<T> ObjectiveEvidence<T> {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub const fn direction(&self) -> EvidenceObjectiveDirection {
        self.direction
    }

    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstrainedAlternativeEvidence<R, T> {
    Admissible {
        id: EvidenceAlternativeId,
        objectives: Vec<ObjectiveEvidence<T>>,
    },
    Rejected {
        id: EvidenceAlternativeId,
        reason: R,
    },
}

impl<R, T> ConstrainedAlternativeEvidence<R, T> {
    #[must_use]
    pub const fn id(&self) -> &EvidenceAlternativeId {
        match self {
            Self::Admissible { id, .. } | Self::Rejected { id, .. } => id,
        }
    }

    #[must_use]
    pub const fn is_admissible(&self) -> bool {
        matches!(self, Self::Admissible { .. })
    }

    #[must_use]
    pub fn objectives(&self) -> Option<&[ObjectiveEvidence<T>]> {
        match self {
            Self::Admissible { objectives, .. } => Some(objectives),
            Self::Rejected { .. } => None,
        }
    }

    #[must_use]
    pub const fn rejection_reason(&self) -> Option<&R> {
        match self {
            Self::Admissible { .. } => None,
            Self::Rejected { reason, .. } => Some(reason),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstrainedDecisionEvidence<R, T> {
    schema: &'static str,
    run_id: RunId,
    sources: Vec<EvidenceSource>,
    alternatives: Vec<ConstrainedAlternativeEvidence<R, T>>,
    lexicographic_selected: Option<EvidenceAlternativeId>,
    pareto_front: Vec<EvidenceAlternativeId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstrainedEvidenceError {
    Evidence(EvidenceError),
    EmptyAlternatives,
    BaselineMustBeFirst,
    DuplicateAlternative,
    EmptyObjectiveVector,
    EmptyObjectiveId,
    DuplicateObjectiveId,
    ObjectiveSchemaMismatch,
    UnknownLexicographicSelection,
    LexicographicSelectionMismatch,
    DuplicateParetoAlternative,
    UnknownParetoAlternative,
    ParetoFrontMismatch,
}

#[derive(Debug)]
pub enum ConstrainedEvidenceCodecError {
    Json(serde_json::Error),
    UnsupportedSchema,
    InvalidScenarioId,
    Invalid(ConstrainedEvidenceError),
    NonCanonical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstrainedReplayMismatch {
    Sources,
    Alternatives,
    LexicographicSelection,
    ParetoFront,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum AlternativeIdWire {
    Baseline,
    Scenario { scenario_id: String },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectiveWire<T> {
    id: String,
    direction: EvidenceObjectiveDirection,
    value: T,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum AlternativeWire<R, T> {
    Admissible {
        alternative: AlternativeIdWire,
        objectives: Vec<ObjectiveWire<T>>,
    },
    Rejected {
        alternative: AlternativeIdWire,
        reason: R,
    },
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
struct ConstrainedEvidenceWire<R, T> {
    schema: String,
    run_id: String,
    sources: Vec<SourceWire>,
    alternatives: Vec<AlternativeWire<R, T>>,
    lexicographic_selected: Option<AlternativeIdWire>,
    pareto_front: Vec<AlternativeIdWire>,
}

impl<R, T> ConstrainedDecisionEvidence<R, T>
where
    R: Clone,
    T: Clone + Ord,
{
    /// Capture a validated decision set and its derived selections.
    ///
    /// Sources are sorted and deduplicated. Alternative order is retained because
    /// lexicographic exact-tie behavior is order-sensitive by design.
    pub fn from_decision_set(
        run_id: RunId,
        sources: Vec<EvidenceSource>,
        decision: &ConstrainedDecisionSet<R, T>,
    ) -> Result<Self, ConstrainedEvidenceError> {
        let sources = normalize_sources(sources)?;
        let alternatives = decision
            .alternatives()
            .iter()
            .map(|alternative| match alternative.assessment() {
                AlternativeAssessment::Admissible { objectives } => {
                    ConstrainedAlternativeEvidence::Admissible {
                        id: EvidenceAlternativeId::from(alternative.id()),
                        objectives: objectives
                            .iter()
                            .map(|objective| ObjectiveEvidence {
                                id: objective.id().to_owned(),
                                direction: objective.direction().into(),
                                value: objective.value().clone(),
                            })
                            .collect(),
                    }
                }
                AlternativeAssessment::Rejected { reason } => {
                    ConstrainedAlternativeEvidence::Rejected {
                        id: EvidenceAlternativeId::from(alternative.id()),
                        reason: reason.clone(),
                    }
                }
            })
            .collect::<Vec<_>>();
        let lexicographic_selected = decision
            .lexicographic_best()
            .map(|alternative| EvidenceAlternativeId::from(alternative.id()));
        let pareto_front = decision
            .pareto_front()
            .into_iter()
            .map(|alternative| EvidenceAlternativeId::from(alternative.id()))
            .collect();
        let evidence = Self {
            schema: CONSTRAINED_DECISION_EVIDENCE_SCHEMA_V1,
            run_id,
            sources,
            alternatives,
            lexicographic_selected,
            pareto_front,
        };
        evidence.validate()?;
        Ok(evidence)
    }
}

impl<R, T> ConstrainedDecisionEvidence<R, T> {
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
    pub fn alternatives(&self) -> &[ConstrainedAlternativeEvidence<R, T>] {
        &self.alternatives
    }

    #[must_use]
    pub const fn lexicographic_selected(&self) -> Option<&EvidenceAlternativeId> {
        self.lexicographic_selected.as_ref()
    }

    #[must_use]
    pub fn pareto_front(&self) -> &[EvidenceAlternativeId] {
        &self.pareto_front
    }

    pub fn canonical_json(&self) -> Result<String, ConstrainedEvidenceCodecError>
    where
        R: Serialize,
        T: Serialize,
    {
        to_canonical_json(&self.to_wire()).map_err(ConstrainedEvidenceCodecError::Json)
    }

    pub fn from_canonical_json(json: &str) -> Result<Self, ConstrainedEvidenceCodecError>
    where
        R: DeserializeOwned + Serialize,
        T: DeserializeOwned + Serialize + Ord,
    {
        let wire: ConstrainedEvidenceWire<R, T> =
            serde_json::from_str(json).map_err(ConstrainedEvidenceCodecError::Json)?;
        if wire.schema != CONSTRAINED_DECISION_EVIDENCE_SCHEMA_V1 {
            return Err(ConstrainedEvidenceCodecError::UnsupportedSchema);
        }
        let evidence = Self::from_wire(wire)?;
        evidence
            .validate()
            .map_err(ConstrainedEvidenceCodecError::Invalid)?;
        let canonical = evidence.canonical_json()?;
        if canonical != json {
            return Err(ConstrainedEvidenceCodecError::NonCanonical);
        }
        Ok(evidence)
    }

    pub fn verify_replay(&self, replayed: &Self) -> Result<(), ConstrainedReplayMismatch>
    where
        R: PartialEq,
        T: PartialEq,
    {
        if self.sources != replayed.sources {
            return Err(ConstrainedReplayMismatch::Sources);
        }
        if self.alternatives != replayed.alternatives {
            return Err(ConstrainedReplayMismatch::Alternatives);
        }
        if self.lexicographic_selected != replayed.lexicographic_selected {
            return Err(ConstrainedReplayMismatch::LexicographicSelection);
        }
        if self.pareto_front != replayed.pareto_front {
            return Err(ConstrainedReplayMismatch::ParetoFront);
        }
        Ok(())
    }

    fn to_wire(&self) -> ConstrainedEvidenceWire<&R, &T> {
        ConstrainedEvidenceWire {
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
            alternatives: self
                .alternatives
                .iter()
                .map(|alternative| match alternative {
                    ConstrainedAlternativeEvidence::Admissible { id, objectives } => {
                        AlternativeWire::Admissible {
                            alternative: id_to_wire(id),
                            objectives: objectives
                                .iter()
                                .map(|objective| ObjectiveWire {
                                    id: objective.id.clone(),
                                    direction: objective.direction,
                                    value: &objective.value,
                                })
                                .collect(),
                        }
                    }
                    ConstrainedAlternativeEvidence::Rejected { id, reason } => {
                        AlternativeWire::Rejected {
                            alternative: id_to_wire(id),
                            reason,
                        }
                    }
                })
                .collect(),
            lexicographic_selected: self.lexicographic_selected.as_ref().map(id_to_wire),
            pareto_front: self.pareto_front.iter().map(id_to_wire).collect(),
        }
    }

    fn from_wire(
        wire: ConstrainedEvidenceWire<R, T>,
    ) -> Result<Self, ConstrainedEvidenceCodecError> {
        let run_id = RunId::new(wire.run_id).map_err(|error| {
            ConstrainedEvidenceCodecError::Invalid(ConstrainedEvidenceError::Evidence(error))
        })?;
        let mut sources = Vec::with_capacity(wire.sources.len());
        for source in wire.sources {
            let mut converted =
                EvidenceSource::new_with_nature(source.component, source.revision, source.nature)
                    .map_err(|error| {
                    ConstrainedEvidenceCodecError::Invalid(ConstrainedEvidenceError::Evidence(
                        error,
                    ))
                })?;
            if let Some(hash) = source.content_hash {
                converted = converted.with_content_hash(hash).map_err(|error| {
                    ConstrainedEvidenceCodecError::Invalid(ConstrainedEvidenceError::Evidence(
                        error,
                    ))
                })?;
            }
            sources.push(converted);
        }
        sources = normalize_sources(sources).map_err(ConstrainedEvidenceCodecError::Invalid)?;
        let alternatives = wire
            .alternatives
            .into_iter()
            .map(|alternative| match alternative {
                AlternativeWire::Admissible {
                    alternative,
                    objectives,
                } => Ok(ConstrainedAlternativeEvidence::Admissible {
                    id: id_from_wire(alternative)?,
                    objectives: objectives
                        .into_iter()
                        .map(|objective| ObjectiveEvidence {
                            id: objective.id,
                            direction: objective.direction,
                            value: objective.value,
                        })
                        .collect(),
                }),
                AlternativeWire::Rejected {
                    alternative,
                    reason,
                } => Ok(ConstrainedAlternativeEvidence::Rejected {
                    id: id_from_wire(alternative)?,
                    reason,
                }),
            })
            .collect::<Result<Vec<_>, ConstrainedEvidenceCodecError>>()?;
        Ok(Self {
            schema: CONSTRAINED_DECISION_EVIDENCE_SCHEMA_V1,
            run_id,
            sources,
            alternatives,
            lexicographic_selected: wire.lexicographic_selected.map(id_from_wire).transpose()?,
            pareto_front: wire
                .pareto_front
                .into_iter()
                .map(id_from_wire)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

impl<R, T: Ord> ConstrainedDecisionEvidence<R, T> {
    fn validate(&self) -> Result<(), ConstrainedEvidenceError> {
        if self.alternatives.is_empty() {
            return Err(ConstrainedEvidenceError::EmptyAlternatives);
        }
        if self.alternatives[0].id() != &EvidenceAlternativeId::Baseline {
            return Err(ConstrainedEvidenceError::BaselineMustBeFirst);
        }
        let mut alternatives_seen = BTreeSet::new();
        let mut schema: Option<Vec<(String, EvidenceObjectiveDirection)>> = None;
        for alternative in &self.alternatives {
            if !alternatives_seen.insert(alternative.id().clone()) {
                return Err(ConstrainedEvidenceError::DuplicateAlternative);
            }
            if let ConstrainedAlternativeEvidence::Admissible { objectives, .. } = alternative {
                if objectives.is_empty() {
                    return Err(ConstrainedEvidenceError::EmptyObjectiveVector);
                }
                let mut ids = BTreeSet::new();
                let mut current = Vec::with_capacity(objectives.len());
                for objective in objectives {
                    if objective.id.trim().is_empty() {
                        return Err(ConstrainedEvidenceError::EmptyObjectiveId);
                    }
                    if !ids.insert(objective.id.as_str()) {
                        return Err(ConstrainedEvidenceError::DuplicateObjectiveId);
                    }
                    current.push((objective.id.clone(), objective.direction));
                }
                match &schema {
                    None => schema = Some(current),
                    Some(expected) if *expected == current => {}
                    Some(_) => return Err(ConstrainedEvidenceError::ObjectiveSchemaMismatch),
                }
            }
        }

        let admissible = self
            .alternatives
            .iter()
            .filter(|alternative| alternative.is_admissible())
            .collect::<Vec<_>>();
        let expected_lexicographic = lexicographic_best(&admissible).map(|item| item.id());
        if self.lexicographic_selected.as_ref() != expected_lexicographic {
            if self
                .lexicographic_selected
                .as_ref()
                .is_some_and(|id| !alternatives_seen.contains(id))
            {
                return Err(ConstrainedEvidenceError::UnknownLexicographicSelection);
            }
            return Err(ConstrainedEvidenceError::LexicographicSelectionMismatch);
        }

        let mut front_seen = BTreeSet::new();
        for id in &self.pareto_front {
            if !front_seen.insert(id.clone()) {
                return Err(ConstrainedEvidenceError::DuplicateParetoAlternative);
            }
            if !alternatives_seen.contains(id) {
                return Err(ConstrainedEvidenceError::UnknownParetoAlternative);
            }
        }
        let expected_front = pareto_front(&admissible)
            .into_iter()
            .map(ConstrainedAlternativeEvidence::id)
            .collect::<Vec<_>>();
        if self.pareto_front.iter().collect::<Vec<_>>() != expected_front {
            return Err(ConstrainedEvidenceError::ParetoFrontMismatch);
        }
        Ok(())
    }
}

fn normalize_sources(
    mut sources: Vec<EvidenceSource>,
) -> Result<Vec<EvidenceSource>, ConstrainedEvidenceError> {
    if sources.is_empty() {
        return Err(ConstrainedEvidenceError::Evidence(
            EvidenceError::EmptySources,
        ));
    }
    sources.sort();
    if sources.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ConstrainedEvidenceError::Evidence(
            EvidenceError::DuplicateSource,
        ));
    }
    Ok(sources)
}

fn id_to_wire(id: &EvidenceAlternativeId) -> AlternativeIdWire {
    match id {
        EvidenceAlternativeId::Baseline => AlternativeIdWire::Baseline,
        EvidenceAlternativeId::Scenario(id) => AlternativeIdWire::Scenario {
            scenario_id: id.as_str().to_owned(),
        },
    }
}

fn id_from_wire(
    id: AlternativeIdWire,
) -> Result<EvidenceAlternativeId, ConstrainedEvidenceCodecError> {
    match id {
        AlternativeIdWire::Baseline => Ok(EvidenceAlternativeId::Baseline),
        AlternativeIdWire::Scenario { scenario_id } => ScenarioId::new(scenario_id)
            .map(EvidenceAlternativeId::Scenario)
            .map_err(|_| ConstrainedEvidenceCodecError::InvalidScenarioId),
    }
}

fn lexicographic_best<'a, R, T: Ord>(
    alternatives: &[&'a ConstrainedAlternativeEvidence<R, T>],
) -> Option<&'a ConstrainedAlternativeEvidence<R, T>> {
    let mut best = None;
    for candidate in alternatives {
        match best {
            None => best = Some(*candidate),
            Some(current) => {
                if compare_vectors(
                    candidate.objectives().expect("admissible"),
                    current.objectives().expect("admissible"),
                ) == Ordering::Greater
                {
                    best = Some(*candidate);
                }
            }
        }
    }
    best
}

fn pareto_front<'a, R, T: Ord>(
    alternatives: &[&'a ConstrainedAlternativeEvidence<R, T>],
) -> Vec<&'a ConstrainedAlternativeEvidence<R, T>> {
    alternatives
        .iter()
        .copied()
        .filter(|candidate| {
            !alternatives.iter().copied().any(|other| {
                !core::ptr::eq(*candidate, other)
                    && dominates(
                        other.objectives().expect("admissible"),
                        candidate.objectives().expect("admissible"),
                    )
            })
        })
        .collect()
}

fn compare_vectors<T: Ord>(
    left: &[ObjectiveEvidence<T>],
    right: &[ObjectiveEvidence<T>],
) -> Ordering {
    for (left, right) in left.iter().zip(right) {
        let order = preferred_cmp(left.value(), right.value(), left.direction);
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

fn dominates<T: Ord>(left: &[ObjectiveEvidence<T>], right: &[ObjectiveEvidence<T>]) -> bool {
    let mut strict = false;
    for (left, right) in left.iter().zip(right) {
        match preferred_cmp(left.value(), right.value(), left.direction) {
            Ordering::Less => return false,
            Ordering::Greater => strict = true,
            Ordering::Equal => {}
        }
    }
    strict
}

fn preferred_cmp<T: Ord>(left: &T, right: &T, direction: EvidenceObjectiveDirection) -> Ordering {
    match direction {
        EvidenceObjectiveDirection::Maximize => left.cmp(right),
        EvidenceObjectiveDirection::Minimize => right.cmp(left),
    }
}

impl fmt::Display for ConstrainedEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evidence(error) => error.fmt(formatter),
            Self::EmptyAlternatives => {
                formatter.write_str("constrained evidence has no alternatives")
            }
            Self::BaselineMustBeFirst => {
                formatter.write_str("baseline must be the first constrained alternative")
            }
            Self::DuplicateAlternative => {
                formatter.write_str("constrained evidence contains a duplicate alternative")
            }
            Self::EmptyObjectiveVector => {
                formatter.write_str("admissible constrained alternative has no objectives")
            }
            Self::EmptyObjectiveId => {
                formatter.write_str("constrained evidence contains an empty objective id")
            }
            Self::DuplicateObjectiveId => {
                formatter.write_str("constrained evidence repeats an objective id")
            }
            Self::ObjectiveSchemaMismatch => formatter
                .write_str("constrained evidence objective schema differs across alternatives"),
            Self::UnknownLexicographicSelection => {
                formatter.write_str("lexicographic selection references an unknown alternative")
            }
            Self::LexicographicSelectionMismatch => {
                formatter.write_str("lexicographic selection disagrees with recorded objectives")
            }
            Self::DuplicateParetoAlternative => {
                formatter.write_str("Pareto front contains a duplicate alternative")
            }
            Self::UnknownParetoAlternative => {
                formatter.write_str("Pareto front references an unknown alternative")
            }
            Self::ParetoFrontMismatch => {
                formatter.write_str("Pareto front disagrees with recorded objectives")
            }
        }
    }
}

impl std::error::Error for ConstrainedEvidenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for ConstrainedEvidenceCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid constrained evidence JSON: {error}"),
            Self::UnsupportedSchema => {
                formatter.write_str("unsupported constrained evidence schema")
            }
            Self::InvalidScenarioId => {
                formatter.write_str("invalid scenario id in constrained evidence")
            }
            Self::Invalid(error) => {
                write!(formatter, "invalid constrained decision evidence: {error}")
            }
            Self::NonCanonical => formatter.write_str("constrained evidence JSON is not canonical"),
        }
    }
}

impl std::error::Error for ConstrainedEvidenceCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Invalid(error) => Some(error),
            Self::UnsupportedSchema | Self::InvalidScenarioId | Self::NonCanonical => None,
        }
    }
}

impl fmt::Display for ConstrainedReplayMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Sources => "constrained replay provenance differs",
            Self::Alternatives => "constrained replay alternatives differ",
            Self::LexicographicSelection => "constrained replay lexicographic selection differs",
            Self::ParetoFront => "constrained replay Pareto front differs",
        })
    }
}

impl std::error::Error for ConstrainedReplayMismatch {}

#[cfg(test)]
mod tests {
    use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};
    use prospect_scenario::decision::{Objective, assess_decision_set};
    use prospect_scenario::evaluate_batch;

    use super::*;

    struct Engine;
    impl ProspectiveEngine<i32, i32> for Engine {
        type Signature = i32;
        type Error = core::convert::Infallible;
        fn baseline(&self, state: &i32) -> Result<i32, Self::Error> {
            Ok(*state)
        }
        fn evaluate(&self, state: &i32, intervention: &i32) -> Result<i32, Self::Error> {
            Ok(*state + *intervention)
        }
    }

    fn decision() -> ConstrainedDecisionSet<String, i32> {
        let batch = evaluate_batch(
            &Engine,
            &10,
            vec![
                Scenario::new(ScenarioId::new("safe").unwrap(), 1),
                Scenario::new(ScenarioId::new("fast").unwrap(), 2),
                Scenario::new(ScenarioId::new("blocked").unwrap(), 20),
            ],
        )
        .unwrap();
        assess_decision_set(&batch, |id, _, signature| {
            if matches!(id, AlternativeId::Scenario(scenario) if scenario.as_str() == "blocked") {
                return Err("hard-limit".to_owned());
            }
            let cost = match id {
                AlternativeId::Baseline => 0,
                AlternativeId::Scenario(_) => *signature - 10,
            };
            Ok(vec![
                Objective::maximize("quality", *signature),
                Objective::minimize("cost", cost),
            ])
        })
        .unwrap()
    }

    fn source(component: &str) -> EvidenceSource {
        EvidenceSource::new_with_nature(component, "rev-1", EvidenceNature::Observed).unwrap()
    }

    fn evidence() -> ConstrainedDecisionEvidence<String, i32> {
        ConstrainedDecisionEvidence::from_decision_set(
            RunId::new("run-constrained").unwrap(),
            vec![source("TDI"), source("ElasticXxx")],
            &decision(),
        )
        .unwrap()
    }

    #[test]
    fn captures_constraint_reasons_and_derived_selections() {
        let evidence = evidence();
        assert_eq!(evidence.sources()[0].component(), "ElasticXxx");
        assert_eq!(evidence.alternatives().len(), 4);
        assert_eq!(
            evidence.alternatives()[3]
                .rejection_reason()
                .map(String::as_str),
            Some("hard-limit")
        );
        assert_eq!(
            evidence.lexicographic_selected().unwrap().to_string(),
            "fast"
        );
        assert_eq!(
            evidence
                .pareto_front()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["baseline", "safe", "fast"]
        );
    }

    #[test]
    fn canonical_json_roundtrips_exactly() {
        let evidence = evidence();
        let json = evidence.canonical_json().unwrap();
        let decoded =
            ConstrainedDecisionEvidence::<String, i32>::from_canonical_json(&json).unwrap();
        assert_eq!(decoded, evidence);
        assert_eq!(decoded.canonical_json().unwrap(), json);
    }

    #[test]
    fn noncanonical_unknown_and_wrong_schema_inputs_fail_closed() {
        let json = evidence().canonical_json().unwrap();
        assert!(
            ConstrainedDecisionEvidence::<String, i32>::from_canonical_json(&(json.clone() + "\n"))
                .is_err()
        );
        let wrong_schema = json.replacen(
            CONSTRAINED_DECISION_EVIDENCE_SCHEMA_V1,
            "prospect.constrained-decision-evidence/v999",
            1,
        );
        assert!(matches!(
            ConstrainedDecisionEvidence::<String, i32>::from_canonical_json(&wrong_schema),
            Err(ConstrainedEvidenceCodecError::UnsupportedSchema)
        ));
        let unknown = json.replacen("{", "{\"unexpected\":true,", 1);
        assert!(ConstrainedDecisionEvidence::<String, i32>::from_canonical_json(&unknown).is_err());
    }

    #[test]
    fn stale_lexicographic_selection_is_rejected() {
        let mut evidence = evidence();
        evidence.lexicographic_selected = Some(EvidenceAlternativeId::Baseline);
        let json = evidence.canonical_json().unwrap();
        assert!(matches!(
            ConstrainedDecisionEvidence::<String, i32>::from_canonical_json(&json),
            Err(ConstrainedEvidenceCodecError::Invalid(
                ConstrainedEvidenceError::LexicographicSelectionMismatch
            ))
        ));
    }

    #[test]
    fn stale_pareto_front_is_rejected() {
        let mut evidence = evidence();
        evidence.pareto_front.pop();
        let json = evidence.canonical_json().unwrap();
        assert!(matches!(
            ConstrainedDecisionEvidence::<String, i32>::from_canonical_json(&json),
            Err(ConstrainedEvidenceCodecError::Invalid(
                ConstrainedEvidenceError::ParetoFrontMismatch
            ))
        ));
    }

    #[test]
    fn duplicate_sources_are_rejected_before_capture() {
        let duplicate = source("TDI");
        let result = ConstrainedDecisionEvidence::from_decision_set(
            RunId::new("run").unwrap(),
            vec![duplicate.clone(), duplicate],
            &decision(),
        );
        assert!(matches!(
            result,
            Err(ConstrainedEvidenceError::Evidence(
                EvidenceError::DuplicateSource
            ))
        ));
    }

    #[test]
    fn replay_reports_each_mismatch_class() {
        let original = evidence();
        let mut replayed = original.clone();
        replayed.sources = vec![source("Other")];
        assert_eq!(
            original.verify_replay(&replayed),
            Err(ConstrainedReplayMismatch::Sources)
        );

        replayed = original.clone();
        if let ConstrainedAlternativeEvidence::Admissible { objectives, .. } =
            &mut replayed.alternatives[0]
        {
            objectives[0].value += 1;
        }
        assert_eq!(
            original.verify_replay(&replayed),
            Err(ConstrainedReplayMismatch::Alternatives)
        );

        replayed = original.clone();
        replayed.lexicographic_selected = Some(EvidenceAlternativeId::Baseline);
        assert_eq!(
            original.verify_replay(&replayed),
            Err(ConstrainedReplayMismatch::LexicographicSelection)
        );

        replayed = original.clone();
        replayed.pareto_front.pop();
        assert_eq!(
            original.verify_replay(&replayed),
            Err(ConstrainedReplayMismatch::ParetoFront)
        );
    }

    #[test]
    fn nested_hash_map_rejection_reason_is_canonical_and_roundtrips() {
        use std::collections::HashMap;

        let batch = evaluate_batch(
            &Engine,
            &10,
            vec![Scenario::new(ScenarioId::new("blocked").unwrap(), 20)],
        )
        .unwrap();
        let set = assess_decision_set(&batch, |id, _, _| {
            if matches!(id, AlternativeId::Scenario(_)) {
                let mut reason = HashMap::new();
                reason.insert("zeta".to_owned(), "last".to_owned());
                reason.insert("alpha".to_owned(), "first".to_owned());
                return Err(reason);
            }
            Ok(vec![Objective::maximize("quality", 1)])
        })
        .unwrap();
        let evidence = ConstrainedDecisionEvidence::from_decision_set(
            RunId::new("hash-map-reason").unwrap(),
            vec![source("Policy")],
            &set,
        )
        .unwrap();
        let json = evidence.canonical_json().unwrap();
        assert!(json.contains("\"reason\":{\"alpha\":\"first\",\"zeta\":\"last\"}"));
        let decoded =
            ConstrainedDecisionEvidence::<HashMap<String, String>, i32>::from_canonical_json(&json)
                .unwrap();
        assert_eq!(decoded.canonical_json().unwrap(), json);
    }

    #[test]
    fn baseline_exact_tie_remains_auditable() {
        let batch = evaluate_batch(
            &Engine,
            &10,
            vec![Scenario::new(ScenarioId::new("same").unwrap(), 1)],
        )
        .unwrap();
        let set = assess_decision_set(&batch, |_id, _, _| {
            Ok::<_, String>(vec![Objective::maximize("utility", 1)])
        })
        .unwrap();
        let evidence = ConstrainedDecisionEvidence::from_decision_set(
            RunId::new("tie").unwrap(),
            vec![source("Policy")],
            &set,
        )
        .unwrap();
        assert_eq!(
            evidence.lexicographic_selected(),
            Some(&EvidenceAlternativeId::Baseline)
        );
        assert_eq!(evidence.pareto_front().len(), 2);
    }
}
