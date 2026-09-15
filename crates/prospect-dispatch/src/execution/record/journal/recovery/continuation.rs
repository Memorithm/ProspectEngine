//! Typed continuation of an externally admitted, pure-independent source run.
//!
//! This module never retries failed/unknown calls and never appends to a parent
//! journal. It restores only acknowledged payloads through explicit application
//! decoders, then starts a fresh parent-linked journal for the never-started suffix.

use prospect_bundle::ScenarioBundle;
use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};
use prospect_evidence::RunId;
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};
use prospect_scenario::ScenarioOutcome;
use prospect_scenario::controlled::{
    BatchExecution, BatchStatus, EvaluationControl, ExecutionState, InterruptionReason,
    evaluate_batch_controlled,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::fmt;

use super::super::super::super::{BundleExecutionError, ExecutableAdapterRegistry};
use super::super::super::{
    ExecutionRecordError, PayloadCodecs, RecordInterruption, canonical, check_payload, digest,
    invalid,
};
use super::super::{
    CallTarget as ParentCallTarget, EngineIdentity, Entry as ParentEntry, Event as ParentEvent,
    JournalError, JournalSink, MAX_JOURNAL_BYTES, MAX_JOURNAL_ENTRY_BYTES,
    inspect_execution_journal,
};
use super::{RestartBlocker, RestartExpectations, RestartSemantics, preflight_journal_restart};
use crate::resolve_bundle_requirements;

const CONTINUATION_SCHEMA: &str = "prospect.execution-continuation-journal/v1";
const MAX_CONTINUATION_ENTRIES: usize = 2 * super::super::super::MAX_RECORD_SCENARIOS + 2;

/// One signature restored from an acknowledged parent-journal return.
#[derive(Clone, Debug)]
pub struct RestoredOutcome<S> {
    scenario_id: ScenarioId,
    signature: S,
}
impl<S> RestoredOutcome<S> {
    #[must_use]
    pub const fn scenario_id(&self) -> &ScenarioId {
        &self.scenario_id
    }
    #[must_use]
    pub const fn signature(&self) -> &S {
        &self.signature
    }
}

/// Typed source prefix plus the exact suffix which may be attempted in a child run.
///
/// Construct only through `prepare_typed_continuation`; there is no public field
/// mutation and no retry candidate is inferred from a failed or unknown parent call.
#[derive(Debug)]
#[must_use = "a prepared continuation is not itself execution authorization"]
pub struct TypedContinuationPlan<I, S> {
    source_run_id: String,
    source_journal_sha256: String,
    source_bundle_sha256: String,
    expectation_sha256: String,
    implementation: EngineIdentity,
    codecs: PayloadCodecs,
    adapter_sha256: String,
    baseline: S,
    restored: Vec<RestoredOutcome<S>>,
    remaining: Vec<Scenario<I>>,
}
impl<I, S> TypedContinuationPlan<I, S> {
    #[must_use]
    pub fn source_run_id(&self) -> &str {
        &self.source_run_id
    }
    #[must_use]
    pub fn source_journal_sha256(&self) -> &str {
        &self.source_journal_sha256
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
    pub const fn implementation(&self) -> &EngineIdentity {
        &self.implementation
    }
    #[must_use]
    pub const fn codecs(&self) -> &PayloadCodecs {
        &self.codecs
    }
    #[must_use]
    pub const fn baseline(&self) -> &S {
        &self.baseline
    }
    #[must_use]
    pub fn restored_outcomes(&self) -> &[RestoredOutcome<S>] {
        &self.restored
    }
    #[must_use]
    pub fn remaining(&self) -> &[Scenario<I>] {
        &self.remaining
    }
}

/// Preparation failure. `Blocked` preserves why the read-only external preflight
/// refused continuation; decoding failures never cause an engine call.
#[derive(Debug)]
pub enum ContinuationError<E = core::convert::Infallible> {
    Contract(ExecutionRecordError),
    Blocked(Vec<RestartBlocker>),
    Decode(String),
    Dispatch(BundleExecutionError<E>),
    Journal(JournalError),
}
impl<E: fmt::Display> fmt::Display for ContinuationError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(error) => error.fmt(f),
            Self::Blocked(blockers) => write!(f, "continuation blocked: {blockers:?}"),
            Self::Decode(error) => write!(f, "continuation payload decode failed: {error}"),
            Self::Dispatch(error) => error.fmt(f),
            Self::Journal(error) => error.fmt(f),
        }
    }
}
impl<E: fmt::Debug + fmt::Display> std::error::Error for ContinuationError<E> {}
impl<E> From<ExecutionRecordError> for ContinuationError<E> {
    fn from(value: ExecutionRecordError) -> Self {
        Self::Contract(value)
    }
}

/// Decode the exact acknowledged parent prefix after external admission succeeds.
///
/// The caller supplies the signature decoder for the codec named in the separately
/// trusted expectations. Decoder semantics are not inferred from the codec string.
/// Only a source with a successful baseline, pure-independent semantics, no failed
/// or unknown call, no torn tail and at least one never-started candidate can form
/// a plan. No engine or sink is touched by this function.
pub fn prepare_typed_continuation<State, I, S, D>(
    parent_journal: &str,
    bundle: &ScenarioBundle<State, I>,
    expected: &RestartExpectations,
    actual_artifact_sha256: &str,
    mut decode_signature: D,
) -> Result<TypedContinuationPlan<I, S>, ContinuationError>
where
    State: Serialize,
    I: Clone + Serialize,
    D: FnMut(&str) -> Result<S, String>,
{
    let input = bundle
        .canonical_json()
        .map_err(|_| ExecutionRecordError::Invalid("continuation bundle serialization failed"))?;
    let preflight =
        preflight_journal_restart(parent_journal, &input, expected, actual_artifact_sha256)?;
    if !preflight.continuation_preparation_allowed {
        return Err(ContinuationError::Blocked(preflight.blockers));
    }
    if expected.wire.semantics != RestartSemantics::PureIndependent {
        return Err(ContinuationError::Blocked(vec![
            RestartBlocker::StatefulOrEffectfulEngine,
        ]));
    }
    if !preflight.baseline_available {
        return Err(ContinuationError::Contract(ExecutionRecordError::Invalid(
            "continuation requires an acknowledged parent baseline",
        )));
    }

    let mut baseline = None;
    let mut restored = Vec::new();
    for raw in parent_journal.split_terminator('\n') {
        let entry: ParentEntry = serde_json::from_str(raw).map_err(ExecutionRecordError::Json)?;
        if let ParentEvent::CallSucceeded { target, payload } = entry.event {
            let signature = decode_signature(&payload).map_err(ContinuationError::Decode)?;
            match target {
                ParentCallTarget::Baseline => {
                    if baseline.replace(signature).is_some() {
                        return invalid("duplicate restored baseline")
                            .map_err(ContinuationError::Contract);
                    }
                }
                ParentCallTarget::Scenario { id } => {
                    let source = bundle.scenarios().get(restored.len()).ok_or(
                        ExecutionRecordError::Invalid("restored prefix longer than input"),
                    )?;
                    if source.id().as_str() != id {
                        return invalid("restored scenario order mismatch")
                            .map_err(ContinuationError::Contract);
                    }
                    restored.push(RestoredOutcome {
                        scenario_id: source.id().clone(),
                        signature,
                    });
                }
            }
        }
    }
    let baseline = baseline.ok_or(ExecutionRecordError::Invalid("missing restored baseline"))?;
    if restored.len() != preflight.successful_candidates {
        return invalid("restored candidate count mismatch").map_err(ContinuationError::Contract);
    }
    let remaining = bundle.scenarios()[restored.len()..]
        .iter()
        .map(|scenario| Scenario::new(scenario.id().clone(), scenario.intervention().clone()))
        .collect::<Vec<_>>();
    let ids = remaining
        .iter()
        .map(|scenario| scenario.id().as_str())
        .collect::<Vec<_>>();
    if ids
        != preflight
            .never_started_scenario_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    {
        return invalid("restored suffix identity mismatch").map_err(ContinuationError::Contract);
    }
    Ok(TypedContinuationPlan {
        source_run_id: preflight.source_run_id,
        source_journal_sha256: preflight.source_journal_sha256,
        source_bundle_sha256: preflight.source_bundle_sha256,
        expectation_sha256: preflight.expectation_sha256,
        implementation: expected.wire.implementation.clone(),
        codecs: expected.wire.codecs.clone(),
        adapter_sha256: expected.wire.anchors.adapter_sha256.clone(),
        baseline,
        restored,
        remaining,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParentLink {
    run_id: String,
    journal_sha256: String,
    bundle_sha256: String,
    expectation_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContinuationHeader {
    schema: String,
    run_id: String,
    parent: ParentLink,
    adapter: Value,
    implementation: EngineIdentity,
    codecs: PayloadCodecs,
    restored_candidate_ids: Vec<String>,
    remaining_candidate_ids: Vec<String>,
    max_evaluations: usize,
    deadline_configured: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum ContinuationTerminal {
    Completed,
    Interrupted { reason: RecordInterruption },
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum ContinuationEvent {
    Initialized {
        header: Box<ContinuationHeader>,
    },
    CallStarted {
        scenario_id: String,
    },
    CallSucceeded {
        scenario_id: String,
        payload: String,
    },
    CallFailed {
        scenario_id: String,
        error_payload: String,
    },
    Finished {
        terminal: ContinuationTerminal,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContinuationEntry {
    sequence: usize,
    previous_sha256: Option<String>,
    event: ContinuationEvent,
}

struct ContinuationEmitter<'a, W: ?Sized> {
    sink: &'a mut W,
    sequence: usize,
    previous: Option<String>,
    bytes: usize,
}
impl<W: JournalSink + ?Sized> ContinuationEmitter<'_, W> {
    fn append(&mut self, event: ContinuationEvent) -> Result<(), JournalError> {
        if self.sequence >= MAX_CONTINUATION_ENTRIES {
            return Err(ExecutionRecordError::Invalid("too many continuation entries").into());
        }
        let mut line = canonical(&ContinuationEntry {
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
                "continuation byte accounting overflow",
            ))?;
        if line.len() > MAX_JOURNAL_ENTRY_BYTES || total > MAX_JOURNAL_BYTES {
            return Err(
                ExecutionRecordError::Invalid("continuation journal byte limit exceeded").into(),
            );
        }
        self.sink
            .append_record(line.as_bytes())
            .map_err(JournalError::Storage)?;
        self.bytes = total;
        self.previous = Some(hash);
        self.sequence += 1;
        Ok(())
    }
}

/// Fresh child-journal configuration. It must use the same implementation and
/// codec declarations admitted for the parent source. The sink must be new.
pub struct ContinuationCapture<'a, W: ?Sized, FS, FE> {
    sink: &'a mut W,
    run_id: RunId,
    implementation: EngineIdentity,
    codecs: PayloadCodecs,
    encode_signature: FS,
    encode_error: FE,
}
impl<'a, W: JournalSink + ?Sized, FS, FE> ContinuationCapture<'a, W, FS, FE> {
    pub fn new(
        sink: &'a mut W,
        run_id: RunId,
        implementation: EngineIdentity,
        codecs: PayloadCodecs,
        encode_signature: FS,
        encode_error: FE,
    ) -> Self {
        Self {
            sink,
            run_id,
            implementation,
            codecs,
            encode_signature,
            encode_error,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContinuationRunState {
    Completed,
    Interrupted,
    EngineFailed,
    JournalFailed,
}
#[derive(Debug)]
enum ChildFailure<E> {
    BlockedBeforeEngine,
    Engine(E),
}

/// Parent-linked child execution. Restored prefix and newly executed suffix remain
/// separately inspectable; this type deliberately has no implicit ranking method.
#[derive(Debug)]
#[must_use = "inspect child state before treating continuation as complete"]
pub struct ContinuationRun<I, S, E> {
    source_run_id: String,
    child_run_id: String,
    baseline: S,
    restored: Vec<RestoredOutcome<S>>,
    evaluation: BatchExecution<I, S, ChildFailure<E>>,
    journal_error: Option<JournalError>,
}
impl<I, S, E> ContinuationRun<I, S, E> {
    #[must_use]
    pub fn source_run_id(&self) -> &str {
        &self.source_run_id
    }
    #[must_use]
    pub fn child_run_id(&self) -> &str {
        &self.child_run_id
    }
    #[must_use]
    pub const fn baseline(&self) -> &S {
        &self.baseline
    }
    #[must_use]
    pub fn restored_outcomes(&self) -> &[RestoredOutcome<S>] {
        &self.restored
    }
    #[must_use]
    pub fn new_outcomes(&self) -> &[ScenarioOutcome<I, S>] {
        self.evaluation.outcomes()
    }
    #[must_use]
    pub const fn journal_error(&self) -> Option<&JournalError> {
        self.journal_error.as_ref()
    }
    #[must_use]
    pub fn state(&self) -> ContinuationRunState {
        if self.journal_error.is_some() {
            return ContinuationRunState::JournalFailed;
        }
        match self.evaluation.state() {
            ExecutionState::Completed => ContinuationRunState::Completed,
            ExecutionState::Interrupted => ContinuationRunState::Interrupted,
            ExecutionState::Failed | ExecutionState::Rejected => ContinuationRunState::EngineFailed,
        }
    }
    #[must_use]
    pub fn engine_error(&self) -> Option<&E> {
        match self.evaluation.status() {
            BatchStatus::EngineFailed {
                error: ChildFailure::Engine(error),
                ..
            } => Some(error),
            _ => None,
        }
    }
    #[must_use]
    pub fn never_started(&self) -> Vec<&Scenario<I>> {
        let mut result = Vec::new();
        if let BatchStatus::EngineFailed {
            scenario: Some(scenario),
            error: ChildFailure::BlockedBeforeEngine,
        } = self.evaluation.status()
        {
            result.push(scenario);
        }
        result.extend(self.evaluation.pending());
        result
    }
    #[must_use]
    pub fn confirmed_candidate_count(&self) -> usize {
        self.restored.len() + self.evaluation.outcomes().len()
    }
}

struct ChildSession<'a, W: ?Sized, FS, FE> {
    emitter: ContinuationEmitter<'a, W>,
    error: Option<JournalError>,
    ids: Vec<String>,
    next: usize,
    encode_signature: FS,
    encode_error: FE,
}
impl<W: JournalSink + ?Sized, FS, FE> ChildSession<'_, W, FS, FE> {
    fn emit(&mut self, event: ContinuationEvent) -> bool {
        if self.error.is_some() {
            return false;
        }
        match self.emitter.append(event) {
            Ok(()) => true,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }
    fn returned<S, E>(&mut self, id: String, result: &Result<S, E>)
    where
        FS: FnMut(&S) -> Result<String, String>,
        FE: FnMut(&E) -> Result<String, String>,
    {
        let encoded = match result {
            Ok(value) => (self.encode_signature)(value),
            Err(error) => (self.encode_error)(error),
        }
        .map_err(ExecutionRecordError::Encoding)
        .and_then(|payload| {
            check_payload(&payload)?;
            Ok(payload)
        });
        match encoded {
            Ok(payload) => {
                let event = if result.is_ok() {
                    ContinuationEvent::CallSucceeded {
                        scenario_id: id,
                        payload,
                    }
                } else {
                    ContinuationEvent::CallFailed {
                        scenario_id: id,
                        error_payload: payload,
                    }
                };
                self.emit(event);
            }
            Err(error) => self.error = Some(error.into()),
        }
    }
}

struct ContinuationEngine<'a, E: ?Sized, W: ?Sized, FS, FE, S> {
    engine: &'a E,
    restored_baseline: S,
    session: RefCell<ChildSession<'a, W, FS, FE>>,
}
impl<State, I, E, W, FS, FE, S> ProspectiveEngine<State, I>
    for ContinuationEngine<'_, E, W, FS, FE, S>
where
    E: ProspectiveEngine<State, I, Signature = S> + ?Sized,
    W: JournalSink + ?Sized,
    FS: FnMut(&S) -> Result<String, String>,
    FE: FnMut(&E::Error) -> Result<String, String>,
    S: Clone,
{
    type Signature = S;
    type Error = ChildFailure<E::Error>;
    fn baseline(&self, _: &State) -> Result<S, Self::Error> {
        // Deliberately restored from the independently admitted parent; no domain
        // baseline call occurs in a continuation child run.
        Ok(self.restored_baseline.clone())
    }
    fn evaluate(&self, state: &State, intervention: &I) -> Result<S, Self::Error> {
        let id = {
            let mut session = self.session.borrow_mut();
            if session.error.is_some() {
                return Err(ChildFailure::BlockedBeforeEngine);
            }
            let Some(id) = session.ids.get(session.next).cloned() else {
                session.error = Some(
                    ExecutionRecordError::Invalid("continuation input order exhausted").into(),
                );
                return Err(ChildFailure::BlockedBeforeEngine);
            };
            session.next += 1;
            if !session.emit(ContinuationEvent::CallStarted {
                scenario_id: id.clone(),
            }) {
                return Err(ChildFailure::BlockedBeforeEngine);
            }
            id
        };
        let result = self.engine.evaluate(state, intervention);
        self.session.borrow_mut().returned(id, &result);
        result.map_err(ChildFailure::Engine)
    }
}

/// Execute exactly the previously never-started suffix as a fresh child run.
///
/// The source plan is consumed. The current canonical bundle, registered adapter
/// metadata, implementation declaration and codec IDs must still match the plan.
/// The domain baseline is NOT called again: its acknowledged parent value is
/// restored through the caller decoder and cloned into the generic evaluator.
/// Only candidate calls in `plan.remaining()` can reach the registered engine.
///
/// The child journal is a new hash-chained file/sink with an explicit parent link;
/// the parent journal is never opened for writing. Storage/encoding failure blocks
/// later child calls and preserves actual returned values/errors in memory. No
/// failed/unknown parent call can reach this function because preparation rejects it.
pub fn execute_typed_continuation<State, I, S, E, M, P, W, FS, FE>(
    plan: TypedContinuationPlan<I, S>,
    bundle: &ScenarioBundle<State, I>,
    adapters: &ExecutableAdapterRegistry<State, I, S, E>,
    metrics: &MetricRegistry<S, M>,
    policies: &DecisionPolicyRegistry<S, P>,
    control: &EvaluationControl,
    capture: ContinuationCapture<'_, W, FS, FE>,
) -> Result<ContinuationRun<I, S, E>, ContinuationError<E>>
where
    State: Serialize,
    I: Clone + Serialize,
    S: Clone,
    P: Ord,
    W: JournalSink + ?Sized,
    FS: FnMut(&S) -> Result<String, String>,
    FE: FnMut(&E) -> Result<String, String>,
{
    if capture.run_id.as_str() == plan.source_run_id {
        return Err(ContinuationError::Contract(ExecutionRecordError::Invalid(
            "continuation child run ID must differ from parent",
        )));
    }
    let current_input = bundle.canonical_json().map_err(|_| {
        ContinuationError::Contract(ExecutionRecordError::Invalid(
            "continuation bundle serialization failed",
        ))
    })?;
    if digest(current_input.as_bytes()) != plan.source_bundle_sha256 {
        return Err(ContinuationError::Contract(ExecutionRecordError::Invalid(
            "continuation source bundle changed",
        )));
    }
    if canonical(&capture.implementation)? != canonical(&plan.implementation)?
        || canonical(&capture.codecs)? != canonical(&plan.codecs)?
    {
        return Err(ContinuationError::Contract(ExecutionRecordError::Invalid(
            "continuation implementation or codec mismatch",
        )));
    }
    let catalog = adapters.owned_metadata_catalog();
    let resolved = resolve_bundle_requirements(bundle, &catalog, metrics, policies)
        .map_err(|error| ContinuationError::Dispatch(BundleExecutionError::Dispatch(error)))?;
    let adapter = resolved.adapter().clone();
    if digest(canonical(&adapter)?.as_bytes()) != plan.adapter_sha256 {
        return Err(ContinuationError::Contract(ExecutionRecordError::Invalid(
            "continuation adapter metadata changed",
        )));
    }
    let adapter_id = adapter.adapter_id().as_str();
    let engine = adapters.engine(adapter_id).ok_or_else(|| {
        ContinuationError::Dispatch(BundleExecutionError::RegistryInvariant(
            adapter_id.to_owned(),
        ))
    })?;
    let expected_ids = plan
        .remaining
        .iter()
        .map(|s| s.id().as_str())
        .collect::<Vec<_>>();
    let current_ids = bundle.scenarios()[plan.restored.len()..]
        .iter()
        .map(|s| s.id().as_str())
        .collect::<Vec<_>>();
    if expected_ids != current_ids {
        return Err(ContinuationError::Contract(ExecutionRecordError::Invalid(
            "continuation suffix changed",
        )));
    }
    let header = ContinuationHeader {
        schema: CONTINUATION_SCHEMA.into(),
        run_id: capture.run_id.as_str().to_owned(),
        parent: ParentLink {
            run_id: plan.source_run_id.clone(),
            journal_sha256: plan.source_journal_sha256.clone(),
            bundle_sha256: plan.source_bundle_sha256.clone(),
            expectation_sha256: plan.expectation_sha256.clone(),
        },
        adapter: serde_json::to_value(&adapter).map_err(ExecutionRecordError::Json)?,
        implementation: capture.implementation,
        codecs: capture.codecs,
        restored_candidate_ids: plan
            .restored
            .iter()
            .map(|o| o.scenario_id.as_str().to_owned())
            .collect(),
        remaining_candidate_ids: plan
            .remaining
            .iter()
            .map(|s| s.id().as_str().to_owned())
            .collect(),
        max_evaluations: control.max_evaluations(),
        deadline_configured: control.deadline().is_some(),
    };
    let child_run_id = header.run_id.clone();
    let mut session = ChildSession {
        emitter: ContinuationEmitter {
            sink: capture.sink,
            sequence: 0,
            previous: None,
            bytes: 0,
        },
        error: None,
        ids: header.remaining_candidate_ids.clone(),
        next: 0,
        encode_signature: capture.encode_signature,
        encode_error: capture.encode_error,
    };
    session.emit(ContinuationEvent::Initialized {
        header: Box::new(header),
    });
    let wrapped = ContinuationEngine {
        engine,
        restored_baseline: plan.baseline.clone(),
        session: RefCell::new(session),
    };
    let evaluation =
        evaluate_batch_controlled(&wrapped, bundle.state(), plan.remaining, control, |_| {});
    let mut session = wrapped.session.into_inner();
    if session.error.is_none() {
        let terminal = match evaluation.status() {
            BatchStatus::Completed => Some(ContinuationTerminal::Completed),
            BatchStatus::Interrupted(reason) => Some(ContinuationTerminal::Interrupted {
                reason: match reason {
                    InterruptionReason::Cancelled => RecordInterruption::Cancelled,
                    InterruptionReason::DeadlineReached => RecordInterruption::DeadlineReached,
                    InterruptionReason::EvaluationLimitReached => {
                        RecordInterruption::EvaluationLimitReached
                    }
                },
            }),
            BatchStatus::EngineFailed {
                error: ChildFailure::Engine(_),
                ..
            } => Some(ContinuationTerminal::Failed),
            _ => None,
        };
        if let Some(terminal) = terminal {
            session.emit(ContinuationEvent::Finished { terminal });
        } else {
            session.error = Some(
                ExecutionRecordError::Invalid("unexpected continuation evaluator state").into(),
            );
        }
    }
    Ok(ContinuationRun {
        source_run_id: plan.source_run_id,
        child_run_id,
        baseline: plan.baseline,
        restored: plan.restored,
        evaluation,
        journal_error: session.error,
    })
}

/// Structural inspection of a child continuation journal and its exact parent.
#[derive(Clone, Debug, Serialize)]
pub struct ContinuationJournalSummary {
    pub schema: &'static str,
    pub journal_sha256: String,
    pub child_run_id: String,
    pub parent_run_id: String,
    pub parent_journal_sha256: String,
    pub source_bundle_sha256: String,
    pub expectation_sha256: String,
    pub state: &'static str,
    pub successful_new_candidates: usize,
    pub failed_candidate: Option<String>,
    pub unknown_candidate: Option<String>,
    pub never_started_candidates: usize,
    pub terminal_recorded: bool,
    pub resume_authorized: bool,
    pub evidence_kind: &'static str,
}

/// Verify child linkage, canonical hash chain and suffix lifecycle without decoding
/// payloads or authenticating the implementation artifact. `expected` remains the
/// separately trusted parent policy; this inspector never mutates either journal.
pub fn inspect_continuation_journal(
    child: &str,
    parent: &str,
    bundle_json: &str,
    expected: &RestartExpectations,
) -> Result<ContinuationJournalSummary, ExecutionRecordError> {
    if child.len() > MAX_JOURNAL_BYTES {
        return invalid("continuation journal exceeds byte limit");
    }
    if digest(parent.as_bytes()) != expected.wire.anchors.journal_sha256
        || digest(bundle_json.as_bytes()) != expected.wire.anchors.bundle_sha256
    {
        return invalid("continuation parent anchors mismatch");
    }
    let bundle = super::super::super::parse_bundle(bundle_json)?;
    let parent_summary = inspect_execution_journal(parent, bundle_json)?;
    if parent_summary.incomplete_tail_bytes != 0
        || parent_summary.failed_call.is_some()
        || parent_summary.unknown_call_result.is_some()
        || !parent_summary.baseline_succeeded
        || parent_summary.never_started_candidates == 0
        || parent_summary.successful_candidates + parent_summary.never_started_candidates
            != bundle.scenarios().len()
    {
        return invalid("continuation parent lifecycle is not admissible");
    }
    let mut previous = None;
    let mut header = None;
    let mut active = None::<String>;
    let mut successful = 0usize;
    let mut failed = None::<String>;
    let mut terminal = None::<ContinuationTerminal>;
    for (entry_index, raw) in child.split_inclusive('\n').enumerate() {
        if entry_index >= MAX_CONTINUATION_ENTRIES {
            return invalid("too many continuation entries");
        }
        if raw.len() > MAX_JOURNAL_ENTRY_BYTES {
            return invalid("continuation entry exceeds byte limit");
        }
        if !raw.ends_with('\n') {
            return invalid("unterminated continuation entry");
        }
        if terminal.is_some() {
            return invalid("bytes after continuation terminal");
        }
        let line = raw.strip_suffix('\n').unwrap();
        let entry: ContinuationEntry = serde_json::from_str(line)?;
        if canonical(&entry)? != line
            || entry.sequence != entry_index
            || entry.previous_sha256 != previous
        {
            return invalid("continuation canonical sequence or hash mismatch");
        }
        match entry.event {
            ContinuationEvent::Initialized { header: value } if entry_index == 0 => {
                let value = *value;
                if value.schema != CONTINUATION_SCHEMA
                    || value.parent.run_id != expected.wire.anchors.run_id
                    || value.parent.journal_sha256 != expected.wire.anchors.journal_sha256
                    || value.parent.bundle_sha256 != expected.wire.anchors.bundle_sha256
                    || value.parent.expectation_sha256 != expected.sha256()
                    || value.run_id == expected.wire.anchors.run_id
                    || canonical(&value.implementation)?
                        != canonical(&expected.wire.implementation)?
                    || canonical(&value.codecs)? != canonical(&expected.wire.codecs)?
                    || digest(canonical(&value.adapter)?.as_bytes())
                        != expected.wire.anchors.adapter_sha256
                {
                    return invalid("continuation header identity mismatch");
                }
                let restored = value.restored_candidate_ids.len();
                if restored != parent_summary.successful_candidates
                    || value.remaining_candidate_ids.len()
                        != parent_summary.never_started_candidates
                    || restored > bundle.scenarios().len()
                    || bundle.scenarios()[..restored]
                        .iter()
                        .map(|s| s.id().as_str())
                        .ne(value.restored_candidate_ids.iter().map(String::as_str))
                    || bundle.scenarios()[restored..]
                        .iter()
                        .map(|s| s.id().as_str())
                        .ne(value.remaining_candidate_ids.iter().map(String::as_str))
                {
                    return invalid("continuation restored/suffix identity mismatch");
                }
                RunId::new(&value.run_id)
                    .map_err(|_| ExecutionRecordError::Invalid("invalid continuation run ID"))?;
                header = Some(value);
            }
            ContinuationEvent::Initialized { .. } => {
                return invalid("duplicate continuation header");
            }
            ContinuationEvent::CallStarted { scenario_id } => {
                let h = header
                    .as_ref()
                    .ok_or(ExecutionRecordError::Invalid("continuation missing header"))?;
                if active.is_some()
                    || failed.is_some()
                    || successful >= h.max_evaluations
                    || h.remaining_candidate_ids
                        .get(successful)
                        .map(String::as_str)
                        != Some(scenario_id.as_str())
                {
                    return invalid("inadmissible continuation call intent");
                }
                active = Some(scenario_id);
            }
            ContinuationEvent::CallSucceeded {
                scenario_id,
                payload,
            } => {
                check_payload(&payload)?;
                if active.as_deref() != Some(scenario_id.as_str()) {
                    return invalid("continuation return without intent");
                }
                active = None;
                successful += 1;
            }
            ContinuationEvent::CallFailed {
                scenario_id,
                error_payload,
            } => {
                check_payload(&error_payload)?;
                if active.as_deref() != Some(scenario_id.as_str()) {
                    return invalid("continuation failure without intent");
                }
                active = None;
                failed = Some(scenario_id);
            }
            ContinuationEvent::Finished { terminal: value } => {
                if active.is_some() {
                    return invalid("continuation terminal with unknown result");
                }
                let h = header
                    .as_ref()
                    .ok_or(ExecutionRecordError::Invalid("continuation missing header"))?;
                match &value {
                    ContinuationTerminal::Completed
                        if failed.is_none() && successful == h.remaining_candidate_ids.len() => {}
                    ContinuationTerminal::Interrupted {
                        reason: RecordInterruption::EvaluationLimitReached,
                    } if failed.is_none()
                        && successful == h.max_evaluations
                        && successful < h.remaining_candidate_ids.len() => {}
                    ContinuationTerminal::Interrupted {
                        reason: RecordInterruption::Cancelled,
                    } if failed.is_none() && successful <= h.remaining_candidate_ids.len() => {}
                    ContinuationTerminal::Interrupted {
                        reason: RecordInterruption::DeadlineReached,
                    } if failed.is_none()
                        && h.deadline_configured
                        && successful <= h.remaining_candidate_ids.len() => {}
                    ContinuationTerminal::Failed if failed.is_some() => {}
                    _ => return invalid("continuation terminal disagrees with lifecycle"),
                }
                terminal = Some(value);
            }
        }
        previous = Some(digest(line.as_bytes()));
    }
    let header = header.ok_or(ExecutionRecordError::Invalid("no continuation header"))?;
    let state = if active.is_some() {
        "unknown_call_result"
    } else if failed.is_some() && terminal.is_none() {
        "open_after_failure"
    } else {
        match &terminal {
            Some(ContinuationTerminal::Completed) => "completed",
            Some(ContinuationTerminal::Interrupted { .. }) => "interrupted",
            Some(ContinuationTerminal::Failed) => "failed",
            None => "open_after_return",
        }
    };
    let occupied = if failed.is_some() || active.is_some() {
        1
    } else {
        0
    };
    let never_started = header
        .remaining_candidate_ids
        .len()
        .saturating_sub(successful + occupied);
    let terminal_recorded = terminal.is_some();
    Ok(ContinuationJournalSummary {
        schema: "prospect.execution-continuation-inspection/v1",
        journal_sha256: digest(child.as_bytes()),
        child_run_id: header.run_id,
        parent_run_id: header.parent.run_id,
        parent_journal_sha256: header.parent.journal_sha256,
        source_bundle_sha256: header.parent.bundle_sha256,
        expectation_sha256: header.parent.expectation_sha256,
        state,
        successful_new_candidates: successful,
        failed_candidate: failed,
        unknown_candidate: active,
        never_started_candidates: never_started,
        terminal_recorded,
        resume_authorized: false,
        evidence_kind: "parent_linked_continuation_consistency_only",
    })
}

#[cfg(test)]
mod tests;
