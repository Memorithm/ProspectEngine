from pathlib import Path

p = Path("crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/assembly/tests.rs")
s = p.read_text()
old = '''        &chain.child,
        &bundle_with_state(11),
'''
new = '''        &chain.child,
        &chain.child_anchor,
        &bundle_with_state(11),
'''
if s.count(old) != 1:
    raise SystemExit(f"changed-bundle child anchor drift: {s.count(old)}")
s = s.replace(old, new, 1)

# This fixture is specifically a lifecycle test, not a tamper test. After
# deliberately constructing an exact child journal with no terminal entry,
# refresh the separately retained anchor to those exact bytes so assembly can
# proceed past identity admission and prove it still rejects the open lifecycle.
old = '''    chain.child.truncate(last_line_start);
    assert!(matches!(
'''
new = '''    chain.child.truncate(last_line_start);
    chain.child_anchor = ContinuationChildAnchor::new(
        RunId::new("assembly-child").unwrap(),
        digest(chain.child.as_bytes()),
    )
    .unwrap();
    assert!(matches!(
'''
if s.count(old) != 1:
    raise SystemExit(f"missing-terminal child anchor drift: {s.count(old)}")
s = s.replace(old, new, 1)
p.write_text(s)
