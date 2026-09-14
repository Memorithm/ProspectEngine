#![forbid(unsafe_code)]

pub mod catalog;
pub mod execution;

use core::fmt;

use prospect_adapter::{AdapterMetadata, ContractVersion};
use prospect_bundle::ScenarioBundle;
use prospect_core::{DecisionPolicy, SignatureMetric};
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry, RegistryLookupError};

pub struct ResolvedBundleRequirements<'a, Signature, MetricScore, PolicyScore>
where
    PolicyScore: Ord,
{
    adapter: &'a AdapterMetadata,
    metric: Option<&'a (dyn SignatureMetric<Signature, Score = MetricScore> + Send + Sync)>,
    policy: Option<&'a (dyn DecisionPolicy<Signature, Score = PolicyScore> + Send + Sync)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BundleDispatchError {
    MissingAdapter(String),
    DuplicateAdapter(String),
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
    Metric(RegistryLookupError),
    Policy(RegistryLookupError),
}

/// Resolve one canonical bundle's declared adapter/metric/policy requirements.
///
/// Resolution is a preflight only. It does not evaluate scenarios, execute an
/// adapter, select a policy result, or turn registry/catalog membership into
/// scientific, safety, performance, or physical-effect evidence.
pub fn resolve_bundle_requirements<'a, State, Intervention, Signature, MetricScore, PolicyScore>(
    bundle: &ScenarioBundle<State, Intervention>,
    adapters: &'a [AdapterMetadata],
    metrics: &'a MetricRegistry<Signature, MetricScore>,
    policies: &'a DecisionPolicyRegistry<Signature, PolicyScore>,
) -> Result<ResolvedBundleRequirements<'a, Signature, MetricScore, PolicyScore>, BundleDispatchError>
where
    PolicyScore: Ord,
{
    let required_adapter = bundle.adapter();
    let adapter_id = required_adapter.adapter_id().as_str();
    let mut candidates = adapters
        .iter()
        .filter(|metadata| metadata.adapter_id().as_str() == adapter_id);
    let adapter = candidates
        .next()
        .ok_or_else(|| BundleDispatchError::MissingAdapter(adapter_id.to_owned()))?;
    if candidates.next().is_some() {
        return Err(BundleDispatchError::DuplicateAdapter(adapter_id.to_owned()));
    }

    let required_version = required_adapter.contract_version();
    let offered_version = adapter.contract_version();
    if !offered_version.supports(required_version) {
        return Err(BundleDispatchError::IncompatibleAdapterVersion {
            id: adapter_id.to_owned(),
            offered: offered_version,
            required: required_version,
        });
    }

    if let Some(required_upstream) = required_adapter.upstream() {
        let matches = adapter.upstream().is_some_and(|offered| {
            offered.component() == required_upstream.component()
                && offered.revision() == required_upstream.revision()
        });
        if !matches {
            return Err(BundleDispatchError::AdapterUpstreamMismatch {
                id: adapter_id.to_owned(),
                required_component: required_upstream.component().as_str().to_owned(),
                required_revision: required_upstream.revision().to_owned(),
            });
        }
    }

    let metric = bundle
        .metric()
        .map(|requirement| {
            metrics
                .resolve(requirement.id().as_str(), requirement.version())
                .map_err(BundleDispatchError::Metric)
        })
        .transpose()?;
    let policy = bundle
        .policy()
        .map(|requirement| {
            policies
                .resolve(requirement.id().as_str(), requirement.version())
                .map_err(BundleDispatchError::Policy)
        })
        .transpose()?;

    Ok(ResolvedBundleRequirements {
        adapter,
        metric,
        policy,
    })
}

impl<'a, Signature, MetricScore, PolicyScore>
    ResolvedBundleRequirements<'a, Signature, MetricScore, PolicyScore>
where
    PolicyScore: Ord,
{
    #[must_use]
    pub fn adapter(&self) -> &'a AdapterMetadata {
        self.adapter
    }

    #[must_use]
    pub fn metric(
        &self,
    ) -> Option<&'a (dyn SignatureMetric<Signature, Score = MetricScore> + Send + Sync)> {
        self.metric
    }

    #[must_use]
    pub fn policy(
        &self,
    ) -> Option<&'a (dyn DecisionPolicy<Signature, Score = PolicyScore> + Send + Sync)> {
        self.policy
    }
}

impl fmt::Display for BundleDispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAdapter(id) => write!(formatter, "bundle adapter {id} is not available"),
            Self::DuplicateAdapter(id) => {
                write!(
                    formatter,
                    "bundle adapter {id} is ambiguous in the adapter catalog"
                )
            }
            Self::IncompatibleAdapterVersion {
                id,
                offered,
                required,
            } => write!(
                formatter,
                "bundle adapter {id} offers {}.{} but {}.{} is required",
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
                "bundle adapter {id} does not match required upstream {required_component}@{required_revision}"
            ),
            Self::Metric(error) => write!(formatter, "bundle metric requirement failed: {error}"),
            Self::Policy(error) => write!(formatter, "bundle policy requirement failed: {error}"),
        }
    }
}

impl std::error::Error for BundleDispatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Metric(error) | Self::Policy(error) => Some(error),
            Self::MissingAdapter(_)
            | Self::DuplicateAdapter(_)
            | Self::IncompatibleAdapterVersion { .. }
            | Self::AdapterUpstreamMismatch { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use prospect_adapter::{AdapterCapability, AdapterMetadata, AdapterUpstream, ContractVersion};
    use prospect_bundle::{
        AdapterBinding, BundleScenario, RegistryRequirement, ScenarioBundle, UpstreamBinding,
    };
    use prospect_core::{DecisionPolicy, ScenarioId, SignatureMetric};
    use prospect_registry::{DecisionPolicyRegistry, MetricRegistry, RegistryLookupError};

    use super::{BundleDispatchError, resolve_bundle_requirements};

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

    fn version(major: u16, minor: u16) -> ContractVersion {
        ContractVersion::new(major, minor).unwrap()
    }

    fn adapter(contract_version: ContractVersion, revision: &str) -> AdapterMetadata {
        AdapterMetadata::new(
            "prospect.fixture",
            contract_version,
            Some(AdapterUpstream::new("memorithm.fixture", revision).unwrap()),
            vec![AdapterCapability::new("fixture.evaluate", version(1, 0)).unwrap()],
        )
        .unwrap()
    }

    fn bundle(
        contract_version: ContractVersion,
        revision: &str,
        metric: Option<RegistryRequirement>,
        policy: Option<RegistryRequirement>,
    ) -> ScenarioBundle<i32, i32> {
        ScenarioBundle::new(
            "experiment.fixture",
            AdapterBinding::new(
                "prospect.fixture",
                contract_version,
                Some(UpstreamBinding::new("memorithm.fixture", revision).unwrap()),
            )
            .unwrap(),
            Some(7),
            10,
            vec![BundleScenario::new(
                ScenarioId::new("candidate").unwrap(),
                3,
            )],
            metric,
            policy,
        )
        .unwrap()
    }

    fn registries() -> (MetricRegistry<i32, i32>, DecisionPolicyRegistry<i32, i32>) {
        let mut metrics = MetricRegistry::new();
        metrics
            .register("metric.absolute_distance", version(1, 2), AbsoluteDistance)
            .unwrap();
        let mut policies = DecisionPolicyRegistry::new();
        policies
            .register("policy.prefer_higher", version(1, 1), PreferHigher)
            .unwrap();
        (metrics, policies)
    }

    #[test]
    fn resolves_explicit_adapter_metric_and_policy_requirements() {
        let bundle = bundle(
            version(1, 0),
            "0123456789abcdef0123456789abcdef01234567",
            Some(RegistryRequirement::new("metric.absolute_distance", version(1, 0)).unwrap()),
            Some(RegistryRequirement::new("policy.prefer_higher", version(1, 0)).unwrap()),
        );
        let adapters = vec![adapter(
            version(1, 2),
            "0123456789abcdef0123456789abcdef01234567",
        )];
        let (metrics, policies) = registries();

        let resolved =
            resolve_bundle_requirements(&bundle, &adapters, &metrics, &policies).unwrap();
        assert_eq!(resolved.adapter().adapter_id().as_str(), "prospect.fixture");
        assert_eq!(resolved.metric().unwrap().compare(&10, &16), 6);
        assert_eq!(resolved.policy().unwrap().utility(&23), 23);
    }

    #[test]
    fn optional_registry_requirements_remain_optional() {
        let bundle = bundle(
            version(1, 0),
            "0123456789abcdef0123456789abcdef01234567",
            None,
            None,
        );
        let adapters = vec![adapter(
            version(1, 0),
            "0123456789abcdef0123456789abcdef01234567",
        )];
        let metrics = MetricRegistry::<i32, i32>::new();
        let policies = DecisionPolicyRegistry::<i32, i32>::new();

        let resolved =
            resolve_bundle_requirements(&bundle, &adapters, &metrics, &policies).unwrap();
        assert!(resolved.metric().is_none());
        assert!(resolved.policy().is_none());
    }

    #[test]
    fn adapter_resolution_fails_closed_on_missing_duplicate_or_version_mismatch() {
        let bundle = bundle(
            version(1, 1),
            "0123456789abcdef0123456789abcdef01234567",
            None,
            None,
        );
        let metrics = MetricRegistry::<i32, i32>::new();
        let policies = DecisionPolicyRegistry::<i32, i32>::new();

        assert!(matches!(
            resolve_bundle_requirements(&bundle, &[], &metrics, &policies),
            Err(BundleDispatchError::MissingAdapter(id)) if id == "prospect.fixture"
        ));

        let duplicate = vec![
            adapter(version(1, 1), "0123456789abcdef0123456789abcdef01234567"),
            adapter(version(1, 2), "0123456789abcdef0123456789abcdef01234567"),
        ];
        assert!(matches!(
            resolve_bundle_requirements(&bundle, &duplicate, &metrics, &policies),
            Err(BundleDispatchError::DuplicateAdapter(id)) if id == "prospect.fixture"
        ));

        let incompatible = vec![adapter(
            version(1, 0),
            "0123456789abcdef0123456789abcdef01234567",
        )];
        assert!(matches!(
            resolve_bundle_requirements(&bundle, &incompatible, &metrics, &policies),
            Err(BundleDispatchError::IncompatibleAdapterVersion { .. })
        ));
    }

    #[test]
    fn adapter_resolution_requires_exact_declared_upstream() {
        let bundle = bundle(
            version(1, 0),
            "0123456789abcdef0123456789abcdef01234567",
            None,
            None,
        );
        let adapters = vec![adapter(
            version(1, 0),
            "ffffffffffffffffffffffffffffffffffffffff",
        )];
        let metrics = MetricRegistry::<i32, i32>::new();
        let policies = DecisionPolicyRegistry::<i32, i32>::new();

        assert!(matches!(
            resolve_bundle_requirements(&bundle, &adapters, &metrics, &policies),
            Err(BundleDispatchError::AdapterUpstreamMismatch { .. })
        ));
    }

    #[test]
    fn unresolved_registry_requirements_propagate_typed_lookup_failures() {
        let bundle = bundle(
            version(1, 0),
            "0123456789abcdef0123456789abcdef01234567",
            Some(RegistryRequirement::new("metric.missing", version(1, 0)).unwrap()),
            None,
        );
        let adapters = vec![adapter(
            version(1, 0),
            "0123456789abcdef0123456789abcdef01234567",
        )];
        let (metrics, policies) = registries();

        assert!(matches!(
            resolve_bundle_requirements(&bundle, &adapters, &metrics, &policies),
            Err(BundleDispatchError::Metric(RegistryLookupError::MissingId(id)))
                if id == "metric.missing"
        ));
    }
}
