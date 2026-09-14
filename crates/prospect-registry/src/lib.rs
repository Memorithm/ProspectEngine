#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeMap;

use prospect_adapter::{ContractVersion, NamespacedId};
use prospect_core::{DecisionPolicy, SignatureMetric};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryEntryMetadata {
    id: NamespacedId,
    version: ContractVersion,
}

impl RegistryEntryMetadata {
    #[must_use]
    pub const fn id(&self) -> &NamespacedId {
        &self.id
    }

    #[must_use]
    pub const fn version(&self) -> ContractVersion {
        self.version
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryRegistrationError {
    InvalidId(String),
    DuplicateId(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryLookupError {
    InvalidId(String),
    MissingId(String),
    IncompatibleVersion {
        id: String,
        offered: ContractVersion,
        required: ContractVersion,
    },
}

struct MetricEntry<Signature, Score> {
    metadata: RegistryEntryMetadata,
    implementation: Box<dyn SignatureMetric<Signature, Score = Score> + Send + Sync>,
}

pub struct MetricRegistry<Signature, Score> {
    entries: BTreeMap<NamespacedId, MetricEntry<Signature, Score>>,
}

impl<Signature, Score> Default for MetricRegistry<Signature, Score> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Signature, Score> MetricRegistry<Signature, Score> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn register<M>(
        &mut self,
        id: impl Into<String>,
        version: ContractVersion,
        metric: M,
    ) -> Result<(), RegistryRegistrationError>
    where
        M: SignatureMetric<Signature, Score = Score> + Send + Sync + 'static,
    {
        let id = parse_id(id.into()).map_err(RegistryRegistrationError::InvalidId)?;
        if self.entries.contains_key(&id) {
            return Err(RegistryRegistrationError::DuplicateId(
                id.as_str().to_owned(),
            ));
        }
        let metadata = RegistryEntryMetadata {
            id: id.clone(),
            version,
        };
        self.entries.insert(
            id,
            MetricEntry {
                metadata,
                implementation: Box::new(metric),
            },
        );
        Ok(())
    }

    pub fn resolve(
        &self,
        id: &str,
        required: ContractVersion,
    ) -> Result<&(dyn SignatureMetric<Signature, Score = Score> + Send + Sync), RegistryLookupError>
    {
        let id = parse_id(id.to_owned()).map_err(RegistryLookupError::InvalidId)?;
        let entry = self
            .entries
            .get(&id)
            .ok_or_else(|| RegistryLookupError::MissingId(id.as_str().to_owned()))?;
        if !entry.metadata.version.supports(required) {
            return Err(RegistryLookupError::IncompatibleVersion {
                id: id.as_str().to_owned(),
                offered: entry.metadata.version,
                required,
            });
        }
        Ok(entry.implementation.as_ref())
    }

    #[must_use]
    pub fn metadata(&self) -> Vec<&RegistryEntryMetadata> {
        self.entries.values().map(|entry| &entry.metadata).collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

struct PolicyEntry<Signature, Score>
where
    Score: Ord,
{
    metadata: RegistryEntryMetadata,
    implementation: Box<dyn DecisionPolicy<Signature, Score = Score> + Send + Sync>,
}

pub struct DecisionPolicyRegistry<Signature, Score>
where
    Score: Ord,
{
    entries: BTreeMap<NamespacedId, PolicyEntry<Signature, Score>>,
}

impl<Signature, Score> Default for DecisionPolicyRegistry<Signature, Score>
where
    Score: Ord,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Signature, Score> DecisionPolicyRegistry<Signature, Score>
where
    Score: Ord,
{
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn register<P>(
        &mut self,
        id: impl Into<String>,
        version: ContractVersion,
        policy: P,
    ) -> Result<(), RegistryRegistrationError>
    where
        P: DecisionPolicy<Signature, Score = Score> + Send + Sync + 'static,
    {
        let id = parse_id(id.into()).map_err(RegistryRegistrationError::InvalidId)?;
        if self.entries.contains_key(&id) {
            return Err(RegistryRegistrationError::DuplicateId(
                id.as_str().to_owned(),
            ));
        }
        let metadata = RegistryEntryMetadata {
            id: id.clone(),
            version,
        };
        self.entries.insert(
            id,
            PolicyEntry {
                metadata,
                implementation: Box::new(policy),
            },
        );
        Ok(())
    }

    pub fn resolve(
        &self,
        id: &str,
        required: ContractVersion,
    ) -> Result<&(dyn DecisionPolicy<Signature, Score = Score> + Send + Sync), RegistryLookupError>
    {
        let id = parse_id(id.to_owned()).map_err(RegistryLookupError::InvalidId)?;
        let entry = self
            .entries
            .get(&id)
            .ok_or_else(|| RegistryLookupError::MissingId(id.as_str().to_owned()))?;
        if !entry.metadata.version.supports(required) {
            return Err(RegistryLookupError::IncompatibleVersion {
                id: id.as_str().to_owned(),
                offered: entry.metadata.version,
                required,
            });
        }
        Ok(entry.implementation.as_ref())
    }

    #[must_use]
    pub fn metadata(&self) -> Vec<&RegistryEntryMetadata> {
        self.entries.values().map(|entry| &entry.metadata).collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn parse_id(value: String) -> Result<NamespacedId, String> {
    NamespacedId::new(value.clone()).map_err(|_| value)
}

impl fmt::Display for RegistryRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(id) => write!(formatter, "invalid registry id {id:?}"),
            Self::DuplicateId(id) => write!(formatter, "duplicate registry id {id}"),
        }
    }
}

impl std::error::Error for RegistryRegistrationError {}

impl fmt::Display for RegistryLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(id) => write!(formatter, "invalid registry id {id:?}"),
            Self::MissingId(id) => write!(formatter, "registry id {id} is not registered"),
            Self::IncompatibleVersion {
                id,
                offered,
                required,
            } => write!(
                formatter,
                "registry id {id} offers {}.{} but {}.{} is required",
                offered.major(),
                offered.minor(),
                required.major(),
                required.minor()
            ),
        }
    }
}

impl std::error::Error for RegistryLookupError {}

#[cfg(test)]
mod tests {
    use prospect_adapter::ContractVersion;
    use prospect_core::{DecisionPolicy, SignatureMetric};

    use super::{
        DecisionPolicyRegistry, MetricRegistry, RegistryLookupError, RegistryRegistrationError,
    };

    struct AbsoluteDistance;

    impl SignatureMetric<i32> for AbsoluteDistance {
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

    #[test]
    fn metric_registry_resolves_and_executes_compatible_metric() {
        let mut registry = MetricRegistry::<i32, i32>::new();
        registry
            .register(
                "metric.absolute_distance",
                ContractVersion::new(1, 2).unwrap(),
                AbsoluteDistance,
            )
            .unwrap();

        let metric = registry
            .resolve(
                "metric.absolute_distance",
                ContractVersion::new(1, 0).unwrap(),
            )
            .unwrap();
        assert_eq!(metric.compare(&10, &17), 7);
    }

    #[test]
    fn metric_registry_rejects_duplicates_and_incompatible_versions() {
        let mut registry = MetricRegistry::<i32, i32>::new();
        registry
            .register(
                "metric.absolute_distance",
                ContractVersion::new(1, 0).unwrap(),
                AbsoluteDistance,
            )
            .unwrap();
        assert!(matches!(
            registry.register(
                "metric.absolute_distance",
                ContractVersion::new(1, 1).unwrap(),
                AbsoluteDistance,
            ),
            Err(RegistryRegistrationError::DuplicateId(id))
                if id == "metric.absolute_distance"
        ));
        assert!(matches!(
            registry.resolve(
                "metric.absolute_distance",
                ContractVersion::new(1, 1).unwrap(),
            ),
            Err(RegistryLookupError::IncompatibleVersion { .. })
        ));
    }

    #[test]
    fn policy_registry_resolves_and_executes_policy() {
        let mut registry = DecisionPolicyRegistry::<i32, i32>::new();
        registry
            .register(
                "policy.prefer_higher",
                ContractVersion::new(1, 0).unwrap(),
                PreferHigher,
            )
            .unwrap();
        let policy = registry
            .resolve("policy.prefer_higher", ContractVersion::new(1, 0).unwrap())
            .unwrap();
        assert_eq!(policy.utility(&23), 23);
    }

    #[test]
    fn metadata_is_deterministic_and_sorted_by_id() {
        let mut registry = MetricRegistry::<i32, i32>::new();
        registry
            .register(
                "metric.zeta",
                ContractVersion::new(1, 0).unwrap(),
                AbsoluteDistance,
            )
            .unwrap();
        registry
            .register(
                "metric.alpha",
                ContractVersion::new(1, 0).unwrap(),
                AbsoluteDistance,
            )
            .unwrap();
        let metadata = registry.metadata();
        assert_eq!(metadata[0].id().as_str(), "metric.alpha");
        assert_eq!(metadata[1].id().as_str(), "metric.zeta");
    }

    #[test]
    fn lookup_rejects_invalid_or_missing_ids() {
        let registry = MetricRegistry::<i32, i32>::new();
        assert!(matches!(
            registry.resolve("not-namespaced", ContractVersion::new(1, 0).unwrap()),
            Err(RegistryLookupError::InvalidId(id)) if id == "not-namespaced"
        ));
        assert!(matches!(
            registry.resolve("metric.missing", ContractVersion::new(1, 0).unwrap()),
            Err(RegistryLookupError::MissingId(id)) if id == "metric.missing"
        ));
    }
}
