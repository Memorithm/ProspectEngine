# Versioned adapter contract

`prospect-adapter` defines the metadata boundary used to discover what a ProspectEngine adapter claims to support without changing the scientific contracts implemented by the adapter itself.

## Contract version

The initial adapter metadata contract is `1.0`.

Compatibility follows a deliberately small rule: a provider version satisfies a required version only when both versions have the same major number and the provider minor number is greater than or equal to the required minor number. Major-version changes are incompatible. Major version zero is rejected because this boundary is intended to be explicitly versioned from its first release.

## Identifiers

Adapter, upstream-component, and capability identifiers are lowercase namespaced identifiers such as `prospect.tdi` or `flat.boolean_hamming_routing`. Each dot-separated segment starts with a lowercase ASCII letter and may then contain lowercase ASCII letters, digits, `-`, or `_`.

This restriction is intentional: identifiers are machine keys, not display labels.

## Metadata

An `AdapterMetadata` record contains:

- one adapter identifier;
- the adapter-contract version implemented by that provider;
- an optional upstream component and exact revision;
- one or more versioned capabilities.

Capabilities are sorted by identifier and duplicate identifiers are rejected. The same capability identifier therefore has one unambiguous advertised version per adapter metadata record.

The upstream revision is separate from the adapter-contract version. For example, the TDI adapter can implement ProspectEngine adapter contract `1.0` while pinning the reviewed TDI commit used by `prospect-tdi`.

## Built-in adapters

The initial built-in metadata implementations cover:

- `prospect.tdi`: baseline/intervention evaluation plus exact finite-state TDI signatures;
- `prospect.elastic`: baseline/intervention evaluation plus prospective resource evaluation;
- `prospect.flat_boolean_attention`: baseline/intervention evaluation plus exact Boolean Hamming routing;
- `prospect.kv_eviction`: baseline/intervention evaluation plus logical oldest-first KV eviction.

The capability list is intentionally narrower than the total code present in each crate. In particular, these metadata records do not claim physical actuation, rollback success, HBM release, memory-traffic reduction, latency or throughput improvement, model-quality preservation, or safety.

## Evidence boundary

`VersionedAdapter` is descriptive metadata, not execution evidence. A matching capability allows orchestration code to determine that an adapter exposes a compatible operation. It does not prove that any particular run occurred or that the operation achieved a desired physical or numerical result. Existing observed/simulated evidence contracts remain authoritative for those claims.
