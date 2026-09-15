//! Cooperative, sequential evaluation with explicit incomplete results.
//!
//! No retry, persistence, rollback, thread termination or engine preemption is
//! performed. Limits apply at call boundaries, not inside a domain adapter.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};

use crate::{BatchResult, ScenarioOutcome};

/// Cloneable, one-way cancellation signal. Clones refer to the same signal.
///
/// ```
/// use prospect_scenario::controlled::CancellationToken;
/// let token = CancellationToken::new();
/// let worker_token = token.clone();
/// token.cancel();
/// assert!(worker_token.is_cancelled());
/// ```
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Construct a fresh, uncancelled signal; there is deliberately no reset.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Does not interrupt an in-flight engine call.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Read the shared cancellation signal.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Immutable policy for one evaluation. The baseline is not a candidate call.
///
/// A zero candidate budget stops a non-empty batch before its baseline. An empty
/// batch still evaluates its baseline, like `evaluate_batch`, unless cancelled
/// or past its deadline. The caller owns allocation of the input scenarios.
///
/// ```
/// use std::time::{Duration, Instant};
/// use prospect_scenario::controlled::EvaluationControl;
/// let deadline = Instant::now().checked_add(Duration::from_secs(30)).unwrap();
/// let control = EvaluationControl::new(100).with_deadline(deadline);
/// assert_eq!(control.max_evaluations(), 100);
/// assert_eq!(control.deadline(), Some(deadline));
/// ```
#[derive(Clone, Debug)]
pub struct EvaluationControl {
    max_evaluations: usize,
    deadline: Option<Instant>,
    cancellation: CancellationToken,
}

impl EvaluationControl {
    /// Set the maximum candidate calls. No default unbounded budget is implied.
    #[must_use]
    pub fn new(max_evaluations: usize) -> Self {
        Self {
            max_evaluations,
            deadline: None,
            cancellation: CancellationToken::new(),
        }
    }

    /// Set an absolute, process-local monotonic deadline; equality is expired.
    #[must_use]
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Use an existing signal, including one cancelled before admission.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// Maximum candidate evaluations; baseline evaluation is counted separately.
    #[must_use]
    pub const fn max_evaluations(&self) -> usize {
        self.max_evaluations
    }

    /// Absolute local deadline, not a timestamp suitable for persistent replay.
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Obtain a handle that can request cancellation from another thread.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    fn interruption_at(
        &self,
        now: Instant,
        completed: usize,
        pending: usize,
    ) -> Option<InterruptionReason> {
        if self.cancellation.is_cancelled() {
            Some(InterruptionReason::Cancelled)
        } else if self.deadline.is_some_and(|deadline| now >= deadline) {
            Some(InterruptionReason::DeadlineReached)
        } else if pending != 0 && completed >= self.max_evaluations {
            Some(InterruptionReason::EvaluationLimitReached)
        } else {
            None
        }
    }
}

/// Which control stopped an otherwise successful evaluation prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptionReason {
    Cancelled,
    DeadlineReached,
    EvaluationLimitReached,
}

/// Terminal state, independent of the domain's error and signature types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionState {
    Completed,
    Interrupted,
    Failed,
    Rejected,
}

/// Terminal detail. The failed candidate is NOT included in `pending()`.
///
/// `scenario: None` identifies a baseline failure. A failed call may have domain
/// side effects; neither its retry safety nor its rollback is inferred here.
#[derive(Debug)]
pub enum BatchStatus<I, E> {
    Completed,
    Interrupted(InterruptionReason),
    EngineFailed {
        scenario: Option<Scenario<I>>,
        error: E,
    },
    DuplicateScenarioId(ScenarioId),
}

/// Completed work and remaining input, even when the overall batch did not finish.
///
/// Only `into_completed_batch` can produce the existing rankable `BatchResult`.
/// A report interrupted after the last call can contain every signature but is
/// still incomplete. This report is in-memory diagnostic data, not a checkpoint,
/// observed scientific evidence or an automatic-resume authorization.
#[derive(Debug)]
#[must_use = "inspect the terminal status; a partial report is not a completed batch"]
pub struct BatchExecution<I, S, E> {
    total_scenarios: usize,
    baseline: Option<S>,
    outcomes: Vec<ScenarioOutcome<I, S>>,
    pending: Vec<Scenario<I>>,
    status: BatchStatus<I, E>,
}

impl<I, S, E> BatchExecution<I, S, E> {
    /// Count of input candidates, including any failed or unstarted candidates.
    #[must_use]
    pub const fn total_scenarios(&self) -> usize {
        self.total_scenarios
    }

    /// Baseline only when its call actually returned success.
    #[must_use]
    pub const fn baseline(&self) -> Option<&S> {
        self.baseline.as_ref()
    }

    /// Successful candidate prefix in input order, without fabricated outcomes.
    #[must_use]
    pub fn outcomes(&self) -> &[ScenarioOutcome<I, S>] {
        &self.outcomes
    }

    /// Candidates never called, in input order. The failed call is in `status`.
    #[must_use]
    pub fn pending(&self) -> &[Scenario<I>] {
        &self.pending
    }

    /// Terminal reason, preserving the actual engine error and failed candidate.
    #[must_use]
    pub const fn status(&self) -> &BatchStatus<I, E> {
        &self.status
    }

    /// Terminal classification; it never derives success merely from counts.
    #[must_use]
    pub const fn state(&self) -> ExecutionState {
        match self.status {
            BatchStatus::Completed => ExecutionState::Completed,
            BatchStatus::Interrupted(_) => ExecutionState::Interrupted,
            BatchStatus::EngineFailed { .. } => ExecutionState::Failed,
            BatchStatus::DuplicateScenarioId(_) => ExecutionState::Rejected,
        }
    }

    /// Convert only a fully completed report; otherwise return it intact.
    ///
    /// This gate prevents an incomplete prefix from entering existing generic
    /// scoring/ranking APIs accidentally. It does not establish scientific validity.
    pub fn into_completed_batch(self) -> Result<BatchResult<I, S>, Self> {
        match self {
            Self {
                baseline: Some(baseline),
                outcomes,
                status: BatchStatus::Completed,
                ..
            } => Ok(BatchResult { baseline, outcomes }),
            incomplete => Err(incomplete),
        }
    }
}

/// Synchronous observations of successful returns and one final terminal state.
///
/// The terminal notification occurs after the final checkpoint and cannot change
/// the finalized report. Other callbacks may request cancellation. Callback and
/// engine panics propagate; the evaluator is not an exception isolation boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressEvent<'a> {
    BaselineCompleted,
    ScenarioCompleted {
        index: usize,
        scenario_id: &'a ScenarioId,
    },
    Finished(ExecutionState),
}

/// Progress counts are successful candidate returns, not attempted calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressUpdate<'a> {
    pub event: ProgressEvent<'a>,
    pub completed_scenarios: usize,
    pub total_scenarios: usize,
}

/// Evaluate sequentially with a candidate quota, cancellation and a deadline.
///
/// Duplicate IDs are rejected before any engine call. Checkpoints occur before
/// the baseline, before each candidate and after each successful call/callback,
/// including the last. Cancellation takes precedence over deadline, then quota.
/// An actual engine error takes precedence over a concurrent control request.
/// Successful in-flight returns are kept even when the following checkpoint stops
/// the run. No thread is killed, failed operation retried, or rollback attempted.
///
/// ```
/// use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};
/// use prospect_scenario::controlled::{evaluate_batch_controlled, EvaluationControl, ExecutionState};
/// struct Add;
/// impl ProspectiveEngine<i32, i32> for Add {
///     type Signature = i32;
///     type Error = std::convert::Infallible;
///     fn baseline(&self, state: &i32) -> Result<i32, Self::Error> { Ok(*state) }
///     fn evaluate(&self, state: &i32, action: &i32) -> Result<i32, Self::Error> {
///         Ok(*state + *action)
///     }
/// }
/// let scenarios = vec![
///     Scenario::new(ScenarioId::new("first").unwrap(), 2),
///     Scenario::new(ScenarioId::new("second").unwrap(), 3),
/// ];
/// let report = evaluate_batch_controlled(&Add, &10, scenarios, &EvaluationControl::new(1), |_| {});
/// assert_eq!(report.state(), ExecutionState::Interrupted);
/// assert_eq!(report.outcomes().len(), 1);
/// assert_eq!(report.pending().len(), 1);
/// assert!(report.into_completed_batch().is_err());
/// ```
pub fn evaluate_batch_controlled<E, State, I, F>(
    engine: &E,
    state: &State,
    scenarios: Vec<Scenario<I>>,
    control: &EvaluationControl,
    progress: F,
) -> BatchExecution<I, E::Signature, E::Error>
where
    E: ProspectiveEngine<State, I> + ?Sized,
    F: FnMut(ProgressUpdate<'_>),
{
    evaluate_with_clock(engine, state, scenarios, control, progress, Instant::now)
}

fn evaluate_with_clock<E, State, I, F, C>(
    engine: &E,
    state: &State,
    scenarios: Vec<Scenario<I>>,
    control: &EvaluationControl,
    mut progress: F,
    mut now: C,
) -> BatchExecution<I, E::Signature, E::Error>
where
    E: ProspectiveEngine<State, I> + ?Sized,
    F: FnMut(ProgressUpdate<'_>),
    C: FnMut() -> Instant,
{
    let total = scenarios.len();
    let duplicate = {
        let mut seen = BTreeSet::new();
        scenarios
            .iter()
            .find_map(|scenario| (!seen.insert(scenario.id())).then(|| scenario.id().clone()))
    };
    let mut report = BatchExecution {
        total_scenarios: total,
        baseline: None,
        outcomes: Vec::new(),
        pending: Vec::new(),
        status: BatchStatus::Completed,
    };
    let mut pending = scenarios.into_iter();
    if let Some(id) = duplicate {
        report.status = BatchStatus::DuplicateScenarioId(id);
    } else {
        loop {
            if let Some(reason) =
                control.interruption_at(now(), report.outcomes.len(), pending.len())
            {
                report.status = BatchStatus::Interrupted(reason);
                break;
            }
            if report.baseline.is_none() {
                match engine.baseline(state) {
                    Ok(baseline) => report.baseline = Some(baseline),
                    Err(error) => {
                        report.status = BatchStatus::EngineFailed {
                            scenario: None,
                            error,
                        };
                        break;
                    }
                }
                progress(ProgressUpdate {
                    event: ProgressEvent::BaselineCompleted,
                    completed_scenarios: 0,
                    total_scenarios: total,
                });
                continue;
            }
            let Some(scenario) = pending.next() else {
                break;
            };
            match engine.evaluate(state, scenario.intervention()) {
                Ok(signature) => {
                    let index = report.outcomes.len();
                    report.outcomes.push(ScenarioOutcome {
                        scenario,
                        signature,
                    });
                    progress(ProgressUpdate {
                        event: ProgressEvent::ScenarioCompleted {
                            index,
                            scenario_id: report.outcomes[index].scenario().id(),
                        },
                        completed_scenarios: index + 1,
                        total_scenarios: total,
                    });
                }
                Err(error) => {
                    report.status = BatchStatus::EngineFailed {
                        scenario: Some(scenario),
                        error,
                    };
                    break;
                }
            }
        }
    }
    report.pending = pending.collect();
    progress(ProgressUpdate {
        event: ProgressEvent::Finished(report.state()),
        completed_scenarios: report.outcomes.len(),
        total_scenarios: total,
    });
    report
}

#[cfg(test)]
mod tests;
