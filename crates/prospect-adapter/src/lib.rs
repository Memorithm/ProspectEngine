#![forbid(unsafe_code)]

use core::fmt;

use prospect_elastic::{ELASTICXXX_REVISION, ElasticEngine};
use prospect_flat::{FLAT_ATTENTION_REVISION, FlatBooleanAttentionEngine};
use prospect_kv::{KVLAB_KV_EVICTION_HANDOFF_REVISION, KvEvictionEngine};
use prospect_tdi::{TDI_REVISION, TdiEngine};
use serde::Serialize;

pub const ADAPTER_CONTRACT_VERSION: ContractVersion = ContractVersion { major: 1, minor: 0 };

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ContractVersion {
    major: u16,
    minor: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NamespacedId(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AdapterCapability {
    id: NamespacedId,
    version: ContractVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AdapterUpstream {
    component: NamespacedId,
    revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AdapterMetadata {
    adapter_id: NamespacedId,
    contract_version: ContractVersion,
    upstream: Option<AdapterUpstream>,
    capabilities: Vec<AdapterCapability>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterMetadataError {
    ZeroMajorVersion,
    InvalidNamespacedId(String),
    EmptyRevision,
    EmptyCapabilities,
    DuplicateCapability(String),
}

/// Stable metadata boundary for ProspectEngine adapters.
///
/// Implementing this trait describes the adapter's API compatibility and
/// declared capabilities only. A capability declaration is not evidence that a
/// particular run was correct, performant, safe, or physically effective.
pub trait VersionedAdapter {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError>;
}

impl ContractVersion {
    pub const fn new(major: u16, minor: u16) -> Result<Self, AdapterMetadataError> {
        if major == 0 {
            return Err(AdapterMetadataError::ZeroMajorVersion);
        }
        Ok(Self { major, minor })
    }

    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }

    /// Returns true when this provider version can satisfy `required`.
    ///
    /// Minor versions within one major are defined as backward-compatible.
    #[must_use]
    pub const fn supports(self, required: Self) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

impl NamespacedId {
    pub fn new(value: impl Into<String>) -> Result<Self, AdapterMetadataError> {
        let value = value.into();
        if !valid_namespaced_id(&value) {
            return Err(AdapterMetadataError::InvalidNamespacedId(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AdapterCapability {
    pub fn new(
        id: impl Into<String>,
        version: ContractVersion,
    ) -> Result<Self, AdapterMetadataError> {
        Ok(Self {
            id: NamespacedId::new(id)?,
            version,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &NamespacedId {
        &self.id
    }

    #[must_use]
    pub const fn version(&self) -> ContractVersion {
        self.version
    }
}

impl AdapterUpstream {
    pub fn new(
        component: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, AdapterMetadataError> {
        let revision = revision.into();
        if revision.trim().is_empty() {
            return Err(AdapterMetadataError::EmptyRevision);
        }
        Ok(Self {
            component: NamespacedId::new(component)?,
            revision,
        })
    }

    #[must_use]
    pub const fn component(&self) -> &NamespacedId {
        &self.component
    }

    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }
}

impl AdapterMetadata {
    pub fn new(
        adapter_id: impl Into<String>,
        contract_version: ContractVersion,
        upstream: Option<AdapterUpstream>,
        mut capabilities: Vec<AdapterCapability>,
    ) -> Result<Self, AdapterMetadataError> {
        if capabilities.is_empty() {
            return Err(AdapterMetadataError::EmptyCapabilities);
        }
        capabilities.sort_by(|left, right| left.id.cmp(&right.id));
        if let Some(duplicate) = capabilities
            .windows(2)
            .find(|pair| pair[0].id == pair[1].id)
        {
            return Err(AdapterMetadataError::DuplicateCapability(
                duplicate[0].id.as_str().to_owned(),
            ));
        }
        Ok(Self {
            adapter_id: NamespacedId::new(adapter_id)?,
            contract_version,
            upstream,
            capabilities,
        })
    }

    #[must_use]
    pub const fn adapter_id(&self) -> &NamespacedId {
        &self.adapter_id
    }

    #[must_use]
    pub const fn contract_version(&self) -> ContractVersion {
        self.contract_version
    }

    #[must_use]
    pub const fn upstream(&self) -> Option<&AdapterUpstream> {
        self.upstream.as_ref()
    }

    #[must_use]
    pub fn capabilities(&self) -> &[AdapterCapability] {
        &self.capabilities
    }

    #[must_use]
    pub fn supports_capability(&self, id: &str, required: ContractVersion) -> bool {
        self.capabilities
            .iter()
            .find(|capability| capability.id.as_str() == id)
            .is_some_and(|capability| capability.version.supports(required))
    }
}

impl<S> VersionedAdapter for TdiEngine<'_, S> {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        built_in_metadata(
            "prospect.tdi",
            "memorithm.tdi",
            TDI_REVISION,
            "tdi.exact_finite_state_signature",
        )
    }
}

impl<M> VersionedAdapter for ElasticEngine<M> {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        built_in_metadata(
            "prospect.elastic",
            "memorithm.elasticxxx",
            ELASTICXXX_REVISION,
            "elastic.prospective_resource_evaluation",
        )
    }
}

impl<M> VersionedAdapter for FlatBooleanAttentionEngine<M> {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        built_in_metadata(
            "prospect.flat_boolean_attention",
            "memorithm.flat_attention",
            FLAT_ATTENTION_REVISION,
            "flat.boolean_hamming_routing",
        )
    }
}

impl<M> VersionedAdapter for KvEvictionEngine<M> {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        built_in_metadata(
            "prospect.kv_eviction",
            "memorithm.kvlab",
            KVLAB_KV_EVICTION_HANDOFF_REVISION,
            "kvlab.logical_oldest_first_eviction",
        )
    }
}

fn built_in_metadata(
    adapter_id: &str,
    upstream_component: &str,
    upstream_revision: &str,
    domain_capability: &str,
) -> Result<AdapterMetadata, AdapterMetadataError> {
    let capability_version = ContractVersion::new(1, 0)?;
    AdapterMetadata::new(
        adapter_id,
        ADAPTER_CONTRACT_VERSION,
        Some(AdapterUpstream::new(
            upstream_component,
            upstream_revision,
        )?),
        vec![
            AdapterCapability::new("prospect.baseline_evaluation", capability_version)?,
            AdapterCapability::new(
                "prospect.intervention_evaluation",
                capability_version,
            )?,
            AdapterCapability::new(domain_capability, capability_version)?,
        ],
    )
}

fn valid_namespaced_id(value: &str) -> bool {
    if value.len() > 128 || !value.contains('.') {
        return false;
    }
    value.split('.').all(|segment| {
        let mut bytes = segment.bytes();
        let Some(first) = bytes.next() else {
            return false;
        };
        first.is_ascii_lowercase()
            && bytes.all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || byte == b'-'
                    || byte == b'_'
            })
    })
}

impl fmt::Display for AdapterMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroMajorVersion => {
                formatter.write_str("adapter contract major version must be positive")
            }
            Self::InvalidNamespacedId(value) => {
                write!(formatter, "invalid namespaced adapter metadata id {value:?}")
            }
            Self::EmptyRevision => formatter.write_str("adapter upstream revision must not be empty"),
            Self::EmptyCapabilities => {
                formatter.write_str("adapter metadata must declare at least one capability")
            }
            Self::DuplicateCapability(id) => {
                write!(formatter, "duplicate adapter capability {id}")
            }
        }
    }
}

impl std::error::Error for AdapterMetadataError {}

#[cfg(test)]
mod tests {
    use super::{
        ADAPTER_CONTRACT_VERSION, AdapterCapability, AdapterMetadata, AdapterMetadataError,
        AdapterUpstream, ContractVersion, NamespacedId, built_in_metadata,
    };

    #[test]
    fn contract_minor_versions_are_backward_compatible_only_within_one_major() {
        let v1_0 = ContractVersion::new(1, 0).unwrap();
        let v1_2 = ContractVersion::new(1, 2).unwrap();
        let v2_0 = ContractVersion::new(2, 0).unwrap();
        assert!(v1_2.supports(v1_0));
        assert!(!v1_0.supports(v1_2));
        assert!(!v2_0.supports(v1_0));
        assert!(matches!(
            ContractVersion::new(0, 1),
            Err(AdapterMetadataError::ZeroMajorVersion)
        ));
    }

    #[test]
    fn namespaced_ids_are_strict_and_machine_stable() {
        assert_eq!(
            NamespacedId::new("flat.boolean_hamming_routing")
                .unwrap()
                .as_str(),
            "flat.boolean_hamming_routing"
        );
        for invalid in ["flat", ".flat", "flat.", "Flat.routing", "flat..routing"] {
            assert!(NamespacedId::new(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn metadata_sorts_capabilities_and_rejects_duplicate_ids() {
        let version = ContractVersion::new(1, 0).unwrap();
        let metadata = AdapterMetadata::new(
            "prospect.fixture",
            ADAPTER_CONTRACT_VERSION,
            None,
            vec![
                AdapterCapability::new("fixture.zeta", version).unwrap(),
                AdapterCapability::new("fixture.alpha", version).unwrap(),
            ],
        )
        .unwrap();
        assert_eq!(metadata.capabilities()[0].id().as_str(), "fixture.alpha");
        assert_eq!(metadata.capabilities()[1].id().as_str(), "fixture.zeta");

        let duplicate = AdapterMetadata::new(
            "prospect.fixture",
            ADAPTER_CONTRACT_VERSION,
            None,
            vec![
                AdapterCapability::new("fixture.same", version).unwrap(),
                AdapterCapability::new("fixture.same", ContractVersion::new(1, 1).unwrap()).unwrap(),
            ],
        );
        assert!(matches!(
            duplicate,
            Err(AdapterMetadataError::DuplicateCapability(id)) if id == "fixture.same"
        ));
    }

    #[test]
    fn upstream_revision_is_explicit_and_capability_queries_are_versioned() {
        let metadata = built_in_metadata(
            "prospect.fixture",
            "memorithm.fixture",
            "0123456789abcdef0123456789abcdef01234567",
            "fixture.exact_operation",
        )
        .unwrap();
        let upstream = metadata.upstream().unwrap();
        assert_eq!(upstream.component().as_str(), "memorithm.fixture");
        assert_eq!(
            upstream.revision(),
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert!(metadata.supports_capability(
            "fixture.exact_operation",
            ContractVersion::new(1, 0).unwrap()
        ));
        assert!(!metadata.supports_capability(
            "fixture.exact_operation",
            ContractVersion::new(1, 1).unwrap()
        ));
    }

    #[test]
    fn upstream_requires_a_nonempty_revision() {
        assert!(matches!(
            AdapterUpstream::new("memorithm.fixture", "  "),
            Err(AdapterMetadataError::EmptyRevision)
        ));
    }
}
