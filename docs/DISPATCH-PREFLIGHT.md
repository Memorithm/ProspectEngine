# Scenario-bundle dispatch preflight

`prospect-dispatch` connects canonical scenario-bundle requirements to concrete ProspectEngine extension registrations without executing the experiment.

## What it resolves

`resolve_bundle_requirements` takes:

- one parsed `ScenarioBundle`;
- an explicit adapter metadata catalog;
- a caller-supplied `MetricRegistry`;
- a caller-supplied `DecisionPolicyRegistry`.

It resolves the bundle's adapter ID/version, checks the exact declared upstream component and revision when present, and resolves optional metric/policy requirements through the existing versioned registries.

Adapter contract compatibility follows the existing same-major/provider-minor rule. Upstream bindings are exact: a bundle that records an upstream component/revision cannot silently dispatch through a different revision.

## Fail-closed behavior

Preflight rejects:

- missing adapter IDs;
- duplicate matching adapter IDs in the supplied catalog;
- incompatible adapter contract versions;
- mismatched or absent required upstream bindings;
- missing or version-incompatible metric registrations;
- missing or version-incompatible policy registrations.

No fallback metric, policy, adapter, or upstream revision is invented.

## Evidence boundary

Successful preflight means only that the declared software requirements can be resolved consistently. It does not evaluate scenarios, run an adapter, select a candidate, establish scientific appropriateness, validate safety, or provide performance/physical-effect evidence.

The caller remains responsible for domain-specific state/intervention decoding, execution, evidence capture, and any scientific qualification required before a result can be used as a claim.
