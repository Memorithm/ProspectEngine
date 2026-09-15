//! Synthetic journal fixtures; no engine is restored or executed by these tests.

use prospect_adapter::{AdapterCapability, AdapterMetadata, ContractVersion};
use prospect_bundle::{AdapterBinding, BundleScenario, ScenarioBundle};
use prospect_core::ScenarioId;
use prospect_evidence::RunId;
use serde_json::Value;

use super::*;
use super::super::{CallTarget, Emitter, Header, JournalSink, SCHEMA, Terminal};
use super::super::super::RecordInterruption;

#[derive(Default)]
struct Memory(Vec<u8>);
impl JournalSink for Memory {
    fn append_record(&mut self, record: &[u8]) -> std::io::Result<()> {
        self.0.extend_from_slice(record);
        Ok(())
    }
}
fn version() -> ContractVersion { ContractVersion::new(1, 0).unwrap() }
fn adapter() -> AdapterMetadata {
    AdapterMetadata::new("fixture.adapter", version(), None,
        vec![AdapterCapability::new("fixture.evaluate", version()).unwrap()]).unwrap()
}
fn implementation() -> EngineIdentity {
    EngineIdentity::new("fixture.engine", &"a".repeat(40), &"b".repeat(64)).unwrap()
}
fn codecs() -> PayloadCodecs { PayloadCodecs::new("fixture.i32.v1", "fixture.error.v1").unwrap() }
fn input() -> String {
    ScenarioBundle::new("fixture.recovery", AdapterBinding::from_metadata(&adapter()), Some(7), 10,
        (1..=3).map(|i| BundleScenario::new(ScenarioId::new(format!("s{i}")).unwrap(), i)).collect(),
        None, None).unwrap().canonical_json().unwrap()
}
fn target(i: usize) -> CallTarget {
    if i == 0 { CallTarget::Baseline } else { CallTarget::Scenario { id: format!("s{i}") } }
}
fn log(events: Vec<Event>, quota: usize) -> String {
    let mut memory = Memory::default();
    {
        let mut emitter = Emitter { sink: &mut memory, sequence: 0, previous: None, bytes: 0 };
        emitter.append(Event::Initialized { header: Header {
            schema: SCHEMA.into(), run_id: "external-run".into(), bundle_sha256: digest(input().as_bytes()),
            adapter: serde_json::to_value(adapter()).unwrap(), implementation: implementation(), codecs: codecs(),
            max_evaluations: quota, deadline_configured: false,
        }}).unwrap();
        for event in events { emitter.append(event).unwrap(); }
    }
    String::from_utf8(memory.0).unwrap()
}
fn successes(n: usize) -> Vec<Event> {
    (0..=n).flat_map(|i| [Event::CallStarted { target: target(i) },
        Event::CallSucceeded { target: target(i), payload: (10+i).to_string() }]).collect()
}
fn expectations(journal: &str, semantics: RestartSemantics) -> RestartExpectations {
    RestartExpectations::new(RestartAnchors::new(&RunId::new("external-run").unwrap(),
        &digest(journal.as_bytes()), &digest(input().as_bytes()),
        &digest(canonical(&adapter()).unwrap().as_bytes())).unwrap(),
        implementation(), codecs(), semantics).unwrap()
}
fn check(journal: &str) -> RestartPreflight {
    preflight_journal_restart(journal, &input(), &expectations(journal, RestartSemantics::PureIndependent), &"b".repeat(64)).unwrap()
}
fn modified_policy(journal: &str, change: impl FnOnce(&mut Value)) -> RestartExpectations {
    let original = expectations(journal, RestartSemantics::PureIndependent);
    let mut value: Value = serde_json::from_str(original.canonical_json()).unwrap();
    change(&mut value);
    RestartExpectations::from_canonical_json(&canonical(&value).unwrap()).unwrap()
}

#[test]
fn canonical_expectations_roundtrip_and_digest() {
    let journal = log(successes(1), 3);
    let policy = expectations(&journal, RestartSemantics::PureIndependent);
    let parsed = RestartExpectations::from_canonical_json(policy.canonical_json()).unwrap();
    assert_eq!(parsed.canonical_json(), policy.canonical_json());
    assert_eq!(parsed.sha256(), digest(policy.canonical_json().as_bytes()));
}

#[test]
fn clean_prefix_identifies_never_started_suffix_without_authorizing_execution() {
    let journal = log(successes(1), 3);
    let result = check(&journal);
    assert!(result.continuation_preparation_allowed);
    assert!(!result.resume_authorized);
    assert!(result.blockers.is_empty());
    assert_eq!(result.source_state, "open_after_return");
    assert_eq!(result.successful_candidates, 1);
    assert!(result.baseline_available);
    assert_eq!(result.never_started_scenario_ids, ["s2", "s3"]);
}

#[test]
fn clean_quota_interruption_allows_preparation_but_not_replay() {
    let mut events = successes(1);
    events.push(Event::Finished { terminal: Terminal::Interrupted { reason: RecordInterruption::EvaluationLimitReached }});
    let result = check(&log(events, 1));
    assert_eq!(result.source_state, "interrupted");
    assert!(result.continuation_preparation_allowed);
    assert!(!result.resume_authorized);
}

#[test]
fn header_only_has_no_restored_baseline_and_all_candidates_unstarted() {
    let result = check(&log(vec![], 3));
    assert!(!result.baseline_available);
    assert_eq!(result.successful_candidates, 0);
    assert_eq!(result.never_started_scenario_ids, ["s1", "s2", "s3"]);
    assert!(result.continuation_preparation_allowed);
    assert!(!result.resume_authorized);
}

#[test]
fn unknown_baseline_or_candidate_blocks_even_pure_engine() {
    for i in [0, 2] {
        let mut events = if i == 0 { vec![] } else { successes(1) };
        events.push(Event::CallStarted { target: target(i) });
        let result = check(&log(events, 3));
        assert!(!result.continuation_preparation_allowed);
        assert!(result.blockers.contains(&RestartBlocker::UnknownCallResult));
        assert!(!result.resume_authorized);
        // The uncertain candidate is never misrepresented as never-started.
        if i == 2 { assert_eq!(result.never_started_scenario_ids, ["s3"]); }
    }
}

#[test]
fn known_failure_requires_reconciliation_with_or_without_terminal() {
    for terminal in [false, true] {
        let mut events = successes(1);
        events.push(Event::CallStarted { target: target(2) });
        events.push(Event::CallFailed { target: target(2), error_payload: "failed".into() });
        if terminal { events.push(Event::Finished { terminal: Terminal::Failed }); }
        let result = check(&log(events, 3));
        assert_eq!(result.never_started_scenario_ids, ["s3"]);
        assert!(result.blockers.contains(&RestartBlocker::FailedCallRequiresReconciliation));
        assert!(!result.continuation_preparation_allowed);
    }
}

#[test]
fn torn_tail_blocks_and_preserves_every_source_byte() {
    let mut journal = log(successes(1), 3);
    journal.push_str("{\"sequence\":5");
    let before = journal.clone();
    let result = check(&journal);
    assert_eq!(journal, before);
    assert!(result.blockers.contains(&RestartBlocker::IncompleteTail));
    assert!(!result.continuation_preparation_allowed);
}

#[test]
fn stateful_or_effectful_declaration_blocks_a_clean_prefix() {
    let journal = log(successes(1), 3);
    let expected = expectations(&journal, RestartSemantics::RequiresReconciliation);
    let result = preflight_journal_restart(&journal, &input(), &expected, &"b".repeat(64)).unwrap();
    assert_eq!(result.blockers, [RestartBlocker::StatefulOrEffectfulEngine]);
    assert!(!result.continuation_preparation_allowed);
}

#[test]
fn complete_and_fully_returned_unclosed_runs_are_not_scheduled_again() {
    for terminal in [false, true] {
        let mut events = successes(3);
        if terminal { events.push(Event::Finished { terminal: Terminal::Completed }); }
        let result = check(&log(events, 3));
        assert!(result.never_started_scenario_ids.is_empty());
        assert!(result.blockers.contains(&RestartBlocker::NoNeverStartedCandidates));
        assert_eq!(result.blockers.contains(&RestartBlocker::AlreadyCompleted), terminal);
        assert!(!result.continuation_preparation_allowed);
    }
}

#[test]
fn external_journal_anchor_rejects_suffix_deletion_and_coherent_rehashing() {
    let journal = log(successes(2), 3);
    let policy = expectations(&journal, RestartSemantics::PureIndependent);
    let shorter = log(successes(1), 3);
    assert!(preflight_journal_restart(&shorter, &input(), &policy, &"b".repeat(64)).is_err());
    let altered = journal.replace("\"12\"", "\"99\"");
    assert!(preflight_journal_restart(&altered, &input(), &policy, &"b".repeat(64)).is_err());
}

#[test]
fn externally_anchored_input_cannot_be_substituted() {
    let journal = log(successes(1), 3);
    let policy = expectations(&journal, RestartSemantics::PureIndependent);
    let altered = input().replace("\"state\":10", "\"state\":99");
    assert_ne!(altered, input());
    assert!(preflight_journal_restart(&journal, &altered, &policy, &"b".repeat(64)).is_err());
}

#[test]
fn independently_retained_run_implementation_codec_and_adapter_are_exact() {
    let journal = log(successes(1), 3);
    let mutations: Vec<Box<dyn Fn(&mut Value)>> = vec![
        Box::new(|v| v["anchors"]["run_id"] = "other-run".into()),
        Box::new(|v| v["implementation"]["revision"] = "c".repeat(40).into()),
        Box::new(|v| v["implementation"]["component"] = "other.engine".into()),
        Box::new(|v| v["anchors"]["adapter_sha256"] = "c".repeat(64).into()),
        Box::new(|v| { v["codecs"] = serde_json::to_value(PayloadCodecs::new("other.i32", "other.error").unwrap()).unwrap(); }),
    ];
    for mutation in mutations {
        let policy = modified_policy(&journal, mutation);
        assert!(preflight_journal_restart(&journal, &input(), &policy, &"b".repeat(64)).is_err());
    }
}

#[test]
fn artifact_digest_is_checked_independently_of_log_header() {
    let journal = log(successes(1), 3);
    let policy = expectations(&journal, RestartSemantics::PureIndependent);
    for actual in ["c".repeat(64), "B".repeat(64), "short".into()] {
        assert!(preflight_journal_restart(&journal, &input(), &policy, &actual).is_err());
    }
}

#[test]
fn malformed_complete_event_is_not_admitted_by_fresh_matching_anchor() {
    let mut journal = log(successes(1), 3);
    journal.push_str("{}\n");
    assert!(preflight_journal_restart(&journal, &input(),
        &expectations(&journal, RestartSemantics::PureIndependent), &"b".repeat(64)).is_err());
}

#[test]
fn strict_expectation_parser_rejects_noncanonical_unknown_missing_and_bad_fields() {
    let journal = log(successes(1), 3);
    let policy = expectations(&journal, RestartSemantics::PureIndependent);
    let text = policy.canonical_json();
    for payload in [format!("{text}\n"), "null".into(), "{".into(), " ".repeat(MAX_RESTART_EXPECTATION_BYTES+1)] {
        assert!(RestartExpectations::from_canonical_json(&payload).is_err());
    }
    let mut value: Value = serde_json::from_str(text).unwrap();
    value["unexpected"] = true.into();
    assert!(RestartExpectations::from_canonical_json(&canonical(&value).unwrap()).is_err());
    value.as_object_mut().unwrap().remove("unexpected");
    value.as_object_mut().unwrap().remove("semantics");
    assert!(RestartExpectations::from_canonical_json(&canonical(&value).unwrap()).is_err());
    let duplicate = text.replacen("{", "{\"schema\":\"prospect.restart-expectations/v1\",", 1);
    assert!(RestartExpectations::from_canonical_json(&duplicate).is_err());
}

#[test]
fn malformed_anchor_hashes_fail_construction() {
    for hash in ["A".repeat(64), "f".repeat(63), "g".repeat(64)] {
        assert!(RestartAnchors::new(&RunId::new("run").unwrap(), &hash, &"b".repeat(64), &"c".repeat(64)).is_err());
    }
}
