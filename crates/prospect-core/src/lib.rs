#![forbid(unsafe_code)]

use core::fmt;

/// Stable identifier for one candidate intervention scenario.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScenarioId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidScenarioId;

impl ScenarioId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidScenarioId> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(InvalidScenarioId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScenarioId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for InvalidScenarioId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("scenario id must not be empty")
    }
}

impl std::error::Error for InvalidScenarioId {}

/// A named candidate intervention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scenario<I> {
    id: ScenarioId,
    intervention: I,
}

impl<I> Scenario<I> {
    #[must_use]
    pub const fn new(id: ScenarioId, intervention: I) -> Self {
        Self { id, intervention }
    }

    #[must_use]
    pub const fn id(&self) -> &ScenarioId {
        &self.id
    }

    #[must_use]
    pub const fn intervention(&self) -> &I {
        &self.intervention
    }

    #[must_use]
    pub fn into_parts(self) -> (ScenarioId, I) {
        (self.id, self.intervention)
    }
}

/// Minimal engine contract. Domain adapters translate their state and
/// interventions into one implementation of this interface.
pub trait ProspectiveEngine<State, Intervention> {
    type Signature;
    type Error;

    fn baseline(&self, state: &State) -> Result<Self::Signature, Self::Error>;

    fn evaluate(
        &self,
        state: &State,
        intervention: &Intervention,
    ) -> Result<Self::Signature, Self::Error>;
}

/// Compares two prospective signatures without imposing a concrete metric.
pub trait SignatureMetric<Signature> {
    type Score;

    fn compare(&self, reference: &Signature, candidate: &Signature) -> Self::Score;
}

/// Assigns an application-specific utility to one prospective signature.
/// Higher scores are preferred by the generic ranking layer.
pub trait DecisionPolicy<Signature> {
    type Score: Ord;

    fn utility(&self, signature: &Signature) -> Self::Score;
}

#[cfg(test)]
mod tests {
    use super::{InvalidScenarioId, ScenarioId};

    #[test]
    fn rejects_empty_scenario_ids() {
        assert_eq!(ScenarioId::new("  "), Err(InvalidScenarioId));
    }

    #[test]
    fn preserves_scenario_ids() {
        let id = ScenarioId::new("gpu-loss").expect("non-empty id");
        assert_eq!(id.as_str(), "gpu-loss");
    }
}
