//! Software-only reconstruction fixtures. No model/GPU/domain-quality claims.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use prospect_adapter::{
    AdapterCapability, AdapterMetadata, AdapterMetadataError, ContractVersion, VersionedAdapter,
};
use prospect_bundle::{AdapterBinding, BundleScenario, ScenarioBundle};
use prospect_core::{ProspectiveEngine, ScenarioId};
use prospect_evidence::RunId;
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};
use prospect_scenario::controlled::EvaluationControl;

use super::super::super::super::super::{PayloadCodecs, canonical, digest};
use super::super::super::super::evaluation::JournalCapture;
use super::super::super::super::{
    EngineIdentity, JournalSink, evaluate_registered_bundle_journaled,
};
use super::super::super::{RestartAnchors, RestartSemantics};
use super::super::{ContinuationCapture, execute_typed_continuation};
use super::*;
use crate::execution::ExecutableAdapterRegistry;

#[derive(Default)]
struct Memory(Vec<u8>);
impl Memory {
    fn text(&self) -> String {
        String::from_utf8(self.0.clone()).unwrap()
    }
}
impl JournalSink for Memory {
    fn append_record(&mut self, record: &[u8]) -> std::io::Result<()> {
        self.0.extend_from_slice(record);
        Ok(())
    }
}

struct Engine {
    baseline_calls: Arc<AtomicUsize>,
    candidate_calls: Arc<AtomicUsize>,
}
impl ProspectiveEngine<i32, i32> for Engine {
    type Signature = i32;
    type Error = &'static str;

    fn baseline(&self, state: &i32) -> Result<i32, Self::Error> {
        self.baseline_calls.fetch_add(1, Ordering::SeqCst);
        Ok(*state)
    }

    fn evaluate(&self, state: &i32, intervention: &i32) -> Result<i32, Self::Error> {
        self.candidate_calls.fetch_add(1, Ordering::SeqCst);
        Ok(state + intervention)
    }
}
impl VersionedAdapter for Engine {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        AdapterMetadata::new(
            "fixture.assembly",
            version(),
            None,
            vec![AdapterCapability::new("fixture.evaluate", version())?],
        )
    }
}

fn version() -> ContractVersion {
    ContractVersion::new(1, 0).unwrap()
}

fn implementation() -> EngineIdentity {
    EngineIdentity::new("fixture.assembly_impl", &"a".repeat(40), &"b".repeat(64)).unwrap()
}

fn codecs() -> PayloadCodecs {
    PayloadCodecs::new("fixture.i32.v1", "fixture.error.v1").unwrap()
}

fn bundle_with_state(state: i32) -> ScenarioBundle<i32, i32> {
    let metadata = Engine {
        baseline_calls: Arc::new(AtomicUsize::new(0)),
        candidate_calls: Arc::new(AtomicUsize::new(0)),
    }
    .adapter_metadata()
    .unwrap();
    ScenarioBundle::new(
        "fixture.assembly_run",
        AdapterBinding::from_metadata(&metadata),
        Some(7),
        state,
        (1..=3)
            .map(|value| BundleScenario::new(ScenarioId::new(format!("s{value}")).unwrap(), value))
            .collect(),
        None,
        None,
    )
    .unwrap()
}

fn registry(
    baseline_calls: &Arc<AtomicUsize>,
    candidate_calls: &Arc<AtomicUsize>,
) -> ExecutableAdapterRegistry<i32, i32, i32, &'static str> {
    let mut registry = ExecutableAdapterRegistry::new();
    registry
        .register(Engine {
            baseline_calls: Arc::clone(baseline_calls),
            candidate_calls: Arc::clone(candidate_calls),
        })
        .unwrap();
    registry
}

struct Chain {
    parent: String,
    child: String,
    expected: RestartExpectations,
    baseline_calls: Arc<AtomicUsize>,
    candidate_calls: Arc<AtomicUsize>,
}

fn chain(child_quota: usize) -> Chain {
    let bundle = bundle_with_state(10);
    let bundle_json = bundle.canonical_json().unwrap();
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls);
    let metrics = MetricRegistry::<i32, i32>::new();
    let policies = DecisionPolicyRegistry::<i32, i32>::new();

    let mut parent_sink = Memory::default();
    let parent_run = evaluate_registered_bundle_journaled(
        &bundle,
        &registry,
        &metrics,
        &policies,
        &EvaluationControl::new(1),
        JournalCapture::new(
            &mut parent_sink,
            RunId::new("assembly-parent").unwrap(),
            implementation(),
            codecs(),
            |value: &i32| Ok(value.to_string()),
            |error: &&str| Ok((*error).to_owned()),
        ),
    )
    .unwrap();
    assert!(parent_run.journal_error().is_none());
    let parent = parent_sink.text();
    let metadata = registry.metadata()[0];
    let expected = RestartExpectations::new(
        RestartAnchors::new(
            &RunId::new("assembly-parent").unwrap(),
            &digest(parent.as_bytes()),
            &digest(bundle_json.as_bytes()),
            &digest(canonical(metadata).unwrap().as_bytes()),
        )
        .unwrap(),
        implementation(),
        codecs(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();

    let plan =
        prepare_typed_continuation(&parent, &bundle, &expected, &"b".repeat(64), |payload| {
            payload.parse::<i32>().map_err(|error| error.to_string())
        })
        .unwrap();
    let mut child_sink = Memory::default();
    let child_run = execute_typed_continuation(
        plan,
        &bundle,
        &registry,
        &metrics,
        &policies,
        &EvaluationControl::new(child_quota),
        ContinuationCapture::new(
            &mut child_sink,
            RunId::new("assembly-child").unwrap(),
            implementation(),
            codecs(),
            |value: &i32| Ok(value.to_string()),
            |error: &&str| Ok((*error).to_owned()),
        ),
    )
    .unwrap();
    assert!(child_run.journal_error().is_none());

    Chain {
        parent,
        child: child_sink.text(),
        expected,
        baseline_calls,
        candidate_calls,
    }
}

fn assemble(chain: &Chain) -> Result<AssembledContinuation<i32, i32>, ContinuationAssemblyError> {
    assemble_completed_continuation(
        &chain.parent,
        &chain.child,
        &bundle_with_state(10),
        &chain.expected,
        &"b".repeat(64),
        |payload| payload.parse::<i32>().map_err(|error| error.to_string()),
    )
}

#[test]
fn complete_chain_reconstructs_exact_original_order_without_more_engine_calls() {
    let chain = chain(2);
    assert_eq!(chain.baseline_calls.load(Ordering::SeqCst), 1);
    assert_eq!(chain.candidate_calls.load(Ordering::SeqCst), 3);
    let before_baseline = chain.baseline_calls.load(Ordering::SeqCst);
    let before_candidates = chain.candidate_calls.load(Ordering::SeqCst);

    let assembled = assemble(&chain).unwrap();
    assert_eq!(assembled.source_run_id(), "assembly-parent");
    assert_eq!(assembled.child_run_id(), "assembly-child");
    assert_eq!(
        assembled.source_journal_sha256(),
        digest(chain.parent.as_bytes())
    );
    assert_eq!(
        assembled.child_journal_sha256(),
        digest(chain.child.as_bytes())
    );
    assert_eq!(
        assembled.source_bundle_sha256(),
        digest(bundle_with_state(10).canonical_json().unwrap().as_bytes())
    );
    assert_eq!(assembled.expectation_sha256(), chain.expected.sha256());
    let batch = assembled.into_batch();
    assert_eq!(*batch.baseline(), 10);
    assert_eq!(
        batch
            .outcomes()
            .iter()
            .map(|outcome| (outcome.scenario().id().as_str(), *outcome.signature()))
            .collect::<Vec<_>>(),
        [("s1", 11), ("s2", 12), ("s3", 13)]
    );
    assert_eq!(chain.baseline_calls.load(Ordering::SeqCst), before_baseline);
    assert_eq!(
        chain.candidate_calls.load(Ordering::SeqCst),
        before_candidates
    );
}

#[test]
fn interrupted_child_never_constructs_a_batch() {
    let chain = chain(1);
    assert!(matches!(
        assemble(&chain),
        Err(ContinuationAssemblyError::ChildNotCompleted("interrupted"))
    ));
}

#[test]
fn altered_child_bytes_fail_before_decoding_a_batch() {
    let mut chain = chain(2);
    chain.child = chain
        .child
        .replacen("\"payload\":\"12\"", "\"payload\":\"99\"", 1);
    assert!(matches!(
        assemble(&chain),
        Err(ContinuationAssemblyError::Contract(_))
    ));
}

#[test]
fn missing_child_terminal_is_not_promoted_to_complete() {
    let mut chain = chain(2);
    chain.child.pop();
    let last_line_start = chain.child.rfind('\n').unwrap() + 1;
    chain.child.truncate(last_line_start);
    assert!(matches!(
        assemble(&chain),
        Err(ContinuationAssemblyError::ChildNotCompleted(
            "open_after_return"
        ))
    ));
}

#[test]
fn changed_bundle_or_wrong_artifact_rejects_at_parent_admission() {
    let chain = chain(2);
    let changed = assemble_completed_continuation(
        &chain.parent,
        &chain.child,
        &bundle_with_state(11),
        &chain.expected,
        &"b".repeat(64),
        |payload| payload.parse::<i32>().map_err(|error| error.to_string()),
    );
    assert!(matches!(changed, Err(ContinuationAssemblyError::Parent(_))));

    let wrong_artifact = assemble_completed_continuation(
        &chain.parent,
        &chain.child,
        &bundle_with_state(10),
        &chain.expected,
        &"c".repeat(64),
        |payload| payload.parse::<i32>().map_err(|error| error.to_string()),
    );
    assert!(matches!(
        wrong_artifact,
        Err(ContinuationAssemblyError::Parent(_))
    ));
}

#[test]
fn child_decode_failure_returns_no_structural_batch() {
    let chain = chain(2);
    let result = assemble_completed_continuation(
        &chain.parent,
        &chain.child,
        &bundle_with_state(10),
        &chain.expected,
        &"b".repeat(64),
        |payload| {
            if payload == "12" {
                Err("child codec refused value".to_owned())
            } else {
                payload.parse::<i32>().map_err(|error| error.to_string())
            }
        },
    );
    assert!(matches!(
        result,
        Err(ContinuationAssemblyError::Decode(message))
            if message == "child codec refused value"
    ));
}
