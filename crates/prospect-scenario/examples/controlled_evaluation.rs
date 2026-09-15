//! Software-control demonstration; no model or physical system is executed.

use std::convert::Infallible;

use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};
use prospect_scenario::controlled::{
    EvaluationControl, ExecutionState, ProgressEvent, evaluate_batch_controlled,
};

struct Add;

impl ProspectiveEngine<i32, i32> for Add {
    type Signature = i32;
    type Error = Infallible;

    fn baseline(&self, state: &i32) -> Result<i32, Self::Error> {
        Ok(*state)
    }

    fn evaluate(&self, state: &i32, intervention: &i32) -> Result<i32, Self::Error> {
        Ok(*state + *intervention)
    }
}

fn scenarios() -> Vec<Scenario<i32>> {
    [1, 2, 3]
        .into_iter()
        .map(|value| {
            Scenario::new(
                ScenarioId::new(format!("candidate-{value}")).unwrap(),
                value,
            )
        })
        .collect()
}

fn main() {
    let complete =
        evaluate_batch_controlled(&Add, &10, scenarios(), &EvaluationControl::new(3), |_| {});
    let batch = complete
        .into_completed_batch()
        .expect("all fixture calls succeed");
    println!(
        "complete: baseline={}, candidates={}",
        batch.baseline(),
        batch.outcomes().len()
    );

    let control = EvaluationControl::new(3);
    let cancel = control.cancellation_token();
    let partial = evaluate_batch_controlled(&Add, &10, scenarios(), &control, |update| {
        println!(
            "progress: {:?} ({}/{})",
            update.event, update.completed_scenarios, update.total_scenarios
        );
        if matches!(
            update.event,
            ProgressEvent::ScenarioCompleted { index: 0, .. }
        ) {
            cancel.cancel();
        }
    });
    assert_eq!(partial.state(), ExecutionState::Interrupted);
    println!(
        "interrupted: completed={}, never_started={}",
        partial.outcomes().len(),
        partial.pending().len()
    );
    assert!(partial.into_completed_batch().is_err());
}
