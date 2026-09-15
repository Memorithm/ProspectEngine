//! Verified reconstruction of a complete typed batch from a parent + child chain.
//!
//! This module executes no engine calls. It independently rechecks the admitted
//! parent and completed child journals, decodes their acknowledged successes using
//! an explicit application codec, then reconstructs a `BatchResult` only when the
//! exact original scenario order is complete with no gap, failure, or unknown call.

use core::fmt;

use prospect_bundle::ScenarioBundle;
use prospect_core::Scenario;
use prospect_scenario::{BatchResult, ScenarioOutcome};
use serde::Serialize;

use super::{
    ContinuationEntry, ContinuationError, ContinuationEvent, RestartExpectations,
    inspect_continuation_journal, prepare_typed_continuation,
};
use crate::execution::record::{ExecutionRecordError, digest};

/// Complete parent/child software reconstruction and its exact source identities.
///
/// The contained batch is structurally complete and may enter the existing scoring
/// APIs. This type does not itself establish scientific validity or hardware trust.
#[derive(Debug)]
#[must_use = "a verified chain is distinct from a policy or scientific conclusion"]
pub struct AssembledContinuation<I, S> {
    source_run_id: String,
    child_run_id: String,
    source_journal_sha256: String,
    child_journal_sha256: String,
    source_bundle_sha256: String,
    expectation_sha256: String,
    batch: BatchResult<I, S>,
}

impl<I, S> AssembledContinuation<I, S> {
    #[must_use]
    pub fn source_run_id(&self) -> &str {
        &self.source_run_id
    }

    #[must_use]
    pub fn child_run_id(&self) -> &str {
        &self.child_run_id
    }

    #[must_use]
    pub fn source_journal_sha256(&self) -> &str {
        &self.source_journal_sha256
    }

    #[must_use]
    pub fn child_journal_sha256(&self) -> &str {
        &self.child_journal_sha256
    }

    #[must_use]
    pub fn source_bundle_sha256(&self) -> &str {
        &self.source_bundle_sha256
    }

    #[must_use]
    pub fn expectation_sha256(&self) -> &str {
        &self.expectation_sha256
    }

    #[must_use]
    pub const fn batch(&self) -> &BatchResult<I, S> {
        &self.batch
    }

    /// Consume the verified wrapper and expose the complete structural batch.
    ///
    /// Existing metric/policy code can be called only after this gate has succeeded.
    #[must_use]
    pub fn into_batch(self) -> BatchResult<I, S> {
        self.batch
    }
}

#[derive(Debug)]
pub enum ContinuationAssemblyError {
    Contract(ExecutionRecordError),
    Parent(ContinuationError),
    ChildNotCompleted(&'static str),
    Decode(String),
    ScenarioPartitionMismatch,
}

impl fmt::Display for ContinuationAssemblyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(error) => error.fmt(formatter),
            Self::Parent(error) => error.fmt(formatter),
            Self::ChildNotCompleted(state) => {
                write!(formatter, "continuation child is not completed: {state}")
            }
            Self::Decode(error) => write!(formatter, "continuation assembly decode failed: {error}"),
            Self::ScenarioPartitionMismatch => {
                formatter.write_str("continuation parent/child scenario partition mismatch")
            }
        }
    }
}

impl std::error::Error for ContinuationAssemblyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(error) => Some(error),
            Self::Parent(error) => Some(error),
            Self::ChildNotCompleted(_) | Self::Decode(_) | Self::ScenarioPartitionMismatch => None,
        }
    }
}

impl From<ExecutionRecordError> for ContinuationAssemblyError {
    fn from(value: ExecutionRecordError) -> Self {
        Self::Contract(value)
    }
}

/// Reconstruct a rankable batch from one exact admitted parent and completed child.
///
/// This function does **not** use an in-memory `ContinuationRun`: it re-verifies the
/// persisted parent/child byte contracts. The parent is admitted again through the
/// same external expectations + implementation-artifact digest used for restart.
/// The child must pass `inspect_continuation_journal` with state `completed`, a
/// recorded terminal, no failed/unknown/never-started suffix calls, and exactly the
/// remaining candidate count from the reconstructed parent plan.
///
/// The caller-supplied decoder owns the semantics of the trusted signature codec.
/// It is used for both parent and child acknowledged success payloads. A decode
/// error returns no `BatchResult`. No metric, decision policy, adapter, or model is
/// invoked here, and neither journal is modified.
pub fn assemble_completed_continuation<State, I, S, D>(
    parent_journal: &str,
    child_journal: &str,
    bundle: &ScenarioBundle<State, I>,
    expected: &RestartExpectations,
    actual_artifact_sha256: &str,
    mut decode_signature: D,
) -> Result<AssembledContinuation<I, S>, ContinuationAssemblyError>
where
    State: Serialize,
    I: Clone + Serialize,
    D: FnMut(&str) -> Result<S, String>,
{
    let bundle_json = bundle.canonical_json().map_err(|_| {
        ContinuationAssemblyError::Contract(ExecutionRecordError::Invalid(
            "continuation assembly bundle serialization failed",
        ))
    })?;

    let plan = prepare_typed_continuation(
        parent_journal,
        bundle,
        expected,
        actual_artifact_sha256,
        &mut decode_signature,
    )
    .map_err(ContinuationAssemblyError::Parent)?;

    let summary = inspect_continuation_journal(
        child_journal,
        parent_journal,
        &bundle_json,
        expected,
    )?;
    if summary.state != "completed"
        || !summary.terminal_recorded
        || summary.failed_candidate.is_some()
        || summary.unknown_candidate.is_some()
        || summary.never_started_candidates != 0
    {
        return Err(ContinuationAssemblyError::ChildNotCompleted(summary.state));
    }
    if summary.successful_new_candidates != plan.remaining.len()
        || plan.restored.len() + plan.remaining.len() != bundle.scenarios().len()
    {
        return Err(ContinuationAssemblyError::ScenarioPartitionMismatch);
    }

    let restored_count = plan.restored.len();
    let mut outcomes = Vec::with_capacity(bundle.scenarios().len());
    for (index, restored) in plan.restored.into_iter().enumerate() {
        let source = bundle
            .scenarios()
            .get(index)
            .ok_or(ContinuationAssemblyError::ScenarioPartitionMismatch)?;
        if source.id() != restored.scenario_id() {
            return Err(ContinuationAssemblyError::ScenarioPartitionMismatch);
        }
        outcomes.push(ScenarioOutcome::from_parts(
            Scenario::new(source.id().clone(), source.intervention().clone()),
            restored.signature,
        ));
    }

    let mut new_index = 0usize;
    for raw in child_journal.split_terminator('\n') {
        let entry: ContinuationEntry = serde_json::from_str(raw)
            .map_err(ExecutionRecordError::Json)?;
        if let ContinuationEvent::CallSucceeded {
            scenario_id,
            payload,
        } = entry.event
        {
            let source = bundle
                .scenarios()
                .get(restored_count + new_index)
                .ok_or(ContinuationAssemblyError::ScenarioPartitionMismatch)?;
            if source.id().as_str() != scenario_id {
                return Err(ContinuationAssemblyError::ScenarioPartitionMismatch);
            }
            let signature =
                decode_signature(&payload).map_err(ContinuationAssemblyError::Decode)?;
            outcomes.push(ScenarioOutcome::from_parts(
                Scenario::new(source.id().clone(), source.intervention().clone()),
                signature,
            ));
            new_index += 1;
        }
    }
    if new_index != plan.remaining.len() || outcomes.len() != bundle.scenarios().len() {
        return Err(ContinuationAssemblyError::ScenarioPartitionMismatch);
    }

    Ok(AssembledContinuation {
        source_run_id: plan.source_run_id,
        child_run_id: summary.child_run_id,
        source_journal_sha256: digest(parent_journal.as_bytes()),
        child_journal_sha256: summary.journal_sha256,
        source_bundle_sha256: digest(bundle_json.as_bytes()),
        expectation_sha256: expected.sha256(),
        batch: BatchResult::from_parts(plan.baseline, outcomes),
    })
}

#[cfg(test)]
mod tests;
