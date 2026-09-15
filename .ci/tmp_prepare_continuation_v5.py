from pathlib import Path

exec(compile(Path('.ci/tmp_prepare_continuation_v4.py').read_text(), '.ci/tmp_prepare_continuation_v4.py', 'exec'))

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation.rs')
source = path.read_text()
old = "    let mut verified_entries = 0usize;\n    for (entry_index, raw) in child.split_inclusive('\\n').enumerate() {"
new = "    for (entry_index, raw) in child.split_inclusive('\\n').enumerate() {"
if source.count(old) != 1:
    raise SystemExit(f'v5 loop declaration anchor drift count={source.count(old)}')
source = source.replace(old, new, 1)
old = "        verified_entries = entry_index + 1;\n    }"
if source.count(old) != 1:
    raise SystemExit(f'v5 loop assignment anchor drift count={source.count(old)}')
source = source.replace(old, "    }", 1)
path.write_text(source)
