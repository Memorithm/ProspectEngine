# Observed position-selected KV evidence

ProspectEngine consumes KVLab's position-native contracts through two compatibility-safe crates:

- `prospect-kv-position` replays `kvlab.prospect-kv-selection/v2` and treats zero-based sequence positions as occurrence identities. Vocabulary token values may repeat. The retained and evicted position lists must be strictly increasing, disjoint, exhaustive, and the evicted list must be the canonical complement of the retained list. Logical byte accounting is recomputed exactly.
- `prospect-kv-position-observed` consumes `kvlab.prospect-kv-real-model-selection/v2`, pinned to KVLab merge `782dde3304f2da984f6544cc0a49bab7f5977ea9`. It validates model/tokenizer/runtime/evaluation/trace provenance, output SHA-256 values, the embedded position selection, paired logical byte accounting, finite metric semantics, and replayed candidate-minus-baseline deltas.

Observed records expose `EvidenceNature::Observed`. Budget-matched comparisons require one exact experimental context, one identical full-cache baseline, equal candidate logical-byte budgets, and unique policy labels. They do not rank policies automatically.

The previous value-identified v1 selection and observed-evidence crates remain unchanged for replay compatibility. Version 2 is required when real model sequences contain repeated vocabulary token values or when a runtime, such as NNIS, addresses retained KV rows directly by sequence position.

This consumer validates evidence structure and provenance only. It does not establish that a representative campaign has run, nor infer HBM release, allocator behavior, memory traffic, latency, throughput, or preserved model quality from logical KV bytes.