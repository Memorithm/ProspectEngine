from pathlib import Path

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/assembly.rs')
source = path.read_text()
old = '''use super::{
    ContinuationEntry, ContinuationError, ContinuationEvent, RestartExpectations,
    inspect_continuation_journal, prepare_typed_continuation,
};'''
new = '''use super::{
    ContinuationEntry, ContinuationError, ContinuationEvent, inspect_continuation_journal,
    prepare_typed_continuation,
};
use super::super::RestartExpectations;'''
if source.count(old) != 1:
    raise SystemExit(f'assembly import anchor drift: {source.count(old)}')
path.write_text(source.replace(old, new, 1))

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/assembly/tests.rs')
source = path.read_text()
old = '''use super::super::{
    ContinuationCapture, EngineIdentity, JournalCapture, JournalSink, PayloadCodecs,
    RestartAnchors, RestartSemantics, evaluate_registered_bundle_journaled,
    execute_typed_continuation,
};
use super::super::super::super::{canonical, digest};'''
new = '''use super::super::{ContinuationCapture, execute_typed_continuation};
use super::super::super::{RestartAnchors, RestartSemantics};
use super::super::super::super::{
    EngineIdentity, JournalSink, evaluate_registered_bundle_journaled,
};
use super::super::super::super::evaluation::JournalCapture;
use super::super::super::super::super::{PayloadCodecs, canonical, digest};'''
if source.count(old) != 1:
    raise SystemExit(f'assembly test import anchor drift: {source.count(old)}')
path.write_text(source.replace(old, new, 1))
