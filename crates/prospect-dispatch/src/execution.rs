pub mod controlled;
pub use controlled::{RegisteredBatchExecution, evaluate_registered_bundle_controlled};

use core::fmt;
use std::collections::BTreeMap;

use prospect_adapter::{AdapterMetadata, AdapterMetadataError, NamespacedId, VersionedAdapter};
use prospect_bundle::ScenarioBundle;
use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};
use prospect_scenario::{
    BatchResult, ScenarioScore, best_by_policy, evaluate_batch, score_against_baseline,
};

use crate::{BundleDispatchError, resolve_bundle_requirements};

type EngineObject<State, Intervention, Signature, EngineError> = dyn ProspectiveEngine<State, Intervention, Signature = Signature, Error = EngineError>
    + Send
    + Sync;

struct ExecutableAdapterEntry<State, Intervention, Signature, EngineError> {
    metadata: AdapterMetadata,
    engine: Box<EngineObject<State, Intervention, Signature, EngineError>>,
}

/// Registry of executable adapters sharing one exact typed engine contract.
///
/// This registry intentionally does not erase `State`, `Intervention`,
/// `Signature`, or `EngineError` into JSON. A caller must choose a concrete
/// typed execution universe and register engines that implement that exact
/// contract. Registration derives metadata from `VersionedAdapter`, so the
/// executable implementation cannot be paired with caller-invented metadata.
pub struct ExecutableAdapterRegistry<State, Intervention, Signature, EngineError> {
    entries:
        BTreeMap<NamespacedId, ExecutableAdapterEntry<State, Intervention, Signature, EngineError>>,
}

#[derive(Debug)]
pub enum ExecutableAdapterRegistrationError {
    Metadata(AdapterMetadataError),
    DuplicateId(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicySelection<Score> {
    scenario_id: ScenarioId,
    score: Score,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleExecutionResult<Intervention, Signature, MetricScore, PolicyScore> {
    bundle_id: String,
    seed: Option<u64>,
    adapter: AdapterMetadata,
    batch: BatchResult<Intervention, Signature>,
    metric_scores: Option<Vec<ScenarioScore<MetricScore>>>,
    policy_selection: Option<PolicySelection<PolicyScore>>,
}

#[derive(Debug)]
pub enum BundleExecutionError<EngineError> {
    Dispatch(BundleDispatchError),
    Engine(EngineError),
    RegistryInvariant(String),
}

impl<State, Intervention, Signature, EngineError> Default
    for ExecutableAdapterRegistry<State, Intervention, Signature, EngineError>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<State, Intervention, Signature, EngineError>
    ExecutableAdapterRegistry<State, Intervention, Signature, EngineError>
{
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn register<E>(&mut self, engine: E) -> Result<(), ExecutableAdapterRegistrationError>
    where
        E: ProspectiveEngine<State, Intervention, Signature = Signature, Error = EngineError>
            + VersionedAdapter
            + Send
            + Sync
            + 'static,
    {
        let metadata = engine
            .adapter_metadata()
            .map_err(ExecutableAdapterRegistrationError::Metadata)?;
        let id = metadata.adapter_id().clone();
        if self.entries.contains_key(&id) {
            return Err(ExecutableAdapterRegistrationError::DuplicateId(
                id.as_str().to_owned(),
            ));
        }
        self.entries.insert(
            id,
            ExecutableAdapterEntry {
                metadata,
                engine: Box::new(engine),
            },
        );
        Ok(())
    }

    #[must_use]
    pub fn metadata(&self) -> Vec<&AdapterMetadata> {
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

    fn owned_metadata_catalog(&self) -> Vec<AdapterMetadata> {
        self.entries
            .values()
            .map(|entry| entry.metadata.clone())
            .collect()
    }

    fn engine(
        &self,
        id: &str,
    ) -> Option<&EngineObject<State, Intervention, Signature, EngineError>> {
        self.entries
            .iter()
            .find(|(candidate_id, _)| candidate_id.as_str() == id)
            .map(|(_, entry)| entry.engine.as_ref())
    }
}

/// Execute one typed scenario bundle after resolving every declared software
/// requirement against explicit executable and metric/policy registries.
///
/// The function fails closed before invoking the engine when adapter metadata,
/// upstream revision, metric, or policy requirements cannot be satisfied. The
/// execution result is an in-memory software result only: it does not by itself
/// establish scientific validity, representative performance, safety, quality,
/// memory traffic, or physical-effect evidence.
pub fn execute_registered_bundle<
    State,
    Intervention,
    Signature,
    EngineError,
    MetricScore,
    PolicyScore,
>(
    bundle: &ScenarioBundle<State, Intervention>,
    adapters: &ExecutableAdapterRegistry<State, Intervention, Signature, EngineError>,
    metrics: &MetricRegistry<Signature, MetricScore>,
    policies: &DecisionPolicyRegistry<Signature, PolicyScore>,
) -> Result<
    BundleExecutionResult<Intervention, Signature, MetricScore, PolicyScore>,
    BundleExecutionError<EngineError>,
>
where
    Intervention: Clone,
    PolicyScore: Ord,
{
    let adapter_catalog = adapters.owned_metadata_catalog();
    let resolved = resolve_bundle_requirements(bundle, &adapter_catalog, metrics, policies)
        .map_err(BundleExecutionError::Dispatch)?;
    let adapter_metadata = resolved.adapter().clone();
    let adapter_id = adapter_metadata.adapter_id().as_str().to_owned();
    let engine = adapters
        .engine(&adapter_id)
        .ok_or_else(|| BundleExecutionError::RegistryInvariant(adapter_id.clone()))?;

    let scenarios = bundle
        .scenarios()
        .iter()
        .map(|scenario| Scenario::new(scenario.id().clone(), scenario.intervention().clone()))
        .collect();
    let batch =
        evaluate_batch(engine, bundle.state(), scenarios).map_err(BundleExecutionError::Engine)?;
    let metric_scores = resolved
        .metric()
        .map(|metric| score_against_baseline(&batch, metric));
    let policy_selection = resolved.policy().and_then(|policy| {
        best_by_policy(&batch, policy).map(|(outcome, score)| PolicySelection {
            scenario_id: outcome.scenario().id().clone(),
            score,
        })
    });

    Ok(BundleExecutionResult {
        bundle_id: bundle.bundle_id().as_str().to_owned(),
        seed: bundle.seed(),
        adapter: adapter_metadata,
        batch,
        metric_scores,
        policy_selection,
    })
}

impl<Score> PolicySelection<Score> {
    #[must_use]
    pub const fn scenario_id(&self) -> &ScenarioId {
        &self.scenario_id
    }

    #[must_use]
    pub const fn score(&self) -> &Score {
        &self.score
    }
}

impl<Intervention, Signature, MetricScore, PolicyScore>
    BundleExecutionResult<Intervention, Signature, MetricScore, PolicyScore>
{
    #[must_use]
    pub fn bundle_id(&self) -> &str {
        &self.bundle_id
    }

    #[must_use]
    pub const fn seed(&self) -> Option<u64> {
        self.seed
    }

    #[must_use]
    pub const fn adapter(&self) -> &AdapterMetadata {
        &self.adapter
    }

    #[must_use]
    pub const fn batch(&self) -> &BatchResult<Intervention, Signature> {
        &self.batch
    }

    #[must_use]
    pub fn metric_scores(&self) -> Option<&[ScenarioScore<MetricScore>]> {
        self.metric_scores.as_deref()
    }

    #[must_use]
    pub const fn policy_selection(&self) -> Option<&PolicySelection<PolicyScore>> {
        self.policy_selection.as_ref()
    }
}

impl fmt::Display for ExecutableAdapterRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => {
                write!(formatter, "invalid executable adapter metadata: {error}")
            }
            Self::DuplicateId(id) => write!(formatter, "duplicate executable adapter {id}"),
        }
    }
}

impl std::error::Error for ExecutableAdapterRegistrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Metadata(error) => Some(error),
            Self::DuplicateId(_) => None,
        }
    }
}

impl<EngineError> fmt::Display for BundleExecutionError<EngineError>
where
    EngineError: fmt::Display,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dispatch(error) => write!(formatter, "bundle dispatch failed: {error}"),
            Self::Engine(error) => write!(formatter, "bundle engine execution failed: {error}"),
            Self::RegistryInvariant(id) => write!(
                formatter,
                "executable adapter registry lost implementation for validated adapter {id}"
            ),
        }
    }
}

impl<EngineError> std::error::Error for BundleExecutionError<EngineError>
where
    EngineError: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Dispatch(error) => Some(error),
            Self::Engine(error) => Some(error),
            Self::RegistryInvariant(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use prospect_adapter::{
        AdapterCapability, AdapterMetadata, AdapterMetadataError, AdapterUpstream, ContractVersion,
        VersionedAdapter,
    };
    use prospect_bundle::{
        AdapterBinding, BundleScenario, RegistryRequirement, ScenarioBundle, UpstreamBinding,
    };
    use prospect_core::{DecisionPolicy, ProspectiveEngine, ScenarioId, SignatureMetric};
    use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};

    use super::{
        BundleExecutionError, ExecutableAdapterRegistrationError, ExecutableAdapterRegistry,
        execute_registered_bundle,
    };
    use crate::BundleDispatchError;

    const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";

    struct AdditiveEngine {
        calls: Arc<AtomicUsize>,
        fail: bool,
    }

    impl ProspectiveEngine<i32, i32> for AdditiveEngine {
        type Signature = i32;
        type Error = &'static str;

        fn baseline(&self, state: &i32) -> Result<Self::Signature, Self::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err("fixture engine failure")
            } else {
                Ok(*state)
            }
        }

        fn evaluate(
            &self,
            state: &i32,
            intervention: &i32,
        ) -> Result<Self::Signature, Self::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(*state + *intervention)
        }
    }

    impl VersionedAdapter for AdditiveEngine {
        fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
            AdapterMetadata::new(
                "prospect.fixture",
                version(1, 2),
                Some(AdapterUpstream::new("memorithm.fixture", REVISION)?),
                vec![AdapterCapability::new("fixture.evaluate", version(1, 0))?],
            )
        }
    }

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

    fn bundle(revision: &str, metric_id: &str) -> ScenarioBundle<i32, i32> {
        ScenarioBundle::new(
            "experiment.fixture",
            AdapterBinding::new(
                "prospect.fixture",
                version(1, 0),
                Some(UpstreamBinding::new("memorithm.fixture", revision).unwrap()),
            )
            .unwrap(),
            Some(7),
            10,
            vec![
                BundleScenario::new(ScenarioId::new("small").unwrap(), 3),
                BundleScenario::new(ScenarioId::new("large").unwrap(), 7),
            ],
            Some(RegistryRequirement::new(metric_id, version(1, 0)).unwrap()),
            Some(RegistryRequirement::new("policy.prefer_higher", version(1, 0)).unwrap()),
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
    fn executes_registered_bundle_after_fail_closed_requirement_resolution() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut adapters = ExecutableAdapterRegistry::new();
        adapters
            .register(AdditiveEngine {
                calls: Arc::clone(&calls),
                fail: false,
            })
            .unwrap();
        let (metrics, policies) = registries();

        let result = execute_registered_bundle(
            &bundle(REVISION, "metric.absolute_distance"),
            &adapters,
            &metrics,
            &policies,
        )
        .unwrap();

        assert_eq!(result.bundle_id(), "experiment.fixture");
        assert_eq!(result.seed(), Some(7));
        assert_eq!(result.adapter().adapter_id().as_str(), "prospect.fixture");
        assert_eq!(*result.batch().baseline(), 10);
        assert_eq!(result.batch().outcomes().len(), 2);
        let scores = result.metric_scores().unwrap();
        assert_eq!(scores.len(), 2);
        assert!(scores.iter().any(|score| score.score == 7));
        let selected = result.policy_selection().unwrap();
        assert_eq!(selected.scenario_id().as_str(), "large");
        assert_eq!(*selected.score(), 17);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn duplicate_executable_adapter_ids_are_rejected() {
        let mut adapters = ExecutableAdapterRegistry::new();
        for expected_ok in [true, false] {
            let result = adapters.register(AdditiveEngine {
                calls: Arc::new(AtomicUsize::new(0)),
                fail: false,
            });
            if expected_ok {
                result.unwrap();
            } else {
                assert!(matches!(
                    result,
                    Err(ExecutableAdapterRegistrationError::DuplicateId(id))
                        if id == "prospect.fixture"
                ));
            }
        }
    }

    #[test]
    fn failed_preflight_never_invokes_registered_engine() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut adapters = ExecutableAdapterRegistry::new();
        adapters
            .register(AdditiveEngine {
                calls: Arc::clone(&calls),
                fail: false,
            })
            .unwrap();
        let (metrics, policies) = registries();

        let error = execute_registered_bundle(
            &bundle(
                "ffffffffffffffffffffffffffffffffffffffff",
                "metric.absolute_distance",
            ),
            &adapters,
            &metrics,
            &policies,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            BundleExecutionError::Dispatch(BundleDispatchError::AdapterUpstreamMismatch { .. })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unresolved_metric_never_invokes_registered_engine() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut adapters = ExecutableAdapterRegistry::new();
        adapters
            .register(AdditiveEngine {
                calls: Arc::clone(&calls),
                fail: false,
            })
            .unwrap();
        let (metrics, policies) = registries();

        assert!(matches!(
            execute_registered_bundle(
                &bundle(REVISION, "metric.missing"),
                &adapters,
                &metrics,
                &policies,
            ),
            Err(BundleExecutionError::Dispatch(BundleDispatchError::Metric(
                _
            )))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn engine_failures_are_propagated_without_policy_or_metric_execution() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut adapters = ExecutableAdapterRegistry::new();
        adapters
            .register(AdditiveEngine {
                calls: Arc::clone(&calls),
                fail: true,
            })
            .unwrap();
        let (metrics, policies) = registries();

        assert!(matches!(
            execute_registered_bundle(
                &bundle(REVISION, "metric.absolute_distance"),
                &adapters,
                &metrics,
                &policies,
            ),
            Err(BundleExecutionError::Engine("fixture engine failure"))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn registry_metadata_is_stable_and_complete() {
        let mut adapters = ExecutableAdapterRegistry::new();
        adapters
            .register(AdditiveEngine {
                calls: Arc::new(AtomicUsize::new(0)),
                fail: false,
            })
            .unwrap();
        let metadata = adapters.metadata();
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].adapter_id().as_str(), "prospect.fixture");
        assert!(!adapters.is_empty());
        assert_eq!(adapters.len(), 1);
    }
}
