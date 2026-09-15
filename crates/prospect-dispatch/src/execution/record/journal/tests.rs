//! Software fixtures only; no GPU or scientific execution claims.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use prospect_adapter::{
    AdapterCapability, AdapterMetadataError, ContractVersion, VersionedAdapter,
};
use prospect_bundle::{AdapterBinding, BundleScenario, RegistryRequirement};
use prospect_core::{DecisionPolicy, ProspectiveEngine, ScenarioId, SignatureMetric};
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};
use prospect_scenario::controlled::EvaluationControl;

use super::super::super::ExecutableAdapterRegistry;
use super::evaluation::JournalCapture;
use super::*;

type Lines = Arc<Mutex<Vec<Vec<u8>>>>;
#[derive(Default)]
struct Memory {
    lines: Lines,
    attempts: usize,
    fail_at: Option<usize>,
    torn: bool,
}
impl Memory {
    fn text(&self) -> String {
        String::from_utf8(self.lines.lock().unwrap().concat()).unwrap()
    }
}
impl JournalSink for Memory {
    fn append_record(&mut self, record: &[u8]) -> io::Result<()> {
        let index = self.attempts;
        self.attempts += 1;
        if Some(index) == self.fail_at {
            if self.torn {
                self.lines
                    .lock()
                    .unwrap()
                    .push(record[..record.len() / 2].to_vec());
            }
            return Err(io::Error::other("injected storage failure"));
        }
        self.lines.lock().unwrap().push(record.to_vec());
        Ok(())
    }
}
struct Engine {
    calls: Arc<AtomicUsize>,
    lines: Lines,
    fail: Option<i32>,
    cancel: Option<prospect_scenario::controlled::CancellationToken>,
}
impl Engine {
    fn called(&self, target: CallTarget) {
        // The actual engine inspects the acknowledged sink BEFORE executing.
        let lines = self.lines.lock().unwrap();
        let entry: Entry =
            serde_json::from_slice(lines.last().expect("intent precedes engine")).unwrap();
        assert!(matches!(entry.event, Event::CallStarted { target: actual } if actual == target));
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}
impl ProspectiveEngine<i32, i32> for Engine {
    type Signature = i32;
    type Error = &'static str;
    fn baseline(&self, state: &i32) -> Result<i32, Self::Error> {
        self.called(CallTarget::Baseline);
        if self.fail == Some(0) {
            Err("baseline failed")
        } else {
            Ok(*state)
        }
    }
    fn evaluate(&self, state: &i32, intervention: &i32) -> Result<i32, Self::Error> {
        self.called(CallTarget::Scenario {
            id: format!("s{intervention}"),
        });
        if let Some(token) = &self.cancel {
            token.cancel();
        }
        if self.fail == Some(*intervention) {
            Err("candidate failed")
        } else {
            Ok(state + intervention)
        }
    }
}
impl VersionedAdapter for Engine {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        AdapterMetadata::new(
            "example.journal",
            version(),
            None,
            vec![AdapterCapability::new("example.evaluate", version())?],
        )
    }
}
struct NeverScore;
impl SignatureMetric<i32> for NeverScore {
    type Score = i32;
    fn compare(&self, _: &i32, _: &i32) -> i32 {
        panic!("journal evaluation must not score")
    }
}
impl DecisionPolicy<i32> for NeverScore {
    type Score = i32;
    fn utility(&self, _: &i32) -> i32 {
        panic!("journal evaluation must not rank")
    }
}
fn version() -> ContractVersion {
    ContractVersion::new(1, 0).unwrap()
}
fn bundle() -> ScenarioBundle<i32, i32> {
    ScenarioBundle::new(
        "example.journal_run",
        AdapterBinding::new("example.journal", version(), None).unwrap(),
        Some(7),
        10,
        (1..=3)
            .map(|i| BundleScenario::new(ScenarioId::new(format!("s{i}")).unwrap(), i))
            .collect(),
        Some(RegistryRequirement::new("metric.fixture", version()).unwrap()),
        Some(RegistryRequirement::new("policy.fixture", version()).unwrap()),
    )
    .unwrap()
}
fn identity() -> EngineIdentity {
    EngineIdentity::new("example.impl", &"a".repeat(40), &"b".repeat(64)).unwrap()
}
fn codecs() -> PayloadCodecs {
    PayloadCodecs::new("example.i32.v1", "example.error.v1").unwrap()
}
struct Fixture {
    adapters: ExecutableAdapterRegistry<i32, i32, i32, &'static str>,
    metrics: MetricRegistry<i32, i32>,
    policies: DecisionPolicyRegistry<i32, i32>,
    calls: Arc<AtomicUsize>,
}
impl Fixture {
    fn new(memory: &Memory, fail: Option<i32>, control: Option<&EvaluationControl>) -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut adapters = ExecutableAdapterRegistry::new();
        adapters
            .register(Engine {
                calls: calls.clone(),
                lines: memory.lines.clone(),
                fail,
                cancel: control.map(EvaluationControl::cancellation_token),
            })
            .unwrap();
        let mut metrics = MetricRegistry::new();
        metrics
            .register("metric.fixture", version(), NeverScore)
            .unwrap();
        let mut policies = DecisionPolicyRegistry::new();
        policies
            .register("policy.fixture", version(), NeverScore)
            .unwrap();
        Self {
            adapters,
            metrics,
            policies,
            calls,
        }
    }
    fn run(
        &self,
        memory: &mut Memory,
        control: &EvaluationControl,
    ) -> JournalRun<i32, i32, &'static str> {
        evaluate_registered_bundle_journaled(
            &bundle(),
            &self.adapters,
            &self.metrics,
            &self.policies,
            control,
            JournalCapture::new(
                memory,
                RunId::new("fixture-run").unwrap(),
                identity(),
                codecs(),
                |s: &i32| Ok(s.to_string()),
                |e: &&str| Ok((*e).into()),
            ),
        )
        .unwrap()
    }
}
fn inspect(memory: &Memory) -> JournalSummary {
    inspect_execution_journal(&memory.text(), &bundle().canonical_json().unwrap()).unwrap()
}
fn complete() -> String {
    let mut memory = Memory::default();
    let fixture = Fixture::new(&memory, None, None);
    assert_eq!(
        fixture.run(&mut memory, &EvaluationControl::new(3)).state(),
        JournalRunState::Completed
    );
    memory.text()
}
fn changed(mut lines: Vec<Value>, mutation: impl FnOnce(&mut Vec<Value>)) -> String {
    mutation(&mut lines);
    let mut previous = None::<String>;
    let mut out = String::new();
    for (i, value) in lines.iter_mut().enumerate() {
        value["sequence"] = serde_json::json!(i);
        value["previous_sha256"] = serde_json::json!(previous);
        let line = canonical(value).unwrap();
        previous = Some(digest(line.as_bytes()));
        out.push_str(&line);
        out.push('\n');
    }
    out
}
fn values(text: &str) -> Vec<Value> {
    text.lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

#[test]
fn live_intent_precedes_every_actual_call_and_complete_journal_is_checkable() {
    let mut memory = Memory::default();
    let fixture = Fixture::new(&memory, None, None);
    let run = fixture.run(&mut memory, &EvaluationControl::new(3));
    assert_eq!(run.state(), JournalRunState::Completed);
    assert_eq!(run.adapter().adapter_id().as_str(), "example.journal");
    assert_eq!(run.input_json(), bundle().canonical_json().unwrap());
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 4);
    assert_eq!(run.into_completed_batch().unwrap().outcomes().len(), 3);
    let s = inspect(&memory);
    assert_eq!(s.state, "completed");
    assert_eq!(s.verified_entries, 10);
    assert_eq!(s.successful_candidates, 3);
    assert!(s.terminal_recorded);
    assert_eq!(s.incomplete_tail_bytes, 0);
    assert!(!s.resume_authorized);
}

#[test]
fn every_storage_failure_stops_later_engine_calls_and_retains_actual_returns() {
    for failure in 0..10 {
        let mut memory = Memory {
            fail_at: Some(failure),
            ..Memory::default()
        };
        let fixture = Fixture::new(&memory, None, None);
        let control = EvaluationControl::new(3);
        let run = fixture.run(&mut memory, &control);
        let actual = [1, 3, 5, 7].into_iter().filter(|i| *i < failure).count();
        assert_eq!(
            run.state(),
            JournalRunState::JournalFailed,
            "entry {failure}"
        );
        assert_eq!(
            fixture.calls.load(Ordering::SeqCst),
            actual,
            "entry {failure}"
        );
        assert_eq!(run.outcomes().len(), actual.saturating_sub(1));
        assert_eq!(run.never_started().len(), 3 - actual.saturating_sub(1));
        assert_eq!(run.baseline().is_some(), actual != 0);
        assert!(run.engine_error().is_none());
        assert!(run.failed_call().is_none());
        assert!(run.journal_error().is_some());
        assert!(!control.cancellation_token().is_cancelled());
        assert!(run.into_completed_batch().is_err());
        assert_eq!(memory.attempts, failure + 1, "no write retry");
    }
}

#[test]
fn torn_success_record_is_explicit_and_does_not_erase_in_memory_success() {
    let mut memory = Memory {
        fail_at: Some(4),
        torn: true,
        ..Memory::default()
    };
    let fixture = Fixture::new(&memory, None, None);
    let run = fixture.run(&mut memory, &EvaluationControl::new(3));
    assert_eq!(run.outcomes().len(), 1);
    assert_eq!(run.never_started().len(), 2);
    let summary = inspect(&memory);
    assert_eq!(summary.state, "incomplete_tail");
    assert_eq!(summary.successful_candidates, 0);
    assert_eq!(
        summary.unknown_call_result,
        Some(CallTarget::Scenario { id: "s1".into() })
    );
    assert!(summary.incomplete_tail_bytes > 0);
    assert!(!summary.resume_authorized);
}

#[test]
fn domain_error_survives_failure_to_record_that_error() {
    let mut memory = Memory {
        fail_at: Some(6),
        ..Memory::default()
    };
    let fixture = Fixture::new(&memory, Some(2), None);
    let run = fixture.run(&mut memory, &EvaluationControl::new(3));
    assert_eq!(run.state(), JournalRunState::JournalFailed);
    assert_eq!(run.engine_error(), Some(&"candidate failed"));
    assert_eq!(
        run.failed_call(),
        Some(CallTarget::Scenario { id: "s2".into() })
    );
    assert_eq!(run.outcomes().len(), 1);
    assert_eq!(run.never_started().len(), 1);
    assert_eq!(inspect(&memory).state, "unknown_call_result");
}

#[test]
fn baseline_and_candidate_failures_close_without_retry() {
    for failed in [0, 2] {
        let mut memory = Memory::default();
        let fixture = Fixture::new(&memory, Some(failed), None);
        let run = fixture.run(&mut memory, &EvaluationControl::new(3));
        assert_eq!(run.state(), JournalRunState::EngineFailed);
        assert!(run.engine_error().is_some());
        assert!(run.journal_error().is_none());
        assert_eq!(
            fixture.calls.load(Ordering::SeqCst),
            if failed == 0 { 1 } else { 3 }
        );
        let summary = inspect(&memory);
        assert_eq!(summary.state, "failed");
        assert!(summary.failed_call.is_some());
        assert!(!summary.resume_authorized);
    }
}

#[test]
fn quota_and_cancellation_preserve_the_original_control_semantics() {
    for quota in [0, 1, 3] {
        let mut memory = Memory::default();
        let fixture = Fixture::new(&memory, None, None);
        let run = fixture.run(&mut memory, &EvaluationControl::new(quota));
        assert_eq!(run.outcomes().len(), quota);
        assert_eq!(
            inspect(&memory).state,
            if quota == 3 {
                "completed"
            } else {
                "interrupted"
            }
        );
        if quota < 3 {
            assert!(run.interruption().is_some());
        }
    }
    let mut memory = Memory::default();
    let control = EvaluationControl::new(3);
    let fixture = Fixture::new(&memory, None, Some(&control));
    let run = fixture.run(&mut memory, &control);
    assert_eq!(run.state(), JournalRunState::Interrupted);
    assert_eq!(run.outcomes().len(), 1);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 2);
    assert_eq!(inspect(&memory).state, "interrupted");
}

#[test]
fn expired_deadline_and_pre_cancelled_run_do_not_call_engine() {
    for control in [EvaluationControl::new(3).with_deadline(Instant::now()), {
        let value = EvaluationControl::new(3);
        value.cancellation_token().cancel();
        value
    }] {
        let mut memory = Memory::default();
        let fixture = Fixture::new(&memory, None, None);
        let run = fixture.run(&mut memory, &control);
        assert_eq!(run.state(), JournalRunState::Interrupted);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
        assert_eq!(inspect(&memory).verified_entries, 2);
    }
}

#[test]
fn preflight_errors_leave_sink_and_engine_untouched() {
    let mut memory = Memory::default();
    let fixture = Fixture::new(&memory, None, None);
    let empty = MetricRegistry::<i32, i32>::new();
    let result = evaluate_registered_bundle_journaled(
        &bundle(),
        &fixture.adapters,
        &empty,
        &fixture.policies,
        &EvaluationControl::new(3),
        JournalCapture::new(
            &mut memory,
            RunId::new("run").unwrap(),
            identity(),
            codecs(),
            |s: &i32| Ok(s.to_string()),
            |e: &&str| Ok((*e).into()),
        ),
    );
    assert!(result.is_err());
    assert_eq!(memory.attempts, 0);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn codec_failure_preserves_return_and_unknown_journal_result() {
    let mut memory = Memory::default();
    let fixture = Fixture::new(&memory, None, None);
    let run = evaluate_registered_bundle_journaled(
        &bundle(),
        &fixture.adapters,
        &fixture.metrics,
        &fixture.policies,
        &EvaluationControl::new(3),
        JournalCapture::new(
            &mut memory,
            RunId::new("run").unwrap(),
            identity(),
            codecs(),
            |_: &i32| Err("cannot encode".into()),
            |e: &&str| Ok((*e).into()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), JournalRunState::JournalFailed);
    assert_eq!(run.baseline(), Some(&10));
    assert_eq!(run.never_started().len(), 3);
    assert_eq!(memory.attempts, 2);
    assert_eq!(
        inspect(&memory).unknown_call_result,
        Some(CallTarget::Baseline)
    );
}

#[test]
fn oversized_encoded_payload_never_produces_a_success_entry() {
    let mut memory = Memory::default();
    let fixture = Fixture::new(&memory, None, None);
    let run = evaluate_registered_bundle_journaled(
        &bundle(),
        &fixture.adapters,
        &fixture.metrics,
        &fixture.policies,
        &EvaluationControl::new(3),
        JournalCapture::new(
            &mut memory,
            RunId::new("run").unwrap(),
            identity(),
            codecs(),
            |_: &i32| Ok("x".repeat(super::super::MAX_ENCODED_PAYLOAD_BYTES + 1)),
            |e: &&str| Ok((*e).into()),
        ),
    )
    .unwrap();
    assert_eq!(run.state(), JournalRunState::JournalFailed);
    assert_eq!(run.baseline(), Some(&10));
    assert_eq!(memory.attempts, 2);
    assert_eq!(inspect(&memory).state, "unknown_call_result");
}

#[test]
fn every_complete_prefix_has_conservative_recovery_classification() {
    let payload = complete();
    let lines: Vec<_> = payload.split_inclusive('\n').collect();
    let input = bundle().canonical_json().unwrap();
    for length in 1..=lines.len() {
        let prefix = lines[..length].concat();
        let summary = inspect_execution_journal(&prefix, &input).unwrap();
        assert!(!summary.resume_authorized);
        assert_eq!(summary.verified_prefix_sha256, digest(prefix.as_bytes()));
        assert_eq!(summary.terminal_recorded, length == lines.len());
        assert_eq!(
            summary.unknown_call_result.is_some(),
            matches!(length, 2 | 4 | 6 | 8)
        );
    }
}

#[test]
fn an_unterminated_final_entry_is_not_promoted_to_completion() {
    let payload = complete();
    let input = bundle().canonical_json().unwrap();
    let summary = inspect_execution_journal(payload.trim_end_matches('\n'), &input).unwrap();
    assert_eq!(summary.state, "incomplete_tail");
    assert!(!summary.terminal_recorded);
    assert_eq!(summary.successful_candidates, 3);
    assert!(!summary.resume_authorized);
}

#[test]
fn bytes_after_terminal_are_always_rejected_even_without_lf() {
    let payload = complete();
    let input = bundle().canonical_json().unwrap();
    for suffix in ["x", "\n", "{}\n"] {
        assert!(inspect_execution_journal(&(payload.clone() + suffix), &input).is_err());
    }
}

#[test]
fn changed_input_cannot_reuse_journal() {
    let payload = complete();
    let mut input: Value = serde_json::from_str(&bundle().canonical_json().unwrap()).unwrap();
    input["state"] = serde_json::json!(11);
    assert!(inspect_execution_journal(&payload, &canonical(&input).unwrap()).is_err());
}

#[test]
fn deletion_reordering_and_unrehashable_payload_edits_fail() {
    let payload = complete();
    let input = bundle().canonical_json().unwrap();
    let mut lines: Vec<_> = payload.split_inclusive('\n').map(str::to_owned).collect();
    lines.swap(3, 4);
    assert!(inspect_execution_journal(&lines.concat(), &input).is_err());
    lines.swap(3, 4);
    lines.remove(3);
    assert!(inspect_execution_journal(&lines.concat(), &input).is_err());
    assert!(
        inspect_execution_journal(
            &payload.replace("\"payload\":\"11\"", "\"payload\":\"99\""),
            &input
        )
        .is_err()
    );
}

#[test]
fn coherent_hashes_do_not_bypass_lifecycle_or_metadata_checks() {
    let payload = complete();
    let input = bundle().canonical_json().unwrap();
    let mutations: Vec<fn(&mut Vec<Value>)> = vec![
        |v| {
            v.remove(1);
        }, // baseline return without intent
        |v| {
            v.remove(2);
        }, // next call while baseline outcome unknown
        |v| {
            v[3]["event"]["target"]["id"] = serde_json::json!("wrong");
        },
        |v| {
            v[0]["event"]["header"]["max_evaluations"] = serde_json::json!(1);
        },
        |v| {
            v[0]["event"]["header"]["adapter"]["adapter_id"] = serde_json::json!("other.adapter");
        },
        |v| {
            v[0]["event"]["header"]["extra"] = Value::Null;
        },
        |v| {
            v[0]["event"]["header"]["implementation"]["revision"] = serde_json::json!("main");
        },
        |v| {
            v[0]["event"]["header"]["codecs"]["signature"] = serde_json::json!("bad");
        },
    ];
    for (index, mutation) in mutations.into_iter().enumerate() {
        let mutated = changed(values(&payload), mutation);
        assert!(
            inspect_execution_journal(&mutated, &input).is_err(),
            "mutation {index}"
        );
    }
}

#[test]
fn malformed_complete_entries_missing_fields_and_duplicate_keys_fail_closed() {
    let payload = complete();
    let input = bundle().canonical_json().unwrap();
    let header = payload.split_inclusive('\n').next().unwrap();
    for bad in ["{broken}\n", "{}\n", "\n", "{\"x\":NaN}\n"] {
        assert!(inspect_execution_journal(&(header.to_owned() + bad), &input).is_err());
    }
    let duplicate = payload.replacen("\"sequence\":0", "\"sequence\":0,\"sequence\":0", 1);
    assert!(inspect_execution_journal(&duplicate, &input).is_err());
    let missing = changed(values(&payload), |v| {
        v[0].as_object_mut().unwrap().remove("previous_sha256");
    });
    // Rehash helper restores this envelope field; remove it after hashing instead.
    assert!(
        inspect_execution_journal(
            &missing.replacen("\"previous_sha256\":null,", "", 1),
            &input
        )
        .is_err()
    );
}

#[test]
fn no_complete_header_never_creates_recovery_advice() {
    let input = bundle().canonical_json().unwrap();
    for bad in ["", "{", "{}", "{}\n"] {
        assert!(inspect_execution_journal(bad, &input).is_err());
    }
}

#[test]
fn implementation_identity_fields_are_explicit_and_strict() {
    assert!(EngineIdentity::new("no_namespace", &"a".repeat(40), &"b".repeat(64)).is_err());
    assert!(EngineIdentity::new("example.impl", "main", &"b".repeat(64)).is_err());
    assert!(EngineIdentity::new("example.impl", &"A".repeat(40), &"b".repeat(64)).is_err());
    assert!(EngineIdentity::new("example.impl", &"a".repeat(40), "digest").is_err());
}
