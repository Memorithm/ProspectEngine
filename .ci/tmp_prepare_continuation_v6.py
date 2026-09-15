from pathlib import Path

# Apply only the validated integration/import fixes; subsequent edits target the
# repository's deliberately compact pre-rustfmt source representation.
exec(compile(Path('.ci/tmp_prepare_continuation_v3.py').read_text(), '.ci/tmp_prepare_continuation_v3.py', 'exec'))

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation.rs')
source = path.read_text()
replacements = [
    (
        '    Initialized { header: ContinuationHeader },',
        '    Initialized { header: Box<ContinuationHeader> },',
    ),
    (
        '    session.emit(ContinuationEvent::Initialized { header });',
        '    session.emit(ContinuationEvent::Initialized { header: Box::new(header) });',
    ),
    (
        "    let mut entries = 0usize;\n    for raw in child.split_inclusive('\\n') {",
        "    for (entry_index, raw) in child.split_inclusive('\\n').enumerate() {",
    ),
    (
        '        if canonical(&entry)? != line || entry.sequence != entries || entry.previous_sha256 != previous {',
        '        if canonical(&entry)? != line || entry.sequence != entry_index || entry.previous_sha256 != previous {',
    ),
    (
        '            ContinuationEvent::Initialized { header: value } if entries == 0 => {',
        '            ContinuationEvent::Initialized { header: value } if entry_index == 0 => {\n                let value = *value;',
    ),
    ('        entries += 1;\n', ''),
    (
        '    let state = if active.is_some() { "unknown_call_result" }\n        else if failed.is_some() && terminal.is_none() { "open_after_failure" }\n        else { match terminal {\n            Some(ContinuationTerminal::Completed) => "completed",\n            Some(ContinuationTerminal::Interrupted { .. }) => "interrupted",\n            Some(ContinuationTerminal::Failed) => "failed",\n            None => "open_after_return",\n        }};\n    Ok(ContinuationJournalSummary {',
        '    let state = if active.is_some() { "unknown_call_result" }\n        else if failed.is_some() && terminal.is_none() { "open_after_failure" }\n        else { match &terminal {\n            Some(ContinuationTerminal::Completed) => "completed",\n            Some(ContinuationTerminal::Interrupted { .. }) => "interrupted",\n            Some(ContinuationTerminal::Failed) => "failed",\n            None => "open_after_return",\n        }};\n    let occupied = if failed.is_some() || active.is_some() { 1 } else { 0 };\n    let never_started = header.remaining_candidate_ids.len().saturating_sub(successful + occupied);\n    let terminal_recorded = terminal.is_some();\n    Ok(ContinuationJournalSummary {',
    ),
    (
        '        never_started_candidates: header.remaining_candidate_ids.len().saturating_sub(successful + usize::from(failed.is_some() || active.is_some())),\n        terminal_recorded: terminal.is_some(),',
        '        never_started_candidates: never_started,\n        terminal_recorded,',
    ),
]
for old, new in replacements:
    count = source.count(old)
    if count != 1:
        raise SystemExit(f'v6 continuation anchor drift count={count}: {old!r}')
    source = source.replace(old, new, 1)
path.write_text(source)

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/tests.rs')
source = path.read_text()
old = '        let mut changed_bundle = bundle();'
if source.count(old) != 1:
    raise SystemExit(f'v6 unused-mut anchor drift count={source.count(old)}')
source = source.replace(old, '        let changed_bundle = bundle();', 1)
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
if source.count(needle) != 1:
    raise SystemExit(f'v6 must-use anchor drift count={source.count(needle)}')
source = source.replace(needle, replacement, 1)
path.write_text(source)
