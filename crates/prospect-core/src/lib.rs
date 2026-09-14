#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeSet;

/// Adapter metadata contract understood by this ProspectEngine release line.
pub const PROSPECT_ADAPTER_API_V1: ContractVersion = ContractVersion::new(1, 0);

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

/// Major/minor contract version. Compatibility requires the same major
/// version and an implementation minor version at least as new as required.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContractVersion {
    major: u16,
    minor: u16,
}

impl ContractVersion {
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }

    #[must_use]
    pub const fn satisfies(self, required: Self) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

/// Stable machine identifier for one ProspectEngine adapter.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdapterId(String);

/// Stable machine identifier for one adapter capability.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityId(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidStableId(String);

impl AdapterId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidStableId> {
        stable_id(value.into()).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl CapabilityId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidStableId> {
        stable_id(value.into()).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn stable_id(value: String) -> Result<String, InvalidStableId> {
    let valid = !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'-' | b'_')
        })
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    if valid {
        Ok(value)
    } else {
        Err(InvalidStableId(value))
    }
}

impl fmt::Display for InvalidStableId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "stable id {:?} must use lowercase ASCII letters, digits, '.', '-' or '_' and start/end with an alphanumeric character",
            self.0
        )
    }
}

impl std::error::Error for InvalidStableId {}

/// One versioned capability implemented by an adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterCapability {
    id: CapabilityId,
    version: ContractVersion,
}

impl AdapterCapability {
    #[must_use]
    pub const fn new(id: CapabilityId, version: ContractVersion) -> Self {
        Self { id, version }
    }

    #[must_use]
    pub const fn id(&self) -> &CapabilityId {
        &self.id
    }

    #[must_use]
    pub const fn version(&self) -> ContractVersion {
        self.version
    }
}

/// Immutable metadata used to discover an adapter before invoking domain code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterDescriptor {
    id: AdapterId,
    api_version: ContractVersion,
    capabilities: Vec<AdapterCapability>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateCapability(CapabilityId);

impl AdapterDescriptor {
    pub fn new(
        id: AdapterId,
        api_version: ContractVersion,
        mut capabilities: Vec<AdapterCapability>,
    ) -> Result<Self, DuplicateCapability> {
        capabilities.sort_by(|left, right| left.id.cmp(&right.id));
        let mut seen = BTreeSet::new();
        for capability in &capabilities {
            if !seen.insert(capability.id.clone()) {
                return Err(DuplicateCapability(capability.id.clone()));
            }
        }
        Ok(Self {
            id,
            api_version,
            capabilities,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &AdapterId {
        &self.id
    }

    #[must_use]
    pub const fn api_version(&self) -> ContractVersion {
        self.api_version
    }

    #[must_use]
    pub fn capabilities(&self) -> &[AdapterCapability] {
        &self.capabilities
    }

    #[must_use]
    pub fn supports(&self, id: &CapabilityId, required: ContractVersion) -> bool {
        self.capabilities
            .binary_search_by(|capability| capability.id.cmp(id))
            .ok()
            .is_some_and(|index| self.capabilities[index].version.satisfies(required))
    }
}

impl fmt::Display for DuplicateCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "duplicate adapter capability {}", self.0.as_str())
    }
}

impl std::error::Error for DuplicateCapability {}

/// Discovery boundary for a ProspectEngine adapter. This trait intentionally
/// exposes metadata only; execution remains in the existing domain-specific
/// engine contracts.
pub trait ProspectAdapter {
    fn descriptor(&self) -> &AdapterDescriptor;
}

#[cfg(test)]
mod tests {
    use super::{
        AdapterCapability, AdapterDescriptor, AdapterId, CapabilityId, ContractVersion,
        DuplicateCapability, InvalidScenarioId, PROSPECT_ADAPTER_API_V1, ScenarioId,
    };

    #[test]
    fn rejects_empty_scenario_ids() {
        assert_eq!(ScenarioId::new("  "), Err(InvalidScenarioId));
    }

    #[test]
    fn preserves_scenario_ids() {
        let id = ScenarioId::new("gpu-loss").expect("non-empty id");
        assert_eq!(id.as_str(), "gpu-loss");
    }

    #[test]
    fn validates_stable_adapter_and_capability_ids() {
        assert!(AdapterId::new("elastic.runtime").is_ok());
        assert!(CapabilityId::new("observe-v1").is_ok());
        assert!(AdapterId::new("Elastic Runtime").is_err());
        assert!(CapabilityId::new("-observe").is_err());
    }

    #[test]
    fn contract_version_compatibility_is_major_strict_and_minor_monotonic() {
        assert!(ContractVersion::new(1, 3).satisfies(ContractVersion::new(1, 2)));
        assert!(!ContractVersion::new(1, 1).satisfies(ContractVersion::new(1, 2)));
        assert!(!ContractVersion::new(2, 0).satisfies(ContractVersion::new(1, 9)));
    }

    #[test]
    fn descriptor_sorts_capabilities_and_checks_required_versions() {
        let observe = CapabilityId::new("observe").unwrap();
        let evaluate = CapabilityId::new("evaluate").unwrap();
        let descriptor = AdapterDescriptor::new(
            AdapterId::new("elastic.runtime").unwrap(),
            PROSPECT_ADAPTER_API_V1,
            vec![
                AdapterCapability::new(observe.clone(), ContractVersion::new(1, 2)),
                AdapterCapability::new(evaluate.clone(), ContractVersion::new(1, 0)),
            ],
        )
        .unwrap();

        assert_eq!(descriptor.capabilities()[0].id(), &evaluate);
        assert_eq!(descriptor.capabilities()[1].id(), &observe);
        assert!(descriptor.supports(&observe, ContractVersion::new(1, 1)));
        assert!(!descriptor.supports(&observe, ContractVersion::new(2, 0)));
    }

    #[test]
    fn descriptor_rejects_duplicate_capability_ids() {
        let observe = CapabilityId::new("observe").unwrap();
        let error = AdapterDescriptor::new(
            AdapterId::new("elastic.runtime").unwrap(),
            PROSPECT_ADAPTER_API_V1,
            vec![
                AdapterCapability::new(observe.clone(), ContractVersion::new(1, 0)),
                AdapterCapability::new(observe.clone(), ContractVersion::new(1, 1)),
            ],
        )
        .unwrap_err();
        assert_eq!(error, DuplicateCapability(observe));
    }
}
