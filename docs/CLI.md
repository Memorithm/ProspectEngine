# ProspectEngine CLI

The CLI surface is intentionally verification-first. It exposes deterministic replay and provenance checks without introducing a new scientific model or policy layer.

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

Successful commands return exit status 0. Verification/read failures return 1. Invalid CLI usage returns 2.

The CLI verifies structure, provenance, digests, and replay contracts. It does not prove that a representative GPU run occurred, and it does not turn logical KV byte accounting or a scenario-bundle digest into claims about HBM release, memory traffic, latency, throughput, safety, or model-quality preservation.
