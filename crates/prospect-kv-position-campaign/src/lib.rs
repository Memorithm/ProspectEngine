#![forbid(unsafe_code)]

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use prospect_kv_position_observed::{
    KvlabKvRealModelPositionError, KvlabKvRealModelPositionEvidenceV2,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const KVLAB_KV_POSITION_CAMPAIGN_SCHEMA_V1: &str =
    "kvlab.prospect-kv-real-model-position-campaign/v1";
pub const KVLAB_KV_POSITION_CAMPAIGN_RESULT_SCHEMA_V1: &str =
    "kvlab.prospect-kv-real-model-position-campaign-result/v1";
pub const KVLAB_KV_REAL_MODEL_POSITION_TRACE_SCHEMA_V1: &str =
    "kvlab.prospect-kv-real-model-position-trace/v1";
pub const KVLAB_KV_REAL_MODEL_POSITION_EVIDENCE_SCHEMA_V2: &str =
    "kvlab.prospect-kv-real-model-selection/v2";
pub const KVLAB_KV_POSITION_CAMPAIGN_REVISION: &str =
    "9fb6cae9f644daee7904fd14b50c3995c898fa6d";

#[derive(Clone, Copy, Debug)]
pub struct CampaignFilePayload<'a> {
    pub filename: &'a str,
    pub canonical_json: &'a str,
}

#[derive(Clone, Debug)]
pub struct VerifiedPositionCampaign {
    campaign_spec_sha256: String,
    trace_sha256: String,
    policies: Vec<String>,
    records: Vec<KvlabKvRealModelPositionEvidenceV2>,
}

#[derive(Debug)]
pub enum PositionCampaignVerificationError {
    Json(serde_json::Error),
    NonCanonicalManifest,
    NonCanonicalCampaign,
    UnsupportedManifestSchema,
    UnsupportedCampaignSchema,
    UnsupportedEvidenceSchema,
    InvalidSha256(&'static str),
    CampaignDigestMismatch,
    TraceDigestMismatch,
    EmptyRecords,
    RecordIndexMismatch { expected: usize, actual: usize },
    RecordFilenameMismatch { index: usize, filename: String },
    DuplicatePolicy(String),
    DuplicateFilename(String),
    MissingFile(String),
    UnexpectedFile(String),
    RecordDigestMismatch(String),
    Evidence(KvlabKvRealModelPositionError),
    EvidenceSchemaMismatch(String),
    EvidenceTraceMismatch(String),
    EvidenceContextMismatch { filename: String, field: &'static str },
    EvidencePolicyMismatch(String),
    EvidenceSelectionMismatch(String),
    CampaignRecordCountMismatch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestWire {
    schema: String,
    campaign_spec_sha256: String,
    trace_sha256: String,
    evidence_schema: String,
    records: Vec<RecordDescriptorWire>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordDescriptorWire {
    index: usize,
    policy: String,
    filename: String,
    sha256: String,
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
    selections: Vec<CampaignSelectionWire>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CampaignSelectionWire {
    policy: String,
    retained_positions: Vec<usize>,
}

impl VerifiedPositionCampaign {
    pub fn verify(
        manifest_json: &str,
        campaign_json: &str,
        files: &[CampaignFilePayload<'_>],
    ) -> Result<Self, PositionCampaignVerificationError> {
        let manifest_value: Value =
            serde_json::from_str(manifest_json).map_err(PositionCampaignVerificationError::Json)?;
        if canonical_json(&manifest_value).map_err(PositionCampaignVerificationError::Json)?
            != manifest_json
        {
            return Err(PositionCampaignVerificationError::NonCanonicalManifest);
        }
        let manifest: ManifestWire = serde_json::from_value(manifest_value)
            .map_err(PositionCampaignVerificationError::Json)?;
        if manifest.schema != KVLAB_KV_POSITION_CAMPAIGN_RESULT_SCHEMA_V1 {
            return Err(PositionCampaignVerificationError::UnsupportedManifestSchema);
        }
        if manifest.evidence_schema != KVLAB_KV_REAL_MODEL_POSITION_EVIDENCE_SCHEMA_V2 {
            return Err(PositionCampaignVerificationError::UnsupportedEvidenceSchema);
        }
        require_sha256("campaign_spec_sha256", &manifest.campaign_spec_sha256)?;
        require_sha256("trace_sha256", &manifest.trace_sha256)?;
        if manifest.records.is_empty() {
            return Err(PositionCampaignVerificationError::EmptyRecords);
        }

        let campaign_value: Value =
            serde_json::from_str(campaign_json).map_err(PositionCampaignVerificationError::Json)?;
        if canonical_json(&campaign_value).map_err(PositionCampaignVerificationError::Json)?
            != campaign_json
        {
            return Err(PositionCampaignVerificationError::NonCanonicalCampaign);
        }
        let campaign_digest = sha256_hex(campaign_json.as_bytes());
        if campaign_digest != manifest.campaign_spec_sha256 {
            return Err(PositionCampaignVerificationError::CampaignDigestMismatch);
        }
        let campaign: CampaignWire = serde_json::from_value(campaign_value)
            .map_err(PositionCampaignVerificationError::Json)?;
        if campaign.schema != KVLAB_KV_POSITION_CAMPAIGN_SCHEMA_V1 {
            return Err(PositionCampaignVerificationError::UnsupportedCampaignSchema);
        }
        if campaign.selections.len() != manifest.records.len() {
            return Err(PositionCampaignVerificationError::CampaignRecordCountMismatch);
        }

        let trace_value = json!({
            "schema": KVLAB_KV_REAL_MODEL_POSITION_TRACE_SCHEMA_V1,
            "model_input_token_ids": campaign.model_input_token_ids,
            "evaluation_token_ids": campaign.evaluation_token_ids,
        });
        let trace_json =
            canonical_json(&trace_value).map_err(PositionCampaignVerificationError::Json)?;
        if sha256_hex(trace_json.as_bytes()) != manifest.trace_sha256 {
            return Err(PositionCampaignVerificationError::TraceDigestMismatch);
        }

        let mut file_map = BTreeMap::new();
        for file in files {
            if file_map
                .insert(file.filename, file.canonical_json)
                .is_some()
            {
                return Err(PositionCampaignVerificationError::DuplicateFilename(
                    file.filename.to_owned(),
                ));
            }
        }

        let mut expected_filenames = BTreeSet::new();
        let mut seen_policies = BTreeSet::new();
        let mut policies = Vec::with_capacity(manifest.records.len());
        let mut records = Vec::with_capacity(manifest.records.len());

        for (expected_index, (descriptor, selection)) in manifest
            .records
            .iter()
            .zip(&campaign.selections)
            .enumerate()
        {
            if descriptor.index != expected_index {
                return Err(PositionCampaignVerificationError::RecordIndexMismatch {
                    expected: expected_index,
                    actual: descriptor.index,
                });
            }
            let expected_filename = format!("selection-{expected_index:03}.json");
            if descriptor.filename != expected_filename {
                return Err(PositionCampaignVerificationError::RecordFilenameMismatch {
                    index: expected_index,
                    filename: descriptor.filename.clone(),
                });
            }
            if !expected_filenames.insert(descriptor.filename.clone()) {
                return Err(PositionCampaignVerificationError::DuplicateFilename(
                    descriptor.filename.clone(),
                ));
            }
            if !seen_policies.insert(descriptor.policy.clone()) {
                return Err(PositionCampaignVerificationError::DuplicatePolicy(
                    descriptor.policy.clone(),
                ));
            }
            if descriptor.policy != selection.policy {
                return Err(PositionCampaignVerificationError::EvidencePolicyMismatch(
                    descriptor.filename.clone(),
                ));
            }
            require_sha256("record.sha256", &descriptor.sha256)?;

            let Some(payload) = file_map.get(descriptor.filename.as_str()).copied() else {
                return Err(PositionCampaignVerificationError::MissingFile(
                    descriptor.filename.clone(),
                ));
            };
            if sha256_hex(payload.as_bytes()) != descriptor.sha256 {
                return Err(PositionCampaignVerificationError::RecordDigestMismatch(
                    descriptor.filename.clone(),
                ));
            }

            let raw_record: Value =
                serde_json::from_str(payload).map_err(PositionCampaignVerificationError::Json)?;
            verify_raw_record(
                &raw_record,
                &campaign,
                selection,
                &manifest.trace_sha256,
                descriptor,
            )?;
            let record = KvlabKvRealModelPositionEvidenceV2::from_canonical_json(payload)
                .map_err(PositionCampaignVerificationError::Evidence)?;
            if record.selection().outcome().policy() != descriptor.policy {
                return Err(PositionCampaignVerificationError::EvidencePolicyMismatch(
                    descriptor.filename.clone(),
                ));
            }
            if record.selection().outcome().retained_positions()
                != selection.retained_positions.as_slice()
            {
                return Err(PositionCampaignVerificationError::EvidenceSelectionMismatch(
                    descriptor.filename.clone(),
                ));
            }
            policies.push(descriptor.policy.clone());
            records.push(record);
        }

        for filename in file_map.keys() {
            if !expected_filenames.contains(*filename) {
                return Err(PositionCampaignVerificationError::UnexpectedFile(
                    (*filename).to_owned(),
                ));
            }
        }

        Ok(Self {
            campaign_spec_sha256: manifest.campaign_spec_sha256,
            trace_sha256: manifest.trace_sha256,
            policies,
            records,
        })
    }

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
    pub fn records(&self) -> &[KvlabKvRealModelPositionEvidenceV2] {
        &self.records
    }
}

fn verify_raw_record(
    raw: &Value,
    campaign: &CampaignWire,
    selection: &CampaignSelectionWire,
    expected_trace_sha256: &str,
    descriptor: &RecordDescriptorWire,
) -> Result<(), PositionCampaignVerificationError> {
    let Some(object) = raw.as_object() else {
        return Err(PositionCampaignVerificationError::EvidenceSchemaMismatch(
            descriptor.filename.clone(),
        ));
    };
    if object.get("schema")
        != Some(&Value::String(
            KVLAB_KV_REAL_MODEL_POSITION_EVIDENCE_SCHEMA_V2.to_owned(),
        ))
    {
        return Err(PositionCampaignVerificationError::EvidenceSchemaMismatch(
            descriptor.filename.clone(),
        ));
    }
    if object.get("trace_sha256")
        != Some(&Value::String(expected_trace_sha256.to_owned()))
    {
        return Err(PositionCampaignVerificationError::EvidenceTraceMismatch(
            descriptor.filename.clone(),
        ));
    }

    let expected_context = [
        ("experiment_id", Value::String(campaign.experiment_id.clone())),
        (
            "run_repository_revision",
            Value::String(campaign.run_repository_revision.clone()),
        ),
        ("model_id", Value::String(campaign.model_id.clone())),
        (
            "model_revision",
            Value::String(campaign.model_revision.clone()),
        ),
        (
            "tokenizer_revision",
            Value::String(campaign.tokenizer_revision.clone()),
        ),
        (
            "runtime_backend",
            Value::String(campaign.runtime_backend.clone()),
        ),
        (
            "runtime_revision",
            Value::String(campaign.runtime_revision.clone()),
        ),
        (
            "evaluation_id",
            Value::String(campaign.evaluation_id.clone()),
        ),
        ("seed", Value::from(campaign.seed)),
    ];
    for (field, expected) in expected_context {
        if object.get(field) != Some(&expected) {
            return Err(PositionCampaignVerificationError::EvidenceContextMismatch {
                filename: descriptor.filename.clone(),
                field,
            });
        }
    }

    let Some(selection_object) = object.get("selection").and_then(Value::as_object) else {
        return Err(PositionCampaignVerificationError::EvidenceSelectionMismatch(
            descriptor.filename.clone(),
        ));
    };
    if selection_object.get("policy") != Some(&Value::String(selection.policy.clone()))
        || selection_object.get("retained_positions")
            != Some(&json!(selection.retained_positions))
        || selection_object.get("bytes_per_token") != Some(&Value::from(campaign.bytes_per_token))
        || selection_object.get("input_token_ids") != Some(&json!(campaign.model_input_token_ids))
    {
        return Err(PositionCampaignVerificationError::EvidenceSelectionMismatch(
            descriptor.filename.clone(),
        ));
    }
    Ok(())
}

fn require_sha256(
    field: &'static str,
    value: &str,
) -> Result<(), PositionCampaignVerificationError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PositionCampaignVerificationError::InvalidSha256(field));
    }
    Ok(())
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

impl fmt::Display for PositionCampaignVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid campaign JSON: {error}"),
            Self::NonCanonicalManifest => formatter.write_str("campaign manifest is not canonical"),
            Self::NonCanonicalCampaign => formatter.write_str("campaign specification is not canonical"),
            Self::UnsupportedManifestSchema => formatter.write_str("unsupported campaign manifest schema"),
            Self::UnsupportedCampaignSchema => formatter.write_str("unsupported campaign specification schema"),
            Self::UnsupportedEvidenceSchema => formatter.write_str("unsupported campaign evidence schema"),
            Self::InvalidSha256(field) => write!(formatter, "{field} must be a lowercase SHA-256 digest"),
            Self::CampaignDigestMismatch => formatter.write_str("campaign specification SHA-256 mismatch"),
            Self::TraceDigestMismatch => formatter.write_str("campaign trace SHA-256 mismatch"),
            Self::EmptyRecords => formatter.write_str("campaign manifest must contain records"),
            Self::RecordIndexMismatch { expected, actual } => write!(formatter, "campaign record index mismatch: expected {expected}, got {actual}"),
            Self::RecordFilenameMismatch { index, filename } => write!(formatter, "campaign record {index} has non-canonical filename {filename}"),
            Self::DuplicatePolicy(policy) => write!(formatter, "duplicate campaign policy {policy}"),
            Self::DuplicateFilename(filename) => write!(formatter, "duplicate campaign filename {filename}"),
            Self::MissingFile(filename) => write!(formatter, "missing campaign evidence file {filename}"),
            Self::UnexpectedFile(filename) => write!(formatter, "unexpected campaign evidence file {filename}"),
            Self::RecordDigestMismatch(filename) => write!(formatter, "campaign evidence SHA-256 mismatch for {filename}"),
            Self::Evidence(error) => write!(formatter, "invalid campaign evidence: {error}"),
            Self::EvidenceSchemaMismatch(filename) => write!(formatter, "campaign evidence schema mismatch for {filename}"),
            Self::EvidenceTraceMismatch(filename) => write!(formatter, "campaign evidence trace mismatch for {filename}"),
            Self::EvidenceContextMismatch { filename, field } => write!(formatter, "campaign evidence context mismatch for {filename}: {field}"),
            Self::EvidencePolicyMismatch(filename) => write!(formatter, "campaign evidence policy mismatch for {filename}"),
            Self::EvidenceSelectionMismatch(filename) => write!(formatter, "campaign evidence selection mismatch for {filename}"),
            Self::CampaignRecordCountMismatch => formatter.write_str("campaign selection/record count mismatch"),
        }
    }
}

impl std::error::Error for PositionCampaignVerificationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign() -> String {
        canonical_json(&json!({
            "schema":KVLAB_KV_POSITION_CAMPAIGN_SCHEMA_V1,
            "experiment_id":"campaign-001",
            "run_repository_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "model_id":"example/model",
            "model_revision":"model-r1",
            "tokenizer_revision":"tok-r1",
            "runtime_backend":"nnis-kvlab-v4",
            "runtime_revision":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "evaluation_id":"teacher-forced-001",
            "seed":7,
            "bytes_per_token":64,
            "model_input_token_ids":[7,11,7,7,19],
            "evaluation_token_ids":[23,7],
            "selections":[
                {"policy":"lru","retained_positions":[0,2,4]},
                {"policy":"magnitude","retained_positions":[1,2,4]}
            ]
        }))
        .unwrap()
    }

    fn evidence(policy: &str, retained: &[usize], candidate_hash: char) -> String {
        let retained_set = retained.iter().copied().collect::<BTreeSet<_>>();
        let evicted = (0..5)
            .filter(|position| !retained_set.contains(position))
            .collect::<Vec<_>>();
        let retained_bytes = u64::try_from(retained.len()).unwrap() * 64;
        canonical_json(&json!({
            "schema":KVLAB_KV_REAL_MODEL_POSITION_EVIDENCE_SCHEMA_V2,
            "experiment_id":"campaign-001",
            "run_repository_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "model_id":"example/model",
            "model_revision":"model-r1",
            "tokenizer_revision":"tok-r1",
            "runtime_backend":"nnis-kvlab-v4",
            "runtime_revision":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "evaluation_id":"teacher-forced-001",
            "trace_sha256":trace_sha(),
            "seed":7,
            "selection":{
                "schema":"kvlab.prospect-kv-selection/v2",
                "policy":policy,
                "input_token_ids":[7,11,7,7,19],
                "bytes_per_token":64,
                "retained_positions":retained,
                "evicted_positions":evicted,
                "logical_input_bytes":320,
                "logical_retained_bytes":retained_bytes,
                "logical_evicted_bytes":320-retained_bytes
            },
            "baseline_output_sha256":"2".repeat(64),
            "candidate_output_sha256":candidate_hash.to_string().repeat(64),
            "baseline_logical_kv_bytes":320,
            "candidate_logical_kv_bytes":retained_bytes,
            "metrics":[{
                "name":"token_accuracy",
                "kind":"quality",
                "unit":"ratio",
                "preference":"higher_is_better",
                "baseline_value":1.0,
                "candidate_value":0.75,
                "delta":-0.25
            }]
        }))
        .unwrap()
    }

    fn trace_sha() -> String {
        let value = json!({
            "schema":KVLAB_KV_REAL_MODEL_POSITION_TRACE_SCHEMA_V1,
            "model_input_token_ids":[7,11,7,7,19],
            "evaluation_token_ids":[23,7]
        });
        sha256_hex(canonical_json(&value).unwrap().as_bytes())
    }

    fn fixture() -> (String, String, String, String) {
        let campaign = campaign();
        let left = evidence("lru", &[0, 2, 4], '3');
        let right = evidence("magnitude", &[1, 2, 4], '4');
        let manifest = canonical_json(&json!({
            "schema":KVLAB_KV_POSITION_CAMPAIGN_RESULT_SCHEMA_V1,
            "campaign_spec_sha256":sha256_hex(campaign.as_bytes()),
            "trace_sha256":trace_sha(),
            "evidence_schema":KVLAB_KV_REAL_MODEL_POSITION_EVIDENCE_SCHEMA_V2,
            "records":[
                {"index":0,"policy":"lru","filename":"selection-000.json","sha256":sha256_hex(left.as_bytes())},
                {"index":1,"policy":"magnitude","filename":"selection-001.json","sha256":sha256_hex(right.as_bytes())}
            ]
        }))
        .unwrap();
        (manifest, campaign, left, right)
    }

    #[test]
    fn verifies_manifest_campaign_trace_and_observed_records() {
        let (manifest, campaign, left, right) = fixture();
        let verified = VerifiedPositionCampaign::verify(
            &manifest,
            &campaign,
            &[
                CampaignFilePayload {
                    filename: "selection-000.json",
                    canonical_json: &left,
                },
                CampaignFilePayload {
                    filename: "selection-001.json",
                    canonical_json: &right,
                },
            ],
        )
        .unwrap();
        assert_eq!(verified.policies(), &["lru", "magnitude"]);
        assert_eq!(verified.records().len(), 2);
        assert_eq!(
            verified.records()[0].selection().outcome().retained_token_ids(),
            &[7, 7, 19]
        );
    }

    #[test]
    fn rejects_record_digest_or_extra_file() {
        let (manifest, campaign, left, right) = fixture();
        let mut tampered: Value = serde_json::from_str(&left).unwrap();
        tampered["candidate_output_sha256"] = json!("9".repeat(64));
        let tampered = canonical_json(&tampered).unwrap();
        assert!(matches!(
            VerifiedPositionCampaign::verify(
                &manifest,
                &campaign,
                &[
                    CampaignFilePayload {
                        filename: "selection-000.json",
                        canonical_json: &tampered,
                    },
                    CampaignFilePayload {
                        filename: "selection-001.json",
                        canonical_json: &right,
                    },
                ],
            ),
            Err(PositionCampaignVerificationError::RecordDigestMismatch(_))
        ));

        assert!(matches!(
            VerifiedPositionCampaign::verify(
                &manifest,
                &campaign,
                &[
                    CampaignFilePayload {
                        filename: "selection-000.json",
                        canonical_json: &left,
                    },
                    CampaignFilePayload {
                        filename: "selection-001.json",
                        canonical_json: &right,
                    },
                    CampaignFilePayload {
                        filename: "extra.json",
                        canonical_json: &right,
                    },
                ],
            ),
            Err(PositionCampaignVerificationError::UnexpectedFile(_))
        ));
    }

    #[test]
    fn rejects_campaign_context_or_selection_drift_even_with_rehashed_record() {
        let (manifest, campaign, left, right) = fixture();
        let mut record: Value = serde_json::from_str(&left).unwrap();
        record["model_revision"] = json!("other-model-r1");
        let drifted = canonical_json(&record).unwrap();

        let mut manifest_value: Value = serde_json::from_str(&manifest).unwrap();
        manifest_value["records"][0]["sha256"] = json!(sha256_hex(drifted.as_bytes()));
        let rehashed_manifest = canonical_json(&manifest_value).unwrap();
        assert!(matches!(
            VerifiedPositionCampaign::verify(
                &rehashed_manifest,
                &campaign,
                &[
                    CampaignFilePayload {
                        filename: "selection-000.json",
                        canonical_json: &drifted,
                    },
                    CampaignFilePayload {
                        filename: "selection-001.json",
                        canonical_json: &right,
                    },
                ],
            ),
            Err(PositionCampaignVerificationError::EvidenceContextMismatch { .. })
        ));

        let mut record: Value = serde_json::from_str(&left).unwrap();
        record["selection"]["retained_positions"] = json!([0, 3, 4]);
        record["selection"]["evicted_positions"] = json!([1, 2]);
        let drifted = canonical_json(&record).unwrap();
        let mut manifest_value: Value = serde_json::from_str(&manifest).unwrap();
        manifest_value["records"][0]["sha256"] = json!(sha256_hex(drifted.as_bytes()));
        let rehashed_manifest = canonical_json(&manifest_value).unwrap();
        assert!(matches!(
            VerifiedPositionCampaign::verify(
                &rehashed_manifest,
                &campaign,
                &[
                    CampaignFilePayload {
                        filename: "selection-000.json",
                        canonical_json: &drifted,
                    },
                    CampaignFilePayload {
                        filename: "selection-001.json",
                        canonical_json: &right,
                    },
                ],
            ),
            Err(PositionCampaignVerificationError::EvidenceSelectionMismatch(_))
        ));
    }

    #[test]
    fn rejects_manifest_trace_or_campaign_digest_drift() {
        let (manifest, campaign, left, right) = fixture();
        let files = [
            CampaignFilePayload {
                filename: "selection-000.json",
                canonical_json: &left,
            },
            CampaignFilePayload {
                filename: "selection-001.json",
                canonical_json: &right,
            },
        ];

        let mut value: Value = serde_json::from_str(&manifest).unwrap();
        value["trace_sha256"] = json!("0".repeat(64));
        assert!(matches!(
            VerifiedPositionCampaign::verify(
                &canonical_json(&value).unwrap(),
                &campaign,
                &files,
            ),
            Err(PositionCampaignVerificationError::TraceDigestMismatch)
        ));

        let mut value: Value = serde_json::from_str(&manifest).unwrap();
        value["campaign_spec_sha256"] = json!("0".repeat(64));
        assert!(matches!(
            VerifiedPositionCampaign::verify(
                &canonical_json(&value).unwrap(),
                &campaign,
                &files,
            ),
            Err(PositionCampaignVerificationError::CampaignDigestMismatch)
        ));
    }
}
