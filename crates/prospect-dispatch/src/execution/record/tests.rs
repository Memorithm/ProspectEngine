//! Synthetic control/codec fixtures, never domain or model observations.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;

use prospect_adapter::VersionedAdapter;
use prospect_bundle::{AdapterBinding, BundleScenario, UpstreamBinding};
use prospect_core::{ProspectiveEngine, ScenarioId};
use prospect_scenario::controlled::{ExecutionState, ProgressEvent};
use serde_json::json;

use super::*;

const REV: &str = "0123456789abcdef0123456789abcdef01234567";
struct Engine {
    calls: Arc<AtomicUsize>,
    fail: u8,
}
impl ProspectiveEngine<i32, i32> for Engine {
    type Signature = i32;
    type Error = &'static str;
    fn baseline(&self, state: &i32) -> Result<i32, Self::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail == 1 {
            Err("baseline\nerror")
        } else {
            Ok(*state)
        }
    }
    fn evaluate(&self, state: &i32, action: &i32) -> Result<i32, Self::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail == 2 && *action == 2 {
            Err("candidate\nerror")
        } else {
            Ok(state + action)
        }
    }
}
impl VersionedAdapter for Engine {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, prospect_adapter::AdapterMetadataError> {
        AdapterMetadata::new(
            "prospect.record_fixture",
            version(),
            Some(AdapterUpstream::new("memorithm.fixture", REV)?),
            vec![AdapterCapability::new("fixture.evaluate", version())?],
        )
    }
}
fn version() -> ContractVersion {
    ContractVersion::new(1, 0).unwrap()
}
fn bundle(state: i32) -> ScenarioBundle<i32, i32> {
    ScenarioBundle::new(
        "experiment.record_fixture",
        AdapterBinding::new(
            "prospect.record_fixture",
            version(),
            Some(UpstreamBinding::new("memorithm.fixture", REV).unwrap()),
        )
        .unwrap(),
        Some(7),
        state,
        vec![
            BundleScenario::new(ScenarioId::new("a").unwrap(), 1),
            BundleScenario::new(ScenarioId::new("b").unwrap(), 2),
        ],
        None,
        None,
    )
    .unwrap()
}
fn codecs() -> PayloadCodecs {
    PayloadCodecs::new("fixture.i32.v1", "fixture.text.v1").unwrap()
}
fn evaluate(
    control: &EvaluationControl,
    fail: u8,
) -> BoundBundleEvaluation<i32, i32, &'static str> {
    let mut adapters = ExecutableAdapterRegistry::new();
    adapters
        .register(Engine {
            calls: Arc::new(AtomicUsize::new(0)),
            fail,
        })
        .unwrap();
    evaluate_registered_bundle_bound(
        &bundle(10),
        &adapters,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        control,
        |_| {},
    )
    .unwrap()
}
fn capture(bound: &BoundBundleEvaluation<i32, i32, &'static str>) -> ExecutionRecord {
    bound
        .capture_record(
            &RunId::new("run-001").unwrap(),
            codecs(),
            |s| Ok(s.to_string()),
            |e| Ok(e.to_string()),
        )
        .unwrap()
}
fn record() -> ExecutionRecord {
    capture(&evaluate(&EvaluationControl::new(2), 0))
}
fn mutate(edit: impl FnOnce(&mut Value)) -> Result<ExecutionRecord, ExecutionRecordError> {
    let mut value: Value = serde_json::from_str(record().canonical_json()).unwrap();
    edit(&mut value);
    ExecutionRecord::verify_against_bundle(
        &canonical(&value).unwrap(),
        &bundle(10).canonical_json().unwrap(),
    )
}

#[test]
fn roundtrip_binds_exact_bundle_and_preserves_payload_bytes() {
    let bound = evaluate(&EvaluationControl::new(2), 0);
    assert_eq!(bound.input_json(), bundle(10).canonical_json().unwrap());
    let record = bound
        .capture_record(
            &RunId::new("unicode-run-é").unwrap(),
            codecs(),
            |s| Ok(format!("value={s}\n\0é")),
            |e| Ok(e.to_string()),
        )
        .unwrap();
    let decoded =
        ExecutionRecord::verify_against_bundle(record.canonical_json(), bound.input_json())
            .unwrap();
    assert_eq!(record.sha256(), decoded.sha256());
    assert_eq!(decoded.baseline_payload(), Some("value=10\n\0é"));
    assert_eq!(decoded.outcomes()[0].payload(), "value=11\n\0é");
    assert_eq!(decoded.outcomes()[1].scenario_id(), "b");
    assert_eq!(decoded.codecs().signature(), "fixture.i32.v1");
    assert_eq!(decoded.codecs().error(), "fixture.text.v1");
    assert_eq!(decoded.summary().state, "completed");
    assert!(!decoded.summary().resume_authorized);
}

#[test]
fn changed_state_with_identical_labels_cannot_reuse_record() {
    let record = record();
    let other = bundle(999).canonical_json().unwrap();
    assert!(ExecutionRecord::verify_against_bundle(record.canonical_json(), &other).is_err());
}

#[test]
fn quota_interruption_retains_prefix_and_exact_never_started_suffix() {
    let bound = evaluate(&EvaluationControl::new(1), 0);
    let record = capture(&bound);
    assert_eq!(record.summary().state, "interrupted");
    assert_eq!(record.summary().successful_candidates, 1);
    assert_eq!(record.pending(), &["b"]);
    assert_eq!(record.baseline_payload(), Some("10"));
    assert!(matches!(
        record.terminal(),
        RecordTerminal::Interrupted {
            reason: RecordInterruption::EvaluationLimitReached
        }
    ));
}

#[test]
fn zero_quota_records_no_engine_result() {
    let record = capture(&evaluate(&EvaluationControl::new(0), 0));
    assert_eq!(record.pending(), &["a", "b"]);
    assert!(record.baseline_payload().is_none());
    assert!(record.outcomes().is_empty());
}

#[test]
fn pre_cancelled_and_expired_runs_remain_explicitly_incomplete() {
    let cancelled = EvaluationControl::new(2);
    cancelled.cancellation_token().cancel();
    let expired = EvaluationControl::new(2).with_deadline(Instant::now());
    for control in [&cancelled, &expired] {
        let record = capture(&evaluate(control, 0));
        assert_eq!(record.summary().state, "interrupted");
        assert!(record.baseline_payload().is_none());
        assert_eq!(record.pending().len(), 2);
    }
}

#[test]
fn failed_baseline_and_failed_candidate_are_not_retry_queues() {
    let baseline = capture(&evaluate(&EvaluationControl::new(2), 1));
    assert_eq!(baseline.summary().state, "failed");
    assert_eq!(baseline.summary().failed_candidates, 0);
    assert_eq!(baseline.pending().len(), 2);
    assert!(
        matches!(baseline.terminal(), RecordTerminal::Failed { scenario_id: None, error_payload } if error_payload == "baseline\nerror")
    );
    let candidate = capture(&evaluate(&EvaluationControl::new(2), 2));
    assert_eq!(candidate.summary().successful_candidates, 1);
    assert_eq!(candidate.summary().failed_candidates, 1);
    assert!(candidate.pending().is_empty());
    assert!(
        matches!(candidate.terminal(), RecordTerminal::Failed { scenario_id: Some(id), error_payload } if id == "b" && error_payload == "candidate\nerror")
    );
}

#[test]
fn encoder_failure_leaves_original_report_available() {
    let bound = evaluate(&EvaluationControl::new(2), 2);
    let result = bound.capture_record(
        &RunId::new("run").unwrap(),
        codecs(),
        |_| Err("codec refused value".to_owned()),
        |e| Ok(e.to_string()),
    );
    assert!(matches!(result, Err(ExecutionRecordError::Encoding(_))));
    assert_eq!(bound.report().state(), ExecutionState::Failed);
    assert_eq!(bound.report().evaluation().outcomes().len(), 1);
    assert_eq!(capture(&bound).summary().failed_candidates, 1);
}

#[test]
fn cancellation_after_last_return_is_not_promoted_during_capture() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut adapters = ExecutableAdapterRegistry::new();
    adapters.register(Engine { calls, fail: 0 }).unwrap();
    let control = EvaluationControl::new(2);
    let token = control.cancellation_token();
    let bound = evaluate_registered_bundle_bound(
        &bundle(10),
        &adapters,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &control,
        |update| {
            if matches!(
                update.event,
                ProgressEvent::ScenarioCompleted { index: 1, .. }
            ) {
                token.cancel();
            }
        },
    )
    .unwrap();
    let record = capture(&bound);
    assert_eq!(record.summary().state, "interrupted");
    assert_eq!(record.summary().successful_candidates, 2);
    assert!(record.pending().is_empty());
}

#[test]
fn exact_input_snapshot_precedes_baseline_call() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut adapters = ExecutableAdapterRegistry::new();
    adapters
        .register(Engine {
            calls: Arc::clone(&calls),
            fail: 0,
        })
        .unwrap();
    struct SerializationSpy(Arc<AtomicUsize>);
    impl Serialize for SerializationSpy {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            assert_eq!(
                self.0.load(Ordering::SeqCst),
                0,
                "input serialized after engine start"
            );
            serializer.serialize_i32(10)
        }
    }
    struct SpyEngine(Engine);
    impl ProspectiveEngine<SerializationSpy, i32> for SpyEngine {
        type Signature = i32;
        type Error = &'static str;
        fn baseline(&self, _: &SerializationSpy) -> Result<i32, Self::Error> {
            self.0.baseline(&10)
        }
        fn evaluate(&self, _: &SerializationSpy, i: &i32) -> Result<i32, Self::Error> {
            self.0.evaluate(&10, i)
        }
    }
    impl VersionedAdapter for SpyEngine {
        fn adapter_metadata(
            &self,
        ) -> Result<AdapterMetadata, prospect_adapter::AdapterMetadataError> {
            self.0.adapter_metadata()
        }
    }
    let old = bundle(10);
    let spy_bundle = ScenarioBundle::new(
        "experiment.record_fixture",
        old.adapter().clone(),
        Some(7),
        SerializationSpy(Arc::clone(&calls)),
        old.scenarios().to_vec(),
        None,
        None,
    )
    .unwrap();
    let mut spy_adapters = ExecutableAdapterRegistry::new();
    spy_adapters
        .register(SpyEngine(Engine {
            calls: Arc::clone(&calls),
            fail: 0,
        }))
        .unwrap();
    let result = evaluate_registered_bundle_bound(
        &spy_bundle,
        &spy_adapters,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        |_| {},
    )
    .unwrap();
    assert_eq!(result.input_json(), old.canonical_json().unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(capture(&result).summary().state, "completed");
}

#[test]
fn rejects_altered_top_level_identity_and_admission_fields() {
    for (key, value) in [
        ("schema", json!("other")),
        ("evidence_kind", json!("observed")),
        ("run_id", json!(" ")),
        ("bundle_sha256", json!("0".repeat(64))),
        ("bundle_id", json!("experiment.other")),
        ("seed", json!(8)),
        ("max_evaluations", json!(1)),
        ("max_evaluations", json!(2.0)),
    ] {
        assert!(mutate(|v| v[key] = value).is_err(), "accepted {key}");
    }
}

#[test]
fn rejects_unknown_missing_duplicate_or_noncanonical_fields() {
    assert!(mutate(|v| v["unrecognized"] = json!(true)).is_err());
    assert!(
        mutate(|v| {
            v.as_object_mut().unwrap().remove("seed");
        })
        .is_err()
    );
    let record = record();
    let original = record.canonical_json();
    let duplicate = original.replacen("{", "{\"run_id\":\"other\",", 1);
    for payload in [
        duplicate,
        format!("{original}\n"),
        original[..original.len() - 1].to_owned(),
        "[]".to_owned(),
    ] {
        assert!(
            ExecutionRecord::verify_against_bundle(&payload, &bundle(10).canonical_json().unwrap())
                .is_err()
        );
    }
}

#[test]
fn rejects_unknown_or_noncanonical_nested_adapter_metadata() {
    for edit in [
        ("adapter_id", json!("prospect.other")),
        ("contract_version", json!({"major":2,"minor":0})),
        ("contract_version", json!({"major":0,"minor":0})),
        (
            "upstream",
            json!({"component":"memorithm.fixture","revision":"different"}),
        ),
        ("capabilities", json!([])),
    ] {
        assert!(mutate(|v| v["adapter"][edit.0] = edit.1).is_err());
    }
    assert!(mutate(|v| v["adapter"]["extra"] = json!(1)).is_err());
}

#[test]
fn rejects_prefix_suffix_gaps_duplicates_and_partial_completion() {
    assert!(mutate(|v| v["outcomes"][0]["scenario_id"] = json!("b")).is_err());
    assert!(mutate(|v| v["outcomes"][1]["scenario_id"] = json!("a")).is_err());
    assert!(
        mutate(|v| {
            let _ = v["outcomes"].as_array_mut().unwrap().pop();
        })
        .is_err()
    );
    assert!(mutate(|v| v["pending"] = json!(["b"])).is_err());
    assert!(mutate(|v| v["baseline"] = Value::Null).is_err());
    let partial = capture(&evaluate(&EvaluationControl::new(1), 0));
    let mut value: Value = serde_json::from_str(partial.canonical_json()).unwrap();
    value["terminal"] = json!({"state":"completed"});
    assert!(
        ExecutionRecord::verify_against_bundle(
            &canonical(&value).unwrap(),
            &bundle(10).canonical_json().unwrap()
        )
        .is_err()
    );
}

#[test]
fn rejects_impossible_terminal_details() {
    assert!(mutate(|v| v["terminal"] = json!({"state":"failed","scenario_id":null,"error_payload":"x"})).is_err());
    assert!(
        mutate(|v| v["terminal"] = json!({"state":"failed","scenario_id":"b","error_payload":"x"}))
            .is_err()
    );
    assert!(
        mutate(
            |v| v["terminal"] = json!({"state":"interrupted","reason":"evaluation_limit_reached"})
        )
        .is_err()
    );
    assert!(
        mutate(|v| v["terminal"] = json!({"state":"interrupted","reason":"deadline_reached"}))
            .is_err()
    );
    assert!(
        mutate(|v| v["terminal"] = json!({"state":"completed","error_payload":"unexpected"}))
            .is_err()
    );
}

#[test]
fn failed_candidate_cannot_also_be_pending() {
    let failed = capture(&evaluate(&EvaluationControl::new(2), 2));
    let mut value: Value = serde_json::from_str(failed.canonical_json()).unwrap();
    value["pending"] = json!(["b"]);
    assert!(
        ExecutionRecord::verify_against_bundle(
            &canonical(&value).unwrap(),
            &bundle(10).canonical_json().unwrap()
        )
        .is_err()
    );
}

#[test]
fn rejects_invalid_codecs_and_oversized_encoded_values() {
    assert!(PayloadCodecs::new("", "fixture.text.v1").is_err());
    assert!(mutate(|v| v["codecs"]["signature"] = json!("unversioned")).is_err());
    let bound = evaluate(&EvaluationControl::new(2), 0);
    let result = bound.capture_record(
        &RunId::new("run").unwrap(),
        codecs(),
        |_| Ok("x".repeat(MAX_ENCODED_PAYLOAD_BYTES + 1)),
        |e| Ok(e.to_string()),
    );
    assert!(result.is_err());
    assert_eq!(bound.report().state(), ExecutionState::Completed);
}

#[test]
fn envelope_and_input_byte_limits_fail_closed() {
    let oversized = " ".repeat(MAX_RECORD_BYTES + 1);
    assert!(
        ExecutionRecord::verify_against_bundle(&oversized, &bundle(10).canonical_json().unwrap())
            .is_err()
    );
    assert!(ExecutionRecord::verify_against_bundle(record().canonical_json(), &oversized).is_err());
}

#[test]
fn canonical_format_rejects_nonfinite_json_metadata_without_decoding_payloads() {
    for invalid in ["NaN", "Infinity", "1e999"] {
        let payload = record().canonical_json().replace(
            "\"max_evaluations\":2",
            &format!("\"max_evaluations\":{invalid}"),
        );
        assert!(
            ExecutionRecord::verify_against_bundle(&payload, &bundle(10).canonical_json().unwrap())
                .is_err()
        );
    }
    // Payload text is intentionally opaque, not automatically interpreted as a number.
    let bound = evaluate(&EvaluationControl::new(2), 0);
    let record = bound
        .capture_record(
            &RunId::new("run").unwrap(),
            codecs(),
            |_| Ok("NaN".to_owned()),
            |e| Ok(e.to_string()),
        )
        .unwrap();
    assert_eq!(record.baseline_payload(), Some("NaN"));
}
