# Externally bound restart preflight

This is a read-only preparation step after live journal inspection. It does not
resume an engine, append to an old log, repair a torn entry, decode a payload,
retry an operation, or reconstruct a rankable batch. A positive result is not
permission to actuate a system.

## Interfaces

`prospect_dispatch::execution::record::journal::recovery` provides
`RestartAnchors`, `RestartExpectations`, `RestartSemantics`, `RestartPreflight`,
and `preflight_journal_restart`.

`prospect_cli::execution_restart` adds bounded file loading and a streaming hash
of the actual separately supplied implementation artifact. Its
`hash_implementation_artifact` helper is reusable without running that artifact.

The command takes exactly four paths:

```text
prospect preflight-execution-restart <journal.jsonl> <bundle.json> <trusted-expectations.json> <implementation-artifact>
```

A valid diagnostic result returns status 0 even when it is blocked. Invalid
identity, input, syntax or I/O returns status 1 and no success JSON. Wrong arity
returns status 2. Read `continuation_preparation_allowed` and `blockers`, not exit
status alone. Every summary has `resume_authorized=false`.

## External expectations, not self-approval

The canonical `prospect.restart-expectations/v1` file contains:

- `anchors`: independently retained run ID, exact complete-journal SHA-256,
  exact canonical-bundle SHA-256, and canonical adapter-metadata SHA-256;
- `implementation`: expected component, 40-character Git revision and artifact SHA-256;
- `codecs`: exact expected signature and error codec names;
- `semantics`: either `pure_independent` or `requires_reconciliation`.

The application must establish these values through a trusted source separate
from the journal it is evaluating: for example, its controlled run registry and
approved build/codec manifest. The CLI deliberately does not offer a command
which derives an expectation file from an arbitrary journal and calls it trusted.
A file submitted by the same attacker as the journal is not an independent anchor.

The whole-journal digest is a post-run capture anchor, not a value knowable before
the run. When a process dies, a trusted controller may capture the stopped log and
retain its digest before distributing verification copies. That capture does not
prove prior absence of tampering; a stronger adversarial setting needs externally
acknowledged heads, signatures or an authenticated storage service. This feature
implements comparison to caller-supplied expectations, not such a trust service.

All digests must be lower-case 64-character hexadecimal values. Unknown fields,
duplicate fields, unsupported schemas, missing fields, noncanonical whitespace
and oversized policies are rejected. The policy's own SHA-256 is returned for
traceability. None of these hashes is a cryptographic signature.

An artifact's bytes are hashed from the fourth file; they are not trusted merely
because its path or journal header names it. The expected Git revision remains an
external declaration; hashing a file cannot infer its source revision. A matched
artifact does not authenticate an already running process, dynamic libraries,
compiler options, hardware, arbitrary codecs or numerical correctness.

## Admission and blockers

The existing journal inspector remains the single lifecycle validator. It first
checks exact input binding, sequence, hash chain, canonical fields and call order.
Restart preflight then compares the run, implementation, codec and full adapter
metadata to the external expectations. It reports the never-started suffix in
original input order and separately reports the successful prefix and availability
of a baseline. It never treats failed or unmatched calls as never-started work.

A clean header-only log, a clean successful prefix, or a coherent interrupted run
with remaining candidates may allow CONTINUATION PREPARATION, but only under an
explicit `pure_independent` declaration. That declaration promises independent
stateless evaluation with no external side effects; it is not inferred or proved.

Any of the following blocks preparation:

| Blocker | Consequence |
| --- | --- |
| `incomplete_tail` | Do not infer what a partial final entry meant. |
| `unknown_call_result` | Do not retry a call whose outcome or effects are unknown. |
| `failed_call_requires_reconciliation` | Preserve the known failure; it is not a retry queue. |
| `stateful_or_effectful_engine` | Require a separate state/idempotency/reconciliation contract. |
| `already_completed` | Do not restart a completed run. |
| `no_never_started_candidates` | No new candidate call can be planned from this source. |

Blockers have deterministic order and can coexist. An unmatched intent or torn
log is blocked even for a declared pure engine. Diagnostic suffix IDs in a blocked
report describe the verified prefix only; they are not an executable schedule.
A complete set of returned signatures without a terminal record cannot silently
be converted into a completed campaign.

Changing or deleting journal entries while retaining the independently recorded
whole-file anchor is rejected even if the altered log is coherently rehashed.
Changing both the anchor and the source together is outside that comparison's
trust guarantee, as explicitly described above.

## Resource and file policy

Expectations have a dedicated 16 KiB read budget. Journal and bundle share the
existing exact-byte text budget (16 MiB per file, 128 MiB cumulative operation
policy). Artifact hashing accepts nonempty regular files up to 1 GiB. It uses a
fixed 64 KiB buffer and reads at most the limit plus one sentinel byte, so an
oversized stream cannot succeed by truncation. It does not load or execute code.

Static final-component links, directories and other non-regular files are rejected.
Paths, ancestors, serializers and contents must remain trusted and unchanged.
This is not race-free filesystem isolation, a hard process-RSS limit or an I/O
deadline. Reusing an old pinned CLI does not retroactively enable this command.

## Qualification

Unit tests cover external identity changes, coherent rehashing and suffix deletion,
invalid expectation files, unknown/failed calls, incomplete tails, stateful engines,
completed runs, empty/oversized artifact streams and partial read failures.

The permanent CPU check runs the actual fixture executable and release CLI. It
anchors controlled test outputs, then alters verification copies. A quota-limited
run identifies only candidates 2 and 3 as unstarted. An abrupt exit inside candidate
2 must remain blocked with candidate 2 UNKNOWN and only candidate 3 never started.
Changed input, substituted binaries, symlinks, oversized files and incorrect command
arity must fail. This controlled fixture harness is not a production trust service.

```bash
cargo +1.89.0 test --locked -p prospect-dispatch
cargo +1.89.0 test --locked -p prospect-cli
cargo +1.89.0 build --locked --release -p prospect-cli --bin prospect --example journal_fixture
python3 scripts/check_restart_preflight.py \
  --prospect target/release/prospect \
  --fixture target/release/examples/journal_fixture \
  --revision "$(git rev-parse HEAD)"
```

## Remaining continuation work

A separate typed API must validate actual decoder semantics, restore accepted
baseline/signatures or engine state, create a new run linked to its parent, and
execute only approved never-started work. It must keep restored outcomes distinct
from new observations and never silently retry unknown or failed effects. This
preflight does not implement those execution operations or exactly-once actuation.
Existing journal, record, R1/R2 schemas and historical pins are unchanged.

Standard-library byte-limit contract:
https://doc.rust-lang.org/std/io/trait.Read.html#method.take.
