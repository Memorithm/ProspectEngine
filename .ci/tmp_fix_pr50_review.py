from pathlib import Path

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation.rs')
source = path.read_text()
old = '''use super::super::{
    CallTarget as ParentCallTarget, EngineIdentity, Entry as ParentEntry, Event as ParentEvent,
    JournalError, JournalSink, MAX_JOURNAL_BYTES, MAX_JOURNAL_ENTRY_BYTES,
};'''
new = '''use super::super::{
    CallTarget as ParentCallTarget, EngineIdentity, Entry as ParentEntry, Event as ParentEvent,
    JournalError, JournalSink, MAX_JOURNAL_BYTES, MAX_JOURNAL_ENTRY_BYTES,
    inspect_execution_journal,
};'''
if source.count(old) != 1:
    raise SystemExit(f'PR50 import anchor drift: {source.count(old)}')
source = source.replace(old, new, 1)

old = '''    let bundle = super::super::super::parse_bundle(bundle_json)?;
    let mut previous = None;'''
new = '''    let bundle = super::super::super::parse_bundle(bundle_json)?;
    let parent_summary = inspect_execution_journal(parent, bundle_json)?;
    if parent_summary.incomplete_tail_bytes != 0
        || parent_summary.failed_call.is_some()
        || parent_summary.unknown_call_result.is_some()
        || !parent_summary.baseline_succeeded
        || parent_summary.never_started_candidates == 0
        || parent_summary.successful_candidates + parent_summary.never_started_candidates
            != bundle.scenarios().len()
    {
        return invalid("continuation parent lifecycle is not admissible");
    }
    let mut previous = None;'''
if source.count(old) != 1:
    raise SystemExit(f'PR50 parent-inspection anchor drift: {source.count(old)}')
source = source.replace(old, new, 1)

old = '''                let restored = value.restored_candidate_ids.len();
                if restored > bundle.scenarios().len()
                    || bundle.scenarios()[..restored]'''
new = '''                let restored = value.restored_candidate_ids.len();
                if restored != parent_summary.successful_candidates
                    || value.remaining_candidate_ids.len()
                        != parent_summary.never_started_candidates
                    || restored > bundle.scenarios().len()
                    || bundle.scenarios()[..restored]'''
if source.count(old) != 1:
    raise SystemExit(f'PR50 split anchor drift: {source.count(old)}')
source = source.replace(old, new, 1)

old = '''                    ContinuationTerminal::Interrupted {
                        reason: RecordInterruption::EvaluationLimitReached,
                    } if failed.is_none() && successful < h.remaining_candidate_ids.len() => {}
                    ContinuationTerminal::Interrupted {
                        reason: RecordInterruption::Cancelled | RecordInterruption::DeadlineReached,
                    } if failed.is_none() && successful <= h.remaining_candidate_ids.len() => {}'''
new = '''                    ContinuationTerminal::Interrupted {
                        reason: RecordInterruption::EvaluationLimitReached,
                    } if failed.is_none()
                        && successful == h.max_evaluations
                        && successful < h.remaining_candidate_ids.len() => {}
                    ContinuationTerminal::Interrupted {
                        reason: RecordInterruption::Cancelled,
                    } if failed.is_none() && successful <= h.remaining_candidate_ids.len() => {}
                    ContinuationTerminal::Interrupted {
                        reason: RecordInterruption::DeadlineReached,
                    } if failed.is_none()
                        && h.deadline_configured
                        && successful <= h.remaining_candidate_ids.len() => {}'''
if source.count(old) != 1:
    raise SystemExit(f'PR50 terminal anchor drift: {source.count(old)}')
source = source.replace(old, new, 1)
path.write_text(source)

# Add adversarial regressions using coherently re-emitted child journals.
tests = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/tests.rs')
source = tests.read_text()
anchor = '''fn plan(journal: &str, expected: &RestartExpectations) -> TypedContinuationPlan<i32, i32> {
    prepare_typed_continuation(journal, &bundle(), expected, &"b".repeat(64), |payload| {
        payload.parse::<i32>().map_err(|e| e.to_string())
    })
    .unwrap()
}
'''
addition = r'''
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
        .map(|line| serde_json::from_str::<ContinuationEntry>(line).unwrap().event)
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
'''
if source.count(anchor) != 1:
    raise SystemExit(f'PR50 helper insertion anchor drift: {source.count(anchor)}')
source = source.replace(anchor, anchor + addition, 1)

insert_before = '''#[test]
fn child_journal_rejects_tampering_and_wrong_parent() {'''
regressions = r'''
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
    assert!(inspect_continuation_journal(
        &forged,
        &journal,
        &bundle().canonical_json().unwrap(),
        &expected,
    )
    .is_err());
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
    assert!(inspect_continuation_journal(
        &forged,
        &journal,
        &bundle().canonical_json().unwrap(),
        &expected,
    )
    .is_err());
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
    assert!(inspect_continuation_journal(
        &forged,
        &journal,
        &bundle().canonical_json().unwrap(),
        &expected,
    )
    .is_err());
}

'''
if source.count(insert_before) != 1:
    raise SystemExit(f'PR50 regression insertion anchor drift: {source.count(insert_before)}')
source = source.replace(insert_before, regressions + insert_before, 1)
tests.write_text(source)
