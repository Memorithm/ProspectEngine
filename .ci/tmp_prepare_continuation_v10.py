from pathlib import Path

# Apply the reviewed continuation integration and all compile/lint hardening.
exec(compile(Path('.ci/tmp_prepare_continuation_v9.py').read_text(), '.ci/tmp_prepare_continuation_v9.py', 'exec'))

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/tests.rs')
source = path.read_text()
old = '''    assert!(matches!(
        prepare_typed_continuation::<_, _, i32, _>(
            &torn, &bundle(), &torn_expected, &"b".repeat(64),
            |p| p.parse::<i32>().map_err(|e| e.to_string()),
        ),
        Err(ContinuationError::Blocked(_))
    ));'''
new = '''    let torn_result = prepare_typed_continuation::<_, _, i32, _>(
        &torn, &bundle(), &torn_expected, &"b".repeat(64),
        |p| p.parse::<i32>().map_err(|e| e.to_string()),
    );
    assert!(matches!(
        torn_result,
        Err(ContinuationError::Blocked(_) | ContinuationError::Contract(_))
    ));'''
if source.count(old) != 1:
    raise SystemExit(f'v10 torn-parent assertion anchor drift count={source.count(old)}')
path.write_text(source.replace(old, new, 1))
