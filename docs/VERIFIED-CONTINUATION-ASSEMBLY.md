# Verified continuation assembly

`prospect_dispatch::execution::record::journal::recovery::continuation::assembly`
reconstructs one complete typed `BatchResult` from an externally admitted parent
journal and one completed continuation child journal.

The assembly step is the boundary between persistence/recovery evidence and the
existing scoring/decision APIs. Parent and child results are deliberately kept
separate until this verification succeeds.

## Inputs

`assemble_completed_continuation` receives:

- the exact persisted parent journal bytes;
- the exact persisted child continuation journal bytes;
- a separately retained `ContinuationChildAnchor` containing the trusted child run ID and exact child-journal SHA-256;
- the original typed `ScenarioBundle`;
- the separately trusted `RestartExpectations`;
- the independently obtained implementation-artifact SHA-256;
- an application-provided decoder for the trusted signature codec.

The child anchor is an external trust input. It must be retained independently of
the mutable child bytes being inspected; constructing it from those same untrusted
bytes immediately before assembly defeats the tamper-detection boundary.

Assembly does not consume an in-memory `ContinuationRun` as authority and performs
no engine calls.

## Verification sequence

Before decoding either journal, the exact child bytes are hashed and must match the
SHA-256 in the separately retained child anchor. A mismatch fails closed before the
child journal can authenticate itself through a coherently rewritten hash chain.

The function then calls `prepare_typed_continuation` again. This replays the
externally anchored parent admission rules, including the `PureIndependent` semantic
boundary, implementation identity, codec identity, adapter identity, bundle bytes,
parent lifecycle and exact successful-prefix/never-started-suffix partition.

It then calls `inspect_continuation_journal` on the persisted child. The inspected
child run ID must also match the separately retained anchor. Assembly accepts only a
child that is structurally `completed`, has a recorded terminal, has no failed or
unknown child call, leaves no child candidate unstarted, and reports exactly the
number of successes required by the parent suffix.

Only after both persisted contracts and the independent child identity pass are
acknowledged parent and child success payloads decoded. Their scenario IDs must
reconstruct the original bundle order without gaps, duplicates or substitutions.

## Structural reconstruction

A successful result is wrapped in `AssembledContinuation`. It records the exact:

- parent run ID;
- child run ID;
- parent journal SHA-256;
- child journal SHA-256;
- source bundle SHA-256;
- restart-expectation SHA-256;
- reconstructed complete `BatchResult`.

`ScenarioOutcome::from_parts` and `BatchResult::from_parts` are intentionally narrow
structural constructors. They do not validate provenance or scientific meaning by
themselves. This assembly layer establishes the required completeness and identity
conditions before using them.

After `AssembledContinuation::into_batch`, existing metric, scalar-policy,
constraint-first, Pareto and multi-trace layers may consume the batch like any other
complete batch. An interrupted or partial parent/child chain never receives that
capability.

## Failure behavior

Assembly fails closed on parent admission failure, child-anchor digest or run-ID
mismatch, incomplete child lifecycle, parent/child partition mismatch, child
tampering, changed source bundle, wrong implementation-artifact identity or
signature decode failure. It returns no partial `BatchResult` on those paths.

No journal is modified and no automatic resume is authorized by assembly.

## Evidence boundary

A successful assembly proves software-level structural continuity under the recorded
contracts and the separately retained child identity. It does not authenticate
hardware, prove exactly-once external effects, validate a model-quality claim,
establish physical memory savings, or show that an application-defined decision
constraint is scientifically valid.

All current assembly tests use software fixtures. Representative model/GPU evidence
remains a separate experimental requirement.
