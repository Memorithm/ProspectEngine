//! Controlled evaluation using the existing typed registries.
//!
//! Requirement resolution is reused unchanged. Metric and policy implementations
//! are resolved but NOT called here: this API exposes evaluation, not a decision
//! over a partial batch. `execute_registered_bundle` retains its existing behavior.

use prospect_adapter::AdapterMetadata;
use prospect_bundle::ScenarioBundle;
use prospect_core::Scenario;
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};
use prospect_scenario::controlled::{
    BatchExecution, EvaluationControl, ExecutionState, ProgressUpdate, evaluate_batch_controlled,
};

use super::{BundleExecutionError, ExecutableAdapterRegistry};
use crate::resolve_bundle_requirements;

/// Typed bundle identity and the exact completed/interrupted/failed evaluation.
///
/// This is in-memory software progress, not persistent or authenticated evidence.
#[derive(Debug)]
#[must_use = "inspect evaluation status before using a partial bundle result"]
pub struct RegisteredBatchExecution<I, S, E> {
    bundle_id: String,
    seed: Option<u64>,
    adapter: AdapterMetadata,
    evaluation: BatchExecution<I, S, E>,
}

impl<I, S, E> RegisteredBatchExecution<I, S, E> {
    /// Identity of the supplied bundle, not a content hash.
    #[must_use]
    pub fn bundle_id(&self) -> &str {
        &self.bundle_id
    }

    /// Seed declared by the bundle, not a claim that an adapter used it.
    #[must_use]
    pub const fn seed(&self) -> Option<u64> {
        self.seed
    }

    /// Metadata obtained from the registered adapter implementation.
    #[must_use]
    pub const fn adapter(&self) -> &AdapterMetadata {
        &self.adapter
    }

    /// Inspect actual successful work, unstarted inputs and any engine failure.
    pub const fn evaluation(&self) -> &BatchExecution<I, S, E> {
        &self.evaluation
    }

    /// Terminal evaluation status, not a policy or scientific acceptance result.
    #[must_use]
    pub const fn state(&self) -> ExecutionState {
        self.evaluation.state()
    }

    /// Consume the wrapper while retaining the full evaluation report.
    ///
    /// Read or record bundle metadata first when it is needed downstream. Calling
    /// `into_completed_batch` on the returned report still rejects incomplete work.
    pub fn into_evaluation(self) -> BatchExecution<I, S, E> {
        self.evaluation
    }
}

/// Resolve every declared requirement, then evaluate with cooperative controls.
///
/// Missing/incompatible adapter, upstream, metric or policy requirements return
/// a preflight error without an engine call or progress notification. Engine
/// failures instead live in the returned report and preserve successful work.
///
/// Metrics and policies are resolved but never invoked. To score a completed
/// batch, the application must explicitly accept/convert the returned evaluation
/// and use the existing scoring functions. This function never auto-ranks a prefix,
/// retries an intervention or implements independent physical actuation. Registered
/// engines retain responsibility for any effects of their calls; no GPU qualification
/// or rollback is inferred from this report.
///
/// Cancellation/deadline checks govern engine-call boundaries only; they do not
/// preempt requirement resolution, intervention cloning, callbacks or an in-flight
/// adapter. Deadline construction and input allocation remain the caller's policy.
///
/// ```no_run
/// use prospect_bundle::ScenarioBundle;
/// use prospect_dispatch::execution::{
///     ExecutableAdapterRegistry, evaluate_registered_bundle_controlled,
/// };
/// use prospect_registry::{MetricRegistry, DecisionPolicyRegistry};
/// use prospect_scenario::controlled::EvaluationControl;
/// fn evaluate(
///     bundle: &ScenarioBundle<i32, i32>,
///     adapters: &ExecutableAdapterRegistry<i32, i32, i32, std::io::Error>,
///     metrics: &MetricRegistry<i32, i32>,
///     policies: &DecisionPolicyRegistry<i32, i32>,
/// ) {
///     let report = evaluate_registered_bundle_controlled(
///         bundle, adapters, metrics, policies, &EvaluationControl::new(100),
///         |update| eprintln!("{:?}", update.event),
///     ).expect("software requirements must resolve");
///     match report.into_evaluation().into_completed_batch() {
///         Ok(batch) => assert_eq!(batch.outcomes().len(), bundle.scenarios().len()),
///         Err(partial) => eprintln!("not complete: {:?}", partial.state()),
///     }
/// }
/// ```
pub fn evaluate_registered_bundle_controlled<State, I, S, E, M, P, F>(
    bundle: &ScenarioBundle<State, I>,
    adapters: &ExecutableAdapterRegistry<State, I, S, E>,
    metrics: &MetricRegistry<S, M>,
    policies: &DecisionPolicyRegistry<S, P>,
    control: &EvaluationControl,
    progress: F,
) -> Result<RegisteredBatchExecution<I, S, E>, BundleExecutionError<E>>
where
    I: Clone,
    P: Ord,
    F: FnMut(ProgressUpdate<'_>),
{
    let catalog = adapters.owned_metadata_catalog();
    let resolved = resolve_bundle_requirements(bundle, &catalog, metrics, policies)
        .map_err(BundleExecutionError::Dispatch)?;
    let adapter = resolved.adapter().clone();
    let adapter_id = adapter.adapter_id().as_str();
    let engine = adapters
        .engine(adapter_id)
        .ok_or_else(|| BundleExecutionError::RegistryInvariant(adapter_id.to_owned()))?;
    let scenarios = bundle
        .scenarios()
        .iter()
        .map(|scenario| Scenario::new(scenario.id().clone(), scenario.intervention().clone()))
        .collect();
    let evaluation =
        evaluate_batch_controlled(engine, bundle.state(), scenarios, control, progress);
    Ok(RegisteredBatchExecution {
        bundle_id: bundle.bundle_id().as_str().to_owned(),
        seed: bundle.seed(),
        adapter,
        evaluation,
    })
}

#[cfg(test)]
mod tests;
