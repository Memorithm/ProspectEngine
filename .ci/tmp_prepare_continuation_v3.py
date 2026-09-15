from pathlib import Path

# Apply the previously reviewed integration first.
exec(compile(Path('.ci/tmp_prepare_continuation_v2.py').read_text(), '.ci/tmp_prepare_continuation_v2.py', 'exec'))

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation.rs')
source = path.read_text()
replacements = {
    'use std::io;\n\n': '',
    'use prospect_adapter::AdapterMetadata;\n': '',
    '    BoundEvaluationError, ExecutionRecordError, PayloadCodecs, RecordInterruption, canonical,\n':
        '    ExecutionRecordError, PayloadCodecs, RecordInterruption, canonical,\n',
    'use super::super::super::super::super::{\n    BundleExecutionError, ExecutableAdapterRegistry,\n};':
        'use super::super::super::super::{BundleExecutionError, ExecutableAdapterRegistry};',
    '        let entry: ParentEntry = serde_json::from_str(raw)?;':
        '        let entry: ParentEntry = serde_json::from_str(raw).map_err(ExecutionRecordError::Json)?;',
}
for old, new in replacements.items():
    if source.count(old) != 1:
        raise SystemExit(f'continuation v3 anchor drift: {old!r} count={source.count(old)}')
    source = source.replace(old, new, 1)
path.write_text(source)
