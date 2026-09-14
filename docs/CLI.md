# ProspectEngine CLI

The first CLI surface is intentionally narrow. It verifies persisted KVLab position-native real-model campaign evidence without introducing a new scientific model or policy layer.

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

A successful verification writes one compact JSON object to stdout containing the campaign-spec SHA-256, reconstructed trace SHA-256, verified policy names, and record count. Verification failures are written to stderr and return exit status 1. Invalid CLI usage returns exit status 2.

The loader is strict: `campaign.json` and `manifest.json` are mandatory; all other directory entries are treated as evidence inputs, non-file entries are rejected, and unexpected evidence filenames fail verification. Every evidence payload is checked against the manifest SHA-256 and replayed by `prospect-kv-position-campaign` before success is reported.

This command verifies evidence structure, provenance, digests, and exact position selections. It does not prove that a representative GPU run occurred, and it does not turn logical KV byte accounting into claims about HBM release, memory traffic, latency, throughput, or model-quality preservation.
