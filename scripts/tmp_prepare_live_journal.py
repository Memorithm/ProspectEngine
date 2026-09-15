"""One-use branch integration; removed after validation, never shipped."""
from pathlib import Path
import hashlib


def read(path, sha=None):
    data = Path(path).read_bytes()
    if sha is not None:
        actual = hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()
        if actual != sha:
            raise RuntimeError(f"original source drift: {path}")
    return data.decode()


def replace(source, old, new):
    if source.count(old) != 1:
        raise RuntimeError(f"patch anchor drift: {old[:100]}")
    return source.replace(old, new)


path = Path("crates/prospect-dispatch/src/execution/record.rs")
source = read(path, "0d701f90295b7a90801726f2103f4cfc5a7ee326")
path.write_text(replace(source, "use core::fmt;", "pub mod journal;\n\nuse core::fmt;"))

path = Path("crates/prospect-cli/src/lib.rs")
source = read(path, "e962a9c0a3a8a5d5295a16cfe5f72333472aeb5a")
path.write_text(replace(source, "pub mod execution_record;", "pub mod execution_journal;\npub mod execution_record;"))

path = Path("crates/prospect-cli/src/main.rs")
source = read(path, "084832e339ea2e9120ad384b9ffe002e61b2a60e")
source = replace(source, 'Usage:\\n  prospect verify-execution-record',
    'Usage:\\n  prospect inspect-execution-journal <journal.jsonl> <bundle.json>\\n  prospect verify-execution-record')
addition = '''        Some("inspect-execution-journal") => {
            let (journal, bundle) = exactly_two_arguments(&mut arguments,
                "inspect-execution-journal", "journal file", "canonical input bundle")?;
            let summary = prospect_cli::execution_journal::inspect_execution_journal_files(journal, bundle)
                .map_err(|error| CliError::Verification(error.to_string()))?;
            serde_json::to_string(&summary).map_err(|error| CliError::Verification(error.to_string()))
        }
'''
path.write_text(replace(source, '        Some("verify-execution-record") => {', addition + '        Some("verify-execution-record") => {'))

path = Path("crates/prospect-dispatch/src/execution/record/journal.rs")
source = read(path)
source = replace(source, 'pub use evaluation::{JournalRun, JournalRunState, evaluate_registered_bundle_journaled};',
    'pub use evaluation::{JournalCapture, JournalRun, JournalRunState, evaluate_registered_bundle_journaled};')
source = replace(source, 'else { match lifecycle.terminal {', 'else { match &lifecycle.terminal {')
path.write_text(source)

path = Path("crates/prospect-cli/Cargo.toml")
source = read(path, "821ad911077d4bf2047cb80c77b30dc336bd6d67")
assert '[dev-dependencies]' not in source
path.write_text(source + '''
[dev-dependencies]
prospect-core = { path = "../prospect-core" }
prospect-evidence = { path = "../prospect-evidence" }
prospect-registry = { path = "../prospect-registry" }
prospect-scenario = { path = "../prospect-scenario" }
''')
path = Path("Cargo.lock")
source = read(path)
start = source.index('name = "prospect-cli"\n')
end = source.index('\n[[package]]', start)
block = source[start:end]
block = replace(block, ' "prospect-bundle",\n', ' "prospect-bundle",\n "prospect-core",\n')
block = replace(block, ' "prospect-dispatch",\n', ' "prospect-dispatch",\n "prospect-evidence",\n')
block = replace(block, ' "prospect-kv-position-observed",\n', ' "prospect-kv-position-observed",\n "prospect-registry",\n "prospect-scenario",\n')
path.write_text(source[:start] + block + source[end:])

path = Path('.github/workflows/ci.yml')
source = read(path)
anchor = '      - name: Preflight immutable KVLab R2 inputs without model execution'
addition = '''      - name: Qualify live journals and abrupt process exit (software only)
        run: |
          cargo build --locked --release -p prospect-cli --example journal_fixture
          python3 scripts/check_execution_journal.py \\
            --prospect target/release/prospect \\
            --fixture target/release/examples/journal_fixture \\
            --revision "$(git rev-parse HEAD)"

'''
path.write_text(replace(source, anchor, addition + anchor))

path = Path("README.md")
source = read(path)
addition = '''## Live execution journals

`evaluate_registered_bundle_journaled` records acknowledged call intents before
engine invocation and actual returns afterward, reusing the controlled evaluator.
Storage/encoding failures block later calls and preserve actual returns in memory.
`FileJournal` writes a fresh owner-only file with per-entry synchronization.
`prospect inspect-execution-journal <journal.jsonl> <bundle.json>` checks the input
binding and chained lifecycle without changing the journal. An unmatched intent
is an unknown call outcome, never an automatic-retry permission. Torn tails are
reported explicitly. See [live journal contracts](docs/LIVE-EXECUTION-JOURNAL.md).
This is live diagnostic persistence, not safe restart or hardware authentication.

'''
path.write_text(replace(source, '## Validate and build', addition + '## Validate and build'))

path = Path("docs/CLI.md")
path.write_text(read(path) + '''
## Inspect a live execution journal

```bash
cargo run --locked -p prospect-cli --bin prospect -- inspect-execution-journal journal.jsonl bundle.json
```

Both inputs use the shared bounded file reader. Successful inspection can describe
an open, failed or interrupted run; its exit status does not imply campaign success.
An unterminated final tail is reported rather than repaired, and unmatched call
intents remain unknown outcomes. Every summary sets `resume_authorized=false`.
Malformed complete entries, binding/hash/order errors, invalid UTF-8, oversized
inputs and bytes after a terminal event fail with no success JSON. This command
does not execute, resume, decode application payloads or edit the log. See
[LIVE-EXECUTION-JOURNAL.md](LIVE-EXECUTION-JOURNAL.md) for precise trust boundaries.
''')

path = Path("docs/NEXT_MILESTONE.md")
source = read(path)
start = source.index('## Current engineering slice: persistent input-bound terminal records')
end = source.index('## Next empirical slice: exact R2 CUDA qualification')
replacement = '''## Completed terminal-record foundation

ProspectEngine #46 is merged at `aba66f17f9ef8f24ff98bf1e26b6b1317ec14b01`.
Terminal execution records bind canonical input captured before engine calls,
preserve the successful/failed/unstarted partition, and support no-clobber
persistence and independent CLI readback. See [execution records](EXECUTION-RECORDS.md).
They remain terminal records, not engine snapshots or automatic-resume tokens.

## Current engineering slice: live execution journaling

Record and acknowledge each call intent before invoking the engine, followed by
its actual return, under the existing cooperative evaluator. Keep journal errors
separate from actual domain errors and preserve returned work in memory. The file
sink synchronizes each append and never reopens existing journals for writing.
Inspection validates canonical input binding and ordered chained events; an
unmatched intent remains an unknown result, never presumed unexecuted or retryable.
Incomplete tails must be explicit, never silently repaired or marked complete.
See [live execution journals](LIVE-EXECUTION-JOURNAL.md).

Acceptance requires final-head Rust CI, storage-failure injection at every append,
actual file-backed process-exit tests, no implicit replay, and unchanged terminal
record/R2 interoperability. A storage acknowledgement or hash does not authenticate
engine/GPU execution or guarantee power-loss survival.

Safe restart remains separate: compare implementation/codec identities to trusted
expectations, restore appropriate typed state, define purity/idempotency and
explicitly reconcile unknown-side-effect calls. No current reader issues resume
authorization or reconstructs a rankable batch from an untrusted stored record.

'''
path.write_text(source[:start] + replacement + source[end:])
