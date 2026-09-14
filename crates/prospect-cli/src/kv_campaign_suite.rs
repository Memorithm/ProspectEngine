use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use prospect_cli::{CampaignVerificationSummary, verify_kv_campaign_directory};
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
    Io { path: PathBuf, source: io::Error },
    Json(serde_json::Error),
    NonCanonicalManifest,
    UnsupportedSchema,
    ProvenanceMismatch(&'static str),
    InvalidManifest(&'static str),
    NonUtf8EntryName(PathBuf),
    UnexpectedEntry(String),
    EntryTypeMismatch(String),
    CampaignVerification { retained_count: usize, message: String },
    PublishedVerificationMismatch(usize),
    ComparisonInvalid { retained_count: usize, message: String },
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
    let directory = directory.as_ref();
    let manifest_path = directory.join("suite-manifest.json");
    let manifest_json = read_text(&manifest_path)?;
    let manifest_value: Value =
        serde_json::from_str(&manifest_json).map_err(KvCampaignSuiteError::Json)?;
    if canonical_json(&manifest_value).map_err(KvCampaignSuiteError::Json)? != manifest_json {
        return Err(KvCampaignSuiteError::NonCanonicalManifest);
    }
    let manifest: SuiteManifestWire =
        serde_json::from_value(manifest_value).map_err(KvCampaignSuiteError::Json)?;
    validate_manifest(&manifest)?;
    validate_directory_entries(directory, &manifest)?;

    let suite_manifest_sha256 = sha256_hex(manifest_json.as_bytes());
    let mut common_trace = None::<String>;
    let mut common_baseline = None::<BaselineIdentity>;
    let mut campaigns = Vec::with_capacity(manifest.campaigns.len());

    for entry in &manifest.campaigns {
        let campaign_directory = directory.join(&entry.output_directory);
        let summary = verify_kv_campaign_directory(&campaign_directory).map_err(|error| {
            KvCampaignSuiteError::CampaignVerification {
                retained_count: entry.retained_count,
                message: error.to_string(),
            }
        })?;
        verify_manifest_campaign_summary(entry, &summary)?;
        verify_published_summary(directory, entry, &summary)?;
        verify_campaign_context(&campaign_directory, &manifest, entry)?;

        let comparison = load_observed_comparison(&campaign_directory, entry.retained_count)?;
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
        schema: VERIFICATION_SCHEMA_V1.to_owned(),
        suite_manifest_sha256,
        kvlab_suite_provider_revision: KVLAB_SUITE_PROVIDER_REVISION.to_owned(),
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

fn validate_manifest(manifest: &SuiteManifestWire) -> Result<(), KvCampaignSuiteError> {
    if manifest.schema != SUITE_SCHEMA_V1 {
        return Err(KvCampaignSuiteError::UnsupportedSchema);
    }
    for (field, actual, expected) in [
        (
            "kvlab_preregistration_revision",
            manifest.kvlab_preregistration_revision.as_str(),
            KVLAB_PREREGISTRATION_REVISION,
        ),
        (
            "kvlab_execution_revision",
            manifest.kvlab_execution_revision.as_str(),
            KVLAB_EXECUTION_REVISION,
        ),
        (
            "nnis_runtime_revision",
            manifest.nnis_runtime_revision.as_str(),
            NNIS_RUNTIME_REVISION,
        ),
        (
            "prospect_verifier_revision",
            manifest.prospect_verifier_revision.as_str(),
            PROSPECT_LAUNCH_VERIFIER_REVISION,
        ),
        ("model_id", manifest.model_id.as_str(), MODEL_ID),
        ("model_revision", manifest.model_revision.as_str(), MODEL_REVISION),
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
    let _device_ordinal = manifest.device_ordinal;
    if manifest.campaigns.len() != RETAIN_COUNTS.len() {
        return Err(KvCampaignSuiteError::InvalidManifest("campaigns"));
    }
    let mut paths = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    let mut verification_files = BTreeSet::new();
    for (entry, expected_count) in manifest.campaigns.iter().zip(RETAIN_COUNTS) {
        if entry.retained_count != expected_count {
            return Err(KvCampaignSuiteError::InvalidManifest("retained_count"));
        }
        let stem = format!("retain-{expected_count:02}-of-27");
        let expected_campaign_path = format!("experiments/prospect/smollm2-r1/{stem}.json");
        let expected_verification = format!("verification-{stem}.json");
        if entry.campaign_path != expected_campaign_path
            || entry.output_directory != stem
            || entry.verification_file != expected_verification
        {
            return Err(KvCampaignSuiteError::InvalidManifest("campaign paths"));
        }
        if entry.record_count != POLICIES.len()
            || entry.policies.iter().map(String::as_str).collect::<Vec<_>>() != POLICIES
        {
            return Err(KvCampaignSuiteError::InvalidManifest("policies"));
        }
        if !is_lower_hex(&entry.campaign_spec_sha256, 64)
            || !is_lower_hex(&entry.trace_sha256, 64)
        {
            return Err(KvCampaignSuiteError::InvalidManifest("campaign digest"));
        }
        if !paths.insert(&entry.campaign_path)
            || !outputs.insert(&entry.output_directory)
            || !verification_files.insert(&entry.verification_file)
        {
            return Err(KvCampaignSuiteError::InvalidManifest("duplicate campaign entry"));
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
        let file_type = entry.file_type().map_err(|source| KvCampaignSuiteError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() != expect_directory
            || (!expect_directory && !file_type.is_file())
        {
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
) -> Result<(), KvCampaignSuiteError> {
    let path = directory.join(&entry.verification_file);
    let payload = read_text(&path)?;
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
) -> Result<(), KvCampaignSuiteError> {
    let payload = read_text(&campaign_directory.join("campaign.json"))?;
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
    if campaign.experiment_id.trim().is_empty() || campaign.evaluation_id.trim().is_empty() {
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
) -> Result<ObservedKvPositionComparison, KvCampaignSuiteError> {
    let mut records = Vec::with_capacity(POLICIES.len());
    for index in 0..POLICIES.len() {
        let path = campaign_directory.join(format!("selection-{index:03}.json"));
        let payload = read_text(&path)?;
        let record = KvlabKvRealModelPositionEvidenceV2::from_canonical_json(&payload)
            .map_err(|error| KvCampaignSuiteError::ComparisonInvalid {
                retained_count,
                message: error.to_string(),
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
            && self.metrics.iter().zip(&other.metrics).all(|(left, right)| {
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
        .ok_or(KvCampaignSuiteError::InvalidManifest("logical byte overflow"))
}

fn read_text(path: &Path) -> Result<String, KvCampaignSuiteError> {
    fs::read_to_string(path).map_err(|source| KvCampaignSuiteError::Io {
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
            Self::Io { path, source } => write!(formatter, "failed to read {}: {source}", path.display()),
            Self::Json(error) => write!(formatter, "invalid suite JSON: {error}"),
            Self::NonCanonicalManifest => formatter.write_str("suite manifest is not canonical JSON"),
            Self::UnsupportedSchema => formatter.write_str("unsupported KV campaign suite schema"),
            Self::ProvenanceMismatch(field) => write!(formatter, "suite provenance mismatch: {field}"),
            Self::InvalidManifest(field) => write!(formatter, "invalid suite manifest field: {field}"),
            Self::NonUtf8EntryName(path) => write!(formatter, "suite entry name is not UTF-8: {}", path.display()),
            Self::UnexpectedEntry(name) => write!(formatter, "unexpected suite entry: {name}"),
            Self::EntryTypeMismatch(name) => write!(formatter, "suite entry has wrong file type: {name}"),
            Self::CampaignVerification { retained_count, message } => write!(formatter, "retain-{retained_count:02} campaign verification failed: {message}"),
            Self::PublishedVerificationMismatch(retained_count) => write!(formatter, "retain-{retained_count:02} published verification summary does not match independent replay"),
            Self::ComparisonInvalid { retained_count, message } => write!(formatter, "retain-{retained_count:02} observed comparison is invalid: {message}"),
            Self::BudgetMismatch(retained_count) => write!(formatter, "retain-{retained_count:02} logical budget mismatch"),
            Self::CrossCampaignTraceMismatch => formatter.write_str("suite campaigns do not share one trace"),
            Self::CrossCampaignBaselineMismatch => formatter.write_str("suite campaigns do not share one exact observed baseline"),
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

    use serde_json::{Value, json};

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

    fn write_suite(directory: &Path, baseline_drift: bool) {
        let mut suite_entries = Vec::new();
        for retained_count in RETAIN_COUNTS {
            let stem = format!("retain-{retained_count:02}-of-27");
            let campaign_dir = directory.join(&stem);
            fs::create_dir(&campaign_dir).unwrap();
            let baseline_char = if baseline_drift && retained_count == 20 { '9' } else { '2' };
            let (campaign_sha, trace_sha, summary) =
                write_campaign(&campaign_dir, retained_count, baseline_char);
            let verification_file = format!("verification-{stem}.json");
            let verification_value = serde_json::to_value(&summary).unwrap();
            fs::write(
                directory.join(&verification_file),
                canonical_json(&verification_value).unwrap(),
            )
            .unwrap();
            suite_entries.push(json!({
                "retained_count":retained_count,
                "campaign_path":format!("experiments/prospect/smollm2-r1/{stem}.json"),
                "output_directory":stem,
                "campaign_spec_sha256":campaign_sha,
                "trace_sha256":trace_sha,
                "record_count":2,
                "policies":["lru","random_seeded"],
                "verification_file":verification_file
            }));
        }
        let suite = json!({
            "schema":SUITE_SCHEMA_V1,
            "kvlab_preregistration_revision":KVLAB_PREREGISTRATION_REVISION,
            "kvlab_execution_revision":KVLAB_EXECUTION_REVISION,
            "nnis_runtime_revision":NNIS_RUNTIME_REVISION,
            "prospect_verifier_revision":PROSPECT_LAUNCH_VERIFIER_REVISION,
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
    ) -> (String, String, CampaignVerificationSummary) {
        let model_tokens = (100_u64..127).collect::<Vec<_>>();
        let evaluation_tokens = vec![200_u64, 201];
        let lru_positions = ((INPUT_TOKENS - retained_count)..INPUT_TOKENS).collect::<Vec<_>>();
        let random_positions = (0..retained_count).collect::<Vec<_>>();
        let experiment_id = format!("smollm2-r1-position-retain-{retained_count:02}-of-27");
        let campaign = json!({
            "schema":"kvlab.prospect-kv-real-model-position-campaign/v1",
            "experiment_id":experiment_id,
            "run_repository_revision":KVLAB_EXECUTION_REVISION,
            "model_id":MODEL_ID,
            "model_revision":MODEL_REVISION,
            "tokenizer_revision":MODEL_REVISION,
            "runtime_backend":RUNTIME_BACKEND,
            "runtime_revision":NNIS_RUNTIME_REVISION,
            "evaluation_id":"nnis-r1-gravity-is-tail8",
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
        })).unwrap();
        let trace_sha = sha256_hex(trace_json.as_bytes());
        let campaign_sha = sha256_hex(campaign_json.as_bytes());
        fs::write(directory.join("campaign.json"), &campaign_json).unwrap();

        let selections = [
            ("lru", lru_positions),
            ("random_seeded", random_positions),
        ];
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
                "runtime_revision":NNIS_RUNTIME_REVISION,
                "evaluation_id":"nnis-r1-gravity-is-tail8",
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
