# Input-bound terminal execution records

This slice persists completed, interrupted and failed controlled evaluations.
It does **not** implement a resumable engine checkpoint, event journal, recovery
of a process killed mid-run, automatic retry or physical rollback. The historical
controlled/batch APIs remain unchanged.

## Bind before evaluating

`prospect_dispatch::execution::record::evaluate_registered_bundle_bound` captures
the supplied `ScenarioBundle` canonical serialization before calling the existing
controlled dispatcher. State, interventions, scenario order, seed and declared
adapter/metric/policy requirements are included in those canonical bytes. The
returned `BoundBundleEvaluation` privately retains both that input and the actual
controlled report; it cannot be constructed by pairing arbitrary public fields.

Input serialization must be deterministic and faithfully describe the domain's
state/interventions. Callers must keep interior-mutable inputs stable. The binding
is to canonical serialized input, not a proof that arbitrary custom serializers
are injective or that an adapter consumed every declared field. Existing bundle
serialization semantics are reused rather than silently changed in this slice.

Invalid serialization, too many scenarios or dispatch incompatibility fails
before any engine call. Calls that fail during evaluation are retained in the
report, not returned as an admission error. The original error and successful
prefix remain inspectable even if later encoding or persistence fails.

## Explicit codecs and record format

Call `capture_record` on the bound result with the existing `prospect_evidence::RunId`,
versioned signature/error codec IDs and two fallible text encoder callbacks.
The schema is `prospect.bundle-evaluation-record/v1`; the fixed evidence kind is
`software_execution_report`. Neither `Observed` nor a scientific-quality label is
inferred from the name of the API or the status of the run.

Each signature and error is stored as an opaque UTF-8 string under the explicitly
named application codec. All bytes are preserved, including newlines and Unicode.
This avoids imposing arbitrary serde serialization on generic engine results:
codec authors are responsible for numerical validity, reversible encoding and
rejecting unsupported values. The verifier does not decode or scientifically
validate payloads. A string containing `NaN` is just text, not an admitted numeric
measurement. No dynamic codec or plugin is loaded from the record.

The record includes the exact input SHA-256, run and bundle identities, declared
seed, adapter-derived metadata, candidate quota, whether a deadline was configured,
actual successful baseline/candidate payloads, never-started IDs and terminal detail.
No process-local `Instant` is serialized as a portable timestamp.

Capture borrows the original report. An encoder error returns no partial record
and preserves the report for inspection or a deliberate encoding retry; it never
re-executes the engine. Capture and readback use the same validation implementation.

## Independent verification

Run the CLI with the record and a separately supplied trusted canonical input:

```bash
cargo +1.89.0 run --locked -p prospect-cli --bin prospect -- \
  verify-execution-record run.record.json input.bundle.json
```

The command checks the exact bundle SHA-256, bundle identity/seed, adapter metadata
and compatibility, canonical encoding, unknown/duplicate/missing fields, codec IDs
and the ordered lifecycle partition. Successful outcomes must be the input prefix;
a failed candidate must be the next candidate; pending IDs must be exactly the
never-started suffix. Baseline failures cannot claim successful signatures. Quota
and deadline declarations must be consistent with their interruption reasons.
An incomplete prefix cannot be relabelled completed.

The output schema is `prospect.execution-record-verification/v1`. It includes the
record/input hashes, run ID, terminal state and successful/failed/never-started
counts. `resume_authorized` is always false. Exit 0 means a valid report, which may
itself describe an interrupted or failed run. Invalid input returns 1 with no
success JSON; incorrect arguments return 2. No model is executed by verification.

There is deliberately no conversion from a loaded record to `BatchResult`, a
policy winner, an executable adapter, or a retry queue. A checksum is not a signature:
a coherently forged report can satisfy these structural checks. Record-origin
trust, numerical validity, GPU authentication and tamper-resistant logs require
separate evidence and a separately defined trust model.

## Persistence API

`prospect_cli::execution_record::publish_execution_record(&record, destination)`
requires a new path. It writes and syncs an owner-only temporary file on Unix,
closes the file, then creates a same-directory hard link at the destination.
Existing regular files, directories and dangling symlinks are not overwritten.
There is no copy/rename fallback on filesystems without hard-link support.

This exposes only a fully written record at the requested path on supported
filesystems. Temporary cleanup is best effort on return/panic. The trusted parent
directory must already exist and remain unmodified. Directory entries are not
fsynced: this is not a power-loss durability, hostile-filesystem isolation or
crash-recovery guarantee. A process crash may leave a temporary file. Preserve the
matching canonical input separately; the two files are not published as an atomic
pair by this API. Keep sensitive error payloads out of publicly shared artifacts.

`verify_execution_record_files` uses one existing `TextReadBudget` for both input
files, retaining the current static file-type checks and exact-byte semantics.
The record contract additionally admits at most 4,096 scenarios, 1 MiB per encoded
payload and 16 MiB per record/input. Admission limits are not a hard RSS cap:
caller codecs and serialization can allocate before their outputs are checked.
No historical KVLab/NNIS experiment or publication-verifier pin is changed here.

## Executable example and tests

The example emits exactly two canonical JSON lines, input then record. It uses a
synthetic integer engine, a quota of one and two scenarios, hence interruption:

```bash
cargo +1.89.0 run --locked -p prospect-dispatch --example bound_execution_record
cargo +1.89.0 test --locked -p prospect-dispatch
cargo +1.89.0 test --locked -p prospect-cli --test execution_records
```

Tests cover real bound capture, input snapshot timing, opaque payload preservation,
codec failure without report loss, last-call cancellation, baseline/candidate
failures, forged lifecycle partitions, altered input/adapter bindings, strict JSON,
byte limits, roundtrip files, concurrent no-clobber publication, symlinks and CLI
exit semantics. These are software fixtures, not model-quality observations.

## Next functional boundary

Safe resume still needs a versioned live checkpoint/event protocol, exact input
and implementation binding across restart, explicit idempotency rules, and a way
to distinguish completed side effects from calls whose outcomes are unknown.
This terminal-record API intentionally does not infer any of those properties.

Standard-library references:
https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new
https://doc.rust-lang.org/std/fs/fn.hard_link.html
https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all
