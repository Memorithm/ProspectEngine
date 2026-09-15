from pathlib import Path

p = Path("crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/assembly.rs")
s = p.read_text()

# RunId is needed for the separately retained child anchor.
old = "use prospect_core::Scenario;\nuse prospect_scenario::{BatchResult, ScenarioOutcome};"
new = "use prospect_core::Scenario;\nuse prospect_evidence::RunId;\nuse prospect_scenario::{BatchResult, ScenarioOutcome};"
if s.count(old) != 1:
    raise SystemExit("assembly RunId import anchor drift")
s = s.replace(old, new, 1)

anchor = '''/// Complete parent/child software reconstruction and its exact source identities.
///
/// The contained batch is structurally complete and may enter the existing scoring
/// APIs. This type does not itself establish scientific validity or hardware trust.
'''
insert = '''/// Separately retained identity of the exact child journal admitted for assembly.
///
/// This anchor must come from a trust boundary independent of the mutable child
/// journal bytes being inspected. Constructing it from those same untrusted bytes
/// immediately before assembly defeats its purpose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuationChildAnchor {
    run_id: RunId,
    journal_sha256: String,
}

impl ContinuationChildAnchor {
    pub fn new(
        run_id: RunId,
        journal_sha256: impl Into<String>,
    ) -> Result<Self, ExecutionRecordError> {
        let journal_sha256 = journal_sha256.into();
        if journal_sha256.len() != 64
            || !journal_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ExecutionRecordError::Invalid(
                "invalid continuation child SHA-256",
            ));
        }
        Ok(Self {
            run_id,
            journal_sha256,
        })
    }

    #[must_use]
    pub const fn run_id(&self) -> &RunId {
        &self.run_id
    }

    #[must_use]
    pub fn journal_sha256(&self) -> &str {
        &self.journal_sha256
    }
}

/// Complete parent/child software reconstruction and its exact source identities.
///
/// The contained batch is structurally complete and may enter the existing scoring
/// APIs. This type does not itself establish scientific validity or hardware trust.
'''
if s.count(anchor) != 1:
    raise SystemExit("assembly child anchor insertion drift")
s = s.replace(anchor, insert, 1)

old = '''    Parent(ContinuationError),
    ChildNotCompleted(&'static str),
    Decode(String),
    ScenarioPartitionMismatch,
'''
new = '''    Parent(ContinuationError),
    ChildAnchorMismatch,
    ChildNotCompleted(&'static str),
    Decode(String),
    ScenarioPartitionMismatch,
'''
if s.count(old) != 1:
    raise SystemExit("assembly error enum drift")
s = s.replace(old, new, 1)

old = '''            Self::Parent(error) => error.fmt(formatter),
            Self::ChildNotCompleted(state) => {
'''
new = '''            Self::Parent(error) => error.fmt(formatter),
            Self::ChildAnchorMismatch => formatter.write_str(
                "continuation child does not match the separately retained anchor",
            ),
            Self::ChildNotCompleted(state) => {
'''
if s.count(old) != 1:
    raise SystemExit("assembly display drift")
s = s.replace(old, new, 1)

old = '''            Self::Parent(error) => Some(error),
            Self::ChildNotCompleted(_) | Self::Decode(_) | Self::ScenarioPartitionMismatch => None,
'''
new = '''            Self::Parent(error) => Some(error),
            Self::ChildAnchorMismatch
            | Self::ChildNotCompleted(_)
            | Self::Decode(_)
            | Self::ScenarioPartitionMismatch => None,
'''
if s.count(old) != 1:
    raise SystemExit("assembly error source drift")
s = s.replace(old, new, 1)

old = '''    child_journal: &str,
    bundle: &ScenarioBundle<State, I>,
'''
new = '''    child_journal: &str,
    child_anchor: &ContinuationChildAnchor,
    bundle: &ScenarioBundle<State, I>,
'''
if s.count(old) != 1:
    raise SystemExit("assembly signature drift")
s = s.replace(old, new, 1)

old = '''{
    let bundle_json = bundle.canonical_json().map_err(|_| {
'''
new = '''{
    if digest(child_journal.as_bytes()) != child_anchor.journal_sha256() {
        return Err(ContinuationAssemblyError::ChildAnchorMismatch);
    }

    let bundle_json = bundle.canonical_json().map_err(|_| {
'''
if s.count(old) != 1:
    raise SystemExit("assembly early child digest gate drift")
s = s.replace(old, new, 1)

old = '''    let summary =
        inspect_continuation_journal(child_journal, parent_journal, &bundle_json, expected)?;
    if summary.state != "completed"
'''
new = '''    let summary =
        inspect_continuation_journal(child_journal, parent_journal, &bundle_json, expected)?;
    if summary.child_run_id != child_anchor.run_id().as_str() {
        return Err(ContinuationAssemblyError::ChildAnchorMismatch);
    }
    if summary.state != "completed"
'''
if s.count(old) != 1:
    raise SystemExit("assembly child run gate drift")
s = s.replace(old, new, 1)

# Strengthen rustdoc.
old = '''/// The child must pass `inspect_continuation_journal` with state `completed`, a
/// recorded terminal, no failed/unknown/never-started suffix calls, and exactly the
/// remaining candidate count from the reconstructed parent plan.
'''
new = '''/// Before either journal is decoded, the exact child bytes must match the separately
/// retained [`ContinuationChildAnchor`]. The child must then pass
/// `inspect_continuation_journal` with the anchored child run ID, state `completed`,
/// a recorded terminal, no failed/unknown/never-started suffix calls, and exactly the
/// remaining candidate count from the reconstructed parent plan.
'''
if s.count(old) != 1:
    raise SystemExit("assembly rustdoc drift")
s = s.replace(old, new, 1)
p.write_text(s)

# Update tests to supply an independent child anchor and add coherent-rehash regressions.
p = Path("crates/prospect-dispatch/src/execution/record/journal/recovery/continuation/assembly/tests.rs")
s = p.read_text()

old = '''struct Chain {
    parent: String,
    child: String,
    expected: RestartExpectations,
'''
new = '''struct Chain {
    parent: String,
    child: String,
    child_anchor: ContinuationChildAnchor,
    expected: RestartExpectations,
'''
if s.count(old) != 1:
    raise SystemExit("test Chain anchor drift")
s = s.replace(old, new, 1)

old = '''    Chain {
        parent,
        child: child_sink.text(),
        expected,
'''
new = '''    let child = child_sink.text();
    let child_anchor = ContinuationChildAnchor::new(
        RunId::new("assembly-child").unwrap(),
        digest(child.as_bytes()),
    )
    .unwrap();

    Chain {
        parent,
        child,
        child_anchor,
        expected,
'''
if s.count(old) != 1:
    raise SystemExit("test child anchor construction drift")
s = s.replace(old, new, 1)

old = '''        &chain.child,
        &bundle_with_state(10),
'''
new = '''        &chain.child,
        &chain.child_anchor,
        &bundle_with_state(10),
'''
# This appears in helper plus several direct calls; replace all intended occurrences.
if s.count(old) < 3:
    raise SystemExit(f"test assemble call anchors drift: {s.count(old)}")
s = s.replace(old, new)

# The changed-bundle and wrong-artifact calls have the same child but different bundle; above catches both.
# Add a helper to coherently rehash child entries.
test_anchor = '''#[test]
fn altered_child_bytes_fail_before_decoding_a_batch() {'''
insert = r'''fn coherently_rehash_child_with_payload(child: &str, from: &str, to: &str) -> String {
    let mut previous = None;
    let mut output = String::new();
    for (index, raw) in child.split_terminator('\n').enumerate() {
        let mut entry: ContinuationEntry = serde_json::from_str(raw).unwrap();
        entry.sequence = index;
        entry.previous_sha256 = previous.clone();
        if let ContinuationEvent::CallSucceeded { payload, .. } = &mut entry.event
            && payload == from
        {
            *payload = to.to_owned();
        }
        let line = canonical(&entry).unwrap();
        previous = Some(digest(line.as_bytes()));
        output.push_str(&line);
        output.push('\n');
    }
    output
}

#[test]
fn coherent_child_rehash_is_rejected_by_external_anchor_before_decode() {
    let mut chain = chain(2);
    chain.child = coherently_rehash_child_with_payload(&chain.child, "12", "99");
    let mut decode_calls = 0usize;
    let result = assemble_completed_continuation(
        &chain.parent,
        &chain.child,
        &chain.child_anchor,
        &bundle_with_state(10),
        &chain.expected,
        &"b".repeat(64),
        |payload| {
            decode_calls += 1;
            payload.parse::<i32>().map_err(|error| error.to_string())
        },
    );
    assert!(matches!(
        result,
        Err(ContinuationAssemblyError::ChildAnchorMismatch)
    ));
    assert_eq!(decode_calls, 0);
}

#[test]
fn wrong_trusted_child_run_id_is_rejected() {
    let chain = chain(2);
    let wrong_anchor = ContinuationChildAnchor::new(
        RunId::new("other-child").unwrap(),
        digest(chain.child.as_bytes()),
    )
    .unwrap();
    let result = assemble_completed_continuation(
        &chain.parent,
        &chain.child,
        &wrong_anchor,
        &bundle_with_state(10),
        &chain.expected,
        &"b".repeat(64),
        |payload| payload.parse::<i32>().map_err(|error| error.to_string()),
    );
    assert!(matches!(
        result,
        Err(ContinuationAssemblyError::ChildAnchorMismatch)
    ));
}

#[test]
fn malformed_child_anchor_digest_is_rejected_at_construction() {
    assert!(ContinuationChildAnchor::new(
        RunId::new("assembly-child").unwrap(),
        "ABC"
    )
    .is_err());
}

#[test]
fn altered_child_bytes_fail_before_decoding_a_batch() {'''
if s.count(test_anchor) != 1:
    raise SystemExit("test insertion anchor drift")
s = s.replace(test_anchor, insert, 1)

# Simple tampering now fails at external anchor, not the internal chain verifier.
old = '''        assemble(&chain),
        Err(ContinuationAssemblyError::Contract(_))
'''
new = '''        assemble(&chain),
        Err(ContinuationAssemblyError::ChildAnchorMismatch)
'''
if s.count(old) != 1:
    raise SystemExit("tamper expected error drift")
s = s.replace(old, new, 1)

p.write_text(s)
