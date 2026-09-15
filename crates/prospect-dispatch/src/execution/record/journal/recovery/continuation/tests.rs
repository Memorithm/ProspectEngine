//! Software-only continuation fixtures. No GPU, model, or physical actuation.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use prospect_adapter::{
    AdapterCapability, AdapterMetadata, AdapterMetadataError, ContractVersion, VersionedAdapter,
};
use prospect_bundle::{AdapterBinding, BundleScenario, ScenarioBundle};
use prospect_core::{ProspectiveEngine, ScenarioId};
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};
use prospect_scenario::controlled::EvaluationControl;

use super::super::super::evaluation::JournalCapture;
use super::super::super::{EngineIdentity, JournalSink};
use super::super::{RestartAnchors, RestartExpectations, RestartSemantics};
use super::*;

#[derive(Default)]
struct Memory(Vec<u8>);
impl JournalSink for Memory {
    fn append_record(&mut self, record: &[u8]) -> io::Result<()> {
        self.0.extend_from_slice(record);
        Ok(())
    }
}
impl Memory {
    fn text(&self) -> String {
        String::from_utf8(self.0.clone()).unwrap()
    }
}

struct FailFirstSink {
    attempts: usize,
}
impl JournalSink for FailFirstSink {
    fn append_record(&mut self, _: &[u8]) -> io::Result<()> {
        self.attempts += 1;
        Err(io::Error::other("fixture storage failure"))
    }
}

struct Engine {
    baseline_calls: Arc<AtomicUsize>,
    candidate_calls: Arc<AtomicUsize>,
    fail: Option<i32>,
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
        if self.fail == Some(*intervention) {
            Err("fixture failure")
        } else {
            Ok(*state + *intervention)
        }
    }
}
impl VersionedAdapter for Engine {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        AdapterMetadata::new(
            "fixture.adapter",
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
    EngineIdentity::new("fixture.engine", &"a".repeat(40), &"b".repeat(64)).unwrap()
}
fn codecs() -> PayloadCodecs {
    PayloadCodecs::new("fixture.i32.v1", "fixture.error.v1").unwrap()
}
fn bundle() -> ScenarioBundle<i32, i32> {
    ScenarioBundle::new(
        "fixture.continuation",
        AdapterBinding::new("fixture.adapter", version(), None).unwrap(),
        Some(7),
        10,
        (1..=3)
            .map(|i| BundleScenario::new(ScenarioId::new(format!("s{i}")).unwrap(), i))
            .collect(),
        None,
        None,
    )
    .unwrap()
}
fn registry(
    baseline_calls: &Arc<AtomicUsize>,
    candidate_calls: &Arc<AtomicUsize>,
    fail: Option<i32>,
) -> ExecutableAdapterRegistry<i32, i32, i32, &'static str> {
    let mut registry = ExecutableAdapterRegistry::new();
    registry
        .register(Engine {
            baseline_calls: baseline_calls.clone(),
            candidate_calls: candidate_calls.clone(),
            fail,
        })
        .unwrap();
    registry
}

fn parent_fixture(
    quota: usize,
) -> (
    String,
    RestartExpectations,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut sink = Memory::default();
    let run = super::super::super::evaluation::evaluate_registered_bundle_journaled(
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(quota),
        JournalCapture::new(
            &mut sink,
            RunId::new("parent-run").unwrap(),
            implementation(),
            codecs(),
            |s: &i32| Ok(s.to_string()),
            |e: &&str| Ok((*e).to_owned()),
        ),
    )
    .unwrap();
    assert!(run.journal_error().is_none());
    let journal = sink.text();
    let input = bundle().canonical_json().unwrap();
    let adapter = registry.metadata()[0];
    let expected = RestartExpectations::new(
        RestartAnchors::new(
            &RunId::new("parent-run").unwrap(),
            &digest(journal.as_bytes()),
            &digest(input.as_bytes()),
            &digest(canonical(adapter).unwrap().as_bytes()),
        )
        .unwrap(),
        implementation(),
        codecs(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    (journal, expected, baseline_calls, candidate_calls)
}

fn plan(journal: &str, expected: &RestartExpectations) -> TypedContinuationPlan<i32, i32> {
    prepare_typed_continuation(journal, &bundle(), expected, &"b".repeat(64), |payload| {
        payload.parse::<i32>().map_err(|e| e.to_string())
    })
    .unwrap()
}

fn completed_child(journal: &str, expected: &RestartExpectations) -> String {
    let plan = plan(journal, expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut child = Memory::default();
    let run = execute_typed_continuation(
        plan,
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child,
            RunId::new("child-run").unwrap(),
            implementation(),
            codecs(),
            |value: &i32| Ok(value.to_string()),
            |error: &&str| Ok((*error).to_owned()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), ContinuationRunState::Completed);
    child.text()
}

fn quota_child(journal: &str, expected: &RestartExpectations) -> String {
    let plan = plan(journal, expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut child = Memory::default();
    let run = execute_typed_continuation(
        plan,
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(1),
        ContinuationCapture::new(
            &mut child,
            RunId::new("child-run").unwrap(),
            implementation(),
            codecs(),
            |value: &i32| Ok(value.to_string()),
            |error: &&str| Ok((*error).to_owned()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), ContinuationRunState::Interrupted);
    child.text()
}

fn parse_child_events(payload: &str) -> Vec<ContinuationEvent> {
    payload
        .split_terminator('\n')
        .map(|line| {
            serde_json::from_str::<ContinuationEntry>(line)
                .unwrap()
                .event
        })
        .collect()
}

fn emit_child_events(events: Vec<ContinuationEvent>) -> String {
    let mut memory = Memory::default();
    {
        let mut emitter = ContinuationEmitter {
            sink: &mut memory,
            sequence: 0,
            previous: None,
            bytes: 0,
        };
        for event in events {
            emitter.append(event).unwrap();
        }
    }
    memory.text()
}

#[test]
fn restores_acknowledged_prefix_and_exact_never_started_suffix() {
    let (journal, expected, baseline_calls, candidate_calls) = parent_fixture(1);
    let plan = plan(&journal, &expected);
    assert_eq!(*plan.baseline(), 10);
    assert_eq!(plan.restored_outcomes().len(), 1);
    assert_eq!(plan.restored_outcomes()[0].scenario_id().as_str(), "s1");
    assert_eq!(*plan.restored_outcomes()[0].signature(), 11);
    assert_eq!(
        plan.remaining()
            .iter()
            .map(|s| s.id().as_str())
            .collect::<Vec<_>>(),
        ["s2", "s3"]
    );
    assert_eq!(baseline_calls.load(Ordering::SeqCst), 1);
    assert_eq!(candidate_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn continuation_executes_only_suffix_and_never_calls_domain_baseline_again() {
    let (journal, expected, _, _) = parent_fixture(1);
    let plan = plan(&journal, &expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut child = Memory::default();
    let run = execute_typed_continuation(
        plan,
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child,
            RunId::new("child-run").unwrap(),
            implementation(),
            codecs(),
            |s: &i32| Ok(s.to_string()),
            |e: &&str| Ok((*e).to_owned()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), ContinuationRunState::Completed);
    assert_eq!(run.source_run_id(), "parent-run");
    assert_eq!(run.child_run_id(), "child-run");
    assert_eq!(run.confirmed_candidate_count(), 3);
    assert_eq!(
        run.new_outcomes()
            .iter()
            .map(|o| o.scenario().id().as_str())
            .collect::<Vec<_>>(),
        ["s2", "s3"]
    );
    assert_eq!(baseline_calls.load(Ordering::SeqCst), 0);
    assert_eq!(candidate_calls.load(Ordering::SeqCst), 2);
    let summary = inspect_continuation_journal(
        &child.text(),
        &journal,
        &bundle().canonical_json().unwrap(),
        &expected,
    )
    .unwrap();
    assert_eq!(summary.state, "completed");
    assert_eq!(summary.parent_run_id, "parent-run");
    assert_eq!(summary.child_run_id, "child-run");
    assert_eq!(summary.successful_new_candidates, 2);
    assert!(!summary.resume_authorized);
}

#[test]
fn parent_decoder_failure_occurs_before_any_child_engine_call() {
    let (journal, expected, _, _) = parent_fixture(1);
    let error = prepare_typed_continuation::<_, _, i32, _>(
        &journal,
        &bundle(),
        &expected,
        &"b".repeat(64),
        |_| Err("decode refused".into()),
    )
    .unwrap_err();
    assert!(matches!(error, ContinuationError::Decode(message) if message == "decode refused"));
}

#[test]
fn torn_parent_and_effectful_semantics_never_form_a_plan() {
    let (clean, expected_clean, _, _) = parent_fixture(1);
    let mut torn = clean.clone();
    torn.push_str("{\"sequence\":999");
    let torn_expected = RestartExpectations::new(
        RestartAnchors::new(
            &RunId::new("parent-run").unwrap(),
            &digest(torn.as_bytes()),
            &digest(bundle().canonical_json().unwrap().as_bytes()),
            &expected_clean.wire.anchors.adapter_sha256,
        )
        .unwrap(),
        implementation(),
        codecs(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    let torn_result = prepare_typed_continuation::<_, _, i32, _>(
        &torn,
        &bundle(),
        &torn_expected,
        &"b".repeat(64),
        |p| p.parse::<i32>().map_err(|e| e.to_string()),
    );
    assert!(matches!(
        torn_result,
        Err(ContinuationError::Blocked(_) | ContinuationError::Contract(_))
    ));
    let effectful = RestartExpectations::new(
        expected_clean.wire.anchors.clone(),
        implementation(),
        codecs(),
        RestartSemantics::RequiresReconciliation,
    )
    .unwrap();
    assert!(matches!(
        prepare_typed_continuation::<_, _, i32, _>(
            &clean,
            &bundle(),
            &effectful,
            &"b".repeat(64),
            |p| p.parse::<i32>().map_err(|e| e.to_string()),
        ),
        Err(ContinuationError::Blocked(_))
    ));
}

#[test]
fn child_storage_failure_before_header_causes_zero_domain_calls() {
    let (journal, expected, _, _) = parent_fixture(1);
    let plan = plan(&journal, &expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut sink = FailFirstSink { attempts: 0 };
    let run = execute_typed_continuation(
        plan,
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut sink,
            RunId::new("child-run").unwrap(),
            implementation(),
            codecs(),
            |s: &i32| Ok(s.to_string()),
            |e: &&str| Ok((*e).to_owned()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), ContinuationRunState::JournalFailed);
    assert_eq!(run.never_started().len(), 2);
    assert_eq!(baseline_calls.load(Ordering::SeqCst), 0);
    assert_eq!(candidate_calls.load(Ordering::SeqCst), 0);
    assert_eq!(sink.attempts, 1);
}

#[test]
fn child_engine_failure_is_preserved_and_never_retried() {
    let (journal, expected, _, _) = parent_fixture(1);
    let plan = plan(&journal, &expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, Some(2));
    let mut child = Memory::default();
    let run = execute_typed_continuation(
        plan,
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child,
            RunId::new("child-run").unwrap(),
            implementation(),
            codecs(),
            |s: &i32| Ok(s.to_string()),
            |e: &&str| Ok((*e).to_owned()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), ContinuationRunState::EngineFailed);
    assert_eq!(run.engine_error(), Some(&"fixture failure"));
    assert_eq!(
        run.never_started()
            .iter()
            .map(|s| s.id().as_str())
            .collect::<Vec<_>>(),
        ["s3"]
    );
    assert_eq!(candidate_calls.load(Ordering::SeqCst), 1);
    let summary = inspect_continuation_journal(
        &child.text(),
        &journal,
        &bundle().canonical_json().unwrap(),
        &expected,
    )
    .unwrap();
    assert_eq!(summary.state, "failed");
    assert_eq!(summary.failed_candidate.as_deref(), Some("s2"));
}

#[test]
fn child_quota_preserves_remaining_suffix_without_calling_more() {
    let (journal, expected, _, _) = parent_fixture(1);
    let plan = plan(&journal, &expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut child = Memory::default();
    let run = execute_typed_continuation(
        plan,
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(1),
        ContinuationCapture::new(
            &mut child,
            RunId::new("child-run").unwrap(),
            implementation(),
            codecs(),
            |s: &i32| Ok(s.to_string()),
            |e: &&str| Ok((*e).to_owned()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), ContinuationRunState::Interrupted);
    assert_eq!(run.new_outcomes().len(), 1);
    assert_eq!(
        run.never_started()
            .iter()
            .map(|s| s.id().as_str())
            .collect::<Vec<_>>(),
        ["s3"]
    );
    assert_eq!(candidate_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn changed_bundle_implementation_codec_or_adapter_blocks_before_child_calls() {
    let (journal, expected, _, _) = parent_fixture(1);
    for case in 0..3 {
        let plan = plan(&journal, &expected);
        let baseline_calls = Arc::new(AtomicUsize::new(0));
        let candidate_calls = Arc::new(AtomicUsize::new(0));
        let registry = registry(&baseline_calls, &candidate_calls, None);
        let mut sink = Memory::default();
        let changed_bundle = bundle();
        let input = if case == 0 {
            ScenarioBundle::new(
                "fixture.continuation",
                AdapterBinding::new("fixture.adapter", version(), None).unwrap(),
                Some(7),
                99,
                (1..=3)
                    .map(|i| BundleScenario::new(ScenarioId::new(format!("s{i}")).unwrap(), i))
                    .collect(),
                None,
                None,
            )
            .unwrap()
        } else {
            changed_bundle
        };
        let implementation = if case == 1 {
            EngineIdentity::new("fixture.engine", &"c".repeat(40), &"b".repeat(64)).unwrap()
        } else {
            implementation()
        };
        let codecs = if case == 2 {
            PayloadCodecs::new("fixture.other.v1", "fixture.error.v1").unwrap()
        } else {
            codecs()
        };
        let result = execute_typed_continuation(
            plan,
            &input,
            &registry,
            &MetricRegistry::<i32, i32>::new(),
            &DecisionPolicyRegistry::<i32, i32>::new(),
            &EvaluationControl::new(2),
            ContinuationCapture::new(
                &mut sink,
                RunId::new("child-run").unwrap(),
                implementation,
                codecs,
                |s: &i32| Ok(s.to_string()),
                |e: &&str| Ok((*e).to_owned()),
            ),
        );
        assert!(matches!(result, Err(ContinuationError::Contract(_))));
        assert_eq!(baseline_calls.load(Ordering::SeqCst), 0);
        assert_eq!(candidate_calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn child_run_id_cannot_reuse_parent_identity() {
    let (journal, expected, _, _) = parent_fixture(1);
    let plan = plan(&journal, &expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut child = Memory::default();
    let result = execute_typed_continuation(
        plan,
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child,
            RunId::new("parent-run").unwrap(),
            implementation(),
            codecs(),
            |s: &i32| Ok(s.to_string()),
            |e: &&str| Ok((*e).to_owned()),
        ),
    );
    assert!(matches!(result, Err(ContinuationError::Contract(_))));
    assert!(child.0.is_empty());
    assert_eq!(baseline_calls.load(Ordering::SeqCst), 0);
    assert_eq!(candidate_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn inspector_accepts_cancelled_terminal_after_last_success_without_relabeling_complete() {
    let (journal, expected, _, _) = parent_fixture(1);
    let plan = plan(&journal, &expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut child = Memory::default();
    let run = execute_typed_continuation(
        plan,
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child,
            RunId::new("child-run").unwrap(),
            implementation(),
            codecs(),
            |s: &i32| Ok(s.to_string()),
            |e: &&str| Ok((*e).to_owned()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), ContinuationRunState::Completed);
    let text = child.text();
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut last: ContinuationEntry = serde_json::from_str(lines.last().unwrap()).unwrap();
    last.event = ContinuationEvent::Finished {
        terminal: ContinuationTerminal::Interrupted {
            reason: RecordInterruption::Cancelled,
        },
    };
    *lines.last_mut().unwrap() = canonical(&last).unwrap();
    let altered = lines.join("\n") + "\n";
    let summary = inspect_continuation_journal(
        &altered,
        &journal,
        &bundle().canonical_json().unwrap(),
        &expected,
    )
    .unwrap();
    assert_eq!(summary.state, "interrupted");
    assert_eq!(summary.successful_new_candidates, 2);
    assert_eq!(summary.never_started_candidates, 0);
    assert!(summary.terminal_recorded);
    assert!(!summary.resume_authorized);
}

fn forge_child_identity(
    child: &str,
    expected: &RestartExpectations,
    adapter: serde_json::Value,
) -> String {
    let mut events = parse_child_events(child);
    match &mut events[0] {
        ContinuationEvent::Initialized { header } => {
            header.parent.run_id = expected.wire.anchors.run_id.clone();
            header.parent.expectation_sha256 = expected.sha256();
            header.implementation = expected.wire.implementation.clone();
            header.codecs = expected.wire.codecs.clone();
            header.adapter = adapter;
        }
        _ => panic!("first child event must be initialized"),
    }
    emit_child_events(events)
}

#[test]
fn inspector_rejects_child_coherently_forged_to_mismatched_parent_expectations() {
    let (journal, original, _, _) = parent_fixture(1);
    let child = completed_child(&journal, &original);
    let bundle_json = bundle().canonical_json().unwrap();
    let actual_adapter = registry(
        &Arc::new(AtomicUsize::new(0)),
        &Arc::new(AtomicUsize::new(0)),
        None,
    )
    .metadata()[0]
        .clone();

    let wrong_run = RestartExpectations::new(
        RestartAnchors::new(
            &RunId::new("other-parent").unwrap(),
            &digest(journal.as_bytes()),
            &digest(bundle_json.as_bytes()),
            &digest(canonical(&actual_adapter).unwrap().as_bytes()),
        )
        .unwrap(),
        implementation(),
        codecs(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    let forged = forge_child_identity(
        &child,
        &wrong_run,
        serde_json::to_value(&actual_adapter).unwrap(),
    );
    assert!(inspect_continuation_journal(&forged, &journal, &bundle_json, &wrong_run).is_err());

    let wrong_implementation = RestartExpectations::new(
        original.wire.anchors.clone(),
        EngineIdentity::new("fixture.other_engine", &"c".repeat(40), &"d".repeat(64)).unwrap(),
        codecs(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    let forged = forge_child_identity(
        &child,
        &wrong_implementation,
        serde_json::to_value(&actual_adapter).unwrap(),
    );
    assert!(
        inspect_continuation_journal(&forged, &journal, &bundle_json, &wrong_implementation,)
            .is_err()
    );

    let wrong_codecs = RestartExpectations::new(
        original.wire.anchors.clone(),
        implementation(),
        PayloadCodecs::new("fixture.other_i32.v1", "fixture.other_error.v1").unwrap(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    let forged = forge_child_identity(
        &child,
        &wrong_codecs,
        serde_json::to_value(&actual_adapter).unwrap(),
    );
    assert!(inspect_continuation_journal(&forged, &journal, &bundle_json, &wrong_codecs).is_err());

    let wrong_adapter = AdapterMetadata::new(
        "fixture.other_adapter",
        version(),
        None,
        vec![AdapterCapability::new("fixture.evaluate", version()).unwrap()],
    )
    .unwrap();
    let wrong_adapter_expectations = RestartExpectations::new(
        RestartAnchors::new(
            &RunId::new("parent-run").unwrap(),
            &digest(journal.as_bytes()),
            &digest(bundle_json.as_bytes()),
            &digest(canonical(&wrong_adapter).unwrap().as_bytes()),
        )
        .unwrap(),
        implementation(),
        codecs(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    let forged = forge_child_identity(
        &child,
        &wrong_adapter_expectations,
        serde_json::to_value(&wrong_adapter).unwrap(),
    );
    assert!(
        inspect_continuation_journal(&forged, &journal, &bundle_json, &wrong_adapter_expectations,)
            .is_err()
    );
}

#[test]
fn inspector_rejects_coherently_rehashed_child_that_invents_restored_parent_success() {
    let (journal, expected, _, _) = parent_fixture(1);
    let mut events = parse_child_events(&completed_child(&journal, &expected));
    match &mut events[0] {
        ContinuationEvent::Initialized { header } => {
            header.restored_candidate_ids = vec!["s1".into(), "s2".into()];
            header.remaining_candidate_ids = vec!["s3".into()];
        }
        _ => panic!("first child event must be initialized"),
    }
    events.retain(|event| {
        !matches!(event,
            ContinuationEvent::CallStarted { scenario_id }
                | ContinuationEvent::CallSucceeded { scenario_id, .. }
                if scenario_id == "s2"
        )
    });
    let forged = emit_child_events(events);
    assert!(
        inspect_continuation_journal(
            &forged,
            &journal,
            &bundle().canonical_json().unwrap(),
            &expected,
        )
        .is_err()
    );
}

#[test]
fn inspector_rejects_quota_interruption_before_recorded_limit() {
    let (journal, expected, _, _) = parent_fixture(1);
    let mut events = parse_child_events(&quota_child(&journal, &expected));
    match &mut events[0] {
        ContinuationEvent::Initialized { header } => header.max_evaluations = 2,
        _ => panic!("first child event must be initialized"),
    }
    let forged = emit_child_events(events);
    assert!(
        inspect_continuation_journal(
            &forged,
            &journal,
            &bundle().canonical_json().unwrap(),
            &expected,
        )
        .is_err()
    );
}

#[test]
fn inspector_rejects_deadline_terminal_without_configured_deadline() {
    let (journal, expected, _, _) = parent_fixture(1);
    let mut events = parse_child_events(&quota_child(&journal, &expected));
    let last = events.last_mut().expect("terminal event");
    *last = ContinuationEvent::Finished {
        terminal: ContinuationTerminal::Interrupted {
            reason: RecordInterruption::DeadlineReached,
        },
    };
    let forged = emit_child_events(events);
    assert!(
        inspect_continuation_journal(
            &forged,
            &journal,
            &bundle().canonical_json().unwrap(),
            &expected,
        )
        .is_err()
    );
}

#[test]
fn child_journal_rejects_tampering_and_wrong_parent() {
    let (journal, expected, _, _) = parent_fixture(1);
    let plan = plan(&journal, &expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut child = Memory::default();
    let run = execute_typed_continuation(
        plan,
        &bundle(),
        &registry,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child,
            RunId::new("child-run").unwrap(),
            implementation(),
            codecs(),
            |s: &i32| Ok(s.to_string()),
            |e: &&str| Ok((*e).to_owned()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), ContinuationRunState::Completed);
    let text = child.text();
    assert!(
        inspect_continuation_journal(
            &text.replace("\"12\"", "\"99\""),
            &journal,
            &bundle().canonical_json().unwrap(),
            &expected
        )
        .is_err()
    );
    assert!(
        inspect_continuation_journal(
            &text,
            &(journal.clone() + "\n"),
            &bundle().canonical_json().unwrap(),
            &expected
        )
        .is_err()
    );
}
