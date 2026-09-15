//! Read-only restart admission against caller-trusted external expectations.
//!
//! A consistent journal is not permission to replay. This module identifies the
//! never-started suffix only after external identity checks. It never decodes a
//! payload, restores an engine, runs an intervention, or authorizes automatic resume.

use serde::{Deserialize, Serialize};

use super::{EngineIdentity, Entry, Event, inspect_execution_journal};
use super::super::{ExecutionRecordError, PayloadCodecs, canonical, digest, invalid, parse_bundle};

/// Admission bound for the separate trusted expectations file, before parsing.
pub const MAX_RESTART_EXPECTATION_BYTES: usize = 16 * 1024;
const EXPECTATION_SCHEMA: &str = "prospect.restart-expectations/v1";

/// Externally retained identity of a source run and its exact input/adapter bytes.
///
/// Do not derive these values from an untrusted journal and then call them trusted.
/// The adapter digest hashes its canonical metadata JSON, not the engine binary.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartAnchors {
    run_id: String,
    journal_sha256: String,
    bundle_sha256: String,
    adapter_sha256: String,
}

impl RestartAnchors {
    /// Build anchors from independently retained values. No files are read.
    ///
    /// ```
    /// use prospect_evidence::RunId;
    /// use prospect_dispatch::execution::record::journal::recovery::RestartAnchors;
    /// let anchors = RestartAnchors::new(
    ///     &RunId::new("run-1")?, &"a".repeat(64), &"b".repeat(64), &"c".repeat(64),
    /// )?;
    /// # let _ = anchors;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(
        run_id: &prospect_evidence::RunId,
        journal_sha256: &str,
        bundle_sha256: &str,
        adapter_sha256: &str,
    ) -> Result<Self, ExecutionRecordError> {
        let anchors = Self {
            run_id: run_id.as_str().into(),
            journal_sha256: journal_sha256.into(),
            bundle_sha256: bundle_sha256.into(),
            adapter_sha256: adapter_sha256.into(),
        };
        anchors.validate()?;
        Ok(anchors)
    }

    fn validate(&self) -> Result<(), ExecutionRecordError> {
        prospect_evidence::RunId::new(&self.run_id)
            .map_err(|_| ExecutionRecordError::Invalid("invalid restart run ID"))?;
        for value in [&self.journal_sha256, &self.bundle_sha256, &self.adapter_sha256] {
            validate_sha256(value)?;
        }
        Ok(())
    }
}

/// Application declaration, not inferred from an adapter capability or proven here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartSemantics {
    /// Each evaluation is independent and has no externally visible side effects.
    /// Restoring/checking typed values is still a separate requirement.
    PureIndependent,
    /// State restoration, idempotency, or external effects require reconciliation.
    RequiresReconciliation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectationsWire {
    schema: String,
    anchors: RestartAnchors,
    implementation: EngineIdentity,
    codecs: PayloadCodecs,
    semantics: RestartSemantics,
}

/// Validated canonical policy supplied separately from the journal under review.
///
/// The type deliberately has no public Deserialize implementation or mutable
/// fields. New and parsed values pass the same validation before use.
#[derive(Clone, Debug)]
pub struct RestartExpectations {
    wire: ExpectationsWire,
    canonical_json: String,
}

impl RestartExpectations {
    /// Combine external anchors, exact implementation/codec declarations and semantics.
    pub fn new(
        anchors: RestartAnchors,
        implementation: EngineIdentity,
        codecs: PayloadCodecs,
        semantics: RestartSemantics,
    ) -> Result<Self, ExecutionRecordError> {
        Self::validated(ExpectationsWire {
            schema: EXPECTATION_SCHEMA.into(), anchors, implementation, codecs, semantics,
        })
    }

    /// Parse exact canonical bytes. Unknown, duplicate, absent or oversized fields fail.
    pub fn from_canonical_json(payload: &str) -> Result<Self, ExecutionRecordError> {
        if payload.len() > MAX_RESTART_EXPECTATION_BYTES {
            return invalid("restart expectations exceed byte limit");
        }
        let result = Self::validated(serde_json::from_str(payload)?)?;
        if result.canonical_json != payload {
            return invalid("noncanonical, duplicate or missing restart fields");
        }
        Ok(result)
    }

    fn validated(wire: ExpectationsWire) -> Result<Self, ExecutionRecordError> {
        if wire.schema != EXPECTATION_SCHEMA {
            return invalid("unsupported restart expectation schema");
        }
        wire.anchors.validate()?;
        wire.implementation.validate()?;
        wire.codecs.validate()?;
        let canonical_json = canonical(&wire)?;
        if canonical_json.len() > MAX_RESTART_EXPECTATION_BYTES {
            return invalid("restart expectations exceed byte limit");
        }
        Ok(Self { wire, canonical_json })
    }

    /// Exact bytes to retain in a separate trusted registry or file.
    #[must_use]
    pub fn canonical_json(&self) -> &str { &self.canonical_json }

    /// Content identity of this policy; this is not a signature or trust root.
    #[must_use]
    pub fn sha256(&self) -> String { digest(self.canonical_json.as_bytes()) }
}

/// Reasons which forbid preparing a continuation, even for a structurally valid log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartBlocker {
    IncompleteTail,
    UnknownCallResult,
    FailedCallRequiresReconciliation,
    StatefulOrEffectfulEngine,
    AlreadyCompleted,
    NoNeverStartedCandidates,
}

/// Read-only diagnostic plan, NOT a retry queue, engine checkpoint or capability.
#[derive(Clone, Debug, Serialize)]
pub struct RestartPreflight {
    pub schema: &'static str,
    pub expectation_sha256: String,
    pub source_journal_sha256: String,
    pub source_bundle_sha256: String,
    pub source_run_id: String,
    pub implementation: EngineIdentity,
    pub artifact_sha256: String,
    pub source_state: &'static str,
    pub baseline_available: bool,
    pub successful_candidates: usize,
    pub never_started_scenario_ids: Vec<String>,
    pub blockers: Vec<RestartBlocker>,
    pub continuation_preparation_allowed: bool,
    pub resume_authorized: bool,
    pub evidence_kind: &'static str,
}

/// Verify trusted external identities and identify the never-invoked suffix.
///
/// `actual_artifact_sha256` must be independently computed by the caller over the
/// implementation artifact to be admitted; the CLI computes it from a separate
/// regular file without executing it. Equality does not attest a running process.
///
/// Identity mismatches return an error, not a partially approved plan. The existing
/// inspector remains authoritative for all sequence/hash/lifecycle checks. Torn
/// tails, unmatched intents and known failures block preparation even when the
/// application declares a pure engine. Completed runs are never restarted.
///
/// A positive plan permits only the NEXT preparation step: explicit typed payload
/// decoding and state/codec validation under a separately specified continuation
/// API. No value returned here can resume, append to the old journal or become a
/// rankable BatchResult. The original journal and all old contracts are unchanged.
pub fn preflight_journal_restart(
    journal: &str,
    expected_bundle: &str,
    expected: &RestartExpectations,
    actual_artifact_sha256: &str,
) -> Result<RestartPreflight, ExecutionRecordError> {
    validate_sha256(actual_artifact_sha256)?;
    let wire = &expected.wire;
    if actual_artifact_sha256 != wire.implementation.artifact_sha256() {
        return invalid("restart implementation artifact mismatch");
    }
    if digest(journal.as_bytes()) != wire.anchors.journal_sha256
        || digest(expected_bundle.as_bytes()) != wire.anchors.bundle_sha256 {
        return invalid("restart external journal or input digest mismatch");
    }
    let summary = inspect_execution_journal(journal, expected_bundle)?;
    if summary.run_id != wire.anchors.run_id || summary.implementation != wire.implementation {
        return invalid("restart run or implementation identity mismatch");
    }
    // The inspector already validated this entire complete entry. Read only its
    // header here; do not introduce a second, divergent lifecycle implementation.
    let first = journal.split('\n').next().ok_or(ExecutionRecordError::Invalid("missing header"))?;
    let entry: Entry = serde_json::from_str(first)?;
    let Event::Initialized { header } = entry.event else { return invalid("missing restart header") };
    if canonical(&header.codecs)? != canonical(&wire.codecs)? {
        return invalid("restart payload codec mismatch");
    }
    if digest(canonical(&header.adapter)?.as_bytes()) != wire.anchors.adapter_sha256 {
        return invalid("restart adapter metadata mismatch");
    }
    let bundle = parse_bundle(expected_bundle)?;
    let started = bundle.scenarios().len().checked_sub(summary.never_started_candidates)
        .ok_or(ExecutionRecordError::Invalid("invalid recovered candidate count"))?;
    let never_started_scenario_ids = bundle.scenarios()[started..].iter()
        .map(|scenario| scenario.id().as_str().to_owned()).collect::<Vec<_>>();
    let mut blockers = Vec::new();
    if summary.incomplete_tail_bytes != 0 { blockers.push(RestartBlocker::IncompleteTail); }
    if summary.unknown_call_result.is_some() { blockers.push(RestartBlocker::UnknownCallResult); }
    if summary.failed_call.is_some() { blockers.push(RestartBlocker::FailedCallRequiresReconciliation); }
    if wire.semantics != RestartSemantics::PureIndependent { blockers.push(RestartBlocker::StatefulOrEffectfulEngine); }
    if summary.state == "completed" { blockers.push(RestartBlocker::AlreadyCompleted); }
    if never_started_scenario_ids.is_empty() { blockers.push(RestartBlocker::NoNeverStartedCandidates); }
    Ok(RestartPreflight {
        schema: "prospect.restart-preflight/v1",
        expectation_sha256: expected.sha256(),
        source_journal_sha256: summary.journal_sha256,
        source_bundle_sha256: summary.bundle_sha256,
        source_run_id: summary.run_id,
        implementation: summary.implementation,
        artifact_sha256: actual_artifact_sha256.into(),
        source_state: summary.state,
        baseline_available: summary.baseline_succeeded,
        successful_candidates: summary.successful_candidates,
        never_started_scenario_ids,
        continuation_preparation_allowed: blockers.is_empty(),
        blockers,
        resume_authorized: false,
        evidence_kind: "externally_bound_restart_preflight_only",
    })
}

fn validate_sha256(value: &str) -> Result<(), ExecutionRecordError> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return invalid("invalid restart SHA-256");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
