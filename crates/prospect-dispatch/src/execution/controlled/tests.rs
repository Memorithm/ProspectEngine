//! Typed-dispatch contract fixtures, not measured domain results.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use prospect_adapter::{AdapterCapability, AdapterMetadata, AdapterMetadataError, AdapterUpstream, ContractVersion, VersionedAdapter};
use prospect_bundle::{AdapterBinding, BundleScenario, RegistryRequirement, UpstreamBinding};
use prospect_core::{DecisionPolicy, ProspectiveEngine, ScenarioId, SignatureMetric};
use prospect_scenario::controlled::{BatchStatus, InterruptionReason, ProgressEvent};

use super::*;

const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";

struct Engine {
    calls: Arc<AtomicUsize>,
    fail_candidate: bool,
}

impl ProspectiveEngine<i32, i32> for Engine {
    type Signature = i32;
    type Error = &'static str;
    fn baseline(&self, state: &i32) -> Result<i32, Self::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(*state)
    }
    fn evaluate(&self, state: &i32, intervention: &i32) -> Result<i32, Self::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_candidate && *intervention == 2 {
            Err("fixture candidate failed")
        } else {
            Ok(*state + *intervention)
        }
    }
}

impl VersionedAdapter for Engine {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        AdapterMetadata::new("prospect.controlled_fixture", version(),
            Some(AdapterUpstream::new("memorithm.fixture", REVISION)?),
            vec![AdapterCapability::new("fixture.evaluate", version())?])
    }
}

struct NeverScore;
impl SignatureMetric<i32> for NeverScore {
    type Score = i32;
    fn compare(&self, _: &i32, _: &i32) -> i32 {
        panic!("controlled evaluation must not score signatures implicitly")
    }
}
impl DecisionPolicy<i32> for NeverScore {
    type Score = i32;
    fn utility(&self, _: &i32) -> i32 {
        panic!("controlled evaluation must not rank signatures implicitly")
    }
}

fn version() -> ContractVersion { ContractVersion::new(1, 0).unwrap() }

fn input(revision: &str, metric_id: &str, policy_id: &str) -> ScenarioBundle<i32, i32> {
    ScenarioBundle::new("experiment.controlled_fixture",
        AdapterBinding::new("prospect.controlled_fixture", version(),
            Some(UpstreamBinding::new("memorithm.fixture", revision).unwrap())).unwrap(),
        Some(7), 10,
        vec![BundleScenario::new(ScenarioId::new("a").unwrap(), 1),
             BundleScenario::new(ScenarioId::new("b").unwrap(), 2)],
        Some(RegistryRequirement::new(metric_id, version()).unwrap()),
        Some(RegistryRequirement::new(policy_id, version()).unwrap()),
    ).unwrap()
}

fn registries() -> (MetricRegistry<i32, i32>, DecisionPolicyRegistry<i32, i32>) {
    let mut metrics = MetricRegistry::new();
    metrics.register("metric.fixture", version(), NeverScore).unwrap();
    let mut policies = DecisionPolicyRegistry::new();
    policies.register("policy.fixture", version(), NeverScore).unwrap();
    (metrics, policies)
}

fn adapters(calls: &Arc<AtomicUsize>, fail_candidate: bool) -> ExecutableAdapterRegistry<i32, i32, i32, &'static str> {
    let mut adapters = ExecutableAdapterRegistry::new();
    adapters.register(Engine { calls: Arc::clone(calls), fail_candidate }).unwrap();
    adapters
}

#[test]
fn complete_evaluation_retains_identity_without_invoking_scoring_or_policy() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapters = adapters(&calls, false);
    let (metrics, policies) = registries();
    let bundle = input(REVISION, "metric.fixture", "policy.fixture");
    let report = evaluate_registered_bundle_controlled(&bundle, &adapters, &metrics, &policies,
        &EvaluationControl::new(2), |_| {}).unwrap();
    assert_eq!(report.bundle_id(), "experiment.controlled_fixture");
    assert_eq!(report.seed(), Some(7));
    assert_eq!(report.adapter().adapter_id().as_str(), "prospect.controlled_fixture");
    assert_eq!(report.adapter().upstream().unwrap().revision(), REVISION);
    assert_eq!(report.state(), ExecutionState::Completed);
    let batch = report.into_evaluation().into_completed_batch().unwrap();
    assert_eq!(*batch.baseline(), 10);
    assert_eq!(batch.outcomes().len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn quota_returns_explicit_partial_bundle_without_a_winner() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapters = adapters(&calls, false);
    let (metrics, policies) = registries();
    let report = evaluate_registered_bundle_controlled(&input(REVISION, "metric.fixture", "policy.fixture"),
        &adapters, &metrics, &policies, &EvaluationControl::new(1), |_| {}).unwrap();
    assert_eq!(report.state(), ExecutionState::Interrupted);
    assert!(matches!(report.evaluation().status(), BatchStatus::Interrupted(InterruptionReason::EvaluationLimitReached)));
    assert_eq!(report.evaluation().outcomes().len(), 1);
    assert_eq!(report.evaluation().pending()[0].id().as_str(), "b");
    assert!(report.into_evaluation().into_completed_batch().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn all_requirements_are_resolved_even_for_controlled_evaluation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapters = adapters(&calls, false);
    let (metrics, policies) = registries();
    for bundle in [input("other-revision", "metric.fixture", "policy.fixture"),
                   input(REVISION, "metric.missing", "policy.fixture"),
                   input(REVISION, "metric.fixture", "policy.missing")] {
        let result = evaluate_registered_bundle_controlled(&bundle, &adapters, &metrics, &policies,
            &EvaluationControl::new(2), |_| panic!("no progress before successful preflight"));
        assert!(matches!(result, Err(BundleExecutionError::Dispatch(_))));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn missing_adapter_fails_before_progress_or_engine_calls() {
    let adapters = ExecutableAdapterRegistry::<i32, i32, i32, &'static str>::new();
    let (metrics, policies) = registries();
    let result = evaluate_registered_bundle_controlled(&input(REVISION, "metric.fixture", "policy.fixture"),
        &adapters, &metrics, &policies, &EvaluationControl::new(2), |_| panic!("preflight must fail"));
    assert!(matches!(result, Err(BundleExecutionError::Dispatch(_))));
}

#[test]
fn cancellation_after_baseline_stops_typed_candidate_calls() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapters = adapters(&calls, false);
    let (metrics, policies) = registries();
    let control = EvaluationControl::new(2);
    let token = control.cancellation_token();
    let report = evaluate_registered_bundle_controlled(&input(REVISION, "metric.fixture", "policy.fixture"),
        &adapters, &metrics, &policies, &control, |update| {
            if matches!(update.event, ProgressEvent::BaselineCompleted) { token.cancel(); }
        }).unwrap();
    assert_eq!(report.state(), ExecutionState::Interrupted);
    assert_eq!(report.evaluation().baseline(), Some(&10));
    assert_eq!(report.evaluation().pending().len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn typed_candidate_error_is_preserved_with_completed_prefix() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapters = adapters(&calls, true);
    let (metrics, policies) = registries();
    let report = evaluate_registered_bundle_controlled(&input(REVISION, "metric.fixture", "policy.fixture"),
        &adapters, &metrics, &policies, &EvaluationControl::new(2), |_| {}).unwrap();
    assert_eq!(report.state(), ExecutionState::Failed);
    assert_eq!(report.evaluation().outcomes().len(), 1);
    assert!(report.evaluation().pending().is_empty());
    match report.evaluation().status() {
        BatchStatus::EngineFailed { scenario: Some(scenario), error } => {
            assert_eq!(scenario.id().as_str(), "b");
            assert_eq!(*error, "fixture candidate failed");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn expired_deadline_blocks_all_registered_engine_calls() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapters = adapters(&calls, false);
    let (metrics, policies) = registries();
    let control = EvaluationControl::new(2).with_deadline(Instant::now());
    let report = evaluate_registered_bundle_controlled(&input(REVISION, "metric.fixture", "policy.fixture"),
        &adapters, &metrics, &policies, &control, |_| {}).unwrap();
    assert!(matches!(report.evaluation().status(), BatchStatus::Interrupted(InterruptionReason::DeadlineReached)));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn preflight_failure_is_not_hidden_by_a_cancelled_token() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapters = adapters(&calls, false);
    let (metrics, policies) = registries();
    let control = EvaluationControl::new(2);
    control.cancellation_token().cancel();
    let result = evaluate_registered_bundle_controlled(&input(REVISION, "metric.missing", "policy.fixture"),
        &adapters, &metrics, &policies, &control, |_| panic!("preflight must fail"));
    assert!(matches!(result, Err(BundleExecutionError::Dispatch(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
