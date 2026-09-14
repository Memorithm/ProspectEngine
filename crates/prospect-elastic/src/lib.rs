#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use elastic_runtime::ObservationSnapshot;
use prospect_core::ProspectiveEngine;

pub const ELASTICXXX_REVISION: &str = "50bb85ea84191c01d95e5b4e5e3c81af10e95ebd";

#[derive(Clone, Debug, PartialEq)]
pub struct ElasticObservationState {
    signals: BTreeMap<String, f64>,
    unsupported: Vec<UnsupportedObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedObservation {
    key: String,
    reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElasticIntervention {
    kind: String,
    parameters: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElasticBridgeError {
    EmptyInterventionKind,
    EmptyParameterName,
    NonFiniteParameter { name: String },
    NonFiniteObservation { key: String },
    DuplicateObservation { key: String },
}

impl ElasticObservationState {
    pub fn from_snapshot(snapshot: &ObservationSnapshot) -> Result<Self, ElasticBridgeError> {
        let mut signals = BTreeMap::new();
        let mut unsupported = Vec::new();
        let mut seen = BTreeSet::new();

        for observation in snapshot.iter() {
            let key = format!("{}:{}", observation.source(), observation.signal().as_str());
            if !seen.insert(key.clone()) {
                return Err(ElasticBridgeError::DuplicateObservation { key });
            }

            if observation.is_unsupported() {
                unsupported.push(UnsupportedObservation {
                    key,
                    reason: observation
                        .unsupported_reason()
                        .unwrap_or("unspecified")
                        .to_owned(),
                });
                continue;
            }

            let value = observation.value();
            if !value.is_finite() {
                return Err(ElasticBridgeError::NonFiniteObservation { key });
            }
            signals.insert(key, value);
        }

        Ok(Self {
            signals,
            unsupported,
        })
    }

    #[must_use]
    pub fn signals(&self) -> &BTreeMap<String, f64> {
        &self.signals
    }

    #[must_use]
    pub fn unsupported(&self) -> &[UnsupportedObservation] {
        &self.unsupported
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<f64> {
        self.signals.get(key).copied()
    }
}

impl UnsupportedObservation {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl ElasticIntervention {
    pub fn new(
        kind: impl Into<String>,
        parameters: BTreeMap<String, f64>,
    ) -> Result<Self, ElasticBridgeError> {
        let kind = kind.into();
        if kind.trim().is_empty() {
            return Err(ElasticBridgeError::EmptyInterventionKind);
        }

        for (name, value) in &parameters {
            if name.trim().is_empty() {
                return Err(ElasticBridgeError::EmptyParameterName);
            }
            if !value.is_finite() {
                return Err(ElasticBridgeError::NonFiniteParameter { name: name.clone() });
            }
        }

        Ok(Self { kind, parameters })
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

/// Domain model boundary for ElasticXxx prospective evaluation.
///
/// The bridge owns observation conversion and intervention description only.
/// It deliberately does not invent a universal forecast model for resource
/// behavior. Concrete models must be validated separately.
pub trait ElasticProspectiveModel {
    type Signature;
    type Error;

    fn baseline(&self, state: &ElasticObservationState) -> Result<Self::Signature, Self::Error>;

    fn evaluate(
        &self,
        state: &ElasticObservationState,
        intervention: &ElasticIntervention,
    ) -> Result<Self::Signature, Self::Error>;
}

pub struct ElasticEngine<M> {
    model: M,
}

impl<M> ElasticEngine<M> {
    #[must_use]
    pub const fn new(model: M) -> Self {
        Self { model }
    }

    #[must_use]
    pub const fn model(&self) -> &M {
        &self.model
    }
}

impl<M> ProspectiveEngine<ElasticObservationState, ElasticIntervention> for ElasticEngine<M>
where
    M: ElasticProspectiveModel,
{
    type Signature = M::Signature;
    type Error = M::Error;

    fn baseline(&self, state: &ElasticObservationState) -> Result<Self::Signature, Self::Error> {
        self.model.baseline(state)
    }

    fn evaluate(
        &self,
        state: &ElasticObservationState,
        intervention: &ElasticIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        self.model.evaluate(state, intervention)
    }
}

impl fmt::Display for ElasticBridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInterventionKind => {
                formatter.write_str("intervention kind must not be empty")
            }
            Self::EmptyParameterName => {
                formatter.write_str("intervention parameter name must not be empty")
            }
            Self::NonFiniteParameter { name } => {
                write!(formatter, "intervention parameter {name} is not finite")
            }
            Self::NonFiniteObservation { key } => {
                write!(
                    formatter,
                    "valid ElasticXxx observation {key} is not finite"
                )
            }
            Self::DuplicateObservation { key } => {
                write!(
                    formatter,
                    "ElasticXxx snapshot contains duplicate observation {key}"
                )
            }
        }
    }
}

impl std::error::Error for ElasticBridgeError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::convert::Infallible;
    use std::time::Instant;

    use elastic_core::resource::ObservationSignalId;
    use elastic_runtime::{Observation, ObservationSnapshot, ObservationSource};
    use prospect_core::ProspectiveEngine;

    use super::{
        ElasticEngine, ElasticIntervention, ElasticObservationState, ElasticProspectiveModel,
    };

    struct AddParameterModel;

    impl ElasticProspectiveModel for AddParameterModel {
        type Signature = f64;
        type Error = Infallible;

        fn baseline(
            &self,
            state: &ElasticObservationState,
        ) -> Result<Self::Signature, Self::Error> {
            Ok(state.signals().values().sum())
        }

        fn evaluate(
            &self,
            state: &ElasticObservationState,
            intervention: &ElasticIntervention,
        ) -> Result<Self::Signature, Self::Error> {
            let delta = intervention
                .parameters()
                .get("delta")
                .copied()
                .unwrap_or(0.0);
            Ok(state.signals().values().sum::<f64>() + delta)
        }
    }

    #[test]
    fn preserves_valid_and_unsupported_elastic_observations() {
        let now = Instant::now();
        let snapshot = ObservationSnapshot::new(
            now,
            vec![
                Observation::from_source(
                    ObservationSource::runtime("controller"),
                    ObservationSignalId::UTILIZATION,
                    0.75,
                    now,
                ),
                Observation::unsupported_from_source(
                    ObservationSource::runtime("controller"),
                    ObservationSignalId::FREE_CAPACITY,
                    now,
                    "provider unavailable",
                ),
            ],
        );

        let state = ElasticObservationState::from_snapshot(&snapshot).expect("valid snapshot");
        assert_eq!(state.get("runtime:controller:utilization"), Some(0.75));
        assert_eq!(state.unsupported().len(), 1);
        assert_eq!(state.unsupported()[0].reason(), "provider unavailable");
    }

    #[test]
    fn evaluates_a_pluggable_model_through_the_generic_engine() {
        let now = Instant::now();
        let snapshot = ObservationSnapshot::new(
            now,
            vec![Observation::from_source(
                ObservationSource::runtime("controller"),
                ObservationSignalId::UTILIZATION,
                0.75,
                now,
            )],
        );
        let state = ElasticObservationState::from_snapshot(&snapshot).expect("valid snapshot");
        let intervention = ElasticIntervention::new(
            "reduce-concurrency",
            BTreeMap::from([("delta".to_owned(), -0.25)]),
        )
        .expect("valid intervention");
        let engine = ElasticEngine::new(AddParameterModel);

        assert_eq!(engine.baseline(&state), Ok(0.75));
        assert_eq!(engine.evaluate(&state, &intervention), Ok(0.5));
    }

    #[test]
    fn rejects_duplicate_source_signal_pairs() {
        let now = Instant::now();
        let observation = Observation::from_source(
            ObservationSource::runtime("controller"),
            ObservationSignalId::UTILIZATION,
            0.75,
            now,
        );
        let snapshot = ObservationSnapshot::new(now, vec![observation.clone(), observation]);

        assert!(ElasticObservationState::from_snapshot(&snapshot).is_err());
    }
}
