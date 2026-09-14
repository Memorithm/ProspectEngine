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

A successful verification writes one compact JSON object to stdout containing the campaign-spec SHA-256, reconstructed trace SHA-256, verified policy names, and record count. The loader is strict: `campaign.json` and `manifest.json` are mandatory; all other directory entries are treated as evidence inputs, non-file entries are rejected, and unexpected evidence filenames fail verification. Every evidence payload is checked against the manifest SHA-256 and replayed by `prospect-kv-position-campaign` before success is reported.

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
