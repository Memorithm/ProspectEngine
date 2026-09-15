from pathlib import Path
p = Path('crates/prospect-scenario/src/multi_trace.rs')
s = p.read_text()
old = '''        let empty = BatchResult {
            baseline: 10,
            outcomes: vec![],
        };'''
new = '''        let empty: BatchResult<i32, i32> = BatchResult {
            baseline: 10,
            outcomes: vec![],
        };'''
if s.count(old) != 1:
    raise SystemExit(f'multi-trace type-fix anchor drift: {s.count(old)}')
p.write_text(s.replace(old, new, 1))
