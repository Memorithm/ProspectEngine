# Verified KV campaign comparison

ProspectEngine can verify a complete KVLab position-native campaign directory and expose its observed per-policy metrics without inventing rankings or unmeasured effects.

A valid directory contains the canonical `campaign.json`, `manifest.json`, and every `selection-*.json` record referenced by the manifest. Verification binds the campaign SHA-256, trace SHA-256, experimental context, exact retained positions, evidence digests, policy names, logical byte accounting, and metric deltas.

Comparative views derived from a verified campaign must report only values present in the observed evidence. In particular, logical KV bytes are not physical HBM release or avoided traffic, and quality metrics from one campaign are not representative-model claims unless the campaign itself satisfies the corresponding benchmark gate.
