#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeSet;

use prospect_adapter::{AdapterMetadata, AdapterMetadataError, ContractVersion, NamespacedId};
use prospect_core::{Scenario, ScenarioId};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const SCENARIO_BUNDLE_SCHEMA_V1: &str = "prospect.scenario-bundle/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamBinding {
    component: NamespacedId,
    revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterBinding {
    adapter_id: NamespacedId,
    contract_version: ContractVersion,
    upstream: Option<UpstreamBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryRequirement {
    id: NamespacedId,
    version: ContractVersion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleScenario<Intervention> {
    id: ScenarioId,
    intervention: Intervention,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioBundle<State, Intervention> {
    bundle_id: NamespacedId,
    adapter: AdapterBinding,
    seed: Option<u64>,
    state: State,
    scenarios: Vec<BundleScenario<Intervention>>,
    metric: Option<RegistryRequirement>,
    policy: Option<RegistryRequirement>,
}

#[derive(Debug)]
pub enum ScenarioBundleError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    AdapterMetadata(AdapterMetadataError),
    EmptyUpstreamRevision,
    InvalidScenarioId,
    EmptyScenarios,
    DuplicateScenario(String),
}

#[derive(Serialize)]
struct BundleWireRef<'a, State, Intervention> {
    schema: &'static str,
    bundle_id: &'a str,
    adapter: AdapterBindingWireRef<'a>,
    seed: Option<u64>,
    state: &'a State,
    scenarios: Vec<ScenarioWireRef<'a, Intervention>>,
    metric: Option<RequirementWireRef<'a>>,
    policy: Option<RequirementWireRef<'a>>,
}

#[derive(Serialize)]
struct AdapterBindingWireRef<'a> {
    adapter_id: &'a str,
    contract_version: VersionWire,
    upstream: Option<UpstreamWireRef<'a>>,
}

#[derive(Serialize)]
struct UpstreamWireRef<'a> {
    component: &'a str,
    revision: &'a str,
}

#[derive(Serialize)]
struct RequirementWireRef<'a> {
    id: &'a str,
    version: VersionWire,
}

#[derive(Serialize)]
struct ScenarioWireRef<'a, Intervention> {
    id: &'a str,
    intervention: &'a Intervention,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionWire {
    major: u16,
    minor: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleWire<State, Intervention> {
    schema: String,
    bundle_id: String,
    adapter: AdapterBindingWire,
    seed: Option<u64>,
    state: State,
    scenarios: Vec<ScenarioWire<Intervention>>,
    metric: Option<RequirementWire>,
    policy: Option<RequirementWire>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterBindingWire {
    adapter_id: String,
    contract_version: VersionWire,
    upstream: Option<UpstreamWire>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpstreamWire {
    component: String,
    revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequirementWire {
    id: String,
    version: VersionWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioWire<Intervention> {
    id: String,
    intervention: Intervention,
}

impl UpstreamBinding {
    pub fn new(
        component: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, ScenarioBundleError> {
        let revision = revision.into();
        if revision.trim().is_empty() {
            return Err(ScenarioBundleError::EmptyUpstreamRevision);
        }
        Ok(Self {
            component: NamespacedId::new(component)
                .map_err(ScenarioBundleError::AdapterMetadata)?,
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

impl AdapterBinding {
    #[must_use]
    pub fn from_metadata(metadata: &AdapterMetadata) -> Self {
        Self {
            adapter_id: metadata.adapter_id().clone(),
            contract_version: metadata.contract_version(),
            upstream: metadata.upstream().map(|upstream| UpstreamBinding {
                component: upstream.component().clone(),
                revision: upstream.revision().to_owned(),
            }),
        }
    }

    pub fn new(
        adapter_id: impl Into<String>,
        contract_version: ContractVersion,
        upstream: Option<UpstreamBinding>,
    ) -> Result<Self, ScenarioBundleError> {
        Ok(Self {
            adapter_id: NamespacedId::new(adapter_id)
                .map_err(ScenarioBundleError::AdapterMetadata)?,
            contract_version,
            upstream,
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
    pub const fn upstream(&self) -> Option<&UpstreamBinding> {
        self.upstream.as_ref()
    }
}

impl RegistryRequirement {
    pub fn new(
        id: impl Into<String>,
        version: ContractVersion,
    ) -> Result<Self, ScenarioBundleError> {
        Ok(Self {
            id: NamespacedId::new(id).map_err(ScenarioBundleError::AdapterMetadata)?,
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

impl<Intervention> BundleScenario<Intervention> {
    #[must_use]
    pub const fn new(id: ScenarioId, intervention: Intervention) -> Self {
        Self { id, intervention }
    }

    #[must_use]
    pub const fn id(&self) -> &ScenarioId {
        &self.id
    }

    #[must_use]
    pub const fn intervention(&self) -> &Intervention {
        &self.intervention
    }

    #[must_use]
    pub fn into_core(self) -> Scenario<Intervention> {
        Scenario::new(self.id, self.intervention)
    }
}

impl<State, Intervention> ScenarioBundle<State, Intervention> {
    pub fn new(
        bundle_id: impl Into<String>,
        adapter: AdapterBinding,
        seed: Option<u64>,
        state: State,
        mut scenarios: Vec<BundleScenario<Intervention>>,
        metric: Option<RegistryRequirement>,
        policy: Option<RegistryRequirement>,
    ) -> Result<Self, ScenarioBundleError> {
        if scenarios.is_empty() {
            return Err(ScenarioBundleError::EmptyScenarios);
        }
        scenarios.sort_by(|left, right| left.id.cmp(&right.id));
        let mut seen = BTreeSet::new();
        for scenario in &scenarios {
            if !seen.insert(scenario.id.clone()) {
                return Err(ScenarioBundleError::DuplicateScenario(
                    scenario.id.as_str().to_owned(),
                ));
            }
        }
        Ok(Self {
            bundle_id: NamespacedId::new(bundle_id)
                .map_err(ScenarioBundleError::AdapterMetadata)?,
            adapter,
            seed,
            state,
            scenarios,
            metric,
            policy,
        })
    }

    #[must_use]
    pub const fn bundle_id(&self) -> &NamespacedId {
        &self.bundle_id
    }

    #[must_use]
    pub const fn adapter(&self) -> &AdapterBinding {
        &self.adapter
    }

    #[must_use]
    pub const fn seed(&self) -> Option<u64> {
        self.seed
    }

    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }

    #[must_use]
    pub fn scenarios(&self) -> &[BundleScenario<Intervention>] {
        &self.scenarios
    }

    #[must_use]
    pub const fn metric(&self) -> Option<&RegistryRequirement> {
        self.metric.as_ref()
    }

    #[must_use]
    pub const fn policy(&self) -> Option<&RegistryRequirement> {
        self.policy.as_ref()
    }

    #[must_use]
    pub fn into_core_scenarios(self) -> Vec<Scenario<Intervention>> {
        self.scenarios
            .into_iter()
            .map(BundleScenario::into_core)
            .collect()
    }
}

impl<State, Intervention> ScenarioBundle<State, Intervention>
where
    State: Serialize,
    Intervention: Serialize,
{
    pub fn canonical_json(&self) -> Result<String, ScenarioBundleError> {
        let value = serde_json::to_value(self.wire_ref()).map_err(ScenarioBundleError::Json)?;
        canonical_json(&value).map_err(ScenarioBundleError::Json)
    }

    pub fn sha256(&self) -> Result<String, ScenarioBundleError> {
        let payload = self.canonical_json()?;
        Ok(format!("{:x}", Sha256::digest(payload.as_bytes())))
    }

    fn wire_ref(&self) -> BundleWireRef<'_, State, Intervention> {
        BundleWireRef {
            schema: SCENARIO_BUNDLE_SCHEMA_V1,
            bundle_id: self.bundle_id.as_str(),
            adapter: AdapterBindingWireRef {
                adapter_id: self.adapter.adapter_id.as_str(),
                contract_version: self.adapter.contract_version.into(),
                upstream: self
                    .adapter
                    .upstream
                    .as_ref()
                    .map(|upstream| UpstreamWireRef {
                        component: upstream.component.as_str(),
                        revision: &upstream.revision,
                    }),
            },
            seed: self.seed,
            state: &self.state,
            scenarios: self
                .scenarios
                .iter()
                .map(|scenario| ScenarioWireRef {
                    id: scenario.id.as_str(),
                    intervention: &scenario.intervention,
                })
                .collect(),
            metric: self.metric.as_ref().map(|requirement| RequirementWireRef {
                id: requirement.id.as_str(),
                version: requirement.version.into(),
            }),
            policy: self.policy.as_ref().map(|requirement| RequirementWireRef {
                id: requirement.id.as_str(),
                version: requirement.version.into(),
            }),
        }
    }
}

impl<State, Intervention> ScenarioBundle<State, Intervention>
where
    State: Serialize + DeserializeOwned,
    Intervention: Serialize + DeserializeOwned,
{
    pub fn from_canonical_json(payload: &str) -> Result<Self, ScenarioBundleError> {
        let value: Value = serde_json::from_str(payload).map_err(ScenarioBundleError::Json)?;
        if canonical_json(&value).map_err(ScenarioBundleError::Json)? != payload {
            return Err(ScenarioBundleError::NonCanonicalJson);
        }
        let wire: BundleWire<State, Intervention> =
            serde_json::from_value(value).map_err(ScenarioBundleError::Json)?;
        if wire.schema != SCENARIO_BUNDLE_SCHEMA_V1 {
            return Err(ScenarioBundleError::UnsupportedSchema);
        }

        let adapter = AdapterBinding::new(
            wire.adapter.adapter_id,
            wire.adapter.contract_version.try_into()?,
            wire.adapter
                .upstream
                .map(|upstream| UpstreamBinding::new(upstream.component, upstream.revision))
                .transpose()?,
        )?;
        let metric = wire.metric.map(requirement_from_wire).transpose()?;
        let policy = wire.policy.map(requirement_from_wire).transpose()?;
        let scenarios = wire
            .scenarios
            .into_iter()
            .map(|scenario| {
                let id = ScenarioId::new(scenario.id)
                    .map_err(|_| ScenarioBundleError::InvalidScenarioId)?;
                Ok(BundleScenario::new(id, scenario.intervention))
            })
            .collect::<Result<Vec<_>, ScenarioBundleError>>()?;
        let bundle = Self::new(
            wire.bundle_id,
            adapter,
            wire.seed,
            wire.state,
            scenarios,
            metric,
            policy,
        )?;
        if bundle.canonical_json()? != payload {
            return Err(ScenarioBundleError::NonCanonicalJson);
        }
        Ok(bundle)
    }
}

fn requirement_from_wire(
    wire: RequirementWire,
) -> Result<RegistryRequirement, ScenarioBundleError> {
    RegistryRequirement::new(wire.id, wire.version.try_into()?)
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
    type Error = ScenarioBundleError;

    fn try_from(value: VersionWire) -> Result<Self, Self::Error> {
        ContractVersion::new(value.major, value.minor).map_err(ScenarioBundleError::AdapterMetadata)
    }
}

fn canonical_json(value: &Value) -> Result<String, serde_json::Error> {
    fn write_value(value: &Value, output: &mut String) -> Result<(), serde_json::Error> {
        match value {
            Value::Object(map) => {
                output.push('{');
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                for (index, key) in keys.into_iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    output.push_str(&serde_json::to_string(key)?);
                    output.push(':');
                    write_value(&map[key], output)?;
                }
                output.push('}');
            }
            Value::Array(values) => {
                output.push('[');
                for (index, item) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    write_value(item, output)?;
                }
                output.push(']');
            }
            other => output.push_str(&serde_json::to_string(other)?),
        }
        Ok(())
    }

    let mut output = String::new();
    write_value(value, &mut output)?;
    Ok(output)
}

impl fmt::Display for ScenarioBundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid scenario bundle JSON: {error}"),
            Self::NonCanonicalJson => formatter.write_str("scenario bundle JSON is not canonical"),
            Self::UnsupportedSchema => formatter.write_str("unsupported scenario bundle schema"),
            Self::AdapterMetadata(error) => write!(formatter, "invalid bundle metadata: {error}"),
            Self::EmptyUpstreamRevision => {
                formatter.write_str("bundle upstream revision must not be empty")
            }
            Self::InvalidScenarioId => formatter.write_str("bundle scenario id must not be empty"),
            Self::EmptyScenarios => {
                formatter.write_str("scenario bundle must contain at least one scenario")
            }
            Self::DuplicateScenario(id) => write!(formatter, "duplicate bundle scenario {id}"),
        }
    }
}

impl std::error::Error for ScenarioBundleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::AdapterMetadata(error) => Some(error),
            Self::NonCanonicalJson
            | Self::UnsupportedSchema
            | Self::EmptyUpstreamRevision
            | Self::InvalidScenarioId
            | Self::EmptyScenarios
            | Self::DuplicateScenario(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use prospect_adapter::{
        ADAPTER_CONTRACT_VERSION, AdapterCapability, AdapterMetadata, AdapterUpstream,
        ContractVersion,
    };
    use prospect_core::ScenarioId;
    use serde::{Deserialize, Serialize};

    use super::{
        AdapterBinding, BundleScenario, RegistryRequirement, SCENARIO_BUNDLE_SCHEMA_V1,
        ScenarioBundle, ScenarioBundleError,
    };

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct FixtureState {
        load: u32,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct FixtureIntervention {
        delta: i32,
    }

    fn adapter() -> AdapterBinding {
        let version = ContractVersion::new(1, 0).unwrap();
        let metadata = AdapterMetadata::new(
            "prospect.fixture",
            ADAPTER_CONTRACT_VERSION,
            Some(
                AdapterUpstream::new(
                    "memorithm.fixture",
                    "0123456789abcdef0123456789abcdef01234567",
                )
                .unwrap(),
            ),
            vec![AdapterCapability::new("fixture.evaluate", version).unwrap()],
        )
        .unwrap();
        AdapterBinding::from_metadata(&metadata)
    }

    fn scenario(id: &str, delta: i32) -> BundleScenario<FixtureIntervention> {
        BundleScenario::new(ScenarioId::new(id).unwrap(), FixtureIntervention { delta })
    }

    fn bundle(
        scenarios: Vec<BundleScenario<FixtureIntervention>>,
    ) -> ScenarioBundle<FixtureState, FixtureIntervention> {
        ScenarioBundle::new(
            "bundle.fixture",
            adapter(),
            Some(7),
            FixtureState { load: 10 },
            scenarios,
            Some(
                RegistryRequirement::new(
                    "metric.absolute_distance",
                    ContractVersion::new(1, 0).unwrap(),
                )
                .unwrap(),
            ),
            Some(
                RegistryRequirement::new(
                    "policy.prefer_higher",
                    ContractVersion::new(1, 0).unwrap(),
                )
                .unwrap(),
            ),
        )
        .unwrap()
    }

    #[test]
    fn canonical_bundle_round_trips_and_preserves_bindings() {
        let bundle = bundle(vec![scenario("zeta", 2), scenario("alpha", -1)]);
        let payload = bundle.canonical_json().unwrap();
        assert!(payload.contains(SCENARIO_BUNDLE_SCHEMA_V1));
        let replayed =
            ScenarioBundle::<FixtureState, FixtureIntervention>::from_canonical_json(&payload)
                .unwrap();
        assert_eq!(replayed, bundle);
        assert_eq!(replayed.scenarios()[0].id().as_str(), "alpha");
        assert_eq!(replayed.adapter().adapter_id().as_str(), "prospect.fixture");
        assert_eq!(
            replayed.adapter().upstream().unwrap().component().as_str(),
            "memorithm.fixture"
        );
        assert_eq!(
            replayed.metric().unwrap().id().as_str(),
            "metric.absolute_distance"
        );
    }

    #[test]
    fn equivalent_insertion_orders_have_identical_digest() {
        let left = bundle(vec![scenario("zeta", 2), scenario("alpha", -1)]);
        let right = bundle(vec![scenario("alpha", -1), scenario("zeta", 2)]);
        assert_eq!(
            left.canonical_json().unwrap(),
            right.canonical_json().unwrap()
        );
        assert_eq!(left.sha256().unwrap(), right.sha256().unwrap());
    }

    #[test]
    fn duplicate_scenarios_fail_closed() {
        assert!(matches!(
            ScenarioBundle::new(
                "bundle.fixture",
                adapter(),
                None,
                FixtureState { load: 10 },
                vec![scenario("same", 1), scenario("same", 2)],
                None,
                None,
            ),
            Err(ScenarioBundleError::DuplicateScenario(id)) if id == "same"
        ));
    }

    #[test]
    fn noncanonical_json_is_rejected() {
        let payload = bundle(vec![scenario("alpha", 1)]).canonical_json().unwrap();
        let noncanonical = format!(" {payload}");
        assert!(matches!(
            ScenarioBundle::<FixtureState, FixtureIntervention>::from_canonical_json(&noncanonical),
            Err(ScenarioBundleError::NonCanonicalJson)
        ));
    }

    #[test]
    fn semantic_scenario_order_must_also_be_canonical() {
        let payload = bundle(vec![scenario("alpha", 1), scenario("zeta", 2)])
            .canonical_json()
            .unwrap();
        let tampered = payload.replace(
            "{\"id\":\"alpha\",\"intervention\":{\"delta\":1}},{\"id\":\"zeta\",\"intervention\":{\"delta\":2}}",
            "{\"id\":\"zeta\",\"intervention\":{\"delta\":2}},{\"id\":\"alpha\",\"intervention\":{\"delta\":1}}",
        );
        assert!(matches!(
            ScenarioBundle::<FixtureState, FixtureIntervention>::from_canonical_json(&tampered),
            Err(ScenarioBundleError::NonCanonicalJson)
        ));
    }

    #[test]
    fn bundle_scenarios_convert_to_core_scenarios() {
        let bundle = bundle(vec![scenario("alpha", 3)]);
        let core = bundle.into_core_scenarios();
        assert_eq!(core[0].id().as_str(), "alpha");
        assert_eq!(core[0].intervention().delta, 3);
    }
}
