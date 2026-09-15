# Controlled prospective evaluation

This additive API controls the evaluation phase of a campaign without replacing
`evaluate_batch` or `execute_registered_bundle`. It retains completed work when
execution is interrupted or an engine call returns an error. It does not implement
persistent checkpoints, automatic retries, numerical algorithms or physical rollback.

## Direct engine API

Use `prospect_scenario::controlled::evaluate_batch_controlled` with an existing
`ProspectiveEngine<State, Intervention>`, a vector of named scenarios, an explicit
`EvaluationControl` and a synchronous progress callback. Engines need not be
`Clone`, `Send`, or `Sync` for this direct sequential API; borrowed trait objects
are accepted. The registered-adapter API retains its existing `Send + Sync` rules.

```rust
use prospect_core::{ProspectiveEngine, Scenario, ScenarioId};
use prospect_scenario::controlled::{
    EvaluationControl, ExecutionState, ProgressEvent, evaluate_batch_controlled,
};

// Software-control example, not observed model or scientific evidence.
struct Add;
impl ProspectiveEngine<i32, i32> for Add {
    type Signature = i32;
    type Error = std::convert::Infallible;
    fn baseline(&self, state: &i32) -> Result<i32, Self::Error> { Ok(*state) }
    fn evaluate(&self, state: &i32, action: &i32) -> Result<i32, Self::Error> {
        Ok(*state + *action)
    }
}

let control = EvaluationControl::new(2);
let cancel = control.cancellation_token();
let scenarios = vec![
    Scenario::new(ScenarioId::new("a").unwrap(), 1),
    Scenario::new(ScenarioId::new("b").unwrap(), 2),
];
let report = evaluate_batch_controlled(&Add, &10, scenarios, &control, |update| {
    if matches!(update.event, ProgressEvent::ScenarioCompleted { index: 0, .. }) {
        cancel.cancel();
    }
});
assert_eq!(report.state(), ExecutionState::Interrupted);
assert_eq!(report.baseline(), Some(&10));
assert_eq!(report.outcomes().len(), 1);
assert_eq!(report.pending().len(), 1);
assert!(report.into_completed_batch().is_err());
```

The runnable `controlled_evaluation` example demonstrates both a complete run and
an interrupted prefix:

```bash
cargo +1.89.0 run --locked -p prospect-scenario --example controlled_evaluation
```

## Terminal state and preserved work

`BatchExecution` has private fields and exposes these terminal states:

| State | Meaning | Preserved data |
| --- | --- | --- |
| `Completed` | Baseline and every candidate succeeded; final checkpoint allowed completion | Baseline and all outcomes |
| `Interrupted` | Cancellation, deadline, or candidate-call quota stopped the run | Successful baseline/outcomes and never-started candidates |
| `Failed` | An actual engine call returned an error | Original error, failed candidate (or `None` for baseline failure), successful prefix and never-started candidates |
| `Rejected` | Duplicate scenario IDs would make progress ambiguous | Original input, duplicate ID; no engine call |

A failed candidate is separate from `pending()`. The latter contains only
never-started candidates; it is not a retry queue. In a candidate failure,
`successful outcomes + failed candidate + pending = original candidate count`.
Baseline failure preserves all candidates as pending. No failure is replaced by
a zero-valued signature, empty successful result, estimated metric or later retry.

`into_completed_batch` returns the existing `BatchResult` only for `Completed`.
It returns the entire report unchanged for every other status. Existing scoring
and ranking functions continue to require a `BatchResult`; they do not accept an
incomplete report. Applications can inspect partial signatures, but must not
relabel a partial run as a completed experiment.

## Exact control semantics

`EvaluationControl::new(n)` permits at most `n` candidate calls. The baseline is
not counted as a candidate. With nonempty input, a zero budget stops before the
baseline. With empty input, the baseline is still evaluated, matching the old
batch API, unless cancellation or an expired deadline prevents it.

Duplicate-ID admission runs before engine calls. Control checkpoints run before
the baseline, before each candidate, and after each successful call and progress
callback, including the last. At a checkpoint, cancellation wins over deadline,
and deadline wins over exhausted candidate quota. An engine error takes precedence
over a control request raised during that failed call, preserving the actual error.

A successful in-flight return is retained even if its call or callback crossed
the deadline or requested cancellation. The following checkpoint marks the run
interrupted. Therefore an interrupted report may contain all candidate outcomes;
counts alone do not authorize completion or ranking. A quota exactly equal to
the number of successful candidates does not interrupt an otherwise complete run.

The callback receives successful baseline/candidate notifications and one final
`Finished` notification. It is not called for a candidate that failed. The final
notification reports an already-finalized state: cancellation requested inside
that final callback does not retroactively change the report.

A cloned `CancellationToken` shares a one-way atomic signal. It has no reset.
Each `EvaluationControl::new` creates a new signal; cloning the control shares
its cancellation signal but does not create a global shared candidate counter.
Each evaluation uses its own quota. `with_deadline` accepts an absolute
`std::time::Instant`, with `now >= deadline` considered expired. Use
`Instant::checked_add` when deriving a deadline from a duration. An `Instant` is
process-local control data, not a persistent timestamp or portable checkpoint.

## Registered typed evaluation

`prospect_dispatch::execution::evaluate_registered_bundle_controlled` resolves
all adapter, upstream, metric and policy requirements using the existing resolver.
Unresolved requirements fail before engine calls or progress callbacks, including
when the supplied control is already cancelled. Metadata still comes from the
registered engine, not caller-supplied substitute metadata.

On successful preflight it returns `RegisteredBatchExecution`, retaining bundle
ID, declared seed, adapter metadata and the controlled evaluation report. A seed
in that wrapper is a declaration, not proof that a custom adapter consumed it.

This function is deliberately evaluation-only. It resolves metric/policy
requirements but never invokes their implementations, even on full completion.
A caller explicitly accepts the report and uses existing scoring/policy APIs
when appropriate. The original `execute_registered_bundle` continues to evaluate
and score as before; its behavior and signatures are not silently changed.

The generic control can be used with existing TDI, ElasticXxx and FLAT adapters
without copying their mathematics. This API's control tests do not qualify those
adapters' scientific behavior, production safety or physical effects.

## Limitations and next slices

Cancellation and deadlines are cooperative checkpoints, not forced preemption.
An adapter, callback, input clone, or requirement-resolution step can block or
exceed the deadline before control returns. Panics propagate normally. There is
no thread termination, hard wall-clock guarantee, RSS cap, transaction rollback
or I/O sandbox. The caller owns initial input allocation; duplicate-ID admission
and retained reports also use memory. A candidate quota is not a parser quota.

This slice provides no automatic restart, persistent event journal, cryptographic
binding between resumable state and input, or crash recovery. Those need a separate
versioned evidence/checkpoint contract and explicit retry/idempotency rules. In
particular, a failed domain call may have side effects; `pending` and status data
must not be used to infer safe replay or physical reversibility.

Tests use an internal injected clock rather than timing sleeps. They cover quotas,
cancellation around call/callback boundaries, deadlines including the final call,
error precedence, duplicate IDs, event counts, incomplete-conversion rejection,
legacy equivalence, and fail-closed registered preflight. All engines are software
fixtures: no CUDA run or measured model quality is established.

```bash
cargo +1.89.0 test --locked -p prospect-scenario
cargo +1.89.0 test --locked -p prospect-dispatch
cargo +1.89.0 clippy --locked --workspace --all-targets -- -D warnings
cargo +1.89.0 test --locked --workspace
```

Standard-library contracts: https://doc.rust-lang.org/std/time/struct.Instant.html
and https://doc.rust-lang.org/std/sync/atomic/struct.AtomicBool.html.
