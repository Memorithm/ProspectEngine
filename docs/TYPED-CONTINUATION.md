# Parent-linked typed continuation

ProspectEngine can continue a previously journaled run only through the explicit,
fail-closed continuation contract in
`prospect_dispatch::execution::record::journal::recovery::continuation`.

This path is intentionally narrower than a generic retry mechanism. It is admitted
only after the externally anchored restart preflight has accepted a source journal
whose application semantics are declared `PureIndependent`. A failed parent call,
an unknown call result, an incomplete tail, a stateful/effectful declaration, an
already complete run, or the absence of never-started candidates prevents plan
construction.

## Preparation

`prepare_typed_continuation` takes the exact parent journal, the original typed
scenario bundle, the separately trusted `RestartExpectations`, the independently
computed implementation-artifact SHA-256, and an application-provided signature
decoder.

It first reruns the existing external restart preflight. Only acknowledged
`call_succeeded` payloads are decoded. The decoder is responsible for the semantics
of the codec ID recorded in the trusted expectations; ProspectEngine does not infer
a Rust type from an arbitrary string.

The resulting `TypedContinuationPlan` contains:

- the exact source run, journal, bundle and expectation identities;
- the admitted implementation and codec identities;
- the restored baseline signature;
- the ordered prefix of restored candidate signatures;
- exactly the suffix of scenarios that the source journal proves were never started.

No engine or output sink is touched while the plan is prepared. A decoder failure,
identity mismatch or recovery blocker therefore occurs before continuation-side
domain calls.

## Child execution

`execute_typed_continuation` starts a fresh child run and a fresh child journal.
The child `RunId` must differ from the parent `RunId`. The child header binds the
parent run/journal/bundle/expectation identities, the adapter metadata obtained from
the registered engine, implementation and codecs, the restored prefix IDs, the
remaining suffix IDs, and the new cooperative control configuration.

The registered adapter and every declared bundle requirement are resolved again
before the first child domain call. The admitted implementation, codecs and adapter
metadata must match the parent expectations exactly.

The continuation never invokes `ProspectiveEngine::baseline`. The restored parent
baseline is the baseline for the chain. Only the never-started suffix is passed to
the engine. Parent successes are not rerun; failed or unknown parent calls cannot
form a plan in the first place.

Each child candidate follows the same write-ahead discipline as the live parent
journal: its `call_started` intent must be acknowledged before the engine call and
its actual success/failure is written afterwards. Journal/encoding errors stop later
domain calls without fabricating outcomes. Actual successful returns and actual
engine errors remain available in memory even if their later journal write fails.

## Completion and interruption

`ContinuationRunState` distinguishes `Completed`, `Interrupted`, `EngineFailed`,
and `JournalFailed`. Preparation/admission rejection is returned as an error before
a child run state exists; it is not a `ContinuationRunState` variant. A completed
child means the admitted suffix ran to completion under the cooperative control and
its terminal child-journal entry was acknowledged. It does not by itself authenticate
hardware or establish scientific validity.

Quota, cancellation and deadline semantics remain cooperative. In particular, an
in-flight candidate is not preempted. If cancellation/deadline becomes visible only
after the final successful candidate return, that return is retained but the child
remains `Interrupted`; the presence of every suffix signature does not silently
rewrite the terminal state to `Completed`.

An `evaluation_limit_reached` terminal is valid only when the number of successful
child candidates exactly reaches the configured child quota while work remains.
A `deadline_reached` terminal is valid only when the child header actually declared
a deadline. These checks prevent a coherently rehashed child journal from inventing
an impossible interruption reason.

## Independent child-journal inspection

`inspect_continuation_journal` verifies the child journal against the exact parent
journal, original canonical bundle and external restart expectations. It checks:

- the canonical JSON-lines encoding and hash chain;
- the parent/child run linkage;
- exact parent journal, bundle and expectation identities;
- implementation, codec and adapter identities;
- the parent lifecycle itself, including its successful-candidate count and exact
  never-started suffix;
- exact equality between that proven parent partition and the child's declared
  restored-prefix/remaining-suffix split;
- candidate-call ordering and quota;
- child success/failure/terminal lifecycle consistency;
- per-entry and total journal size limits.

The inspector validates trusted identities in both directions. It first parses the
actual parent header and requires its run ID, declared implementation, codec IDs and
canonical adapter metadata to match the separately supplied `RestartExpectations`.
Only then can it compare the child header with the same expectations. Supplying a
self-consistent but wrong expectation object and coherently rehashing the child does
not make that expectation true of the anchored parent.

The parent partition check is also independent of the child's internal consistency.
A child journal cannot enlarge its claimed restored prefix merely by coherently
rehashing itself and omitting one of the candidate calls that the parent never
actually completed.

It never decodes payloads, executes an adapter, modifies either journal or emits a
resume capability. `resume_authorized` remains false in its summary. A child journal
with an unmatched intent represents an unknown child call result and is not a retry
queue.

## What this does not provide

This feature is safe only under the admitted `PureIndependent` application contract.
It does not implement general state restoration, exactly-once external effects,
transactional actuation recovery, physical rollback, process/hardware attestation,
or automatic reconciliation of uncertain operations.

It also deliberately keeps restored parent results and new child results distinct.
A later assembly layer must verify the complete parent+child chain before rebuilding
a rankable `BatchResult`. Partial or interrupted chains must not enter scoring,
Pareto or lexicographic selection as if they were complete experiments.

All current continuation tests use software fixtures. They establish control-flow,
identity and persistence behavior only; they are not CUDA/model-quality evidence.
