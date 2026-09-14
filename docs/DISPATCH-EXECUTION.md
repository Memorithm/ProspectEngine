# Typed bundle execution

`prospect-dispatch::execution` provides the executable half of the ProspectEngine plugin boundary without introducing untyped plugin loading.

## Execution model

`ExecutableAdapterRegistry<State, Intervention, Signature, EngineError>` stores engines that share one exact Rust type universe. Every registered implementation must implement both:

- `ProspectiveEngine<State, Intervention>` with the registry's exact `Signature` and `EngineError` types;
- `VersionedAdapter`, which supplies the adapter ID, contract version, upstream revision and capabilities.

Registration derives metadata from the engine itself. Callers cannot register an implementation under unrelated caller-supplied metadata.

`execute_registered_bundle` performs the following sequence:

1. materialize the executable registry's adapter metadata catalog;
2. resolve the bundle's adapter contract, exact upstream binding and optional metric/policy requirements using the existing fail-closed dispatch preflight;
3. locate the implementation corresponding to the validated adapter metadata;
4. convert the already typed bundle scenarios into core scenarios;
5. execute the baseline and candidate batch through `prospect-scenario`;
6. apply the resolved metric, when requested;
7. apply the resolved decision policy, when requested;
8. return the typed batch, optional metric scores and optional selected scenario.

Any adapter/version/upstream/metric/policy failure occurs before the engine is invoked. Tests explicitly count engine calls to enforce this property.

## Deliberate exclusions

This API does not deserialize arbitrary JSON into domain state, load dynamic libraries, discover executable code from the filesystem, invoke shell commands, or infer a domain type from an adapter ID. A caller must instantiate a concrete typed registry in Rust.

The CLI therefore remains verification/preflight oriented. A generic `execute <bundle.json>` command would require an untyped domain-decoding boundary and is intentionally not introduced by this milestone.

## Evidence boundary

Successful typed execution proves that a registered implementation ran for the supplied typed state and interventions and produced the returned software values. It does not by itself establish that the model is scientifically valid, that an experiment is representative, that a selected intervention is safe, or that any latency, throughput, memory-traffic, quality, financial or physical-effect claim is justified. Those claims require the domain-specific observed-evidence and validation contracts already used elsewhere in ProspectEngine.
