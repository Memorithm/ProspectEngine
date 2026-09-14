use core::fmt;

use prospect_adapter::{AdapterMetadata, AdapterMetadataError, ContractVersion, NamespacedId};
use prospect_bundle::ScenarioBundle;
use serde::{Deserialize, Serialize};

pub const DISPATCH_CATALOG_SCHEMA_V1: &str = "prospect.dispatch-catalog/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableUpstream {
    component: NamespacedId,
    revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableAdapter {
    adapter_id: NamespacedId,
    contract_version: ContractVersion,
    upstream: Option<AvailableUpstream>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableRegistryEntry {
    id: NamespacedId,
    version: ContractVersion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchCatalog {
    adapters: Vec<AvailableAdapter>,
    metrics: Vec<AvailableRegistryEntry>,
    policies: Vec<AvailableRegistryEntry>,
}

pub struct ResolvedCatalogRequirements<'a> {
    adapter: &'a AvailableAdapter,
    metric: Option<&'a AvailableRegistryEntry>,
    policy: Option<&'a AvailableRegistryEntry>,
}

#[derive(Debug)]
pub enum DispatchCatalogError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    AdapterMetadata(AdapterMetadataError),
    EmptyUpstreamRevision,
    DuplicateAdapter(String),
    DuplicateMetric(String),
    DuplicatePolicy(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogPreflightError {
    MissingAdapter(String),
    IncompatibleAdapterVersion {
        id: String,
        offered: ContractVersion,
        required: ContractVersion,
    },
    AdapterUpstreamMismatch {
        id: String,
        required_component: String,
        required_revision: String,
    },
    MissingMetric(String),
    IncompatibleMetricVersion {
        id: String,
        offered: ContractVersion,
        required: ContractVersion,
    },
    MissingPolicy(String),
    IncompatiblePolicyVersion {
        id: String,
        offered: ContractVersion,
        required: ContractVersion,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogWire {
    schema: String,
    adapters: Vec<AdapterWire>,
    metrics: Vec<RegistryWire>,
    policies: Vec<RegistryWire>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterWire {
    adapter_id: String,
    contract_version: VersionWire,
    upstream: Option<UpstreamWire>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpstreamWire {
    component: String,
    revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryWire {
    id: String,
    version: VersionWire,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionWire {
    major: u16,
    minor: u16,
}

impl AvailableUpstream {
    pub fn new(
        component: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, DispatchCatalogError> {
        let revision = revision.into();
        if revision.trim().is_empty() {
            return Err(DispatchCatalogError::EmptyUpstreamRevision);
        }
        Ok(Self {
            component: NamespacedId::new(component)
                .map_err(DispatchCatalogError::AdapterMetadata)?,
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

impl AvailableAdapter {
    pub fn new(
        adapter_id: impl Into<String>,
        contract_version: ContractVersion,
        upstream: Option<AvailableUpstream>,
    ) -> Result<Self, DispatchCatalogError> {
        Ok(Self {
            adapter_id: NamespacedId::new(adapter_id)
                .map_err(DispatchCatalogError::AdapterMetadata)?,
            contract_version,
            upstream,
        })
    }

    #[must_use]
    pub fn from_metadata(metadata: &AdapterMetadata) -> Self {
        Self {
            adapter_id: metadata.adapter_id().clone(),
            contract_version: metadata.contract_version(),
            upstream: metadata.upstream().map(|upstream| AvailableUpstream {
                component: upstream.component().clone(),
                revision: upstream.revision().to_owned(),
            }),
        }
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
    pub const fn upstream(&self) -> Option<&AvailableUpstream> {
        self.upstream.as_ref()
    }
}

impl AvailableRegistryEntry {
    pub fn new(
        id: impl Into<String>,
        version: ContractVersion,
    ) -> Result<Self, DispatchCatalogError> {
        Ok(Self {
            id: NamespacedId::new(id).map_err(DispatchCatalogError::AdapterMetadata)?,
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

impl DispatchCatalog {
    pub fn new(
        mut adapters: Vec<AvailableAdapter>,
        mut metrics: Vec<AvailableRegistryEntry>,
        mut policies: Vec<AvailableRegistryEntry>,
    ) -> Result<Self, DispatchCatalogError> {
        adapters.sort_by(|left, right| left.adapter_id.cmp(&right.adapter_id));
        metrics.sort_by(|left, right| left.id.cmp(&right.id));
        policies.sort_by(|left, right| left.id.cmp(&right.id));
        reject_duplicate_adapters(&adapters)?;
        reject_duplicate_registry(&metrics, true)?;
        reject_duplicate_registry(&policies, false)?;
        Ok(Self {
            adapters,
            metrics,
            policies,
        })
    }

    pub fn from_adapter_metadata(
        adapters: &[AdapterMetadata],
    ) -> Result<Self, DispatchCatalogError> {
        Self::new(
            adapters
                .iter()
                .map(AvailableAdapter::from_metadata)
                .collect(),
            Vec::new(),
            Vec::new(),
        )
    }

    #[must_use]
    pub fn adapters(&self) -> &[AvailableAdapter] {
        &self.adapters
    }

    #[must_use]
    pub fn metrics(&self) -> &[AvailableRegistryEntry] {
        &self.metrics
    }

    #[must_use]
    pub fn policies(&self) -> &[AvailableRegistryEntry] {
        &self.policies
    }

    pub fn canonical_json(&self) -> Result<String, DispatchCatalogError> {
        serde_json::to_string(&self.to_wire()).map_err(DispatchCatalogError::Json)
    }

    pub fn from_canonical_json(payload: &str) -> Result<Self, DispatchCatalogError> {
        let wire: CatalogWire =
            serde_json::from_str(payload).map_err(DispatchCatalogError::Json)?;
        if wire.schema != DISPATCH_CATALOG_SCHEMA_V1 {
            return Err(DispatchCatalogError::UnsupportedSchema);
        }
        let catalog = Self::new(
            wire.adapters
                .into_iter()
                .map(adapter_from_wire)
                .collect::<Result<Vec<_>, _>>()?,
            wire.metrics
                .into_iter()
                .map(registry_from_wire)
                .collect::<Result<Vec<_>, _>>()?,
            wire.policies
                .into_iter()
                .map(registry_from_wire)
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        if catalog.canonical_json()? != payload {
            return Err(DispatchCatalogError::NonCanonicalJson);
        }
        Ok(catalog)
    }

    pub fn resolve_bundle<State, Intervention>(
        &self,
        bundle: &ScenarioBundle<State, Intervention>,
    ) -> Result<ResolvedCatalogRequirements<'_>, CatalogPreflightError> {
        let required_adapter = bundle.adapter();
        let adapter_id = required_adapter.adapter_id().as_str();
        let adapter = self
            .adapters
            .iter()
            .find(|candidate| candidate.adapter_id().as_str() == adapter_id)
            .ok_or_else(|| CatalogPreflightError::MissingAdapter(adapter_id.to_owned()))?;

        let offered_version = adapter.contract_version();
        let required_version = required_adapter.contract_version();
        if !offered_version.supports(required_version) {
            return Err(CatalogPreflightError::IncompatibleAdapterVersion {
                id: adapter_id.to_owned(),
                offered: offered_version,
                required: required_version,
            });
        }

        if let Some(required_upstream) = required_adapter.upstream() {
            let matches = adapter.upstream().is_some_and(|candidate| {
                candidate.component() == required_upstream.component()
                    && candidate.revision() == required_upstream.revision()
            });
            if !matches {
                return Err(CatalogPreflightError::AdapterUpstreamMismatch {
                    id: adapter_id.to_owned(),
                    required_component: required_upstream.component().as_str().to_owned(),
                    required_revision: required_upstream.revision().to_owned(),
                });
            }
        }

        let metric = bundle
            .metric()
            .map(|requirement| {
                resolve_registry_requirement(
                    &self.metrics,
                    requirement.id().as_str(),
                    requirement.version(),
                    true,
                )
            })
            .transpose()?;
        let policy = bundle
            .policy()
            .map(|requirement| {
                resolve_registry_requirement(
                    &self.policies,
                    requirement.id().as_str(),
                    requirement.version(),
                    false,
                )
            })
            .transpose()?;

        Ok(ResolvedCatalogRequirements {
            adapter,
            metric,
            policy,
        })
    }

    fn to_wire(&self) -> CatalogWire {
        CatalogWire {
            schema: DISPATCH_CATALOG_SCHEMA_V1.to_owned(),
            adapters: self
                .adapters
                .iter()
                .map(|adapter| AdapterWire {
                    adapter_id: adapter.adapter_id.as_str().to_owned(),
                    contract_version: adapter.contract_version.into(),
                    upstream: adapter.upstream.as_ref().map(|upstream| UpstreamWire {
                        component: upstream.component.as_str().to_owned(),
                        revision: upstream.revision.clone(),
                    }),
                })
                .collect(),
            metrics: self.metrics.iter().map(registry_to_wire).collect(),
            policies: self.policies.iter().map(registry_to_wire).collect(),
        }
    }
}

impl<'a> ResolvedCatalogRequirements<'a> {
    #[must_use]
    pub const fn adapter(&self) -> &'a AvailableAdapter {
        self.adapter
    }

    #[must_use]
    pub const fn metric(&self) -> Option<&'a AvailableRegistryEntry> {
        self.metric
    }

    #[must_use]
    pub const fn policy(&self) -> Option<&'a AvailableRegistryEntry> {
        self.policy
    }
}

fn reject_duplicate_adapters(adapters: &[AvailableAdapter]) -> Result<(), DispatchCatalogError> {
    if let Some(pair) = adapters
        .windows(2)
        .find(|pair| pair[0].adapter_id == pair[1].adapter_id)
    {
        return Err(DispatchCatalogError::DuplicateAdapter(
            pair[0].adapter_id.as_str().to_owned(),
        ));
    }
    Ok(())
}

fn reject_duplicate_registry(
    entries: &[AvailableRegistryEntry],
    metric: bool,
) -> Result<(), DispatchCatalogError> {
    if let Some(pair) = entries.windows(2).find(|pair| pair[0].id == pair[1].id) {
        let id = pair[0].id.as_str().to_owned();
        return if metric {
            Err(DispatchCatalogError::DuplicateMetric(id))
        } else {
            Err(DispatchCatalogError::DuplicatePolicy(id))
        };
    }
    Ok(())
}

fn adapter_from_wire(wire: AdapterWire) -> Result<AvailableAdapter, DispatchCatalogError> {
    AvailableAdapter::new(
        wire.adapter_id,
        wire.contract_version.try_into()?,
        wire.upstream
            .map(|upstream| AvailableUpstream::new(upstream.component, upstream.revision))
            .transpose()?,
    )
}

fn registry_from_wire(wire: RegistryWire) -> Result<AvailableRegistryEntry, DispatchCatalogError> {
    AvailableRegistryEntry::new(wire.id, wire.version.try_into()?)
}

fn registry_to_wire(entry: &AvailableRegistryEntry) -> RegistryWire {
    RegistryWire {
        id: entry.id.as_str().to_owned(),
        version: entry.version.into(),
    }
}

fn resolve_registry_requirement<'a>(
    entries: &'a [AvailableRegistryEntry],
    id: &str,
    required: ContractVersion,
    metric: bool,
) -> Result<&'a AvailableRegistryEntry, CatalogPreflightError> {
    let Some(entry) = entries.iter().find(|candidate| candidate.id.as_str() == id) else {
        return if metric {
            Err(CatalogPreflightError::MissingMetric(id.to_owned()))
        } else {
            Err(CatalogPreflightError::MissingPolicy(id.to_owned()))
        };
    };
    let offered = entry.version();
    if !offered.supports(required) {
        return if metric {
            Err(CatalogPreflightError::IncompatibleMetricVersion {
                id: id.to_owned(),
                offered,
                required,
            })
        } else {
            Err(CatalogPreflightError::IncompatiblePolicyVersion {
                id: id.to_owned(),
                offered,
                required,
            })
        };
    }
    Ok(entry)
}

impl From<ContractVersion> for VersionWire {
    fn from(value: ContractVersion) -> Self {
        Self {
            major: value.major(),
            minor: value.minor(),
        }
    }
}

impl TryFrom<VersionWire> for ContractVersion {
    type Error = DispatchCatalogError;

    fn try_from(value: VersionWire) -> Result<Self, Self::Error> {
        ContractVersion::new(value.major, value.minor)
            .map_err(DispatchCatalogError::AdapterMetadata)
    }
}

impl fmt::Display for DispatchCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid dispatch catalog JSON: {error}"),
            Self::NonCanonicalJson => formatter.write_str("dispatch catalog JSON is not canonical"),
            Self::UnsupportedSchema => formatter.write_str("unsupported dispatch catalog schema"),
            Self::AdapterMetadata(error) => write!(formatter, "invalid dispatch metadata: {error}"),
            Self::EmptyUpstreamRevision => {
                formatter.write_str("dispatch upstream revision must not be empty")
            }
            Self::DuplicateAdapter(id) => write!(formatter, "duplicate available adapter {id}"),
            Self::DuplicateMetric(id) => write!(formatter, "duplicate available metric {id}"),
            Self::DuplicatePolicy(id) => write!(formatter, "duplicate available policy {id}"),
        }
    }
}

impl std::error::Error for DispatchCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::AdapterMetadata(error) => Some(error),
            Self::NonCanonicalJson
            | Self::UnsupportedSchema
            | Self::EmptyUpstreamRevision
            | Self::DuplicateAdapter(_)
            | Self::DuplicateMetric(_)
            | Self::DuplicatePolicy(_) => None,
        }
    }
}

impl fmt::Display for CatalogPreflightError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAdapter(id) => write!(formatter, "catalog has no adapter {id}"),
            Self::IncompatibleAdapterVersion {
                id,
                offered,
                required,
            } => write!(
                formatter,
                "catalog adapter {id} offers {}.{} but {}.{} is required",
                offered.major(),
                offered.minor(),
                required.major(),
                required.minor()
            ),
            Self::AdapterUpstreamMismatch {
                id,
                required_component,
                required_revision,
            } => write!(
                formatter,
                "catalog adapter {id} does not match required upstream {required_component}@{required_revision}"
            ),
            Self::MissingMetric(id) => write!(formatter, "catalog has no metric {id}"),
            Self::IncompatibleMetricVersion {
                id,
                offered,
                required,
            } => write!(
                formatter,
                "catalog metric {id} offers {}.{} but {}.{} is required",
                offered.major(),
                offered.minor(),
                required.major(),
                required.minor()
            ),
            Self::MissingPolicy(id) => write!(formatter, "catalog has no policy {id}"),
            Self::IncompatiblePolicyVersion {
                id,
                offered,
                required,
            } => write!(
                formatter,
                "catalog policy {id} offers {}.{} but {}.{} is required",
                offered.major(),
                offered.minor(),
                required.major(),
                required.minor()
            ),
        }
    }
}

impl std::error::Error for CatalogPreflightError {}

#[cfg(test)]
mod tests {
    use prospect_adapter::{
        AdapterCapability, AdapterMetadata, AdapterUpstream, ContractVersion,
    };
    use prospect_bundle::{
        AdapterBinding, BundleScenario, RegistryRequirement, ScenarioBundle, UpstreamBinding,
    };
    use prospect_core::ScenarioId;

    use super::{
        AvailableAdapter, AvailableRegistryEntry, AvailableUpstream, CatalogPreflightError,
        DispatchCatalog, DispatchCatalogError,
    };

    fn version(major: u16, minor: u16) -> ContractVersion {
        ContractVersion::new(major, minor).unwrap()
    }

    fn bundle(metric: bool, policy: bool, revision: &str) -> ScenarioBundle<i32, i32> {
        ScenarioBundle::new(
            "experiment.fixture",
            AdapterBinding::new(
                "prospect.fixture",
                version(1, 0),
                Some(UpstreamBinding::new("memorithm.fixture", revision).unwrap()),
            )
            .unwrap(),
            Some(3),
            10,
            vec![BundleScenario::new(
                ScenarioId::new("candidate").unwrap(),
                1,
            )],
            metric.then(|| RegistryRequirement::new("metric.distance", version(1, 0)).unwrap()),
            policy.then(|| RegistryRequirement::new("policy.prefer", version(1, 1)).unwrap()),
        )
        .unwrap()
    }

    fn catalog() -> DispatchCatalog {
        DispatchCatalog::new(
            vec![
                AvailableAdapter::new(
                    "prospect.fixture",
                    version(1, 2),
                    Some(
                        AvailableUpstream::new(
                            "memorithm.fixture",
                            "0123456789abcdef0123456789abcdef01234567",
                        )
                        .unwrap(),
                    ),
                )
                .unwrap(),
            ],
            vec![AvailableRegistryEntry::new("metric.distance", version(1, 3)).unwrap()],
            vec![AvailableRegistryEntry::new("policy.prefer", version(1, 1)).unwrap()],
        )
        .unwrap()
    }

    #[test]
    fn canonical_catalog_round_trips_with_deterministic_ordering() {
        let catalog = DispatchCatalog::new(
            vec![
                AvailableAdapter::new("prospect.zeta", version(1, 0), None).unwrap(),
                AvailableAdapter::new("prospect.alpha", version(1, 0), None).unwrap(),
            ],
            vec![
                AvailableRegistryEntry::new("metric.zeta", version(1, 0)).unwrap(),
                AvailableRegistryEntry::new("metric.alpha", version(1, 0)).unwrap(),
            ],
            Vec::new(),
        )
        .unwrap();
        let payload = catalog.canonical_json().unwrap();
        let replay = DispatchCatalog::from_canonical_json(&payload).unwrap();
        assert_eq!(replay, catalog);
        assert_eq!(replay.adapters()[0].adapter_id().as_str(), "prospect.alpha");
        assert_eq!(replay.metrics()[0].id().as_str(), "metric.alpha");
    }

    #[test]
    fn catalog_rejects_duplicate_ids_and_noncanonical_json() {
        assert!(matches!(
            DispatchCatalog::new(
                vec![
                    AvailableAdapter::new("prospect.same", version(1, 0), None).unwrap(),
                    AvailableAdapter::new("prospect.same", version(1, 1), None).unwrap(),
                ],
                Vec::new(),
                Vec::new(),
            ),
            Err(DispatchCatalogError::DuplicateAdapter(id)) if id == "prospect.same"
        ));
        let payload = catalog().canonical_json().unwrap();
        assert!(matches!(
            DispatchCatalog::from_canonical_json(&format!(" {payload}")),
            Err(DispatchCatalogError::NonCanonicalJson)
        ));
    }

    #[test]
    fn catalog_can_be_built_from_adapter_metadata_without_claiming_registries() {
        let metadata = AdapterMetadata::new(
            "prospect.fixture",
            version(1, 0),
            Some(
                AdapterUpstream::new(
                    "memorithm.fixture",
                    "0123456789abcdef0123456789abcdef01234567",
                )
                .unwrap(),
            ),
            vec![AdapterCapability::new("fixture.evaluate", version(1, 0)).unwrap()],
        )
        .unwrap();
        let catalog = DispatchCatalog::from_adapter_metadata(&[metadata]).unwrap();
        assert_eq!(catalog.adapters().len(), 1);
        assert!(catalog.metrics().is_empty());
        assert!(catalog.policies().is_empty());
    }

    #[test]
    fn catalog_preflight_resolves_compatible_requirements() {
        let catalog = catalog();
        let bundle = bundle(true, true, "0123456789abcdef0123456789abcdef01234567");
        let resolved = catalog.resolve_bundle(&bundle).unwrap();
        assert_eq!(resolved.adapter().adapter_id().as_str(), "prospect.fixture");
        assert_eq!(resolved.metric().unwrap().id().as_str(), "metric.distance");
        assert_eq!(resolved.policy().unwrap().id().as_str(), "policy.prefer");
    }

    #[test]
    fn catalog_preflight_fails_closed_on_missing_registry_and_upstream_drift() {
        let catalog =
            DispatchCatalog::new(catalog().adapters().to_vec(), Vec::new(), Vec::new()).unwrap();
        let bundle = bundle(true, false, "0123456789abcdef0123456789abcdef01234567");
        assert!(matches!(
            catalog.resolve_bundle(&bundle),
            Err(CatalogPreflightError::MissingMetric(id)) if id == "metric.distance"
        ));

        let bundle = bundle(false, false, "ffffffffffffffffffffffffffffffffffffffff");
        assert!(matches!(
            catalog.resolve_bundle(&bundle),
            Err(CatalogPreflightError::AdapterUpstreamMismatch { .. })
        ));
    }
}
