# Bounded verification inputs

File-based verification uses `prospect_cli::input::TextReadBudget`. The default
policy is an engineering admission limit, not a scientific property of a model
or a claim about GPU memory.

| Limit | Default |
| --- | ---: |
| One JSON/text file | 16 MiB (16,777,216 bytes) |
| Cumulative text read by one command | 128 MiB (134,217,728 bytes) |
| Generic campaign directory entries, including its two inputs | 1,024 |
| Files per campaign in a fixed R1/R2 suite | 4 |

## Covered entry points

The policy covers campaign-spec verification, generic campaign verification,
R1 and R2 whole-suite verification, scenario-bundle verification and dispatch
preflight (both bundle and catalog). Adapter discovery reads no input files.
The generic Rust campaign-directory API uses the same defaults.

Each whole-suite operation shares one cumulative budget across its manifest,
all three campaigns, their published verification summaries, and all context
and baseline-comparison rereads. A repeated read counts again even when it opens
an unchanged file. This is intentionally a text-read budget, not a sum of unique
file sizes. A directory-entry cap also bounds preliminary metadata enumeration.

## Exact bytes, no partial success

The reader rejects an oversized advertised file length before allocating its
payload. It also limits the actual read to the allowed bytes plus one lookahead
byte with `std::io::Read::take`. That extra byte distinguishes an exact-sized
file from an oversized input. A valid JSON prefix followed by an oversized tail
must fail; it is never treated as a successfully truncated document. The same
stream bound applies when the advertised length is stale or zero.

Successful text is returned byte-for-byte: the reader does not trim whitespace,
remove a newline, normalize Unicode, sort JSON keys, or change any evidence
checksum. UTF-8 validation and the existing canonical/semantic verifiers remain
separate. A byte-limit error, invalid UTF-8 or I/O failure returns no partial
success and closes the shared read budget. A caller composing verifiers must
abort the entire operation on an error.

CLI read failures retain exit status 1, emit diagnostics on stderr and no success
JSON on stdout. No dependency, evidence schema, frozen experiment JSON, scientific
selection, comparison tolerance or historical revision is changed.

## File types and trust boundary

Both path metadata (without following the final symlink) and opened-file metadata
must identify a regular file. This closes the previous reserved-file gap in the
generic campaign loader: `campaign.json` and `manifest.json` now receive the same
file-type checks as evidence inputs. Static links, including dangling links,
and special entries are rejected. The suite's existing typed errors for static
file-type mismatches are preserved.

The path, its ancestors and the file must still be trusted and unmodified during
verification. These checks are **not** race-free path isolation and do not forbid
all symlinked ancestors. They do not prevent concurrent substitution, authenticate
execution, enforce I/O deadlines, or guarantee crash durability. A regular file
on an unresponsive filesystem can still block. This is not a subprocess-output
cap, a JSON node-count quota, or a hard RSS limit: parsed values, vectors and
serialization add memory overhead beyond admitted input bytes.

The extra sentinel is read at most once on the first overflowing stream read;
that operation then fails and the shared budget cannot be reused. The stated
128 MiB is the maximum cumulative accepted text, not an exact allocator-capacity
or failed-operation byte count.

## Composing verifiers in the Rust API

`verify_kv_campaign_directory_with_budget` is an additive API for shared-budget
operations. Applications may construct a different reviewed policy explicitly;
the CLI provides no implicit override or bypass flag.

```rust,no_run
use prospect_cli::{
    input::TextReadBudget,
    verify_kv_campaign_directory_with_budget,
};

let mut budget = TextReadBudget::new(1024 * 1024, 8 * 1024 * 1024)?;
let first = verify_kv_campaign_directory_with_budget("campaign-a", &mut budget)?;
let second = verify_kv_campaign_directory_with_budget("campaign-b", &mut budget)?;
assert!(!first.policies().is_empty() && !second.policies().is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

The in-memory scientific parsers are unchanged; an application supplying an
already allocated string to them owns its allocation and admission policy.

## Regression coverage

Tests cover exact UTF-8 preservation, exact byte boundaries, sparse oversized
files, the live-stream lookahead bound, valid-prefix truncation rejection,
cumulative and repeated reads, invalid UTF-8, partial I/O failure, invalid limits,
static symlinks, socket/directory entries, generic campaign count limits and one
shared budget across every R1/R2 read. Black-box executable tests require status 1
and empty stdout for rejected inputs. Existing successful canonical/evidence
and Python/Rust interoperability tests must continue to pass.

```bash
cargo +1.89.0 test --locked -p prospect-cli
cargo +1.89.0 clippy --locked --workspace --all-targets -- -D warnings
cargo +1.89.0 test --locked --workspace
```

Historical launchers and verifiers pinned to earlier Git revisions still execute
that earlier code. This change does not silently update KVLab's frozen publication
verifier or retroactively alter its evidence contract.

Rust standard-library references: `std::io::Read::take` limits bytes returned by
a reader; `std::fs::symlink_metadata` inspects the final path without following
its symlink. The standard-library filesystem documentation also describes the
remaining time-of-check/time-of-use risks. See https://doc.rust-lang.org/std/io/trait.Read.html#method.take,
https://doc.rust-lang.org/std/fs/fn.symlink_metadata.html and
https://doc.rust-lang.org/std/fs/#time-of-check-to-time-of-use-toctou.
