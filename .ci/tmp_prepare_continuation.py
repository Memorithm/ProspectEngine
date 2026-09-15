from pathlib import Path
import hashlib


def blob_sha(path: str) -> str:
    raw = Path(path).read_bytes()
    return hashlib.sha1(f"blob {len(raw)}\0".encode() + raw).hexdigest()


def replace(path: str, old: str, new: str, count: int = 1) -> None:
    p = Path(path)
    source = p.read_text()
    actual = source.count(old)
    if actual != count:
        raise SystemExit(f"anchor drift {path}: expected {count}, got {actual}: {old[:100]!r}")
    p.write_text(source.replace(old, new, count))


recovery = Path("crates/prospect-dispatch/src/execution/record/journal/recovery.rs")
if blob_sha(str(recovery)) != "2fbd2523f51d5f56a00523650722ba47ed6f5262":
    raise SystemExit("recovery.rs base drift")
source = recovery.read_text()
recovery.write_text(source.replace(
    "//! Read-only restart admission against caller-trusted external expectations.\n",
    "//! Read-only restart admission against caller-trusted external expectations.\n\npub mod continuation;\n",
    1,
))

path = "crates/prospect-dispatch/src/execution/record/journal/recovery/continuation.rs"
replace(
    path,
    "        adapter: serde_json::to_value(&adapter)?,",
    "        adapter: serde_json::to_value(&adapter).map_err(ExecutionRecordError::Json)?,",
)
replace(
    path,
    "    let state = if active.is_some() { \"unknown_call_result\" }\n        else if failed.is_some() && terminal.is_none() { \"open_after_failure\" }\n        else { match terminal {\n            Some(ContinuationTerminal::Completed) => \"completed\",\n            Some(ContinuationTerminal::Interrupted { .. }) => \"interrupted\",\n            Some(ContinuationTerminal::Failed) => \"failed\",\n            None => \"open_after_return\",\n        }};\n    Ok(ContinuationJournalSummary {",
    "    let state = if active.is_some() { \"unknown_call_result\" }\n        else if failed.is_some() && terminal.is_none() { \"open_after_failure\" }\n        else { match &terminal {\n            Some(ContinuationTerminal::Completed) => \"completed\",\n            Some(ContinuationTerminal::Interrupted { .. }) => \"interrupted\",\n            Some(ContinuationTerminal::Failed) => \"failed\",\n            None => \"open_after_return\",\n        }};\n    let occupied = if failed.is_some() || active.is_some() { 1 } else { 0 };\n    let never_started = header.remaining_candidate_ids.len().saturating_sub(successful + occupied);\n    let terminal_recorded = terminal.is_some();\n    Ok(ContinuationJournalSummary {",
)
replace(
    path,
    "        never_started_candidates: header.remaining_candidate_ids.len().saturating_sub(successful + usize::from(failed.is_some() || active.is_some())),\n        terminal_recorded: terminal.is_some(),",
    "        never_started_candidates: never_started,\n        terminal_recorded,",
)

path = "crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/tests.rs"
p = Path(path)
source = p.read_text()
start = source.index("#[test]\nfn unknown_parent_call_and_effectful_semantics_never_form_a_plan() {")
end = source.index("\n#[test]\nfn child_storage_failure_before_header_causes_zero_domain_calls()", start)
replacement = '''#[test]
fn torn_parent_and_effectful_semantics_never_form_a_plan() {
    let (clean, expected_clean, _, _) = parent_fixture(1);
    let mut torn = clean.clone();
    torn.push_str("{\\\"sequence\\\":999");
    let torn_expected = RestartExpectations::new(
        RestartAnchors::new(
            &RunId::new("parent-run").unwrap(), &digest(torn.as_bytes()),
            &digest(bundle().canonical_json().unwrap().as_bytes()),
            &expected_clean.wire.anchors.adapter_sha256,
        ).unwrap(),
        implementation(), codecs(), RestartSemantics::PureIndependent,
    ).unwrap();
    assert!(matches!(
        prepare_typed_continuation::<_, _, i32, _>(
            &torn, &bundle(), &torn_expected, &"b".repeat(64),
            |p| p.parse::<i32>().map_err(|e| e.to_string()),
        ),
        Err(ContinuationError::Blocked(_))
    ));
    let effectful = RestartExpectations::new(
        expected_clean.wire.anchors.clone(), implementation(), codecs(),
        RestartSemantics::RequiresReconciliation,
    ).unwrap();
    assert!(matches!(
        prepare_typed_continuation::<_, _, i32, _>(
            &clean, &bundle(), &effectful, &"b".repeat(64),
            |p| p.parse::<i32>().map_err(|e| e.to_string()),
        ),
        Err(ContinuationError::Blocked(_))
    ));
}
'''
p.write_text(source[:start] + replacement + source[end:])
