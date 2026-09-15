//! Software-control fixtures only. No observed domain or model execution.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use super::*;

struct Fixture {
    calls: RefCell<Vec<Option<i32>>>,
    baseline: Box<dyn Fn() -> Result<i32, &'static str>>,
    candidate: Box<dyn Fn(i32) -> Result<i32, &'static str>>,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            baseline: Box::new(|| Ok(10)),
            candidate: Box::new(|value| Ok(10 + value)),
        }
    }
}

impl ProspectiveEngine<(), i32> for Fixture {
    type Signature = i32;
    type Error = &'static str;

    fn baseline(&self, _: &()) -> Result<i32, Self::Error> {
        self.calls.borrow_mut().push(None);
        (self.baseline)()
    }

    fn evaluate(&self, _: &(), value: &i32) -> Result<i32, Self::Error> {
        self.calls.borrow_mut().push(Some(*value));
        (self.candidate)(*value)
    }
}

fn candidates() -> Vec<Scenario<i32>> {
    [1, 2, 3].into_iter().map(|value| {
        Scenario::new(ScenarioId::new(format!("candidate-{value}")).unwrap(), value)
    }).collect()
}

fn interrupted(report: &BatchExecution<i32, i32, &'static str>, reason: InterruptionReason) {
    assert_eq!(report.state(), ExecutionState::Interrupted);
    assert!(matches!(report.status(), BatchStatus::Interrupted(actual) if *actual == reason));
}

#[test]
fn completion_matches_existing_batch_without_changing_input_order() {
    let engine = Fixture::default();
    let original = crate::evaluate_batch(&engine, &(), candidates()).unwrap();
    engine.calls.borrow_mut().clear();
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &EvaluationControl::new(3), |_| {});
    assert_eq!(report.state(), ExecutionState::Completed);
    assert_eq!(report.total_scenarios(), 3);
    assert!(report.pending().is_empty());
    assert_eq!(report.into_completed_batch().unwrap(), original);
    assert_eq!(*engine.calls.borrow(), vec![None, Some(1), Some(2), Some(3)]);
}

#[test]
fn quota_preserves_successful_prefix_and_never_started_candidates() {
    let engine = Fixture::default();
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &EvaluationControl::new(1), |_| {});
    interrupted(&report, InterruptionReason::EvaluationLimitReached);
    assert_eq!(report.baseline(), Some(&10));
    assert_eq!(report.outcomes().len(), 1);
    assert_eq!(*report.outcomes()[0].signature(), 11);
    assert_eq!(report.pending().iter().map(|s| *s.intervention()).collect::<Vec<_>>(), vec![2, 3]);
    assert_eq!(*engine.calls.borrow(), vec![None, Some(1)]);
    let intact = report.into_completed_batch().unwrap_err();
    assert_eq!(intact.outcomes().len(), 1);
    assert_eq!(intact.pending().len(), 2);
}

#[test]
fn zero_quota_stops_nonempty_input_before_baseline() {
    let engine = Fixture::default();
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &EvaluationControl::new(0), |_| {});
    interrupted(&report, InterruptionReason::EvaluationLimitReached);
    assert!(report.baseline().is_none());
    assert!(report.outcomes().is_empty());
    assert_eq!(report.pending().len(), 3);
    assert!(engine.calls.borrow().is_empty());
}

#[test]
fn empty_batch_still_requires_a_successful_baseline() {
    let engine = Fixture::default();
    let report = evaluate_batch_controlled(&engine, &(), Vec::new(), &EvaluationControl::new(0), |_| {});
    assert_eq!(report.total_scenarios(), 0);
    assert_eq!(report.state(), ExecutionState::Completed);
    assert_eq!(*report.into_completed_batch().unwrap().baseline(), 10);
    assert_eq!(*engine.calls.borrow(), vec![None]);
}

#[test]
fn pre_cancelled_control_never_calls_the_engine() {
    let control = EvaluationControl::new(3);
    control.cancellation_token().cancel();
    let engine = Fixture::default();
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &control, |_| {});
    interrupted(&report, InterruptionReason::Cancelled);
    assert!(engine.calls.borrow().is_empty());
    assert_eq!(report.pending().len(), 3);
}

#[test]
fn cancellation_signal_is_shared_across_threads_and_never_reset() {
    let token = CancellationToken::new();
    let handle = token.clone();
    std::thread::spawn(move || handle.cancel()).join().unwrap();
    assert!(token.is_cancelled());
    token.cancel();
    let control = EvaluationControl::new(3).with_cancellation(token);
    assert!(control.cancellation_token().is_cancelled());
    assert!(!EvaluationControl::new(3).cancellation_token().is_cancelled());
}

#[test]
fn cancellation_during_baseline_keeps_returned_baseline_without_candidates() {
    let control = EvaluationControl::new(3);
    let token = control.cancellation_token();
    let engine = Fixture { baseline: Box::new(move || { token.cancel(); Ok(10) }), ..Fixture::default() };
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &control, |_| {});
    interrupted(&report, InterruptionReason::Cancelled);
    assert_eq!(report.baseline(), Some(&10));
    assert!(report.outcomes().is_empty());
    assert_eq!(report.pending().len(), 3);
    assert_eq!(*engine.calls.borrow(), vec![None]);
}

#[test]
fn cancellation_during_candidate_keeps_returned_signature_without_retry() {
    let control = EvaluationControl::new(3);
    let token = control.cancellation_token();
    let engine = Fixture { candidate: Box::new(move |value| { token.cancel(); Ok(10 + value) }), ..Fixture::default() };
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &control, |_| {});
    interrupted(&report, InterruptionReason::Cancelled);
    assert_eq!(report.outcomes().len(), 1);
    assert_eq!(report.pending().len(), 2);
    assert_eq!(*engine.calls.borrow(), vec![None, Some(1)]);
}

#[test]
fn progress_callback_can_cancel_before_the_next_candidate() {
    let engine = Fixture::default();
    let control = EvaluationControl::new(3);
    let token = control.cancellation_token();
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &control, |update| {
        if matches!(update.event, ProgressEvent::ScenarioCompleted { index: 0, .. }) { token.cancel(); }
    });
    interrupted(&report, InterruptionReason::Cancelled);
    assert_eq!(*engine.calls.borrow(), vec![None, Some(1)]);
}

#[test]
fn cancellation_after_last_success_is_not_mislabeled_complete() {
    let engine = Fixture::default();
    let control = EvaluationControl::new(3);
    let token = control.cancellation_token();
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &control, |update| {
        if matches!(update.event, ProgressEvent::ScenarioCompleted { index: 2, .. }) { token.cancel(); }
    });
    interrupted(&report, InterruptionReason::Cancelled);
    assert_eq!(report.outcomes().len(), 3);
    assert!(report.pending().is_empty());
    assert!(report.into_completed_batch().is_err());
}

#[test]
fn terminal_notification_does_not_rewrite_finalized_state() {
    let engine = Fixture::default();
    let control = EvaluationControl::new(3);
    let token = control.cancellation_token();
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &control, |update| {
        if matches!(update.event, ProgressEvent::Finished(_)) { token.cancel(); }
    });
    assert!(control.cancellation_token().is_cancelled());
    assert_eq!(report.state(), ExecutionState::Completed);
}

#[test]
fn deadline_is_inclusive_and_blocks_baseline_without_sleep() {
    let deadline = Instant::now();
    let engine = Fixture::default();
    let control = EvaluationControl::new(3).with_deadline(deadline);
    let report = evaluate_with_clock(&engine, &(), candidates(), &control, |_| {}, || deadline);
    interrupted(&report, InterruptionReason::DeadlineReached);
    assert!(engine.calls.borrow().is_empty());
}

#[test]
fn baseline_overrun_is_interrupted_not_successful_even_for_empty_batch() {
    let start = Instant::now();
    let deadline = start.checked_add(Duration::from_secs(1)).unwrap();
    let clock = Rc::new(Cell::new(start));
    let update_clock = Rc::clone(&clock);
    let engine = Fixture { baseline: Box::new(move || { update_clock.set(deadline); Ok(10) }), ..Fixture::default() };
    let report = evaluate_with_clock(&engine, &(), Vec::new(), &EvaluationControl::new(0).with_deadline(deadline), |_| {}, || clock.get());
    interrupted(&report, InterruptionReason::DeadlineReached);
    assert_eq!(report.baseline(), Some(&10));
    assert_eq!(*engine.calls.borrow(), vec![None]);
}

#[test]
fn last_candidate_overrun_is_preserved_but_not_promoted_to_complete() {
    let start = Instant::now();
    let deadline = start.checked_add(Duration::from_secs(1)).unwrap();
    let clock = Rc::new(Cell::new(start));
    let update_clock = Rc::clone(&clock);
    let engine = Fixture { candidate: Box::new(move |value| {
        if value == 3 { update_clock.set(deadline); }
        Ok(10 + value)
    }), ..Fixture::default() };
    let report = evaluate_with_clock(&engine, &(), candidates(), &EvaluationControl::new(3).with_deadline(deadline), |_| {}, || clock.get());
    interrupted(&report, InterruptionReason::DeadlineReached);
    assert_eq!(report.outcomes().len(), 3);
    assert!(report.pending().is_empty());
    assert!(report.into_completed_batch().is_err());
}

#[test]
fn callback_time_is_observed_at_next_checkpoint() {
    let start = Instant::now();
    let deadline = start.checked_add(Duration::from_secs(1)).unwrap();
    let clock = Cell::new(start);
    let engine = Fixture::default();
    let report = evaluate_with_clock(&engine, &(), candidates(), &EvaluationControl::new(3).with_deadline(deadline), |update| {
        if matches!(update.event, ProgressEvent::BaselineCompleted) { clock.set(deadline); }
    }, || clock.get());
    interrupted(&report, InterruptionReason::DeadlineReached);
    assert_eq!(*engine.calls.borrow(), vec![None]);
}

#[test]
fn baseline_failure_preserves_error_and_never_started_input() {
    let engine = Fixture { baseline: Box::new(|| Err("baseline failed")), ..Fixture::default() };
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &EvaluationControl::new(3), |_| {});
    assert_eq!(report.state(), ExecutionState::Failed);
    assert!(matches!(report.status(), BatchStatus::EngineFailed { scenario: None, error: &"baseline failed" }));
    assert!(report.baseline().is_none());
    assert_eq!(report.pending().len(), 3);
    assert_eq!(*engine.calls.borrow(), vec![None]);
}

#[test]
fn candidate_failure_is_distinct_from_pending_and_no_retry_occurs() {
    let engine = Fixture { candidate: Box::new(|value| if value == 2 { Err("candidate failed") } else { Ok(10 + value) }), ..Fixture::default() };
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &EvaluationControl::new(3), |_| {});
    assert_eq!(report.state(), ExecutionState::Failed);
    match report.status() {
        BatchStatus::EngineFailed { scenario: Some(scenario), error } => {
            assert_eq!(scenario.id().as_str(), "candidate-2");
            assert_eq!(*error, "candidate failed");
        }
        other => panic!("unexpected status {other:?}"),
    }
    assert_eq!(report.outcomes().len(), 1);
    assert_eq!(report.pending().len(), 1);
    assert_eq!(*report.pending()[0].intervention(), 3);
    assert_eq!(*engine.calls.borrow(), vec![None, Some(1), Some(2)]);
    assert!(report.into_completed_batch().is_err());
}

#[test]
fn engine_error_takes_precedence_over_a_concurrent_cancellation() {
    let control = EvaluationControl::new(3);
    let token = control.cancellation_token();
    let engine = Fixture { candidate: Box::new(move |_| { token.cancel(); Err("actual engine failure") }), ..Fixture::default() };
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &control, |_| {});
    assert_eq!(report.state(), ExecutionState::Failed);
    assert!(matches!(report.status(), BatchStatus::EngineFailed { error: &"actual engine failure", .. }));
}

#[test]
fn duplicate_ids_reject_before_any_engine_call_and_preserve_all_inputs() {
    let mut input = candidates();
    input.push(Scenario::new(input[0].id().clone(), 99));
    let engine = Fixture::default();
    let mut events = Vec::new();
    let report = evaluate_batch_controlled(&engine, &(), input, &EvaluationControl::new(10), |update| {
        if let ProgressEvent::Finished(state) = update.event { events.push(state); }
    });
    assert_eq!(report.state(), ExecutionState::Rejected);
    assert!(matches!(report.status(), BatchStatus::DuplicateScenarioId(id) if id.as_str() == "candidate-1"));
    assert_eq!(report.pending().len(), 4);
    assert_eq!(report.total_scenarios(), 4);
    assert!(engine.calls.borrow().is_empty());
    assert_eq!(events, vec![ExecutionState::Rejected]);
}

#[test]
fn progress_has_ordered_success_events_and_exactly_one_terminal_event() {
    let engine = Fixture::default();
    let mut events = Vec::new();
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &EvaluationControl::new(3), |update| {
        assert_eq!(update.total_scenarios, 3);
        events.push((format!("{:?}", update.event), update.completed_scenarios));
    });
    assert_eq!(report.state(), ExecutionState::Completed);
    assert_eq!(events.len(), 5);
    assert_eq!(events[0], ("BaselineCompleted".to_owned(), 0));
    for index in 0..3 {
        assert!(events[index + 1].0.contains(&format!("index: {index}")));
        assert!(events[index + 1].0.contains(&format!("candidate-{}", index + 1)));
        assert_eq!(events[index + 1].1, index + 1);
    }
    assert_eq!(events[4], ("Finished(Completed)".to_owned(), 3));
}

#[test]
fn failure_emits_no_success_event_for_the_failed_candidate() {
    let engine = Fixture { candidate: Box::new(|_| Err("failed")), ..Fixture::default() };
    let mut events = Vec::new();
    let report = evaluate_batch_controlled(&engine, &(), candidates(), &EvaluationControl::new(3), |update| {
        events.push(format!("{:?}", update.event));
    });
    assert_eq!(report.state(), ExecutionState::Failed);
    assert_eq!(events, vec!["BaselineCompleted", "Finished(Failed)"]);
}

#[test]
fn control_precedence_is_cancellation_then_deadline_then_candidate_quota() {
    let deadline = Instant::now();
    let control = EvaluationControl::new(0).with_deadline(deadline);
    assert_eq!(control.interruption_at(deadline, 0, 1), Some(InterruptionReason::DeadlineReached));
    control.cancellation_token().cancel();
    assert_eq!(control.interruption_at(deadline, 0, 1), Some(InterruptionReason::Cancelled));
}

#[test]
fn borrowed_dynamic_engines_do_not_require_clone_or_send() {
    let engine = Fixture::default();
    let erased: &dyn ProspectiveEngine<(), i32, Signature = i32, Error = &'static str> = &engine;
    let report = evaluate_batch_controlled(erased, &(), candidates(), &EvaluationControl::new(3), |_| {});
    assert_eq!(report.state(), ExecutionState::Completed);
}

#[test]
fn future_deadline_with_deterministic_clock_allows_completion() {
    let start = Instant::now();
    let deadline = start.checked_add(Duration::from_secs(1)).unwrap();
    let engine = Fixture::default();
    let report = evaluate_with_clock(&engine, &(), candidates(), &EvaluationControl::new(3).with_deadline(deadline), |_| {}, || start);
    assert_eq!(report.state(), ExecutionState::Completed);
}
