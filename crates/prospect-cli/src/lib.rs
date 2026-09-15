#![forbid(unsafe_code)]

pub mod execution_journal;
pub mod execution_record;
pub mod execution_restart;
pub mod input;

use input::{MAX_CAMPAIGN_ENTRIES, TextReadBudget};

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use prospect_kv_position_campaign::{
    CampaignFilePayload, PositionCampaignVerificationError, VerifiedPositionCampaign,
};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CampaignVerificationSummary {
    campaign_spec_sha256: String,
    trace_sha256: String,
    policies: Vec<String>,
    observations: Vec<CampaignPolicyObservationSummary>,
    record_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CampaignMetricObservationSummary {
    name: String,
    kind: String,
    unit: String,
    preference: String,
    baseline_value: f64,
    candidate_value: f64,
    delta: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CampaignPolicyObservationSummary {
    policy: String,
    retained_positions: Vec<usize>,
    logical_retained_bytes: u64,
    logical_evicted_bytes: u64,
    metrics: Vec<CampaignMetricObservationSummary>,
}
#[derive(Debug)]
pub enum CampaignDirectoryError {
    Io { path: PathBuf, source: io::Error },
    NonUtf8EntryName(PathBuf),
    NonFileEntry(PathBuf),
    Verification(PositionCampaignVerificationError),
}

impl CampaignVerificationSummary {
    #[must_use]
    pub fn campaign_spec_sha256(&self) -> &str {
        &self.campaign_spec_sha256
    }

    #[must_use]
    pub fn trace_sha256(&self) -> &str {
        &self.trace_sha256
    }

    #[must_use]
    pub fn policies(&self) -> &[String] {
        &self.policies
    }

    #[must_use]
    pub fn observations(&self) -> &[CampaignPolicyObservationSummary] {
        &self.observations
    }

    #[must_use]
    pub const fn record_count(&self) -> usize {
        self.record_count
    }
}

impl CampaignPolicyObservationSummary {
    #[must_use]
    pub fn policy(&self) -> &str {
        &self.policy
    }
    #[must_use]
    pub fn retained_positions(&self) -> &[usize] {
        &self.retained_positions
    }
    #[must_use]
    pub const fn logical_retained_bytes(&self) -> u64 {
        self.logical_retained_bytes
    }
    #[must_use]
    pub const fn logical_evicted_bytes(&self) -> u64 {
        self.logical_evicted_bytes
    }
    #[must_use]
    pub fn metrics(&self) -> &[CampaignMetricObservationSummary] {
        &self.metrics
    }
}

impl CampaignMetricObservationSummary {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }
    #[must_use]
    pub fn unit(&self) -> &str {
        &self.unit
    }
    #[must_use]
    pub fn preference(&self) -> &str {
        &self.preference
    }
    #[must_use]
    pub const fn baseline_value(&self) -> f64 {
        self.baseline_value
    }
    #[must_use]
    pub const fn candidate_value(&self) -> f64 {
        self.candidate_value
    }
    #[must_use]
    pub const fn delta(&self) -> f64 {
        self.delta
    }
}
/// Verify a persisted campaign using the default bounded file-read policy.
///
/// Accepted inputs are not normalized. This reads evidence; it executes no model.
pub fn verify_kv_campaign_directory(
    directory: impl AsRef<Path>,
) -> Result<CampaignVerificationSummary, CampaignDirectoryError> {
    verify_kv_campaign_directory_with_budget(directory, &mut TextReadBudget::default())
}

/// Verify one campaign while sharing an input-byte budget with other verifiers.
///
/// Manifests and evidence all consume the supplied budget, including repeated
/// reads. A directory contains at most `input::MAX_CAMPAIGN_ENTRIES` entries.
/// The caller must abort the composed operation on any error and keep inputs
/// trusted and unmodified. This is not an overall process-memory limit.
///
/// # Examples
///
/// ```no_run
/// use prospect_cli::{input::TextReadBudget, verify_kv_campaign_directory_with_budget};
/// let mut budget = TextReadBudget::default();
/// let first = verify_kv_campaign_directory_with_budget("campaign-a", &mut budget)?;
/// let second = verify_kv_campaign_directory_with_budget("campaign-b", &mut budget)?;
/// assert!(!first.policies().is_empty() && !second.policies().is_empty());
/// # Ok::<(), prospect_cli::CampaignDirectoryError>(())
/// ```
pub fn verify_kv_campaign_directory_with_budget(
    directory: impl AsRef<Path>,
    budget: &mut TextReadBudget,
) -> Result<CampaignVerificationSummary, CampaignDirectoryError> {
    let directory = directory.as_ref();
    let manifest_path = directory.join("manifest.json");
    let campaign_path = directory.join("campaign.json");
    let manifest_json = read_text(&manifest_path, budget)?;
    let campaign_json = read_text(&campaign_path, budget)?;

    let mut owned_evidence = Vec::<(String, String)>::new();
    let entries = fs::read_dir(directory).map_err(|source| CampaignDirectoryError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    for (entry_index, entry) in entries.enumerate() {
        if entry_index >= MAX_CAMPAIGN_ENTRIES {
            return Err(CampaignDirectoryError::Io {
                path: directory.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("campaign entry limit exceeded ({MAX_CAMPAIGN_ENTRIES})"),
                ),
            });
        }
        let entry = entry.map_err(|source| CampaignDirectoryError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| CampaignDirectoryError::NonUtf8EntryName(path.clone()))?;
        if name == "manifest.json" || name == "campaign.json" {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|source| CampaignDirectoryError::Io {
                path: path.clone(),
                source,
            })?;
        if !file_type.is_file() {
            return Err(CampaignDirectoryError::NonFileEntry(path));
        }
        owned_evidence.push((name, read_text(&path, budget)?));
    }
    owned_evidence.sort_by(|left, right| left.0.cmp(&right.0));

    let payloads = owned_evidence
        .iter()
        .map(|(filename, canonical_json)| CampaignFilePayload {
            filename,
            canonical_json,
        })
        .collect::<Vec<_>>();
    let verified = VerifiedPositionCampaign::verify(&manifest_json, &campaign_json, &payloads)
        .map_err(CampaignDirectoryError::Verification)?;

    Ok(CampaignVerificationSummary {
        campaign_spec_sha256: verified.campaign_spec_sha256().to_owned(),
        trace_sha256: verified.trace_sha256().to_owned(),
        policies: verified.policies().to_vec(),
        observations: verified
            .records()
            .iter()
            .map(|record| {
                let outcome = record.selection().outcome();
                CampaignPolicyObservationSummary {
                    policy: outcome.policy().to_owned(),
                    retained_positions: outcome.retained_positions().to_vec(),
                    logical_retained_bytes: outcome.logical_retained_bytes(),
                    logical_evicted_bytes: outcome.logical_evicted_bytes(),
                    metrics: record
                        .metrics()
                        .iter()
                        .map(|metric| CampaignMetricObservationSummary {
                            name: metric.name().to_owned(),
                            kind: metric.kind().as_str().to_owned(),
                            unit: metric.unit().to_owned(),
                            preference: metric.preference().as_str().to_owned(),
                            baseline_value: metric.baseline_value(),
                            candidate_value: metric.candidate_value(),
                            delta: metric.delta(),
                        })
                        .collect(),
                }
            })
            .collect(),
        record_count: verified.records().len(),
    })
}

fn read_text(path: &Path, budget: &mut TextReadBudget) -> Result<String, CampaignDirectoryError> {
    budget
        .read_text(path)
        .map_err(|source| CampaignDirectoryError::Io {
            path: path.to_path_buf(),
            source,
        })
}

impl fmt::Display for CampaignDirectoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::NonUtf8EntryName(path) => {
                write!(
                    formatter,
                    "campaign entry name is not UTF-8: {}",
                    path.display()
                )
            }
            Self::NonFileEntry(path) => {
                write!(
                    formatter,
                    "campaign contains a non-file entry: {}",
                    path.display()
                )
            }
            Self::Verification(error) => write!(formatter, "campaign verification failed: {error}"),
        }
    }
}

impl std::error::Error for CampaignDirectoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Verification(error) => Some(error),
            Self::NonUtf8EntryName(_) | Self::NonFileEntry(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use prospect_kv_position_campaign::PositionCampaignVerificationError;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    use super::{CampaignDirectoryError, verify_kv_campaign_directory};

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "prospect-cli-{label}-{}-{nonce}",
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
    fn verifies_complete_position_campaign_directory() {
        let directory = TempDirectory::new("valid");
        write_fixture(directory.path());

        let summary = verify_kv_campaign_directory(directory.path()).unwrap();
        assert_eq!(summary.policies(), &["lru"]);
        assert_eq!(summary.record_count(), 1);
        assert_eq!(summary.observations().len(), 1);
        let observation = &summary.observations()[0];
        assert_eq!(observation.policy(), "lru");
        assert_eq!(observation.retained_positions(), &[0, 2, 4]);
        assert_eq!(observation.logical_retained_bytes(), 192);
        assert_eq!(observation.logical_evicted_bytes(), 128);
        assert_eq!(observation.metrics().len(), 1);
        let metric = &observation.metrics()[0];
        assert_eq!(metric.name(), "mean_nll");
        assert_eq!(metric.kind(), "quality");
        assert_eq!(metric.unit(), "nat_per_token");
        assert_eq!(metric.preference(), "lower_is_better");
        assert_eq!(metric.baseline_value(), 1.0);
        assert_eq!(metric.candidate_value(), 1.25);
        assert_eq!(metric.delta(), 0.25);
        assert_eq!(summary.campaign_spec_sha256().len(), 64);
        assert_eq!(summary.trace_sha256().len(), 64);
    }

    #[test]
    fn rejects_unexpected_campaign_file() {
        let directory = TempDirectory::new("extra");
        write_fixture(directory.path());
        fs::write(directory.path().join("notes.txt"), "not evidence").unwrap();

        assert!(matches!(
            verify_kv_campaign_directory(directory.path()),
            Err(CampaignDirectoryError::Verification(
                PositionCampaignVerificationError::UnexpectedFile(filename)
            )) if filename == "notes.txt"
        ));
    }

    #[test]
    fn rejects_non_file_entries() {
        let directory = TempDirectory::new("subdir");
        write_fixture(directory.path());
        fs::create_dir(directory.path().join("extra")).unwrap();

        assert!(matches!(
            verify_kv_campaign_directory(directory.path()),
            Err(CampaignDirectoryError::NonFileEntry(path)) if path.ends_with("extra")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn input_limits_rejects_symlinked_reserved_campaign_files() {
        use std::os::unix::fs::symlink;
        for name in ["campaign.json", "manifest.json"] {
            let directory = TempDirectory::new("reserved-symlink");
            let outside = TempDirectory::new("reserved-target");
            write_fixture(directory.path());
            let path = directory.path().join(name);
            let target = outside.path().join(name);
            fs::rename(&path, &target).unwrap();
            symlink(&target, &path).unwrap();
            assert!(
                verify_kv_campaign_directory(directory.path()).is_err(),
                "reserved symlink was accepted: {name}"
            );
        }
    }

    #[test]
    fn input_limits_shared_budget_covers_manifests_and_evidence() {
        let directory = TempDirectory::new("shared-budget");
        write_fixture(directory.path());
        let total = ["campaign.json", "manifest.json", "selection-000.json"]
            .iter()
            .map(|name| fs::read(directory.path().join(name)).unwrap().len())
            .sum::<usize>();
        let mut exact = super::TextReadBudget::new(total, total).unwrap();
        let summary =
            super::verify_kv_campaign_directory_with_budget(directory.path(), &mut exact).unwrap();
        assert_eq!(summary.record_count(), 1);
        assert_eq!(exact.remaining_bytes(), 0);
        let mut short = super::TextReadBudget::new(total, total - 1).unwrap();
        assert!(
            super::verify_kv_campaign_directory_with_budget(directory.path(), &mut short).is_err()
        );
    }

    #[test]
    fn input_limits_rejects_too_many_directory_entries() {
        let directory = TempDirectory::new("entry-budget");
        write_fixture(directory.path());
        for index in 0..super::MAX_CAMPAIGN_ENTRIES {
            fs::write(directory.path().join(format!("extra-{index:04}.json")), "").unwrap();
        }
        let error = verify_kv_campaign_directory(directory.path()).unwrap_err();
        assert!(error.to_string().contains("entry limit exceeded"));
    }

    fn write_fixture(directory: &Path) {
        let campaign_value = json!({
            "schema":"kvlab.prospect-kv-real-model-position-campaign/v1",
            "experiment_id":"campaign-cli",
            "run_repository_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "model_id":"example/model",
            "model_revision":"model-r1",
            "tokenizer_revision":"tok-r1",
            "runtime_backend":"nnis-kvlab-v4",
            "runtime_revision":"runtime-r1",
            "evaluation_id":"eval-001",
            "seed":7,
            "bytes_per_token":64,
            "model_input_token_ids":[7,11,7,7,19],
            "evaluation_token_ids":[1,2],
            "selections":[{"policy":"lru","retained_positions":[0,2,4]}]
        });
        let campaign = canonical_json(&campaign_value);

        let trace = canonical_json(&json!({
            "schema":"kvlab.prospect-kv-real-model-position-trace/v1",
            "model_input_token_ids":[7,11,7,7,19],
            "evaluation_token_ids":[1,2]
        }));
        let trace_sha256 = sha256_hex(trace.as_bytes());

        let evidence = canonical_json(&json!({
            "schema":"kvlab.prospect-kv-real-model-selection/v2",
            "experiment_id":"campaign-cli",
            "run_repository_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "model_id":"example/model",
            "model_revision":"model-r1",
            "tokenizer_revision":"tok-r1",
            "runtime_backend":"nnis-kvlab-v4",
            "runtime_revision":"runtime-r1",
            "evaluation_id":"eval-001",
            "trace_sha256":trace_sha256,
            "seed":7,
            "selection":{
                "schema":"kvlab.prospect-kv-selection/v2",
                "policy":"lru",
                "input_token_ids":[7,11,7,7,19],
                "bytes_per_token":64,
                "retained_positions":[0,2,4],
                "evicted_positions":[1,3],
                "logical_input_bytes":320,
                "logical_retained_bytes":192,
                "logical_evicted_bytes":128
            },
            "baseline_output_sha256":"2222222222222222222222222222222222222222222222222222222222222222",
            "candidate_output_sha256":"3333333333333333333333333333333333333333333333333333333333333333",
            "baseline_logical_kv_bytes":320,
            "candidate_logical_kv_bytes":192,
            "metrics":[{
                "name":"mean_nll",
                "kind":"quality",
                "unit":"nat_per_token",
                "preference":"lower_is_better",
                "baseline_value":1.0,
                "candidate_value":1.25,
                "delta":0.25
            }]
        }));
        let manifest = canonical_json(&json!({
            "schema":"kvlab.prospect-kv-real-model-position-campaign-result/v1",
            "campaign_spec_sha256":sha256_hex(campaign.as_bytes()),
            "trace_sha256":trace_sha256,
            "evidence_schema":"kvlab.prospect-kv-real-model-selection/v2",
            "records":[{
                "index":0,
                "policy":"lru",
                "filename":"selection-000.json",
                "sha256":sha256_hex(evidence.as_bytes())
            }]
        }));

        fs::write(directory.join("campaign.json"), campaign).unwrap();
        fs::write(directory.join("manifest.json"), manifest).unwrap();
        fs::write(directory.join("selection-000.json"), evidence).unwrap();
    }

    fn canonical_json(value: &Value) -> String {
        fn write_value(value: &Value, output: &mut String) {
            match value {
                Value::Object(map) => {
                    output.push('{');
                    let mut keys = map.keys().collect::<Vec<_>>();
                    keys.sort_unstable();
                    for (index, key) in keys.into_iter().enumerate() {
                        if index != 0 {
                            output.push(',');
                        }
                        output.push_str(&serde_json::to_string(key).unwrap());
                        output.push(':');
                        write_value(&map[key], output);
                    }
                    output.push('}');
                }
                Value::Array(values) => {
                    output.push('[');
                    for (index, item) in values.iter().enumerate() {
                        if index != 0 {
                            output.push(',');
                        }
                        write_value(item, output);
                    }
                    output.push(']');
                }
                other => output.push_str(&serde_json::to_string(other).unwrap()),
            }
        }

        let mut output = String::new();
        write_value(value, &mut output);
        output
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
}
