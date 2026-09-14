# ProspectEngine CLI

The CLI surface is intentionally verification-first. It exposes deterministic replay, provenance checks, adapter discovery, and non-executing dispatch preflight without introducing a new scientific model or policy layer.

## List built-in adapters

Run:

```bash
cargo run -p prospect-cli --bin prospect -- list-adapters
```

The command emits a deterministic JSON array describing every built-in adapter known to ProspectEngine. Each entry contains the stable adapter ID, adapter-contract version, exact upstream component/revision, and declared versioned capabilities.

The catalog is a discovery surface only. An adapter being present, or declaring a capability, is not evidence that a particular run occurred or that the adapter is performant, safe, scientifically validated, or physically effective.

## Preflight a scenario bundle against a dispatch catalog

Run:

```bash
cargo run -p prospect-cli --bin prospect -- \
  preflight-scenario-bundle experiment.bundle.json dispatch-catalog.json
```

Both files must be canonical: the bundle must satisfy `prospect.scenario-bundle/v1` and the availability catalog must satisfy `prospect.dispatch-catalog/v1`. The command resolves only the declared software contracts:

- exact adapter ID;
- compatible adapter contract version using the existing same-major/provider-minor rule;
- exact upstream component/revision when the bundle declares one;
- optional metric ID/version;
- optional decision-policy ID/version.

Successful output is a compact JSON summary containing SHA-256 identities for both canonical input files plus requested/offered versions for every resolved requirement. Missing, incompatible, or upstream-drifted requirements fail closed.

This command does not load plugin code, decode domain state/interventions, evaluate scenarios, invoke a metric/policy, or create execution evidence. It establishes software-contract compatibility only.

## Verify a KV campaign specification before execution

Run:

```bash
cargo run -p prospect-cli --bin prospect -- \
  verify-kv-campaign-spec campaign.json
```

The input must be canonical `kvlab.prospect-kv-real-model-position-campaign/v1` JSON. The command validates the same pre-execution invariants required by the KVLab v4 position-native campaign contract: non-empty provenance fields, a lowercase full Git SHA for `run_repository_revision`, non-empty model/evaluation traces, positive logical bytes per token, unique non-empty policy names, strictly increasing in-range retained positions, and rejection of a candidate that duplicates the full-cache baseline.

Successful output is a compact deterministic JSON summary containing the exact campaign-spec SHA-256, reconstructed position-trace SHA-256, declared model/tokenizer/runtime provenance, seed, input/evaluation token counts, total logical input bytes, and one budget summary per policy. Each policy summary contains retained positions, retained/evicted token counts, and logical retained/evicted bytes.

This is a pre-execution input verifier. It does not invoke KVLab or NNIS, does not assert that the declared Git/runtime revisions are present locally, and does not create observed evidence. Logical byte accounting is only the declared campaign budget; physical HBM residency, traffic, latency, throughput, and model quality remain unmeasured until an execution backend records them explicitly.

## Verify a KV campaign directory

Expected directory layout:

```text
campaign-output/
├── campaign.json
├── manifest.json
├── selection-000.json
├── selection-001.json
└── ...
```

Run:

```bash
cargo run -p prospect-cli --bin prospect -- verify-kv-campaign campaign-output
```

A successful verification writes one compact JSON object to stdout containing the campaign-spec SHA-256, reconstructed trace SHA-256, verified policy names, record count, and one observed summary per policy. Each policy summary contains exact retained positions, logical KV bytes retained/evicted, and every verified metric with its declared kind, unit, preference, baseline value, candidate value, and delta. ProspectEngine does not infer a winner from these values. The loader is strict: `campaign.json` and `manifest.json` are mandatory; all other directory entries are treated as evidence inputs, non-file entries are rejected, and unexpected evidence filenames fail verification. Every evidence payload is checked against the manifest SHA-256 and replayed by `prospect-kv-position-campaign` before success is reported.

## Verify a scenario bundle

Run:

```bash
cargo run -p prospect-cli --bin prospect -- verify-scenario-bundle experiment.bundle.json
```

The command parses the file as `prospect.scenario-bundle/v1` using generic JSON values for state and interventions. This deliberately avoids inventing domain-specific decoding in the CLI. Successful output includes the canonical SHA-256, bundle and adapter IDs, adapter contract version, optional upstream binding and seed, ordered scenario IDs, and optional metric/policy requirements.

The command rejects non-canonical or semantically non-canonical bundles through the `prospect-bundle` replay contract. The digest identifies the exact canonical experiment input only; it is not execution evidence.

## Exit status and evidence boundary

Successful commands return exit status 0. Verification/read/preflight failures return 1. Invalid CLI usage returns 2.

The CLI verifies structure, provenance, digests, replay contracts, discovery metadata, and declared dispatch compatibility. It does not prove that a representative GPU run occurred, and it does not turn logical KV byte accounting, a scenario-bundle digest, catalog membership, or capability metadata into claims about HBM release, memory traffic, latency, throughput, safety, or model-quality preservation.

## Verify the frozen SmolLM2 R1 suite

```bash
cargo run -p prospect-cli --bin prospect -- verify-kv-campaign-suite suite-output
```

The verifier checks all three budget directories, their published summaries,
common baseline and trace, and the exact SHA-256 of each preregistered
campaign at KVLab revision `51f2f414c6ca3ef0260d885c72b8f5863bd66047`.
Recomputing a manifest after substituting tokens, policy positions or an
evaluation identifier cannot make a different experiment pass this fixed
suite contract. Generic campaign verification remains available for other
experiments. Synthetic test outputs do not establish a real model run.


## Reproducible CLI builds

The workspace tracks `Cargo.lock` because it ships the `prospect` executable.
Build a reviewed checkout with:

```bash
cargo +1.89.0 build --locked --release -p prospect-cli --bin prospect
```

CI runs locked Clippy, tests and the release CLI build. A dependency change
must include its reviewed lockfile update; do not remove `--locked` from
experiment launchers to make a missing or stale lockfile pass. Earlier
ProspectEngine revisions without a committed lockfile are not repaired
retroactively. Campaigns requiring locked verifier builds need an explicit
successor verifier pin. A reproducible build is not model-execution evidence.
