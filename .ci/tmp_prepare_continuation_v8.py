from pathlib import Path

# Start from the reviewed integration/import corrections only.
exec(compile(Path('.ci/tmp_prepare_continuation_v3.py').read_text(), '.ci/tmp_prepare_continuation_v3.py', 'exec'))

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation.rs')
source = path.read_text()

def one(old: str, new: str) -> None:
    global source
    count = source.count(old)
    if count != 1:
        raise SystemExit(f'v8 continuation anchor drift count={count}: {old!r}')
    source = source.replace(old, new, 1)

one('    Initialized { header: ContinuationHeader },',
    '    Initialized { header: Box<ContinuationHeader> },')
one('    session.emit(ContinuationEvent::Initialized { header });',
    '    session.emit(ContinuationEvent::Initialized { header: Box::new(header) });')
one("    let mut entries = 0usize;\n    for raw in child.split_inclusive('\\n') {",
    "    for (entry_index, raw) in child.split_inclusive('\\n').enumerate() {")
one('entry.sequence != entries', 'entry.sequence != entry_index')
one('ContinuationEvent::Initialized { header: value } if entries == 0 => {',
    'ContinuationEvent::Initialized { header: value } if entry_index == 0 => {\n                let value = *value;')
one('        entries += 1;\n', '')
one('else { match terminal {', 'else { match &terminal {')
one('    Ok(ContinuationJournalSummary {',
    '    let occupied = if failed.is_some() || active.is_some() { 1 } else { 0 };\n    let never_started = header.remaining_candidate_ids.len().saturating_sub(successful + occupied);\n    let terminal_recorded = terminal.is_some();\n    Ok(ContinuationJournalSummary {')
one('        never_started_candidates: header.remaining_candidate_ids.len().saturating_sub(successful + usize::from(failed.is_some() || active.is_some())),\n        terminal_recorded: terminal.is_some(),',
    '        never_started_candidates: never_started,\n        terminal_recorded,')
# Child identity must be distinct and the inspector enforces the same invariant.
one('    let current_input = bundle.canonical_json().map_err(|_| {',
    '    if capture.run_id.as_str() == plan.source_run_id {\n        return Err(ContinuationError::Contract(ExecutionRecordError::Invalid(\n            "continuation child run ID must differ from parent",\n        )));\n    }\n    let current_input = bundle.canonical_json().map_err(|_| {')
one("    for (entry_index, raw) in child.split_inclusive('\\n').enumerate() {\n        if !raw.ends_with('\\n') { return invalid(\"unterminated continuation entry\"); }",
    "    for (entry_index, raw) in child.split_inclusive('\\n').enumerate() {\n        if entry_index >= MAX_CONTINUATION_ENTRIES { return invalid(\"too many continuation entries\"); }\n        if raw.len() > MAX_JOURNAL_ENTRY_BYTES { return invalid(\"continuation entry exceeds byte limit\"); }\n        if !raw.ends_with('\\n') { return invalid(\"unterminated continuation entry\"); }")
one('                    || value.parent.expectation_sha256 != expected.sha256()\n                    || canonical(&value.implementation)? != canonical(&expected.wire.implementation)?',
    '                    || value.parent.expectation_sha256 != expected.sha256()\n                    || value.run_id == expected.wire.anchors.run_id\n                    || canonical(&value.implementation)? != canonical(&expected.wire.implementation)?')
one('                    ContinuationTerminal::Interrupted { .. } if failed.is_none() && successful < h.remaining_candidate_ids.len() => {}',
    '                    ContinuationTerminal::Interrupted { reason: RecordInterruption::EvaluationLimitReached }\n                        if failed.is_none() && successful < h.remaining_candidate_ids.len() => {}\n                    ContinuationTerminal::Interrupted { reason: RecordInterruption::Cancelled | RecordInterruption::DeadlineReached }\n                        if failed.is_none() && successful <= h.remaining_candidate_ids.len() => {}')
path.write_text(source)

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/tests.rs')
source = path.read_text()

def test_one(old: str, new: str) -> None:
    global source
    count = source.count(old)
    if count != 1:
        raise SystemExit(f'v8 test anchor drift count={count}: {old[:100]!r}')
    source = source.replace(old, new, 1)

test_one('        let mut changed_bundle = bundle();', '        let changed_bundle = bundle();')
needle = '''    execute_typed_continuation(
        plan, &bundle(), &registry,
        &MetricRegistry::<i32, i32>::new(), &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child, RunId::new("child-run").unwrap(), implementation(), codecs(),
            |s: &i32| Ok(s.to_string()), |e: &&str| Ok((*e).to_owned()),
        ),
    ).unwrap();
    let text = child.text();'''
replacement = '''    let run = execute_typed_continuation(
        plan, &bundle(), &registry,
        &MetricRegistry::<i32, i32>::new(), &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(2),
        ContinuationCapture::new(
            &mut child, RunId::new("child-run").unwrap(), implementation(), codecs(),
            |s: &i32| Ok(s.to_string()), |e: &&str| Ok((*e).to_owned()),
        ),
    ).unwrap();
    assert_eq!(run.state(), ContinuationRunState::Completed);
    let text = child.text();'''
test_one(needle, replacement)
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
test_one(insert_before, addition + insert_before)
path.write_text(source)
