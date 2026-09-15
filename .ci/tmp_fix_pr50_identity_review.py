from pathlib import Path

path = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation.rs')
source = path.read_text()
old = '''    let parent_summary = inspect_execution_journal(parent, bundle_json)?;
    if parent_summary.incomplete_tail_bytes != 0'''
new = '''    let parent_summary = inspect_execution_journal(parent, bundle_json)?;
    let parent_line = parent
        .split_terminator('\\n')
        .next()
        .ok_or(ExecutionRecordError::Invalid("continuation parent has no header"))?;
    let parent_entry: ParentEntry = serde_json::from_str(parent_line)?;
    let ParentEvent::Initialized { header: parent_header } = parent_entry.event else {
        return invalid("continuation parent has no initialized header");
    };
    if parent_summary.run_id != expected.wire.anchors.run_id
        || parent_summary.implementation != expected.wire.implementation
        || canonical(&parent_header.codecs)? != canonical(&expected.wire.codecs)?
        || digest(canonical(&parent_header.adapter)?.as_bytes())
            != expected.wire.anchors.adapter_sha256
    {
        return invalid("continuation parent trusted identity mismatch");
    }
    if parent_summary.incomplete_tail_bytes != 0'''
if source.count(old) != 1:
    raise SystemExit(f'parent identity validation anchor drift: {source.count(old)}')
path.write_text(source.replace(old, new, 1))

tests = Path('crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/tests.rs')
source = tests.read_text()
insert_before = '''#[test]
fn inspector_rejects_coherently_rehashed_child_that_invents_restored_parent_success() {'''
addition = r'''
fn forge_child_identity(
    child: &str,
    expected: &RestartExpectations,
    adapter: serde_json::Value,
) -> String {
    let mut events = parse_child_events(child);
    match &mut events[0] {
        ContinuationEvent::Initialized { header } => {
            header.parent.run_id = expected.wire.anchors.run_id.clone();
            header.parent.expectation_sha256 = expected.sha256();
            header.implementation = expected.wire.implementation.clone();
            header.codecs = expected.wire.codecs.clone();
            header.adapter = adapter;
        }
        _ => panic!("first child event must be initialized"),
    }
    emit_child_events(events)
}

#[test]
fn inspector_rejects_child_coherently_forged_to_mismatched_parent_expectations() {
    let (journal, original, _, _) = parent_fixture(1);
    let child = completed_child(&journal, &original);
    let bundle_json = bundle().canonical_json().unwrap();
    let actual_adapter = registry(
        &Arc::new(AtomicUsize::new(0)),
        &Arc::new(AtomicUsize::new(0)),
        None,
    )
    .metadata()[0]
    .clone();

    let wrong_run = RestartExpectations::new(
        RestartAnchors::new(
            &RunId::new("other-parent").unwrap(),
            &digest(journal.as_bytes()),
            &digest(bundle_json.as_bytes()),
            &digest(canonical(&actual_adapter).unwrap().as_bytes()),
        )
        .unwrap(),
        implementation(),
        codecs(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    let forged = forge_child_identity(
        &child,
        &wrong_run,
        serde_json::to_value(&actual_adapter).unwrap(),
    );
    assert!(inspect_continuation_journal(&forged, &journal, &bundle_json, &wrong_run).is_err());

    let wrong_implementation = RestartExpectations::new(
        original.wire.anchors.clone(),
        EngineIdentity::new("fixture.other_engine", &"c".repeat(40), &"d".repeat(64)).unwrap(),
        codecs(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    let forged = forge_child_identity(
        &child,
        &wrong_implementation,
        serde_json::to_value(&actual_adapter).unwrap(),
    );
    assert!(inspect_continuation_journal(
        &forged,
        &journal,
        &bundle_json,
        &wrong_implementation,
    )
    .is_err());

    let wrong_codecs = RestartExpectations::new(
        original.wire.anchors.clone(),
        implementation(),
        PayloadCodecs::new("fixture.other_i32.v1", "fixture.other_error.v1").unwrap(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    let forged = forge_child_identity(
        &child,
        &wrong_codecs,
        serde_json::to_value(&actual_adapter).unwrap(),
    );
    assert!(inspect_continuation_journal(&forged, &journal, &bundle_json, &wrong_codecs).is_err());

    let wrong_adapter = AdapterMetadata::new(
        "fixture.other_adapter",
        version(),
        None,
        vec![AdapterCapability::new("fixture.evaluate", version()).unwrap()],
    )
    .unwrap();
    let wrong_adapter_expectations = RestartExpectations::new(
        RestartAnchors::new(
            &RunId::new("parent-run").unwrap(),
            &digest(journal.as_bytes()),
            &digest(bundle_json.as_bytes()),
            &digest(canonical(&wrong_adapter).unwrap().as_bytes()),
        )
        .unwrap(),
        implementation(),
        codecs(),
        RestartSemantics::PureIndependent,
    )
    .unwrap();
    let forged = forge_child_identity(
        &child,
        &wrong_adapter_expectations,
        serde_json::to_value(&wrong_adapter).unwrap(),
    );
    assert!(inspect_continuation_journal(
        &forged,
        &journal,
        &bundle_json,
        &wrong_adapter_expectations,
    )
    .is_err());
}

'''
if source.count(insert_before) != 1:
    raise SystemExit(f'identity regression insertion anchor drift: {source.count(insert_before)}')
tests.write_text(source.replace(insert_before, addition + insert_before, 1))
