//! Live execution journal: acknowledge call intent before invoking the engine.
//!
//! Entries bind canonical input, declared implementation identity and ordered
//! call events. Hash chaining checks consistency, NOT execution authenticity.
//! An unmatched call intent means an unknown result, never permission to retry.

mod evaluation;
pub use evaluation::{
    JournalCapture, JournalRun, JournalRunState, evaluate_registered_bundle_journaled,
};

use std::io;

use prospect_adapter::{AdapterMetadata, NamespacedId};
use prospect_bundle::ScenarioBundle;
use prospect_evidence::RunId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    ExecutionRecordError, MAX_RECORD_BYTES, PayloadCodecs, RecordInterruption, canonical,
    check_payload, digest, invalid, parse_bundle, validate_metadata,
};

/// Maximum admitted raw bytes in a journal (not an allocator or RSS bound).
pub const MAX_JOURNAL_BYTES: usize = MAX_RECORD_BYTES;
/// Maximum encoded entry, including its final LF. Payload escaping consumes space.
pub const MAX_JOURNAL_ENTRY_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 2 * super::MAX_RECORD_SCENARIOS + 4;
const SCHEMA: &str = "prospect.execution-journal/v1";

/// Caller-declared implementation identity. Not independent binary attestation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineIdentity {
    component: String,
    revision: String,
    artifact_sha256: String,
}

impl EngineIdentity {
    /// Require a namespaced component, lower-case Git SHA and SHA-256 digest.
    ///
    /// ```
    /// use prospect_dispatch::execution::record::journal::EngineIdentity;
    /// let identity = EngineIdentity::new("example.engine", &"a".repeat(40), &"b".repeat(64))?;
    /// assert_eq!(identity.component(), "example.engine");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(
        component: &str,
        revision: &str,
        artifact_sha256: &str,
    ) -> Result<Self, ExecutionRecordError> {
        let value = Self {
            component: component.into(),
            revision: revision.into(),
            artifact_sha256: artifact_sha256.into(),
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), ExecutionRecordError> {
        NamespacedId::new(self.component.as_str())
            .map_err(|_| ExecutionRecordError::Invalid("invalid implementation component"))?;
        for (value, length) in [(&self.revision, 40), (&self.artifact_sha256, 64)] {
            if value.len() != length
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return invalid("invalid implementation revision or artifact digest");
            }
        }
        Ok(())
    }
    /// Namespaced implementation component, as declared by the application.
    #[must_use]
    pub fn component(&self) -> &str {
        &self.component
    }
    /// Declared exact Git revision; not inferred from adapter metadata.
    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }
    /// Declared implementation-artifact digest; not measured by this module.
    #[must_use]
    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }
}

/// A single-writer destination which acknowledges an entire LF-terminated entry.
///
/// Return success only after the destination's promised persistence operation
/// succeeds. An error may follow a partial write: preserve the bytes, do not retry
/// that entry, and never reuse this sink for another run. Implementations own
/// their storage semantics; the file sink uses write_all followed by sync_all.
pub trait JournalSink {
    fn append_record(&mut self, record: &[u8]) -> io::Result<()>;
}

/// Journal or encoder failure, kept separate from an actual domain-engine error.
#[derive(Debug)]
pub enum JournalError {
    Contract(ExecutionRecordError),
    Storage(io::Error),
}
impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract(e) => e.fmt(f),
            Self::Storage(e) => write!(f, "journal storage: {e}"),
        }
    }
}
impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(e) => Some(e),
            Self::Storage(e) => Some(e),
        }
    }
}
impl From<ExecutionRecordError> for JournalError {
    fn from(value: ExecutionRecordError) -> Self {
        Self::Contract(value)
    }
}

/// Target of an acknowledged call intent. Baseline is not a candidate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CallTarget {
    Baseline,
    Scenario { id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    run_id: String,
    bundle_sha256: String,
    adapter: Value,
    implementation: EngineIdentity,
    codecs: PayloadCodecs,
    max_evaluations: usize,
    deadline_configured: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Terminal {
    Completed,
    Interrupted { reason: RecordInterruption },
    Failed,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Initialized {
        header: Header,
    },
    CallStarted {
        target: CallTarget,
    },
    CallSucceeded {
        target: CallTarget,
        payload: String,
    },
    CallFailed {
        target: CallTarget,
        error_payload: String,
    },
    Finished {
        terminal: Terminal,
    },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    sequence: usize,
    previous_sha256: Option<String>,
    event: Event,
}

struct Emitter<'a, W: ?Sized> {
    sink: &'a mut W,
    sequence: usize,
    previous: Option<String>,
    bytes: usize,
}
impl<W: JournalSink + ?Sized> Emitter<'_, W> {
    fn append(&mut self, event: Event) -> Result<(), JournalError> {
        if self.sequence >= MAX_ENTRIES {
            return Err(ExecutionRecordError::Invalid("too many journal entries").into());
        }
        let mut line = canonical(&Entry {
            sequence: self.sequence,
            previous_sha256: self.previous.clone(),
            event,
        })?;
        let hash = digest(line.as_bytes());
        line.push('\n');
        let total = self
            .bytes
            .checked_add(line.len())
            .ok_or(ExecutionRecordError::Invalid(
                "journal byte accounting overflow",
            ))?;
        if line.len() > MAX_JOURNAL_ENTRY_BYTES || total > MAX_JOURNAL_BYTES {
            return Err(ExecutionRecordError::Invalid("journal byte limit exceeded").into());
        }
        self.sink
            .append_record(line.as_bytes())
            .map_err(JournalError::Storage)?;
        // Never advance the acknowledged prefix when storage returned an error.
        self.bytes = total;
        self.previous = Some(hash);
        self.sequence += 1;
        Ok(())
    }
}

/// Inspection is a file/input consistency result, not a resumable checkpoint.
#[derive(Clone, Debug, Serialize)]
pub struct JournalSummary {
    pub schema: &'static str,
    pub journal_sha256: String,
    pub verified_prefix_sha256: String,
    pub bundle_sha256: String,
    pub run_id: String,
    pub implementation: EngineIdentity,
    pub verified_entries: usize,
    pub incomplete_tail_bytes: usize,
    pub state: &'static str,
    pub baseline_succeeded: bool,
    pub successful_candidates: usize,
    pub failed_call: Option<CallTarget>,
    pub unknown_call_result: Option<CallTarget>,
    pub never_started_candidates: usize,
    pub terminal_recorded: bool,
    pub resume_authorized: bool,
    pub evidence_kind: &'static str,
}

/// Inspect an entire bounded journal against independently supplied input bytes.
///
/// Complete lines must be canonical, hash-chained and lifecycle-consistent. A
/// non-LF-terminated tail is reported explicitly as incomplete, never parsed as a
/// successful event or silently discarded. Missing/incomplete initial headers,
/// malformed COMPLETE entries, and any bytes after a terminal event are errors.
///
/// A valid prefix ending in CallStarted has an UNKNOWN outcome: absence of a
/// success/failure entry cannot establish whether the engine ran. No return value
/// from this function authorizes replay, appending to the old file or conversion
/// to a rankable BatchResult. Coherently rehashed forgery and prefix deletion are
/// not authenticated by this consistency-only format.
pub fn inspect_execution_journal(
    payload: &str,
    expected_bundle: &str,
) -> Result<JournalSummary, ExecutionRecordError> {
    if payload.len() > MAX_JOURNAL_BYTES {
        return invalid("journal exceeds byte limit");
    }
    let bundle = parse_bundle(expected_bundle)?;
    let mut header = None;
    let mut previous = None;
    let mut count = 0;
    let mut prefix_bytes = 0;
    let mut incomplete_tail_bytes = 0;
    let mut lifecycle = Lifecycle::default();
    for raw in payload.split_inclusive('\n') {
        if lifecycle.terminal.is_some() {
            return invalid("bytes after journal terminal");
        }
        if raw.len() > MAX_JOURNAL_ENTRY_BYTES {
            return invalid("journal entry exceeds byte limit");
        }
        let Some(line) = raw.strip_suffix('\n') else {
            incomplete_tail_bytes = raw.len();
            break;
        };
        if count >= MAX_ENTRIES {
            return invalid("too many journal entries");
        }
        let entry: Entry = serde_json::from_str(line)?;
        if canonical(&entry)? != line {
            return invalid("noncanonical, duplicate or missing journal fields");
        }
        if entry.sequence != count || entry.previous_sha256 != previous {
            return invalid("journal sequence or hash-chain mismatch");
        }
        match &entry.event {
            Event::Initialized { header: value } if count == 0 => {
                validate_header(value, &bundle, expected_bundle)?;
                header = Some(value.clone());
            }
            Event::Initialized { .. } => return invalid("duplicate journal header"),
            event => {
                let header = header.as_ref().ok_or(ExecutionRecordError::Invalid(
                    "journal must start with header",
                ))?;
                lifecycle.accept(event, header, &bundle)?;
            }
        }
        previous = Some(digest(line.as_bytes()));
        prefix_bytes += raw.len();
        count += 1;
    }
    let header = header.ok_or(ExecutionRecordError::Invalid("no complete journal header"))?;
    let state = if incomplete_tail_bytes != 0 {
        "incomplete_tail"
    } else if lifecycle.active.is_some() {
        "unknown_call_result"
    } else {
        match &lifecycle.terminal {
            Some(Terminal::Completed) => "completed",
            Some(Terminal::Interrupted { .. }) => "interrupted",
            Some(Terminal::Failed) => "failed",
            None if lifecycle.failed.is_some() => "open_after_failure",
            None if lifecycle.baseline => "open_after_return",
            None => "not_started",
        }
    };
    Ok(JournalSummary {
        schema: "prospect.execution-journal-inspection/v1",
        journal_sha256: digest(payload.as_bytes()),
        verified_prefix_sha256: digest(&payload.as_bytes()[..prefix_bytes]),
        bundle_sha256: header.bundle_sha256,
        run_id: header.run_id,
        implementation: header.implementation,
        verified_entries: count,
        incomplete_tail_bytes,
        state,
        baseline_succeeded: lifecycle.baseline,
        successful_candidates: lifecycle.successful,
        failed_call: lifecycle.failed,
        unknown_call_result: lifecycle.active,
        never_started_candidates: bundle.scenarios().len() - lifecycle.started,
        terminal_recorded: lifecycle.terminal.is_some(),
        resume_authorized: false,
        evidence_kind: "journal_consistency_only",
    })
}

fn validate_header(
    header: &Header,
    bundle: &ScenarioBundle<Value, Value>,
    input: &str,
) -> Result<AdapterMetadata, ExecutionRecordError> {
    if header.schema != SCHEMA || header.bundle_sha256 != digest(input.as_bytes()) {
        return invalid("journal input binding mismatch");
    }
    RunId::new(&header.run_id).map_err(|_| ExecutionRecordError::Invalid("invalid run ID"))?;
    header.implementation.validate()?;
    header.codecs.validate()?;
    let metadata = validate_metadata(&header.adapter)?;
    let required = bundle.adapter();
    if metadata.adapter_id() != required.adapter_id()
        || !metadata
            .contract_version()
            .supports(required.contract_version())
    {
        return invalid("journal adapter contract mismatch");
    }
    if let Some(required) = required.upstream() {
        let actual = metadata
            .upstream()
            .ok_or(ExecutionRecordError::Invalid("missing journal upstream"))?;
        if actual.component() != required.component() || actual.revision() != required.revision() {
            return invalid("journal upstream mismatch");
        }
    }
    Ok(metadata)
}

#[derive(Default)]
struct Lifecycle {
    baseline: bool,
    started: usize,
    successful: usize,
    active: Option<CallTarget>,
    failed: Option<CallTarget>,
    terminal: Option<Terminal>,
}
impl Lifecycle {
    fn accept(
        &mut self,
        event: &Event,
        header: &Header,
        bundle: &ScenarioBundle<Value, Value>,
    ) -> Result<(), ExecutionRecordError> {
        match event {
            Event::Initialized { .. } => return invalid("unexpected journal header"),
            Event::CallStarted { target } => {
                if self.active.is_some() || self.failed.is_some() || header.max_evaluations == 0 {
                    return invalid("inadmissible call intent");
                }
                match target {
                    CallTarget::Baseline if !self.baseline && self.started == 0 => {}
                    CallTarget::Scenario { id }
                        if self.baseline
                            && self.started < header.max_evaluations
                            && bundle
                                .scenarios()
                                .get(self.started)
                                .is_some_and(|s| s.id().as_str() == id) =>
                    {
                        self.started += 1;
                    }
                    _ => return invalid("call target out of order or quota exceeded"),
                }
                self.active = Some(target.clone());
            }
            Event::CallSucceeded { target, payload } => {
                check_payload(payload)?;
                if self.active.as_ref() != Some(target) {
                    return invalid("return without matching intent");
                }
                match target {
                    CallTarget::Baseline => self.baseline = true,
                    CallTarget::Scenario { .. } => self.successful += 1,
                }
                self.active = None;
            }
            Event::CallFailed {
                target,
                error_payload,
            } => {
                check_payload(error_payload)?;
                if self.active.as_ref() != Some(target) {
                    return invalid("failure without matching intent");
                }
                self.failed = Some(target.clone());
                self.active = None;
            }
            Event::Finished { terminal } => {
                if self.active.is_some() {
                    return invalid("terminal while call result is unknown");
                }
                match terminal {
                    Terminal::Completed
                        if !self.baseline
                            || self.failed.is_some()
                            || self.successful != bundle.scenarios().len() =>
                    {
                        return invalid("incomplete journal relabelled completed");
                    }
                    Terminal::Failed if self.failed.is_none() => {
                        return invalid("terminal failure without failed call");
                    }
                    Terminal::Interrupted { reason } => {
                        if self.failed.is_some() {
                            return invalid("engine error replaced by interruption");
                        }
                        match reason {
                            RecordInterruption::DeadlineReached if !header.deadline_configured => {
                                return invalid("deadline not configured");
                            }
                            RecordInterruption::EvaluationLimitReached
                                if self.successful != header.max_evaluations
                                    || self.started == bundle.scenarios().len() =>
                            {
                                return invalid("invalid quota interruption");
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                self.terminal = Some(terminal.clone());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
