# Metric and decision-policy registries

`prospect-registry` provides versioned runtime registries for implementations of the existing `prospect-core` extension traits.

## Registry identity

Registry entries use the same strict namespaced identifiers and major/minor compatibility semantics as `prospect-adapter` metadata. A provider version satisfies a requirement only when the major version is identical and the provider minor version is greater than or equal to the required minor version.

Examples:

- `metric.absolute_distance`
- `policy.prefer_higher`

Registration rejects duplicate IDs. Metadata iteration is deterministic because entries are stored in namespaced-ID order.

## MetricRegistry

`MetricRegistry<Signature, Score>` owns `SignatureMetric<Signature, Score = Score>` implementations behind `Send + Sync` trait objects. A caller resolves a metric by namespaced ID and required contract version, then invokes `compare` through the existing core trait.

A registry is intentionally homogeneous in `Signature` and `Score`. Different score types use different registries instead of type erasure or implicit numeric conversion.

## DecisionPolicyRegistry

`DecisionPolicyRegistry<Signature, Score>` follows the same rules for `DecisionPolicy<Signature, Score = Score>` implementations. `Score` remains constrained by the existing `Ord` contract.

## Boundaries

Registry presence is discovery and dispatch metadata only. It is not evidence that a metric is scientifically appropriate for a domain, that a policy is safe, or that an adapter produced valid observations. Existing evidence, replay and domain-validation contracts remain authoritative.
