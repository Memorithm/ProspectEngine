# Dispatch availability catalog

`prospect-dispatch` defines the canonical schema `prospect.dispatch-catalog/v1` for recording which adapter, metric, and decision-policy contracts are available to a ProspectEngine orchestration environment.

The catalog is metadata only. It contains no executable code, state decoder, model, scenario result, benchmark result, or scientific claim.

## Contents

A catalog contains three independently ordered collections:

- adapters: stable adapter ID, offered adapter-contract version, and optional exact upstream component/revision;
- metrics: namespaced metric ID and offered contract version;
- policies: namespaced decision-policy ID and offered contract version.

All identifiers use the existing `prospect-adapter` namespaced-ID validation. Collections are sorted deterministically and duplicate IDs within one category are rejected.

`DispatchCatalog::from_adapter_metadata` can construct the adapter portion from an existing `AdapterMetadata` catalog. It deliberately leaves metric and policy availability empty: adapter capability metadata is not evidence that a metric or policy implementation is registered.

## Canonical serialization

`DispatchCatalog::canonical_json` emits recursively key-sorted compact JSON. `from_canonical_json` rejects:

- non-canonical bytes;
- unknown fields;
- unsupported schemas;
- invalid namespaced IDs or zero-major versions;
- empty upstream revisions;
- duplicate adapter, metric, or policy IDs;
- semantically non-canonical ordering.

The catalog can therefore be persisted or exchanged as a reproducible software-availability input.

## Bundle preflight

`DispatchCatalog::resolve_bundle` checks a parsed scenario bundle without executing it:

- the requested adapter must exist;
- the offered adapter version must satisfy the existing same-major/provider-minor compatibility rule;
- any requested upstream component/revision must match exactly;
- each requested metric and policy must exist and satisfy its requested version.

Missing or incompatible requirements fail closed. No default adapter, metric, policy, revision, or version is substituted.

## Evidence boundary

A successful catalog preflight proves only that declared software contracts are mutually compatible. It does not prove that implementations are loaded, that scenarios were executed, that a metric is scientifically appropriate, that a policy is safe, or that any performance, quality, memory, traffic, or physical effect occurred.
