//! Two JSON lines: canonical input, then an interrupted software report.
//! Deliberately uses integer/text codecs; no model or scientific result is claimed.
use prospect_adapter::{
    AdapterCapability, AdapterMetadata, AdapterMetadataError, ContractVersion, VersionedAdapter,
};
use prospect_bundle::{AdapterBinding, BundleScenario, ScenarioBundle};
use prospect_core::{ProspectiveEngine, ScenarioId};
use prospect_dispatch::execution::{
    ExecutableAdapterRegistry,
    record::{PayloadCodecs, evaluate_registered_bundle_bound},
};
use prospect_evidence::RunId;
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};
use prospect_scenario::controlled::EvaluationControl;

struct Add;
impl ProspectiveEngine<i32, i32> for Add {
    type Signature = i32;
    type Error = &'static str;
    fn baseline(&self, state: &i32) -> Result<i32, Self::Error> {
        Ok(*state)
    }
    fn evaluate(&self, state: &i32, action: &i32) -> Result<i32, Self::Error> {
        state.checked_add(*action).ok_or("integer overflow")
    }
}
impl VersionedAdapter for Add {
    fn adapter_metadata(&self) -> Result<AdapterMetadata, AdapterMetadataError> {
        let version = ContractVersion::new(1, 0)?;
        AdapterMetadata::new(
            "example.add",
            version,
            None,
            vec![AdapterCapability::new("example.evaluate", version)?],
        )
    }
}
fn main() {
    let bundle = ScenarioBundle::new(
        "example.record_run",
        AdapterBinding::from_metadata(&Add.adapter_metadata().unwrap()),
        Some(7),
        10,
        vec![
            BundleScenario::new(ScenarioId::new("a").unwrap(), 1),
            BundleScenario::new(ScenarioId::new("b").unwrap(), 2),
        ],
        None,
        None,
    )
    .unwrap();
    let mut adapters = ExecutableAdapterRegistry::new();
    adapters.register(Add).unwrap();
    let bound = evaluate_registered_bundle_bound(
        &bundle,
        &adapters,
        &MetricRegistry::<i32, i32>::new(),
        &DecisionPolicyRegistry::<i32, i32>::new(),
        &EvaluationControl::new(1),
        |_| {},
    )
    .unwrap();
    let record = bound
        .capture_record(
            &RunId::new("example-run-001").unwrap(),
            PayloadCodecs::new("example.i32.v1", "example.error_text.v1").unwrap(),
            |signature| Ok(signature.to_string()),
            |error| Ok(error.to_string()),
        )
        .unwrap();
    assert_eq!(record.summary().state, "interrupted");
    assert!(!record.summary().resume_authorized);
    println!("{}", bound.input_json());
    println!("{}", record.canonical_json());
}
