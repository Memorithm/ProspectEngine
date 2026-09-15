from pathlib import Path

exec(compile(Path('.ci/tmp_prepare_continuation_v3.py').read_text(), '.ci/tmp_prepare_continuation_v3.py', 'exec'))

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation.rs')
source = path.read_text()
replacements = {
    '    Initialized {\n        header: ContinuationHeader,\n    },':
        '    Initialized {\n        header: Box<ContinuationHeader>,\n    },',
    '    session.emit(ContinuationEvent::Initialized { header });':
        '    session.emit(ContinuationEvent::Initialized { header: Box::new(header) });',
    '    let mut entries = 0usize;\n    for raw in child.split_inclusive(\'\\n\') {':
        '    let mut verified_entries = 0usize;\n    for (entry_index, raw) in child.split_inclusive(\'\\n\').enumerate() {',
    '        if canonical(&entry)? != line || entry.sequence != entries || entry.previous_sha256 != previous {':
        '        if canonical(&entry)? != line || entry.sequence != entry_index || entry.previous_sha256 != previous {',
    '            ContinuationEvent::Initialized { header: value } if entries == 0 => {':
        '            ContinuationEvent::Initialized { header: value } if entry_index == 0 => {\n                let value = *value;',
    '        entries += 1;\n    }':
        '        verified_entries = entry_index + 1;\n    }',
}
for old, new in replacements.items():
    count = source.count(old)
    if count != 1:
        raise SystemExit(f'v4 continuation anchor drift: {old!r} count={count}')
    source = source.replace(old, new, 1)
path.write_text(source)

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/tests.rs')
source = path.read_text()
old = '        let mut changed_bundle = bundle();'
if source.count(old) != 1:
    raise SystemExit('v4 unused-mut anchor drift')
source = source.replace(old, '        let changed_bundle = bundle();', 1)
old = '''    execute_typed_continuation(
        plan,
        &bundle(),
        &registry,'''
# The tamper test is the last bare invocation; target it through its following assertion context.
needle = '''    execute_typed_continuation(
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
    let text = child.text();'''
replacement = '''    let run = execute_typed_continuation(
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
    let text = child.text();'''
if source.count(needle) != 1:
    raise SystemExit(f'v4 must-use anchor drift count={source.count(needle)}')
source = source.replace(needle, replacement, 1)
path.write_text(source)
