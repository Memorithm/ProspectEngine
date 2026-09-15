//! Reuse the existing evaluator; do not introduce a second scheduling loop.

use std::cell::RefCell;

use prospect_adapter::AdapterMetadata;
use prospect_bundle::ScenarioBundle;
use prospect_core::{ProspectiveEngine, Scenario};
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};
use prospect_scenario::{BatchResult, ScenarioOutcome};
use prospect_scenario::controlled::{
    BatchExecution, BatchStatus, EvaluationControl, ExecutionState, InterruptionReason,
    evaluate_batch_controlled,
};
use serde::Serialize;

use super::{CallTarget, Emitter, EngineIdentity, Event, Header, JournalError, JournalSink, Terminal, SCHEMA, validate_header};
use super::super::{
    BoundEvaluationError, ExecutionRecordError, MAX_RECORD_SCENARIOS, PayloadCodecs,
    RecordInterruption, check_payload, digest, parse_bundle,
};
use super::super::super::{BundleExecutionError, ExecutableAdapterRegistry};
use crate::resolve_bundle_requirements;
use prospect_evidence::RunId;

/// Explicit destination, implementation declarations and fallible application codecs.
///
/// Each value is consumed by one run. A sink must be fresh and single-writer;
/// sharing an existing destination across runs violates its contract.
pub struct JournalCapture<'a, W: ?Sized, FS, FE> {
    sink: &'a mut W,
    run_id: RunId,
    implementation: EngineIdentity,
    codecs: PayloadCodecs,
    encode_signature: FS,
    encode_error: FE,
}
impl<'a, W: JournalSink + ?Sized, FS, FE> JournalCapture<'a, W, FS, FE> {
    /// Configure a run without writing or invoking a model. Codecs remain explicit.
    pub fn new(sink: &'a mut W, run_id: RunId, implementation: EngineIdentity,
               codecs: PayloadCodecs, encode_signature: FS, encode_error: FE) -> Self {
        Self { sink, run_id, implementation, codecs, encode_signature, encode_error }
    }
}

/// Journal failure is distinct from an engine failure and from cancellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalRunState { Completed, Interrupted, EngineFailed, JournalFailed, Rejected }

#[derive(Debug)]
enum CallFailure<E> { BlockedBeforeEngine, Engine(E) }

/// In-memory truth and storage failure are retained separately.
///
/// A successful engine return survives a failed append/codec. A call blocked by
/// journaling is never reported as an engine failure or removed from unstarted
/// work. No record loaded from disk can construct this value or authorize replay.
#[derive(Debug)]
#[must_use = "inspect both journal state and the actual engine outcome"]
pub struct JournalRun<I, S, E> {
    input_json: String,
    adapter: AdapterMetadata,
    evaluation: BatchExecution<I, S, CallFailure<E>>,
    journal_error: Option<JournalError>,
}
impl<I, S, E> JournalRun<I, S, E> {
    /// Canonical input captured and validated before all engine calls.
    #[must_use]
    pub fn input_json(&self) -> &str { &self.input_json }
    /// Metadata obtained from the actual registered adapter, not from a file.
    #[must_use]
    pub const fn adapter(&self) -> &AdapterMetadata { &self.adapter }
    /// Classify storage failures independently, including after a successful last call.
    #[must_use]
    pub fn state(&self) -> JournalRunState {
        if self.journal_error.is_some() { return JournalRunState::JournalFailed; }
        match self.evaluation.state() {
            ExecutionState::Completed => JournalRunState::Completed,
            ExecutionState::Interrupted => JournalRunState::Interrupted,
            ExecutionState::Failed => JournalRunState::EngineFailed,
            ExecutionState::Rejected => JournalRunState::Rejected,
        }
    }
    /// Original storage or encoding failure; no retry of the failed write occurred.
    #[must_use]
    pub const fn journal_error(&self) -> Option<&JournalError> { self.journal_error.as_ref() }
    /// Actual successful baseline, even if its journal acknowledgement failed.
    #[must_use]
    pub fn baseline(&self) -> Option<&S> { self.evaluation.baseline() }
    /// Actual successful returns, in input order; not necessarily persisted.
    #[must_use]
    pub fn outcomes(&self) -> &[ScenarioOutcome<I, S>] { self.evaluation.outcomes() }
    /// Actual engine error, retained even if encoding or persisting that error failed.
    #[must_use]
    pub fn engine_error(&self) -> Option<&E> {
        match self.evaluation.status() {
            BatchStatus::EngineFailed { error: CallFailure::Engine(error), .. } => Some(error),
            _ => None,
        }
    }
    /// Target of the actual engine failure, not a call blocked before invocation.
    #[must_use]
    pub fn failed_call(&self) -> Option<CallTarget> {
        match self.evaluation.status() {
            BatchStatus::EngineFailed { scenario, error: CallFailure::Engine(_) } => Some(match scenario {
                Some(s) => CallTarget::Scenario { id: s.id().as_str().to_owned() },
                None => CallTarget::Baseline,
            }),
            _ => None,
        }
    }
    /// Never-invoked domain candidates, including any blocked wrapper call.
    #[must_use]
    pub fn never_started(&self) -> Vec<&Scenario<I>> {
        let mut pending = Vec::new();
        if let BatchStatus::EngineFailed { scenario: Some(s), error: CallFailure::BlockedBeforeEngine } = self.evaluation.status() {
            pending.push(s);
        }
        pending.extend(self.evaluation.pending().iter());
        pending
    }
    /// Cooperative interruption only when no journal error superseded its diagnosis.
    #[must_use]
    pub fn interruption(&self) -> Option<InterruptionReason> {
        if self.journal_error.is_some() { return None; }
        match self.evaluation.status() { BatchStatus::Interrupted(reason) => Some(*reason), _ => None }
    }
    /// Accept only a completed, journal-acknowledged run for existing ranking APIs.
    ///
    /// Return incomplete/failed state intact on the error path. A terminal journal
    /// acknowledgement is necessary but is not scientific or hardware attestation.
    pub fn into_completed_batch(self) -> Result<BatchResult<I, S>, Box<Self>> {
        if self.state() != JournalRunState::Completed { return Err(Box::new(self)); }
        let Self { input_json, adapter, evaluation, journal_error } = self;
        match evaluation.into_completed_batch() {
            Ok(batch) => Ok(batch),
            Err(evaluation) => Err(Box::new(Self { input_json, adapter, evaluation, journal_error })),
        }
    }
}

struct Session<'a, W: ?Sized, FS, FE> {
    emitter: Emitter<'a, W>,
    error: Option<JournalError>,
    ids: Vec<String>,
    next: usize,
    encode_signature: FS,
    encode_error: FE,
}
impl<W: JournalSink + ?Sized, FS, FE> Session<'_, W, FS, FE> {
    fn emit(&mut self, event: Event) -> bool {
        if self.error.is_some() { return false; }
        match self.emitter.append(event) {
            Ok(()) => true,
            Err(error) => { self.error = Some(error); false }
        }
    }
    fn returned<S, E>(&mut self, target: CallTarget, result: &Result<S, E>)
    where FS: FnMut(&S) -> Result<String, String>, FE: FnMut(&E) -> Result<String, String> {
        let encoded = match result {
            Ok(signature) => (self.encode_signature)(signature),
            Err(error) => (self.encode_error)(error),
        }.map_err(ExecutionRecordError::Encoding).and_then(|payload| { check_payload(&payload)?; Ok(payload) });
        match encoded {
            Ok(payload) => {
                let event = if result.is_ok() { Event::CallSucceeded { target, payload } }
                    else { Event::CallFailed { target, error_payload: payload } };
                self.emit(event);
            }
            Err(error) => self.error = Some(error.into()),
        }
    }
}
struct RecordingEngine<'a, E: ?Sized, W: ?Sized, FS, FE> {
    engine: &'a E,
    session: RefCell<Session<'a, W, FS, FE>>,
}
fn invoke<W: JournalSink + ?Sized, FS, FE, S, E>(
    session: &RefCell<Session<'_, W, FS, FE>>, target: CallTarget,
    call: impl FnOnce() -> Result<S, E>,
) -> Result<S, CallFailure<E>>
where FS: FnMut(&S) -> Result<String, String>, FE: FnMut(&E) -> Result<String, String> {
    if !session.borrow_mut().emit(Event::CallStarted { target: target.clone() }) {
        return Err(CallFailure::BlockedBeforeEngine);
    }
    let result = call();
    session.borrow_mut().returned(target, &result);
    // Preserve actual engine returns even if the subsequent journal write failed.
    // The poisoned session blocks the next DOMAIN call without mutating user control.
    result.map_err(CallFailure::Engine)
}
impl<State, I, E, W, FS, FE> ProspectiveEngine<State, I> for RecordingEngine<'_, E, W, FS, FE>
where E: ProspectiveEngine<State, I> + ?Sized, W: JournalSink + ?Sized,
      FS: FnMut(&E::Signature) -> Result<String, String>, FE: FnMut(&E::Error) -> Result<String, String> {
    type Signature = E::Signature;
    type Error = CallFailure<E::Error>;
    fn baseline(&self, state: &State) -> Result<Self::Signature, Self::Error> {
        invoke(&self.session, CallTarget::Baseline, || self.engine.baseline(state))
    }
    fn evaluate(&self, state: &State, intervention: &I) -> Result<Self::Signature, Self::Error> {
        let target = {
            let mut session = self.session.borrow_mut();
            if session.error.is_some() { return Err(CallFailure::BlockedBeforeEngine); }
            let Some(id) = session.ids.get(session.next).cloned() else {
                session.error = Some(ExecutionRecordError::Invalid("journal input order exhausted").into());
                return Err(CallFailure::BlockedBeforeEngine);
            };
            session.next += 1;
            CallTarget::Scenario { id }
        };
        invoke(&self.session, target, || self.engine.evaluate(state, intervention))
    }
}

/// Journal before/after each domain call while reusing the controlled evaluator.
///
/// All bundle/adapter/metric/policy admission occurs before the initial journal
/// entry and before the engine. Every call requires an acknowledged intent.
/// Storage or encoding failure blocks all later domain calls, preserves actual
/// returned values/errors in memory and leaves the failed journal untouched.
/// No partial result is ranked, no engine call retried and no control token altered.
///
/// This is single-writer live journaling, not an engine checkpoint or automatic
/// resume. Panics propagate and can leave an unmatched intent. Codecs and sinks
/// are trusted; checkpoints do not preempt a blocking journal, codec or engine.
pub fn evaluate_registered_bundle_journaled<State, I, S, E, M, P, W, FS, FE>(
    bundle: &ScenarioBundle<State, I>,
    adapters: &ExecutableAdapterRegistry<State, I, S, E>,
    metrics: &MetricRegistry<S, M>, policies: &DecisionPolicyRegistry<S, P>,
    control: &EvaluationControl, capture: JournalCapture<'_, W, FS, FE>,
) -> Result<JournalRun<I, S, E>, BoundEvaluationError<E>>
where State: Serialize, I: Clone + Serialize, P: Ord, W: JournalSink + ?Sized,
      FS: FnMut(&S) -> Result<String, String>, FE: FnMut(&E) -> Result<String, String> {
    if bundle.scenarios().len() > MAX_RECORD_SCENARIOS {
        return Err(BoundEvaluationError::Input(ExecutionRecordError::Invalid("too many scenarios")));
    }
    let input_json = bundle.canonical_json().map_err(|_| BoundEvaluationError::Input(ExecutionRecordError::Invalid("bundle serialization failed")))?;
    let parsed = parse_bundle(&input_json).map_err(BoundEvaluationError::Input)?;
    let catalog = adapters.owned_metadata_catalog();
    let resolved = resolve_bundle_requirements(bundle, &catalog, metrics, policies)
        .map_err(|e| BoundEvaluationError::Dispatch(BundleExecutionError::Dispatch(e)))?;
    let adapter = resolved.adapter().clone();
    let id = adapter.adapter_id().as_str();
    let engine = adapters.engine(id).ok_or_else(|| BoundEvaluationError::Dispatch(BundleExecutionError::RegistryInvariant(id.to_owned())))?;
    let header = Header {
        schema: SCHEMA.into(), run_id: capture.run_id.as_str().to_owned(),
        bundle_sha256: digest(input_json.as_bytes()),
        adapter: serde_json::to_value(&adapter).map_err(|e| BoundEvaluationError::Input(e.into()))?,
        implementation: capture.implementation, codecs: capture.codecs,
        max_evaluations: control.max_evaluations(), deadline_configured: control.deadline().is_some(),
    };
    validate_header(&header, &parsed, &input_json).map_err(BoundEvaluationError::Input)?;
    let ids = bundle.scenarios().iter().map(|s| s.id().as_str().to_owned()).collect();
    let scenarios = bundle.scenarios().iter().map(|s| Scenario::new(s.id().clone(), s.intervention().clone())).collect();
    let mut session = Session {
        emitter: Emitter { sink: capture.sink, sequence: 0, previous: None, bytes: 0 },
        error: None, ids, next: 0, encode_signature: capture.encode_signature, encode_error: capture.encode_error,
    };
    session.emit(Event::Initialized { header });
    let wrapped = RecordingEngine { engine, session: RefCell::new(session) };
    let evaluation = evaluate_batch_controlled(&wrapped, bundle.state(), scenarios, control, |_| {});
    let mut session = wrapped.session.into_inner();
    if session.error.is_none() {
        let terminal = match evaluation.status() {
            BatchStatus::Completed => Some(Terminal::Completed),
            BatchStatus::Interrupted(reason) => Some(Terminal::Interrupted { reason: match reason {
                InterruptionReason::Cancelled => RecordInterruption::Cancelled,
                InterruptionReason::DeadlineReached => RecordInterruption::DeadlineReached,
                InterruptionReason::EvaluationLimitReached => RecordInterruption::EvaluationLimitReached,
            }}),
            BatchStatus::EngineFailed { error: CallFailure::Engine(_), .. } => Some(Terminal::Failed),
            _ => None,
        };
        if let Some(terminal) = terminal { session.emit(Event::Finished { terminal }); }
        else { session.error = Some(ExecutionRecordError::Invalid("unexpected journal evaluator state").into()); }
    }
    Ok(JournalRun { input_json, adapter, evaluation, journal_error: session.error })
}
