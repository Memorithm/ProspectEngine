from pathlib import Path

exec(compile(Path('.ci/tmp_prepare_continuation_v6.py').read_text(), '.ci/tmp_prepare_continuation_v6.py', 'exec'))

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation.rs')
source = path.read_text()
replacements = [
    (
        '    let current_input = bundle.canonical_json().map_err(|_| {',
        '    if capture.run_id.as_str() == plan.source_run_id {\n        return Err(ContinuationError::Contract(ExecutionRecordError::Invalid(\n            "continuation child run ID must differ from parent",\n        )));\n    }\n    let current_input = bundle.canonical_json().map_err(|_| {',
    ),
    (
        '    for (entry_index, raw) in child.split_inclusive(\'\\n\').enumerate() {\n        if !raw.ends_with(\'\\n\') { return invalid("unterminated continuation entry"); }',
        '    for (entry_index, raw) in child.split_inclusive(\'\\n\').enumerate() {\n        if entry_index >= MAX_CONTINUATION_ENTRIES { return invalid("too many continuation entries"); }\n        if raw.len() > MAX_JOURNAL_ENTRY_BYTES { return invalid("continuation entry exceeds byte limit"); }\n        if !raw.ends_with(\'\\n\') { return invalid("unterminated continuation entry"); }',
    ),
    (
        '                    || value.parent.expectation_sha256 != expected.sha256()\n                    || canonical(&value.implementation)? != canonical(&expected.wire.implementation)?',
        '                    || value.parent.expectation_sha256 != expected.sha256()\n                    || value.run_id == expected.wire.anchors.run_id\n                    || canonical(&value.implementation)? != canonical(&expected.wire.implementation)?',
    ),
    (
        '                    ContinuationTerminal::Interrupted { .. } if failed.is_none() && successful < h.remaining_candidate_ids.len() => {}',
        '                    ContinuationTerminal::Interrupted { reason: RecordInterruption::EvaluationLimitReached }\n                        if failed.is_none() && successful < h.remaining_candidate_ids.len() => {}\n                    ContinuationTerminal::Interrupted { reason: RecordInterruption::Cancelled | RecordInterruption::DeadlineReached }\n                        if failed.is_none() && successful <= h.remaining_candidate_ids.len() => {}',
    ),
]
for old, new in replacements:
    count = source.count(old)
    if count != 1:
        raise SystemExit(f'v7 continuation anchor drift count={count}: {old!r}')
    source = source.replace(old, new, 1)
path.write_text(source)

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/tests.rs')
source = path.read_text()
insert_before = '''#[test]
fn child_journal_rejects_tampering_and_wrong_parent() {'''
addition = r'''#[test]
fn child_run_id_cannot_reuse_parent_identity() {
    let (journal, expected, _, _) = parent_fixture(1);
    let plan = plan(&journal, &expected);
    let baseline_calls = Arc::new(AtomicUsize::new(0));
    let candidate_calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(&baseline_calls, &candidate_calls, None);
    let mut child = Memory::default();
    let result = execute_typed_continuation(
        plan, &bundle(), &registry,
        &MetricRegistry::<i32, i32>::new(), &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child, RunId::new("parent-run").unwrap(), implementation(), codecs(),
            |s: &i32| Ok(s.to_string()), |e: &&str| Ok((*e).to_owned()),
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
        plan, &bundle(), &registry,
        &MetricRegistry::<i32, i32>::new(), &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child, RunId::new("child-run").unwrap(), implementation(), codecs(),
            |s: &i32| Ok(s.to_string()), |e: &&str| Ok((*e).to_owned()),
        ),
    ).unwrap();
    assert_eq!(run.state(), ContinuationRunState::Completed);
    let text = child.text();
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut last: ContinuationEntry = serde_json::from_str(lines.last().unwrap()).unwrap();
    last.event = ContinuationEvent::Finished {
        terminal: ContinuationTerminal::Interrupted { reason: RecordInterruption::Cancelled },
    };
    *lines.last_mut().unwrap() = canonical(&last).unwrap();
    let altered = lines.join("\n") + "\n";
    let summary = inspect_continuation_journal(
        &altered, &journal, &bundle().canonical_json().unwrap(), &expected,
    ).unwrap();
    assert_eq!(summary.state, "interrupted");
    assert_eq!(summary.successful_new_candidates, 2);
    assert_eq!(summary.never_started_candidates, 0);
    assert!(summary.terminal_recorded);
    assert!(!summary.resume_authorized);
}

'''
if source.count(insert_before) != 1:
    raise SystemExit('v7 test insertion anchor drift')
source = source.replace(insert_before, addition + insert_before, 1)
path.write_text(source)
