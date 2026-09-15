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
p.write_text(s.replace(old, new, 1))
