# Reproducible scenario bundles

`prospect-bundle` defines `prospect.scenario-bundle/v1`, a canonical input artifact for replaying a prospective experiment setup.

A bundle binds:

- a machine-stable bundle ID;
- the adapter ID and adapter contract version;
- the adapter's explicit upstream component/revision when available;
- an optional deterministic seed;
- typed domain state;
- one or more typed interventions with stable scenario IDs;
- optional versioned metric and decision-policy requirements.

Scenarios are sorted by `ScenarioId` and duplicate IDs are rejected. Canonical JSON recursively sorts object keys and requires the semantic scenario order to already be canonical when replayed. Equivalent bundles therefore produce the same SHA-256 regardless of caller insertion order.

A bundle is an **input description**, not evidence. Its digest proves the bytes of the canonical experimental setup only. It does not prove that the adapter was executed, that an observation occurred, that a metric or policy is scientifically appropriate, or that any performance, safety, memory or quality property holds. Runtime results continue to use the separate ProspectEngine evidence contracts.
