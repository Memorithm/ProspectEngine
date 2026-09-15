use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use prospect_cli::input::TextReadBudget;
#[cfg(test)]
use prospect_cli::verify_kv_campaign_directory;
use prospect_cli::{CampaignVerificationSummary, verify_kv_campaign_directory_with_budget};
use prospect_kv_position_observed::{
    KvlabKvRealModelPositionEvidenceV2, ObservedKvPositionComparison,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SUITE_SCHEMA_V1: &str = "kvlab.smollm2-r1-position-suite-result/v1";
const VERIFICATION_SCHEMA_V1: &str = "prospect.kv-campaign-suite-verification/v1";
const KVLAB_SUITE_PROVIDER_REVISION: &str = "fc969997eb3fed718e7ae6c6a7791c4a776eeead";
const KVLAB_PREREGISTRATION_REVISION: &str = "51f2f414c6ca3ef0260d885c72b8f5863bd66047";
const KVLAB_EXECUTION_REVISION: &str = "404577ce939093767dc75d2d67de2fe3c16fa4dc";
const NNIS_RUNTIME_REVISION: &str = "58e7db8e1c4b471a7fe82a4beba11904240c4e89";
const PROSPECT_LAUNCH_VERIFIER_REVISION: &str = "328dfc0c2989b9cfb2dc6b251c181141844f5241";
const MODEL_ID: &str = "HuggingFaceTB/SmolLM2-135M";
const MODEL_REVISION: &str = "93efa2f097d58c2a74874c7e644dbc9b0cee75a2";
const MODEL_SHA256: &str = "80521b40281d6ce74e35c9282c22539e75aa0ac8578892b2a59955ef78d55da1";
const RUNTIME_BACKEND: &str = "nnis-kvlab-v4";
const BYTES_PER_TOKEN: u64 = 46_080;
const INPUT_TOKENS: usize = 27;
const RETAIN_COUNTS: [usize; 3] = [7, 14, 20];
const POLICIES: [&str; 2] = ["lru", "random_seeded"];
// SHA-256 of the exact UTF-8 bytes at KVLAB_PREREGISTRATION_REVISION.
// These are input identities, never evidence of a model execution.
const PREREGISTERED_CAMPAIGN_SHA256: [&str; 3] = [
    "5220e3910750ffad24dad1b023d6393491f9ef2ee3a14621b0dd122fd79d81bb",
    "0b0d461b5a29ed34e22803a287bdc054622008f87073b9f8956a8392cb4605b2",
    "909d0339fda4d1c1272a6da72aa14ab19fab971c68c560f7c82fb25d45dcbb28",
];
const PREREGISTERED_TRACE_SHA256: &str =
    "3411f378fb3c7010eb94361128c019206fb47bda4271f721498d517eb07ca65f";

// Profiles are fixed software contracts, never supplied by an untrusted manifest.
// R1 keeps its original pins; R2 cannot silently fall back to the R1 contract.
struct SuiteContract {
    input_schema: &'static str,
    verification_schema: &'static str,
    provider_revision: &'static str,
    preregistration_revision: &'static str,
    runtime_revision: &'static str,
    launch_verifier_revision: &'static str,
    campaign_root: &'static str,
    campaign_sha256: [&'static str; 3],
    generation: &'static str,
}

const R1_CONTRACT: SuiteContract = SuiteContract {
    input_schema: SUITE_SCHEMA_V1,
    verification_schema: VERIFICATION_SCHEMA_V1,
    provider_revision: KVLAB_SUITE_PROVIDER_REVISION,
    preregistration_revision: KVLAB_PREREGISTRATION_REVISION,
    runtime_revision: NNIS_RUNTIME_REVISION,
    launch_verifier_revision: PROSPECT_LAUNCH_VERIFIER_REVISION,
    campaign_root: "experiments/prospect/smollm2-r1",
    campaign_sha256: PREREGISTERED_CAMPAIGN_SHA256,
    generation: "r1",
};

const R2_CONTRACT: SuiteContract = SuiteContract {
    input_schema: "kvlab.smollm2-r2-position-suite-result/v1",
    verification_schema: "prospect.kv-campaign-suite-r2-verification/v1",
    provider_revision: "cefe129f126c865819545ea94b3d3510400d8964",
    preregistration_revision: "216b49ae4d62ed4c4c2edfd1e88f929d0a0fd9e5",
    runtime_revision: "091aabbb3e132627cf64716720aae530442d2a32",
    launch_verifier_revision: "298acdc91682ef1d09914b6f964e8934828825c0",
    campaign_root: "experiments/prospect/smollm2-r2",
    campaign_sha256: [
        "d826e0ca1869b6f3134e8b34bb65db14aa034d2518bf9559810ae80f19012346",
        "01eeec54e02bf3f56bbd2e75175e2f04fc4593701875a21b90981b40cfa4eff5",
        "e09b14c8479bac98b93625f3d98f667943d8318ff769dbbbf3db989959dcba07",
    ],
    generation: "r2",
};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BaselineMetricSummary {
    pub name: String,
    pub kind: String,
    pub unit: String,
    pub preference: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SuiteCampaignSummary {
    pub retained_count: usize,
    pub logical_retained_bytes: u64,
    pub campaign: CampaignVerificationSummary,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KvCampaignSuiteSummary {
    pub schema: String,
    pub suite_manifest_sha256: String,
    pub kvlab_suite_provider_revision: String,
    pub kvlab_preregistration_revision: String,
    pub kvlab_execution_revision: String,
    pub nnis_runtime_revision: String,
    pub prospect_launch_verifier_revision: String,
    pub model_id: String,
    pub model_revision: String,
    pub source_model_sha256: String,
    pub runtime_backend: String,
    pub bytes_per_token: u64,
    pub trace_sha256: String,
    pub baseline_output_sha256: String,
    pub baseline_logical_kv_bytes: u64,
    pub baseline_metrics: Vec<BaselineMetricSummary>,
    pub campaigns: Vec<SuiteCampaignSummary>,
}

#[derive(Debug)]
pub enum KvCampaignSuiteError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json(serde_json::Error),
    NonCanonicalManifest,
    UnsupportedSchema,
    ProvenanceMismatch(&'static str),
    InvalidManifest(&'static str),
    NonUtf8EntryName(PathBuf),
    UnexpectedEntry(String),
    EntryTypeMismatch(String),
    CampaignVerification {
        retained_count: usize,
        message: String,
    },
    PublishedVerificationMismatch(usize),
    ComparisonInvalid {
        retained_count: usize,
        message: String,
    },
    BudgetMismatch(usize),
    CrossCampaignTraceMismatch,
    CrossCampaignBaselineMismatch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteManifestWire {
    schema: String,
    kvlab_preregistration_revision: String,
    kvlab_execution_revision: String,
    nnis_runtime_revision: String,
    prospect_verifier_revision: String,
    model_id: String,
    model_revision: String,
    source_model_sha256: String,
    runtime_backend: String,
    bytes_per_token: u64,
    device_ordinal: usize,
    campaigns: Vec<SuiteCampaignWire>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteCampaignWire {
    retained_count: usize,
    campaign_path: String,
    output_directory: String,
    campaign_spec_sha256: String,
    trace_sha256: String,
    record_count: usize,
    policies: Vec<String>,
    verification_file: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CampaignWire {
    schema: String,
    experiment_id: String,
    run_repository_revision: String,
    model_id: String,
    model_revision: String,
    tokenizer_revision: String,
    runtime_backend: String,
    runtime_revision: String,
    evaluation_id: String,
    seed: u64,
    bytes_per_token: u64,
    model_input_token_ids: Vec<u64>,
    evaluation_token_ids: Vec<u64>,
    selections: Vec<SelectionWire>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionWire {
    policy: String,
    retained_positions: Vec<usize>,
}

pub fn verify_kv_campaign_suite_directory(
    directory: impl AsRef<Path>,
) -> Result<KvCampaignSuiteSummary, KvCampaignSuiteError> {
    verify_suite_with_contract(directory.as_ref(), &R1_CONTRACT)
}

/// Independently verify the frozen R2 suite. Does not execute a model.
pub fn verify_kv_campaign_suite_r2_directory(
    directory: impl AsRef<Path>,
) -> Result<KvCampaignSuiteSummary, KvCampaignSuiteError> {
    verify_suite_with_contract(directory.as_ref(), &R2_CONTRACT)
}

fn verify_suite_with_contract(
    directory: &Path,
    contract: &SuiteContract,
) -> Result<KvCampaignSuiteSummary, KvCampaignSuiteError> {
    verify_suite_with_budget(directory, contract, &mut TextReadBudget::default())
}

fn verify_suite_with_budget(
    directory: &Path,
    contract: &SuiteContract,
    budget: &mut TextReadBudget,
) -> Result<KvCampaignSuiteSummary, KvCampaignSuiteError> {
    let manifest_path = directory.join("suite-manifest.json");
    let manifest_json = read_text(&manifest_path, budget)?;
    let manifest_value: Value =
        serde_json::from_str(&manifest_json).map_err(KvCampaignSuiteError::Json)?;
    if canonical_json(&manifest_value).map_err(KvCampaignSuiteError::Json)? != manifest_json {
        return Err(KvCampaignSuiteError::NonCanonicalManifest);
    }
    let manifest: SuiteManifestWire =
        serde_json::from_value(manifest_value).map_err(KvCampaignSuiteError::Json)?;
    validate_manifest(&manifest, contract)?;
    validate_directory_entries(directory, &manifest)?;

    let suite_manifest_sha256 = sha256_hex(manifest_json.as_bytes());
    let mut common_trace = None::<String>;
    let mut common_baseline = None::<BaselineIdentity>;
    let mut campaigns = Vec::with_capacity(manifest.campaigns.len());

    for entry in &manifest.campaigns {
        let campaign_directory = directory.join(&entry.output_directory);
        validate_campaign_entry_types(&campaign_directory)?;
        let summary = verify_kv_campaign_directory_with_budget(&campaign_directory, budget)
            .map_err(|error| KvCampaignSuiteError::CampaignVerification {
                retained_count: entry.retained_count,
                message: error.to_string(),
            })?;
        verify_manifest_campaign_summary(entry, &summary)?;
        verify_published_summary(directory, entry, &summary, budget)?;
        verify_campaign_context(&campaign_directory, &manifest, entry, contract, budget)?;

        let comparison =
            load_observed_comparison(&campaign_directory, entry.retained_count, budget)?;
        let expected_budget = checked_bytes(entry.retained_count)?;
        if comparison.logical_budget_bytes() != expected_budget {
            return Err(KvCampaignSuiteError::BudgetMismatch(entry.retained_count));
        }
        let baseline = BaselineIdentity::from_comparison(&comparison);
        if baseline.logical_kv_bytes != checked_bytes(INPUT_TOKENS)? {
            return Err(KvCampaignSuiteError::BudgetMismatch(entry.retained_count));
        }
        if let Some(expected) = &common_baseline {
            if !expected.same_observation(&baseline) {
                return Err(KvCampaignSuiteError::CrossCampaignBaselineMismatch);
            }
        } else {
            common_baseline = Some(baseline);
        }

        if let Some(trace) = &common_trace {
            if trace != summary.trace_sha256() {
                return Err(KvCampaignSuiteError::CrossCampaignTraceMismatch);
            }
        } else {
            common_trace = Some(summary.trace_sha256().to_owned());
        }

        for observation in summary.observations() {
            if observation.retained_positions().len() != entry.retained_count
                || observation.logical_retained_bytes() != expected_budget
                || observation.logical_evicted_bytes()
                    != checked_bytes(INPUT_TOKENS - entry.retained_count)?
            {
                return Err(KvCampaignSuiteError::BudgetMismatch(entry.retained_count));
            }
        }

        campaigns.push(SuiteCampaignSummary {
            retained_count: entry.retained_count,
            logical_retained_bytes: expected_budget,
            campaign: summary,
        });
    }

    let baseline = common_baseline.ok_or(KvCampaignSuiteError::InvalidManifest("campaigns"))?;
    let trace_sha256 = common_trace.ok_or(KvCampaignSuiteError::InvalidManifest("campaigns"))?;
    Ok(KvCampaignSuiteSummary {
        schema: contract.verification_schema.to_owned(),
        suite_manifest_sha256,
        kvlab_suite_provider_revision: contract.provider_revision.to_owned(),
        kvlab_preregistration_revision: manifest.kvlab_preregistration_revision,
        kvlab_execution_revision: manifest.kvlab_execution_revision,
        nnis_runtime_revision: manifest.nnis_runtime_revision,
        prospect_launch_verifier_revision: manifest.prospect_verifier_revision,
        model_id: manifest.model_id,
        model_revision: manifest.model_revision,
        source_model_sha256: manifest.source_model_sha256,
        runtime_backend: manifest.runtime_backend,
        bytes_per_token: manifest.bytes_per_token,
        trace_sha256,
        baseline_output_sha256: baseline.output_sha256,
        baseline_logical_kv_bytes: baseline.logical_kv_bytes,
        baseline_metrics: baseline.metrics,
        campaigns,
    })
}

fn validate_manifest(
    manifest: &SuiteManifestWire,
    contract: &SuiteContract,
) -> Result<(), KvCampaignSuiteError> {
    if manifest.schema != contract.input_schema {
        return Err(KvCampaignSuiteError::UnsupportedSchema);
    }
    for (field, actual, expected) in [
        (
            "kvlab_preregistration_revision",
            manifest.kvlab_preregistration_revision.as_str(),
            contract.preregistration_revision,
        ),
        (
            "kvlab_execution_revision",
            manifest.kvlab_execution_revision.as_str(),
            KVLAB_EXECUTION_REVISION,
        ),
        (
            "nnis_runtime_revision",
            manifest.nnis_runtime_revision.as_str(),
            contract.runtime_revision,
        ),
        (
            "prospect_verifier_revision",
            manifest.prospect_verifier_revision.as_str(),
            contract.launch_verifier_revision,
        ),
        ("model_id", manifest.model_id.as_str(), MODEL_ID),
        (
            "model_revision",
            manifest.model_revision.as_str(),
            MODEL_REVISION,
        ),
        (
            "source_model_sha256",
            manifest.source_model_sha256.as_str(),
            MODEL_SHA256,
        ),
        (
            "runtime_backend",
            manifest.runtime_backend.as_str(),
            RUNTIME_BACKEND,
        ),
    ] {
        if actual != expected {
            return Err(KvCampaignSuiteError::ProvenanceMismatch(field));
        }
    }
    if manifest.bytes_per_token != BYTES_PER_TOKEN {
        return Err(KvCampaignSuiteError::ProvenanceMismatch("bytes_per_token"));
    }
    if manifest.device_ordinal > i32::MAX as usize {
        return Err(KvCampaignSuiteError::InvalidManifest("device_ordinal"));
    }
    if manifest.campaigns.len() != RETAIN_COUNTS.len() {
        return Err(KvCampaignSuiteError::InvalidManifest("campaigns"));
    }
    let mut paths = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    let mut verification_files = BTreeSet::new();
    for (index, (entry, expected_count)) in manifest.campaigns.iter().zip(RETAIN_COUNTS).enumerate()
    {
        if entry.retained_count != expected_count {
            return Err(KvCampaignSuiteError::InvalidManifest("retained_count"));
        }
        if entry.campaign_spec_sha256 != contract.campaign_sha256[index]
            || entry.trace_sha256 != PREREGISTERED_TRACE_SHA256
        {
            return Err(KvCampaignSuiteError::ProvenanceMismatch(
                "preregistered campaign bytes",
            ));
        }
        let stem = format!("retain-{expected_count:02}-of-27");
        let expected_campaign_path = format!("{}/{stem}.json", contract.campaign_root);
        let expected_verification = format!("verification-{stem}.json");
        if entry.campaign_path != expected_campaign_path
            || entry.output_directory != stem
            || entry.verification_file != expected_verification
        {
            return Err(KvCampaignSuiteError::InvalidManifest("campaign paths"));
        }
        if entry.record_count != POLICIES.len()
            || entry
                .policies
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                != POLICIES
        {
            return Err(KvCampaignSuiteError::InvalidManifest("policies"));
        }
        if !is_lower_hex(&entry.campaign_spec_sha256, 64) || !is_lower_hex(&entry.trace_sha256, 64)
        {
            return Err(KvCampaignSuiteError::InvalidManifest("campaign digest"));
        }
        if !paths.insert(&entry.campaign_path)
            || !outputs.insert(&entry.output_directory)
            || !verification_files.insert(&entry.verification_file)
        {
            return Err(KvCampaignSuiteError::InvalidManifest(
                "duplicate campaign entry",
            ));
        }
    }
    Ok(())
}

fn validate_directory_entries(
    directory: &Path,
    manifest: &SuiteManifestWire,
) -> Result<(), KvCampaignSuiteError> {
    let mut expected = BTreeMap::<String, bool>::new();
    expected.insert("suite-manifest.json".to_owned(), false);
    for campaign in &manifest.campaigns {
        expected.insert(campaign.output_directory.clone(), true);
        expected.insert(campaign.verification_file.clone(), false);
    }
    let entries = fs::read_dir(directory).map_err(|source| KvCampaignSuiteError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    let mut seen = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|source| KvCampaignSuiteError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| KvCampaignSuiteError::NonUtf8EntryName(path.clone()))?;
        let Some(expect_directory) = expected.get(&name).copied() else {
            return Err(KvCampaignSuiteError::UnexpectedEntry(name));
        };
        let file_type = entry
            .file_type()
            .map_err(|source| KvCampaignSuiteError::Io {
                path: path.clone(),
                source,
            })?;
        if file_type.is_dir() != expect_directory || (!expect_directory && !file_type.is_file()) {
            return Err(KvCampaignSuiteError::EntryTypeMismatch(name));
        }
        seen.insert(name);
    }
    if seen.len() != expected.len() || expected.keys().any(|name| !seen.contains(name)) {
        return Err(KvCampaignSuiteError::InvalidManifest("missing suite entry"));
    }
    Ok(())
}

fn verify_manifest_campaign_summary(
    entry: &SuiteCampaignWire,
    summary: &CampaignVerificationSummary,
) -> Result<(), KvCampaignSuiteError> {
    if summary.campaign_spec_sha256() != entry.campaign_spec_sha256
        || summary.trace_sha256() != entry.trace_sha256
        || summary.record_count() != entry.record_count
        || summary.policies() != entry.policies.as_slice()
    {
        return Err(KvCampaignSuiteError::CampaignVerification {
            retained_count: entry.retained_count,
            message: "suite manifest does not match independently verified campaign".to_owned(),
        });
    }
    Ok(())
}

fn verify_published_summary(
    directory: &Path,
    entry: &SuiteCampaignWire,
    summary: &CampaignVerificationSummary,
    budget: &mut TextReadBudget,
) -> Result<(), KvCampaignSuiteError> {
    let path = directory.join(&entry.verification_file);
    let payload = read_text(&path, budget)?;
    let value: Value = serde_json::from_str(&payload).map_err(KvCampaignSuiteError::Json)?;
    if canonical_json(&value).map_err(KvCampaignSuiteError::Json)? != payload {
        return Err(KvCampaignSuiteError::PublishedVerificationMismatch(
            entry.retained_count,
        ));
    }
    let expected = serde_json::to_value(summary).map_err(KvCampaignSuiteError::Json)?;
    if value != expected {
        return Err(KvCampaignSuiteError::PublishedVerificationMismatch(
            entry.retained_count,
        ));
    }
    Ok(())
}

fn verify_campaign_context(
    campaign_directory: &Path,
    manifest: &SuiteManifestWire,
    entry: &SuiteCampaignWire,
    contract: &SuiteContract,
    budget: &mut TextReadBudget,
) -> Result<(), KvCampaignSuiteError> {
    let payload = read_text(&campaign_directory.join("campaign.json"), budget)?;
    let campaign: CampaignWire =
        serde_json::from_str(&payload).map_err(KvCampaignSuiteError::Json)?;
    if campaign.schema != "kvlab.prospect-kv-real-model-position-campaign/v1"
        || campaign.run_repository_revision != manifest.kvlab_execution_revision
        || campaign.model_id != manifest.model_id
        || campaign.model_revision != manifest.model_revision
        || campaign.tokenizer_revision != MODEL_REVISION
        || campaign.runtime_backend != manifest.runtime_backend
        || campaign.runtime_revision != manifest.nnis_runtime_revision
        || campaign.bytes_per_token != manifest.bytes_per_token
        || campaign.seed != 7
        || campaign.model_input_token_ids.len() != INPUT_TOKENS
        || campaign.evaluation_token_ids.len() != 8
        || campaign.selections.len() != POLICIES.len()
    {
        return Err(KvCampaignSuiteError::CampaignVerification {
            retained_count: entry.retained_count,
            message: "campaign execution context drifted from suite manifest".to_owned(),
        });
    }
    let expected_experiment = format!(
        "smollm2-{}-position-retain-{:02}-of-27",
        contract.generation, entry.retained_count
    );
    if campaign.experiment_id != expected_experiment || campaign.evaluation_id.trim().is_empty() {
        return Err(KvCampaignSuiteError::CampaignVerification {
            retained_count: entry.retained_count,
            message: "campaign experiment/evaluation identity is empty".to_owned(),
        });
    }
    for (selection, expected_policy) in campaign.selections.iter().zip(POLICIES) {
        if selection.policy != expected_policy
            || selection.retained_positions.len() != entry.retained_count
        {
            return Err(KvCampaignSuiteError::BudgetMismatch(entry.retained_count));
        }
    }
    Ok(())
}

fn load_observed_comparison(
    campaign_directory: &Path,
    retained_count: usize,
    budget: &mut TextReadBudget,
) -> Result<ObservedKvPositionComparison, KvCampaignSuiteError> {
    let mut records = Vec::with_capacity(POLICIES.len());
    for index in 0..POLICIES.len() {
        let path = campaign_directory.join(format!("selection-{index:03}.json"));
        let payload = read_text(&path, budget)?;
        let record =
            KvlabKvRealModelPositionEvidenceV2::from_canonical_json(&payload).map_err(|error| {
                KvCampaignSuiteError::ComparisonInvalid {
                    retained_count,
                    message: error.to_string(),
                }
            })?;
        records.push(record);
    }
    ObservedKvPositionComparison::new(records).map_err(|error| {
        KvCampaignSuiteError::ComparisonInvalid {
            retained_count,
            message: format!("{error:?}"),
        }
    })
}

#[derive(Clone, Debug)]
struct BaselineIdentity {
    output_sha256: String,
    logical_kv_bytes: u64,
    retained_positions: Vec<usize>,
    retained_token_ids: Vec<u64>,
    metrics: Vec<BaselineMetricSummary>,
}

impl BaselineIdentity {
    fn from_comparison(comparison: &ObservedKvPositionComparison) -> Self {
        let baseline = comparison.baseline();
        Self {
            output_sha256: baseline.output_sha256().to_owned(),
            logical_kv_bytes: baseline.logical_kv_bytes(),
            retained_positions: baseline.retained_positions().to_vec(),
            retained_token_ids: baseline.retained_token_ids().to_vec(),
            metrics: baseline
                .metrics()
                .iter()
                .map(|metric| BaselineMetricSummary {
                    name: metric.name().to_owned(),
                    kind: metric.kind().as_str().to_owned(),
                    unit: metric.unit().to_owned(),
                    preference: metric.preference().as_str().to_owned(),
                    value: metric.value(),
                })
                .collect(),
        }
    }

    fn same_observation(&self, other: &Self) -> bool {
        self.output_sha256 == other.output_sha256
            && self.logical_kv_bytes == other.logical_kv_bytes
            && self.retained_positions == other.retained_positions
            && self.retained_token_ids == other.retained_token_ids
            && self.metrics.len() == other.metrics.len()
            && self
                .metrics
                .iter()
                .zip(&other.metrics)
                .all(|(left, right)| {
                    left.name == right.name
                        && left.kind == right.kind
                        && left.unit == right.unit
                        && left.preference == right.preference
                        && left.value.to_bits() == right.value.to_bits()
                })
    }
}

fn checked_bytes(tokens: usize) -> Result<u64, KvCampaignSuiteError> {
    u64::try_from(tokens)
        .ok()
        .and_then(|tokens| tokens.checked_mul(BYTES_PER_TOKEN))
        .ok_or(KvCampaignSuiteError::InvalidManifest(
            "logical byte overflow",
        ))
}

// The input directory must remain trusted and unmodified during verification.
// These checks reject static symlinks/special files; they are not an OS sandbox.
fn require_regular_file(path: &Path) -> Result<(), KvCampaignSuiteError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| KvCampaignSuiteError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(KvCampaignSuiteError::EntryTypeMismatch(
            path.display().to_string(),
        ));
    }
    Ok(())
}

fn validate_campaign_entry_types(directory: &Path) -> Result<(), KvCampaignSuiteError> {
    let entries = fs::read_dir(directory).map_err(|source| KvCampaignSuiteError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    for (index, entry) in entries.enumerate() {
        if index >= POLICIES.len() + 2 {
            return Err(KvCampaignSuiteError::InvalidManifest(
                "too many campaign entries",
            ));
        }
        let entry = entry.map_err(|source| KvCampaignSuiteError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        require_regular_file(&entry.path())?;
    }
    Ok(())
}

fn read_text(path: &Path, budget: &mut TextReadBudget) -> Result<String, KvCampaignSuiteError> {
    require_regular_file(path)?;
    budget
        .read_text(path)
        .map_err(|source| KvCampaignSuiteError::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical_json(value: &Value) -> Result<String, serde_json::Error> {
    fn write_value(value: &Value, output: &mut String) -> Result<(), serde_json::Error> {
        match value {
            Value::Object(map) => {
                output.push('{');
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                for (index, key) in keys.into_iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    output.push_str(&serde_json::to_string(key)?);
                    output.push(':');
                    write_value(&map[key], output)?;
                }
                output.push('}');
            }
            Value::Array(values) => {
                output.push('[');
                for (index, item) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    write_value(item, output)?;
                }
                output.push(']');
            }
            other => output.push_str(&serde_json::to_string(other)?),
        }
        Ok(())
    }
    let mut output = String::new();
    write_value(value, &mut output)?;
    Ok(output)
}

impl fmt::Display for KvCampaignSuiteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::Json(error) => write!(formatter, "invalid suite JSON: {error}"),
            Self::NonCanonicalManifest => {
                formatter.write_str("suite manifest is not canonical JSON")
            }
            Self::UnsupportedSchema => formatter.write_str("unsupported KV campaign suite schema"),
            Self::ProvenanceMismatch(field) => {
                write!(formatter, "suite provenance mismatch: {field}")
            }
            Self::InvalidManifest(field) => {
                write!(formatter, "invalid suite manifest field: {field}")
            }
            Self::NonUtf8EntryName(path) => write!(
                formatter,
                "suite entry name is not UTF-8: {}",
                path.display()
            ),
            Self::UnexpectedEntry(name) => write!(formatter, "unexpected suite entry: {name}"),
            Self::EntryTypeMismatch(name) => {
                write!(formatter, "suite entry has wrong file type: {name}")
            }
            Self::CampaignVerification {
                retained_count,
                message,
            } => write!(
                formatter,
                "retain-{retained_count:02} campaign verification failed: {message}"
            ),
            Self::PublishedVerificationMismatch(retained_count) => write!(
                formatter,
                "retain-{retained_count:02} published verification summary does not match independent replay"
            ),
            Self::ComparisonInvalid {
                retained_count,
                message,
            } => write!(
                formatter,
                "retain-{retained_count:02} observed comparison is invalid: {message}"
            ),
            Self::BudgetMismatch(retained_count) => write!(
                formatter,
                "retain-{retained_count:02} logical budget mismatch"
            ),
            Self::CrossCampaignTraceMismatch => {
                formatter.write_str("suite campaigns do not share one trace")
            }
            Self::CrossCampaignBaselineMismatch => {
                formatter.write_str("suite campaigns do not share one exact observed baseline")
            }
        }
    }
}

impl std::error::Error for KvCampaignSuiteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::*;

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "prospect-kv-suite-{label}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn verifies_complete_three_budget_suite() {
        let directory = TempDirectory::new("valid");
        write_suite(directory.path(), false);
        let summary = verify_kv_campaign_suite_directory(directory.path()).unwrap();
        assert_eq!(summary.campaigns.len(), 3);
        assert_eq!(summary.trace_sha256.len(), 64);
        assert_eq!(summary.baseline_logical_kv_bytes, 27 * BYTES_PER_TOKEN);
        assert_eq!(summary.baseline_metrics.len(), 2);
        assert_eq!(
            summary
                .campaigns
                .iter()
                .map(|campaign| campaign.retained_count)
                .collect::<Vec<_>>(),
            RETAIN_COUNTS
        );
    }

    #[test]
    fn rejects_cross_budget_baseline_drift() {
        let directory = TempDirectory::new("baseline-drift");
        write_suite(directory.path(), true);
        assert!(matches!(
            verify_kv_campaign_suite_directory(directory.path()),
            Err(KvCampaignSuiteError::CrossCampaignBaselineMismatch)
        ));
    }

    #[test]
    fn rejects_unexpected_suite_entry() {
        let directory = TempDirectory::new("extra");
        write_suite(directory.path(), false);
        fs::write(directory.path().join("notes.txt"), "not evidence").unwrap();
        assert!(matches!(
            verify_kv_campaign_suite_directory(directory.path()),
            Err(KvCampaignSuiteError::UnexpectedEntry(name)) if name == "notes.txt"
        ));
    }

    // Output metrics and output digests in these fixtures are synthetic.
    // Their campaign input bytes are the frozen real R2 preregistration only.
    fn write_r2_suite(directory: &Path) {
        write_suite_for_contract(directory, false, None, &R2_CONTRACT);
    }

    fn edit_json_file(path: &Path, edit: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        edit(&mut value);
        fs::write(path, canonical_json(&value).unwrap()).unwrap();
    }

    fn refresh_r2_records(root: &Path, count: usize) {
        let directory = root.join(format!("retain-{count:02}-of-27"));
        edit_json_file(&directory.join("manifest.json"), |manifest| {
            for record in manifest["records"].as_array_mut().unwrap() {
                let path = directory.join(record["filename"].as_str().unwrap());
                record["sha256"] = json!(sha256_hex(&fs::read(path).unwrap()));
            }
        });
        let summary = verify_kv_campaign_directory(&directory).unwrap();
        fs::write(
            root.join(format!("verification-retain-{count:02}-of-27.json")),
            canonical_json(&serde_json::to_value(summary).unwrap()).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn r2_verifies_exact_frozen_inputs_and_is_deterministic() {
        let directory = TempDirectory::new("r2-valid");
        write_r2_suite(directory.path());
        let summary = verify_kv_campaign_suite_r2_directory(directory.path()).unwrap();
        let again = verify_kv_campaign_suite_r2_directory(directory.path()).unwrap();
        assert_eq!(
            serde_json::to_string(&summary).unwrap(),
            serde_json::to_string(&again).unwrap()
        );
        assert_eq!(summary.schema, R2_CONTRACT.verification_schema);
        assert_eq!(
            summary.kvlab_suite_provider_revision,
            R2_CONTRACT.provider_revision
        );
        assert_eq!(
            summary.kvlab_preregistration_revision,
            R2_CONTRACT.preregistration_revision
        );
        assert_eq!(summary.nnis_runtime_revision, R2_CONTRACT.runtime_revision);
        assert_eq!(
            summary.prospect_launch_verifier_revision,
            R2_CONTRACT.launch_verifier_revision
        );
        assert_eq!(summary.trace_sha256, PREREGISTERED_TRACE_SHA256);
        assert_eq!(summary.baseline_logical_kv_bytes, 27 * BYTES_PER_TOKEN);
        assert_eq!(summary.campaigns.len(), 3);
        for (index, campaign) in summary.campaigns.iter().enumerate() {
            assert_eq!(campaign.retained_count, RETAIN_COUNTS[index]);
            assert_eq!(
                campaign.campaign.campaign_spec_sha256(),
                R2_CONTRACT.campaign_sha256[index]
            );
            assert_eq!(
                campaign.logical_retained_bytes,
                RETAIN_COUNTS[index] as u64 * BYTES_PER_TOKEN
            );
            assert_eq!(campaign.campaign.record_count(), 2);
        }
    }

    #[test]
    fn r1_and_r2_commands_do_not_accept_each_others_suite() {
        let r1 = TempDirectory::new("r1-disjoint");
        let r2 = TempDirectory::new("r2-disjoint");
        write_suite(r1.path(), false);
        write_r2_suite(r2.path());
        assert!(verify_kv_campaign_suite_directory(r1.path()).is_ok());
        assert!(verify_kv_campaign_suite_r2_directory(r2.path()).is_ok());
        assert!(matches!(
            verify_kv_campaign_suite_directory(r2.path()),
            Err(KvCampaignSuiteError::UnsupportedSchema)
        ));
        assert!(matches!(
            verify_kv_campaign_suite_r2_directory(r1.path()),
            Err(KvCampaignSuiteError::UnsupportedSchema)
        ));
    }

    #[test]
    fn r2_rejects_coherently_rehashed_input_selection_and_evaluation_substitutions() {
        for drift in ["input", "selection", "evaluation"] {
            let directory = TempDirectory::new("r2-rehashed");
            write_suite_for_contract(directory.path(), false, Some(drift), &R2_CONTRACT);
            assert!(
                matches!(
                    verify_kv_campaign_suite_r2_directory(directory.path()),
                    Err(KvCampaignSuiteError::ProvenanceMismatch(
                        "preregistered campaign bytes"
                    ))
                ),
                "accepted {drift}"
            );
        }
    }

    #[test]
    fn r2_rejects_each_drifted_manifest_identity() {
        for field in [
            "kvlab_preregistration_revision",
            "kvlab_execution_revision",
            "nnis_runtime_revision",
            "prospect_verifier_revision",
            "model_id",
            "model_revision",
            "source_model_sha256",
            "runtime_backend",
        ] {
            let directory = TempDirectory::new("r2-identity");
            write_r2_suite(directory.path());
            edit_json_file(&directory.path().join("suite-manifest.json"), |v| {
                v[field] = json!("drifted")
            });
            assert!(
                matches!(
                    verify_kv_campaign_suite_r2_directory(directory.path()),
                    Err(KvCampaignSuiteError::ProvenanceMismatch(_))
                ),
                "accepted {field}"
            );
        }
    }

    #[test]
    fn r2_rejects_preflight_receipts_and_unknown_schemas() {
        for schema in [
            "kvlab.smollm2-r2-position-suite-preflight/v1",
            "unknown/v1",
            "kvlab.smollm2-r2-position-suite-result/v2",
        ] {
            let directory = TempDirectory::new("r2-schema");
            write_r2_suite(directory.path());
            edit_json_file(&directory.path().join("suite-manifest.json"), |v| {
                v["schema"] = json!(schema)
            });
            assert!(matches!(
                verify_kv_campaign_suite_r2_directory(directory.path()),
                Err(KvCampaignSuiteError::UnsupportedSchema)
            ));
        }
    }

    #[test]
    fn r2_rejects_manifest_structure_order_paths_and_device_drift() {
        for change in [
            "missing",
            "duplicate",
            "order",
            "path",
            "extra",
            "device",
            "float",
        ] {
            let directory = TempDirectory::new("r2-manifest");
            write_r2_suite(directory.path());
            edit_json_file(
                &directory.path().join("suite-manifest.json"),
                |v| match change {
                    "missing" => {
                        v["campaigns"].as_array_mut().unwrap().pop();
                    }
                    "duplicate" => v["campaigns"][1] = v["campaigns"][0].clone(),
                    "order" => v["campaigns"].as_array_mut().unwrap().swap(0, 1),
                    "path" => v["campaigns"][0]["output_directory"] = json!("../outside"),
                    "extra" => v["unrecognized"] = json!(true),
                    "device" => v["device_ordinal"] = json!(2147483648_u64),
                    "float" => v["campaigns"][0]["retained_count"] = json!(7.0),
                    _ => unreachable!(),
                },
            );
            assert!(
                verify_kv_campaign_suite_r2_directory(directory.path()).is_err(),
                "accepted {change}"
            );
        }
    }

    #[test]
    fn r2_rejects_noncanonical_and_duplicate_manifest_keys() {
        for duplicate in [false, true] {
            let directory = TempDirectory::new("r2-canonical");
            write_r2_suite(directory.path());
            let path = directory.path().join("suite-manifest.json");
            let payload = fs::read_to_string(&path).unwrap();
            let invalid = if duplicate {
                payload.replacen('{', "{\"device_ordinal\":0,", 1)
            } else {
                format!("{payload}\n")
            };
            fs::write(&path, invalid).unwrap();
            assert!(matches!(
                verify_kv_campaign_suite_r2_directory(directory.path()),
                Err(KvCampaignSuiteError::NonCanonicalManifest)
            ));
        }
    }

    #[test]
    fn r2_rejects_published_summary_tampering() {
        let directory = TempDirectory::new("r2-summary");
        write_r2_suite(directory.path());
        edit_json_file(
            &directory.path().join("verification-retain-07-of-27.json"),
            |v| v["observations"][0]["metrics"][0]["candidate_value"] = json!(42.0),
        );
        assert!(matches!(
            verify_kv_campaign_suite_r2_directory(directory.path()),
            Err(KvCampaignSuiteError::PublishedVerificationMismatch(7))
        ));
    }

    #[test]
    fn r2_rejects_cross_budget_baseline_output_drift() {
        let directory = TempDirectory::new("r2-baseline-output");
        write_suite_for_contract(directory.path(), true, None, &R2_CONTRACT);
        assert!(matches!(
            verify_kv_campaign_suite_r2_directory(directory.path()),
            Err(KvCampaignSuiteError::CrossCampaignBaselineMismatch)
        ));
    }

    #[test]
    fn r2_rejects_coherently_rehashed_cross_budget_baseline_metric_drift() {
        let directory = TempDirectory::new("r2-baseline-metric");
        write_r2_suite(directory.path());
        for index in 0..2 {
            let path = directory
                .path()
                .join(format!("retain-20-of-27/selection-{index:03}.json"));
            edit_json_file(&path, |v| {
                let metric = &mut v["metrics"][0];
                metric["baseline_value"] = json!(1.5);
                metric["delta"] = json!(metric["candidate_value"].as_f64().unwrap() - 1.5);
            });
        }
        refresh_r2_records(directory.path(), 20);
        assert!(matches!(
            verify_kv_campaign_suite_r2_directory(directory.path()),
            Err(KvCampaignSuiteError::CrossCampaignBaselineMismatch)
        ));
    }

    #[test]
    fn r2_rejects_missing_evidence_and_unexpected_entries() {
        for extra in [false, true] {
            let directory = TempDirectory::new("r2-file-set");
            write_r2_suite(directory.path());
            if extra {
                fs::write(directory.path().join("not-evidence.txt"), "x").unwrap();
            } else {
                fs::remove_file(directory.path().join("retain-07-of-27/selection-000.json"))
                    .unwrap();
            }
            assert!(verify_kv_campaign_suite_r2_directory(directory.path()).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn r2_rejects_symlinks_at_every_evidence_boundary() {
        use std::os::unix::fs::symlink;
        for relative in [
            "suite-manifest.json",
            "verification-retain-07-of-27.json",
            "retain-07-of-27/campaign.json",
            "retain-07-of-27/manifest.json",
            "retain-07-of-27/selection-000.json",
        ] {
            let directory = TempDirectory::new("r2-symlink");
            let outside = TempDirectory::new("r2-symlink-target");
            write_r2_suite(directory.path());
            let path = directory.path().join(relative);
            let target = outside.path().join("payload.json");
            fs::rename(&path, &target).unwrap();
            symlink(&target, &path).unwrap();
            assert!(
                matches!(
                    verify_kv_campaign_suite_r2_directory(directory.path()),
                    Err(KvCampaignSuiteError::EntryTypeMismatch(_))
                ),
                "accepted symlink {relative}"
            );
        }
    }

    #[test]
    fn r1_summary_schema_and_pins_stay_unchanged() {
        let directory = TempDirectory::new("r1-regression");
        write_suite(directory.path(), false);
        let summary = verify_kv_campaign_suite_directory(directory.path()).unwrap();
        assert_eq!(summary.schema, VERIFICATION_SCHEMA_V1);
        assert_eq!(
            summary.kvlab_suite_provider_revision,
            KVLAB_SUITE_PROVIDER_REVISION
        );
        assert_eq!(
            summary.kvlab_preregistration_revision,
            KVLAB_PREREGISTRATION_REVISION
        );
        assert_eq!(summary.nnis_runtime_revision, NNIS_RUNTIME_REVISION);
        assert_eq!(
            summary.prospect_launch_verifier_revision,
            PROSPECT_LAUNCH_VERIFIER_REVISION
        );
    }

    #[test]
    fn input_limits_suite_shares_one_budget_across_all_reads() {
        for contract in [&R1_CONTRACT, &R2_CONTRACT] {
            let directory = TempDirectory::new("suite-read-budget");
            write_suite_for_contract(directory.path(), false, None, contract);
            let size = |path: PathBuf| fs::read(path).unwrap().len();
            let mut total = size(directory.path().join("suite-manifest.json"));
            for count in RETAIN_COUNTS {
                let campaign = directory.path().join(format!("retain-{count:02}-of-27"));
                total += size(campaign.join("manifest.json"));
                // Context and baseline comparison reread these exact inputs.
                total += 2 * size(campaign.join("campaign.json"));
                for index in 0..POLICIES.len() {
                    total += 2 * size(campaign.join(format!("selection-{index:03}.json")));
                }
                total += size(
                    directory
                        .path()
                        .join(format!("verification-retain-{count:02}-of-27.json")),
                );
            }
            let mut exact = TextReadBudget::new(total, total).unwrap();
            assert!(verify_suite_with_budget(directory.path(), contract, &mut exact).is_ok());
            assert_eq!(exact.remaining_bytes(), 0);
            let mut short = TextReadBudget::new(total, total - 1).unwrap();
            assert!(verify_suite_with_budget(directory.path(), contract, &mut short).is_err());
        }
    }

    fn write_suite(directory: &Path, baseline_drift: bool) {
        write_suite_variant(directory, baseline_drift, None);
    }

    #[test]
    fn rejects_coherently_rehashed_preregistration_substitutions() {
        for drift in ["input", "selection", "evaluation"] {
            let directory = TempDirectory::new(drift);
            // Every generated campaign still passes the generic verifier.
            // Only the fixed-suite identity check must reject substitution.
            write_suite_variant(directory.path(), false, Some(drift));
            assert!(
                matches!(
                    verify_kv_campaign_suite_directory(directory.path()),
                    Err(KvCampaignSuiteError::ProvenanceMismatch(
                        "preregistered campaign bytes"
                    ))
                ),
                "accepted rehashed {drift} substitution"
            );
        }
    }

    fn write_suite_variant(directory: &Path, baseline_drift: bool, drift: Option<&str>) {
        write_suite_for_contract(directory, baseline_drift, drift, &R1_CONTRACT);
    }

    fn write_suite_for_contract(
        directory: &Path,
        baseline_drift: bool,
        drift: Option<&str>,
        contract: &SuiteContract,
    ) {
        let mut suite_entries = Vec::new();
        for retained_count in RETAIN_COUNTS {
            let stem = format!("retain-{retained_count:02}-of-27");
            let campaign_dir = directory.join(&stem);
            fs::create_dir(&campaign_dir).unwrap();
            let baseline_char = if baseline_drift && retained_count == 20 {
                '9'
            } else {
                '2'
            };
            let (campaign_sha, trace_sha, summary) = write_campaign(
                &campaign_dir,
                retained_count,
                baseline_char,
                drift,
                contract,
            );
            let verification_file = format!("verification-{stem}.json");
            let verification_value = serde_json::to_value(&summary).unwrap();
            fs::write(
                directory.join(&verification_file),
                canonical_json(&verification_value).unwrap(),
            )
            .unwrap();
            suite_entries.push(json!({
                "retained_count":retained_count,
                "campaign_path":format!("{}/{stem}.json", contract.campaign_root),
                "output_directory":stem,
                "campaign_spec_sha256":campaign_sha,
                "trace_sha256":trace_sha,
                "record_count":2,
                "policies":["lru","random_seeded"],
                "verification_file":verification_file
            }));
        }
        let suite = json!({
            "schema":contract.input_schema,
            "kvlab_preregistration_revision":contract.preregistration_revision,
            "kvlab_execution_revision":KVLAB_EXECUTION_REVISION,
            "nnis_runtime_revision":contract.runtime_revision,
            "prospect_verifier_revision":contract.launch_verifier_revision,
            "model_id":MODEL_ID,
            "model_revision":MODEL_REVISION,
            "source_model_sha256":MODEL_SHA256,
            "runtime_backend":RUNTIME_BACKEND,
            "bytes_per_token":BYTES_PER_TOKEN,
            "device_ordinal":0,
            "campaigns":suite_entries
        });
        fs::write(
            directory.join("suite-manifest.json"),
            canonical_json(&suite).unwrap(),
        )
        .unwrap();
    }

    fn write_campaign(
        directory: &Path,
        retained_count: usize,
        baseline_char: char,
        drift: Option<&str>,
        contract: &SuiteContract,
    ) -> (String, String, CampaignVerificationSummary) {
        // Actual frozen inputs; only output metrics/digests below are synthetic fixtures.
        let mut model_tokens = vec![
            22007_u64, 6463, 314, 260, 3075, 338, 6650, 260, 2591, 284, 260, 8872, 1592, 30, 198,
            198, 504, 8872, 314, 253, 8304, 282, 260, 2591, 30, 657, 314,
        ];
        let evaluation_tokens = vec![253_u64, 19284, 1248, 338, 21837, 260, 2591, 30];
        if drift == Some("input") {
            model_tokens[0] += 1;
        }
        let evaluation_id = if drift == Some("evaluation") {
            "substituted-evaluation"
        } else {
            "nnis-r1-gravity-is-tail8"
        };
        let lru_positions = ((INPUT_TOKENS - retained_count)..INPUT_TOKENS).collect::<Vec<_>>();
        let mut random_positions: Vec<usize> = match retained_count {
            7 => vec![1, 2, 4, 10, 12, 17, 20],
            14 => vec![0, 1, 2, 3, 4, 6, 10, 11, 12, 16, 17, 20, 22, 23],
            20 => vec![
                0, 1, 2, 3, 4, 6, 10, 11, 12, 13, 15, 16, 17, 18, 19, 20, 22, 23, 24, 26,
            ],
            _ => unreachable!(),
        };
        if drift == Some("selection") {
            random_positions = (0..retained_count).collect();
        }
        let experiment_id = format!(
            "smollm2-{}-position-retain-{retained_count:02}-of-27",
            contract.generation
        );
        let campaign = json!({
            "schema":"kvlab.prospect-kv-real-model-position-campaign/v1",
            "experiment_id":experiment_id,
            "run_repository_revision":KVLAB_EXECUTION_REVISION,
            "model_id":MODEL_ID,
            "model_revision":MODEL_REVISION,
            "tokenizer_revision":MODEL_REVISION,
            "runtime_backend":RUNTIME_BACKEND,
            "runtime_revision":contract.runtime_revision,
            "evaluation_id":evaluation_id,
            "seed":7,
            "bytes_per_token":BYTES_PER_TOKEN,
            "model_input_token_ids":model_tokens,
            "evaluation_token_ids":evaluation_tokens,
            "selections":[
                {"policy":"lru","retained_positions":lru_positions},
                {"policy":"random_seeded","retained_positions":random_positions}
            ]
        });
        let campaign_json = canonical_json(&campaign).unwrap();
        let trace_json = canonical_json(&json!({
            "schema":"kvlab.prospect-kv-real-model-position-trace/v1",
            "model_input_token_ids":campaign["model_input_token_ids"],
            "evaluation_token_ids":campaign["evaluation_token_ids"]
        }))
        .unwrap();
        let trace_sha = sha256_hex(trace_json.as_bytes());
        let campaign_sha = sha256_hex(campaign_json.as_bytes());
        fs::write(directory.join("campaign.json"), &campaign_json).unwrap();

        let selections = [("lru", lru_positions), ("random_seeded", random_positions)];
        let mut descriptors = Vec::new();
        for (index, (policy, retained_positions)) in selections.into_iter().enumerate() {
            let retained = retained_positions.iter().copied().collect::<BTreeSet<_>>();
            let evicted_positions = (0..INPUT_TOKENS)
                .filter(|position| !retained.contains(position))
                .collect::<Vec<_>>();
            let retained_bytes = u64::try_from(retained_count).unwrap() * BYTES_PER_TOKEN;
            let candidate_nll = 1.0 + (index as f64 + 1.0) / 10.0;
            let candidate_accuracy = 0.5 - (index as f64 + 1.0) / 10.0;
            let evidence = json!({
                "schema":"kvlab.prospect-kv-real-model-selection/v2",
                "experiment_id":campaign["experiment_id"],
                "run_repository_revision":KVLAB_EXECUTION_REVISION,
                "model_id":MODEL_ID,
                "model_revision":MODEL_REVISION,
                "tokenizer_revision":MODEL_REVISION,
                "runtime_backend":RUNTIME_BACKEND,
                "runtime_revision":contract.runtime_revision,
                "evaluation_id":evaluation_id,
                "trace_sha256":trace_sha,
                "seed":7,
                "selection":{
                    "schema":"kvlab.prospect-kv-selection/v2",
                    "policy":policy,
                    "input_token_ids":campaign["model_input_token_ids"],
                    "bytes_per_token":BYTES_PER_TOKEN,
                    "retained_positions":retained_positions,
                    "evicted_positions":evicted_positions,
                    "logical_input_bytes":27 * BYTES_PER_TOKEN,
                    "logical_retained_bytes":retained_bytes,
                    "logical_evicted_bytes":u64::try_from(27-retained_count).unwrap() * BYTES_PER_TOKEN
                },
                "baseline_output_sha256":baseline_char.to_string().repeat(64),
                "candidate_output_sha256":char::from(b'3' + u8::try_from(index).unwrap()).to_string().repeat(64),
                "baseline_logical_kv_bytes":27 * BYTES_PER_TOKEN,
                "candidate_logical_kv_bytes":retained_bytes,
                "metrics":[
                    {
                        "name":"mean_nll",
                        "kind":"quality",
                        "unit":"nat_per_token",
                        "preference":"lower_is_better",
                        "baseline_value":1.0,
                        "candidate_value":candidate_nll,
                        "delta":candidate_nll-1.0
                    },
                    {
                        "name":"token_accuracy",
                        "kind":"quality",
                        "unit":"ratio",
                        "preference":"higher_is_better",
                        "baseline_value":0.5,
                        "candidate_value":candidate_accuracy,
                        "delta":candidate_accuracy-0.5
                    }
                ]
            });
            let evidence_json = canonical_json(&evidence).unwrap();
            let filename = format!("selection-{index:03}.json");
            fs::write(directory.join(&filename), &evidence_json).unwrap();
            descriptors.push(json!({
                "index":index,
                "policy":policy,
                "filename":filename,
                "sha256":sha256_hex(evidence_json.as_bytes())
            }));
        }
        let manifest = json!({
            "schema":"kvlab.prospect-kv-real-model-position-campaign-result/v1",
            "campaign_spec_sha256":campaign_sha,
            "trace_sha256":trace_sha,
            "evidence_schema":"kvlab.prospect-kv-real-model-selection/v2",
            "records":descriptors
        });
        fs::write(
            directory.join("manifest.json"),
            canonical_json(&manifest).unwrap(),
        )
        .unwrap();
        let summary = verify_kv_campaign_directory(directory).unwrap();
        (campaign_sha, trace_sha, summary)
    }
}
