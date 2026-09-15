# Live execution journal

This extends controlled evaluation and terminal records with a journal written
DURING execution. It does not turn a terminal record or a recovered log into a
resumable engine checkpoint. All examples/tests use software fixtures, not models.

## Entry points

- `prospect_dispatch::execution::record::journal::evaluate_registered_bundle_journaled`
- `JournalCapture`, `JournalSink`, `EngineIdentity`, and `JournalRun`
- `prospect_cli::execution_journal::FileJournal`
- `prospect inspect-execution-journal <journal.jsonl> <bundle.json>`

The new evaluator captures and validates the canonical input, resolves every
adapter/upstream/metric/policy requirement, and obtains metadata from the actual
registered adapter before writing its header. It wraps the registered engine and
reuses `evaluate_batch_controlled`: there is no second scheduling loop or copied
scientific algorithm. Existing controlled/terminal APIs and their schemas remain
unchanged. The new path does not run metrics/policies or rank incomplete outcomes.

`JournalCapture::new` requires a fresh single-writer sink, a RunId, a declared
EngineIdentity, named PayloadCodecs, and fallible signature/error encoders. Payloads
are opaque UTF-8 strings; codec semantics are the application's responsibility.
They are not converted implicitly to JSON numeric values.

`EngineIdentity` requires a namespaced component, a lower-case 40-character Git
revision and a 64-character artifact SHA-256. These are explicit declarations,
not independently measured attestation. The fixture hashes its actual executable
and accepts an explicitly supplied Git revision; general callers must supply and
substantiate their own implementation bindings.

## Call protocol

The entry format is canonical JSON Lines, schema `prospect.execution-journal/v1`
in the first `initialized` header. Every envelope contains `sequence`,
`previous_sha256`, and `event`. The first predecessor is null; each later hash is
the SHA-256 of the preceding canonical line WITHOUT its LF. The LF commits a
complete entry in the inspection format; a storage acknowledgement also requires
the sink's promised persistence operation to return success.

1. Append and acknowledge the input/adapter/implementation/codec/control header.
2. Append and acknowledge `call_started` for the baseline or exact next candidate.
3. Invoke that domain engine call only after acknowledgement.
4. Encode its actual result and append `call_succeeded` or `call_failed`.
5. Proceed through the existing control checkpoints. Append a final `finished`
   entry only after the evaluator reaches its terminal state and prior journal
   operations succeeded.

The final classification is completed, interrupted, or engine-failed; a persistence
or encoding failure instead returns `JournalRunState::JournalFailed` in memory.
No successful terminal is invented after such a failure.

## Failures and preservation of actual work

An intent-write failure prevents the corresponding domain call. A return-write
or codec failure preserves the actual successful signature or engine error in
memory, but the on-disk intent may have no matching return. The poisoned session
blocks all later DOMAIN calls and performs no retry of a write or intervention.
The existing evaluator can reach one blocked wrapper invocation while stopping;
that invocation is not reported as an engine attempt. `never_started()` includes
it and the remaining input suffix. Caller cancellation tokens are not modified.

A domain error and a journal error can coexist. `engine_error()`/`failed_call()`
retain the actual error even if its persistence failed; `journal_error()` describes
the independent storage/encoding failure. Successful outcomes and baseline are
not fabricated or erased. Only a fully completed, journal-acknowledged in-memory
run can convert into the existing rankable BatchResult.

Control deadlines/cancellation remain cooperative. Journal writes, codecs,
callbacks inside an adapter, input serialization, and engine calls can block.
There is no hard wall-clock, RSS or physical rollback guarantee. Panics propagate;
an engine panic can leave an acknowledged unmatched intent. The final append
happens after evaluation's terminal checkpoint and cannot rewrite that decision.

## File persistence

`FileJournal::create` uses `OpenOptions::create_new` to refuse existing files,
directories and dangling/live symlinks. Unix permissions are owner-only (0600,
subject to stricter umask). Every append calls `write_all`, then `sync_all`, before
acknowledgement. A failure latches the handle and prevents further writes. No
existing file is reopened for writing, truncated, repaired, replaced or removed
by recovery. The journal is retained on drop and on failure for inspection.

Only one fresh single-writer sink per run is supported. Parents and files must be
trusted and unchanged. No parent-directory fsync, authenticated hardware receipt,
concurrent-writer isolation, power-loss survival or race-free filesystem sandbox
is promised. A successful file sync is the operating system's acknowledgement,
not independent proof of durable hardware storage. A custom JournalSink owns the
truth of its own acknowledgement contract.

## Inspecting after a process stops

The CLI reads journal and canonical bundle under one shared TextReadBudget. It
checks each complete line for canonical bytes, strict fields, sequence, hash chain,
input digest, adapter compatibility, implementation declaration, codec identifiers
and the permitted lifecycle. Its summary always sets `resume_authorized=false`.

| State | Meaning |
| --- | --- |
| `completed` / `interrupted` / `failed` | A coherent final event is present |
| `not_started` | Header exists but no engine call intent is recorded |
| `unknown_call_result` | An acknowledged-looking intent has no matching return |
| `open_after_return` | Successful return(s) exist but no terminal entry |
| `open_after_failure` | Failed call is recorded but terminal entry is absent |
| `incomplete_tail` | A non-LF-terminated tail remains after a verified prefix |

`unknown_call_result` identifies baseline versus candidate explicitly. It does
NOT mean the call did not run: the process might have stopped before, during, or
after the call and before its result was recorded. Automatic retry is forbidden
by this API regardless of the apparently remaining work.

A final non-LF tail is never parsed or treated as a successful event. Its exact
byte count, the verified-prefix digest and whole-file digest are reported. The
file is not changed. A malformed COMPLETE entry is an error, not silently skipped.
Missing/torn initial headers, invalid UTF-8, oversized inputs, unknown/duplicate
fields and ANY bytes after a recorded terminal are rejected. A valid inspection
can describe an incomplete or failed execution; exit 0 means inspection succeeded,
not that the campaign succeeded. Rejected inputs return exit 1 with no success JSON;
wrong command arity returns 2.

Hash chaining detects inconsistent edits/reordering, but does not authenticate
outputs. A coherent forgery can pass, and a log cut at an entry boundary is an
open prefix rather than cryptographically detectable tail deletion. Artifact and
Git identities in the header are declarations. No loaded log constructs a
BatchResult, engine snapshot, replay queue, resume token or permission to actuate.

## Admission policy

The input bundle reuses the existing record limits: at most 4,096 scenarios and
16 MiB canonical input. The journal permits at most 8,196 entries, 16 MiB total
raw bytes including LF, and 8 MiB per encoded entry. Each application payload is
at most 1 MiB; JSON escaping also consumes the entry/total budget. Exceeding a
budget blocks later calls. Encoder/serialization allocations happen before some
size checks; these are admission limits, not hard allocator/process caps.

## Executed qualification

The permanent integration check builds a real fixture executable and real CLI.
It runs a complete evaluation, a normal engine failure, and a child process which
exits with status 23 inside the second candidate call. The latter bypasses Rust
cleanup and must leave one successful candidate, an unknown second-call result,
one never-started candidate and no terminal entry. The inspector must neither
change the file nor authorize replay.

Run the software qualification from the repository root:

```bash
cargo +1.89.0 build --locked --release -p prospect-cli --bin prospect --example journal_fixture
python3 scripts/check_execution_journal.py \
  --prospect target/release/prospect \
  --fixture target/release/examples/journal_fixture \
  --revision "$(git rev-parse HEAD)"
```

Unit tests also inject failures at every append boundary, including the final
terminal write, and partial return writes. They verify intent ordering from inside
the actual fixture engine, preservation of errors/signatures, no implicit scoring,
quota/deadline/cancellation, hash/lifecycle rejection and conservative prefix states.
This is process/software persistence evidence, not power-loss or GPU qualification.

## Remaining restart work

A separate recovery protocol must verify implementation/codec identities against
trusted external expectations, restore typed engine state where applicable, and
define which operations are pure/idempotent. Unknown-side-effect calls require
explicit reconciliation rather than inference from missing results. Live journal
inspection alone does not implement safe resume or exactly-once actuation.

References: Rust standard-library `OpenOptions::create_new`, `File::sync_all`, and
`Write::write_all` contracts: https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new,
https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all,
https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all.
