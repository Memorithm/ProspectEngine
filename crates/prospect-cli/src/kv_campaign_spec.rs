use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const CAMPAIGN_SCHEMA_V1: &str = "kvlab.prospect-kv-real-model-position-campaign/v1";
const TRACE_SCHEMA_V1: &str = "kvlab.prospect-kv-real-model-position-trace/v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CampaignPolicySummary {
    pub policy: String,
    pub retained_positions: Vec<usize>,
    pub retained_tokens: usize,
    pub evicted_tokens: usize,
    pub logical_retained_bytes: u64,
    pub logical_evicted_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CampaignSpecSummary {
    pub campaign_spec_sha256: String,
    pub trace_sha256: String,
    pub experiment_id: String,
    pub run_repository_revision: String,
    pub model_id: String,
    pub model_revision: String,
    pub tokenizer_revision: String,
    pub runtime_backend: String,
    pub runtime_revision: String,
    pub evaluation_id: String,
    pub seed: u64,
    pub bytes_per_token: u64,
    pub model_input_tokens: usize,
    pub evaluation_tokens: usize,
    pub logical_input_bytes: u64,
    pub policies: Vec<CampaignPolicySummary>,
}

#[derive(Debug)]
pub enum CampaignSpecError {
    Io(std::io::Error),
    Json(serde_json::Error),
    NonCanonical,
    UnsupportedSchema,
    InvalidField(&'static str),
    DuplicatePolicy(String),
    SelectionDuplicatesBaseline(String),
    PositionOutOfRange(String),
    PositionsNotStrictlyIncreasing(String),
    ByteAccountingOverflow,
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

pub fn verify_kv_campaign_spec_file(
    path: impl AsRef<Path>,
) -> Result<CampaignSpecSummary, CampaignSpecError> {
    let payload = fs::read_to_string(path).map_err(CampaignSpecError::Io)?;
    verify_kv_campaign_spec(&payload)
}

pub fn verify_kv_campaign_spec(payload: &str) -> Result<CampaignSpecSummary, CampaignSpecError> {
    let value: Value = serde_json::from_str(payload).map_err(CampaignSpecError::Json)?;
    if canonical_json(&value).map_err(CampaignSpecError::Json)? != payload {
        return Err(CampaignSpecError::NonCanonical);
    }
    let campaign: CampaignWire =
        serde_json::from_value(value).map_err(CampaignSpecError::Json)?;
    validate_campaign(&campaign)?;

    let campaign_spec_sha256 = sha256_hex(payload.as_bytes());
    let trace = serde_json::json!({
        "schema": TRACE_SCHEMA_V1,
        "model_input_token_ids": &campaign.model_input_token_ids,
        "evaluation_token_ids": &campaign.evaluation_token_ids,
    });
    let trace_json = canonical_json(&trace).map_err(CampaignSpecError::Json)?;
    let trace_sha256 = sha256_hex(trace_json.as_bytes());

    let input_tokens = campaign.model_input_token_ids.len();
    let logical_input_bytes = checked_bytes(input_tokens, campaign.bytes_per_token)?;
    let policies = campaign
        .selections
        .into_iter()
        .map(|selection| {
            let retained_tokens = selection.retained_positions.len();
            let logical_retained_bytes = checked_bytes(retained_tokens, campaign.bytes_per_token)?;
            Ok(CampaignPolicySummary {
                policy: selection.policy,
                retained_positions: selection.retained_positions,
                retained_tokens,
                evicted_tokens: input_tokens - retained_tokens,
                logical_retained_bytes,
                logical_evicted_bytes: logical_input_bytes - logical_retained_bytes,
            })
        })
        .collect::<Result<Vec<_>, CampaignSpecError>>()?;

    Ok(CampaignSpecSummary {
        campaign_spec_sha256,
        trace_sha256,
        experiment_id: campaign.experiment_id,
        run_repository_revision: campaign.run_repository_revision,
        model_id: campaign.model_id,
        model_revision: campaign.model_revision,
        tokenizer_revision: campaign.tokenizer_revision,
        runtime_backend: campaign.runtime_backend,
        runtime_revision: campaign.runtime_revision,
        evaluation_id: campaign.evaluation_id,
        seed: campaign.seed,
        bytes_per_token: campaign.bytes_per_token,
        model_input_tokens: input_tokens,
        evaluation_tokens: campaign.evaluation_token_ids.len(),
        logical_input_bytes,
        policies,
    })
}

fn validate_campaign(campaign: &CampaignWire) -> Result<(), CampaignSpecError> {
    if campaign.schema != CAMPAIGN_SCHEMA_V1 {
        return Err(CampaignSpecError::UnsupportedSchema);
    }
    for (field, value) in [
        ("experiment_id", campaign.experiment_id.as_str()),
        ("model_id", campaign.model_id.as_str()),
        ("model_revision", campaign.model_revision.as_str()),
        ("tokenizer_revision", campaign.tokenizer_revision.as_str()),
        ("runtime_backend", campaign.runtime_backend.as_str()),
        ("runtime_revision", campaign.runtime_revision.as_str()),
        ("evaluation_id", campaign.evaluation_id.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(CampaignSpecError::InvalidField(field));
        }
    }
    if !is_lower_hex(&campaign.run_repository_revision, 40) {
        return Err(CampaignSpecError::InvalidField("run_repository_revision"));
    }
    if campaign.bytes_per_token == 0 {
        return Err(CampaignSpecError::InvalidField("bytes_per_token"));
    }
    if campaign.model_input_token_ids.is_empty() {
        return Err(CampaignSpecError::InvalidField("model_input_token_ids"));
    }
    if campaign.evaluation_token_ids.is_empty() {
        return Err(CampaignSpecError::InvalidField("evaluation_token_ids"));
    }
    if campaign.selections.is_empty() {
        return Err(CampaignSpecError::InvalidField("selections"));
    }

    let input_len = campaign.model_input_token_ids.len();
    let mut seen_policies = BTreeSet::new();
    for selection in &campaign.selections {
        if selection.policy.trim().is_empty() {
            return Err(CampaignSpecError::InvalidField("selection.policy"));
        }
        if !seen_policies.insert(selection.policy.as_str()) {
            return Err(CampaignSpecError::DuplicatePolicy(selection.policy.clone()));
        }
        let mut previous = None;
        for &position in &selection.retained_positions {
            if position >= input_len {
                return Err(CampaignSpecError::PositionOutOfRange(
                    selection.policy.clone(),
                ));
            }
            if previous.is_some_and(|last| position <= last) {
                return Err(CampaignSpecError::PositionsNotStrictlyIncreasing(
                    selection.policy.clone(),
                ));
            }
            previous = Some(position);
        }
        if selection.retained_positions.len() == input_len
            && selection
                .retained_positions
                .iter()
                .copied()
                .eq(0..input_len)
        {
            return Err(CampaignSpecError::SelectionDuplicatesBaseline(
                selection.policy.clone(),
            ));
        }
    }
    let _ = checked_bytes(input_len, campaign.bytes_per_token)?;
    Ok(())
}

fn checked_bytes(count: usize, bytes_per_token: u64) -> Result<u64, CampaignSpecError> {
    let count = u64::try_from(count).map_err(|_| CampaignSpecError::ByteAccountingOverflow)?;
    count
        .checked_mul(bytes_per_token)
        .ok_or(CampaignSpecError::ByteAccountingOverflow)
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

impl fmt::Display for CampaignSpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "failed to read campaign specification: {error}"),
            Self::Json(error) => write!(formatter, "invalid campaign specification JSON: {error}"),
            Self::NonCanonical => formatter.write_str("campaign specification is not canonical JSON"),
            Self::UnsupportedSchema => formatter.write_str("unsupported campaign specification schema"),
            Self::InvalidField(field) => write!(formatter, "invalid campaign specification field {field}"),
            Self::DuplicatePolicy(policy) => write!(formatter, "duplicate campaign policy {policy}"),
            Self::SelectionDuplicatesBaseline(policy) => {
                write!(formatter, "campaign policy {policy} duplicates the full-cache baseline")
            }
            Self::PositionOutOfRange(policy) => {
                write!(formatter, "campaign policy {policy} contains an out-of-range retained position")
            }
            Self::PositionsNotStrictlyIncreasing(policy) => write!(
                formatter,
                "campaign policy {policy} retained positions must be strictly increasing and unique"
            ),
            Self::ByteAccountingOverflow => formatter.write_str("campaign logical byte accounting overflow"),
        }
    }
}

impl std::error::Error for CampaignSpecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMOLLM2_RETAIN_07: &str = "{\"bytes_per_token\":46080,\"evaluation_id\":\"nnis-r1-gravity-is-tail8\",\"evaluation_token_ids\":[253,19284,1248,338,21837,260,2591,30],\"experiment_id\":\"smollm2-r1-position-retain-07-of-27\",\"model_id\":\"HuggingFaceTB/SmolLM2-135M\",\"model_input_token_ids\":[22007,6463,314,260,3075,338,6650,260,2591,284,260,8872,1592,30,198,198,504,8872,314,253,8304,282,260,2591,30,657,314],\"model_revision\":\"93efa2f097d58c2a74874c7e644dbc9b0cee75a2\",\"run_repository_revision\":\"404577ce939093767dc75d2d67de2fe3c16fa4dc\",\"runtime_backend\":\"nnis-kvlab-v4\",\"runtime_revision\":\"58e7db8e1c4b471a7fe82a4beba11904240c4e89\",\"schema\":\"kvlab.prospect-kv-real-model-position-campaign/v1\",\"seed\":7,\"selections\":[{\"policy\":\"lru\",\"retained_positions\":[20,21,22,23,24,25,26]},{\"policy\":\"random_seeded\",\"retained_positions\":[1,2,4,10,12,17,20]}],\"tokenizer_revision\":\"93efa2f097d58c2a74874c7e644dbc9b0cee75a2\"}";

    #[test]
    fn verifies_merged_smollm2_preregistration() {
        let summary = verify_kv_campaign_spec(SMOLLM2_RETAIN_07).unwrap();
        assert_eq!(summary.model_input_tokens, 27);
        assert_eq!(summary.evaluation_tokens, 8);
        assert_eq!(summary.logical_input_bytes, 1_244_160);
        assert_eq!(summary.policies.len(), 2);
        assert_eq!(summary.policies[0].policy, "lru");
        assert_eq!(summary.policies[0].retained_tokens, 7);
        assert_eq!(summary.policies[0].logical_retained_bytes, 322_560);
        assert_eq!(summary.policies[0].logical_evicted_bytes, 921_600);
        assert_eq!(summary.policies[1].policy, "random_seeded");
        assert_eq!(summary.policies[1].logical_retained_bytes, 322_560);
        assert_eq!(summary.run_repository_revision, "404577ce939093767dc75d2d67de2fe3c16fa4dc");
        assert_eq!(summary.runtime_revision, "58e7db8e1c4b471a7fe82a4beba11904240c4e89");
    }

    #[test]
    fn rejects_noncanonical_or_unknown_fields() {
        let pretty = serde_json::to_string_pretty(
            &serde_json::from_str::<Value>(SMOLLM2_RETAIN_07).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            verify_kv_campaign_spec(&pretty),
            Err(CampaignSpecError::NonCanonical)
        ));

        let mut value: Value = serde_json::from_str(SMOLLM2_RETAIN_07).unwrap();
        value["unexpected"] = Value::Bool(true);
        let payload = canonical_json(&value).unwrap();
        assert!(matches!(
            verify_kv_campaign_spec(&payload),
            Err(CampaignSpecError::Json(_))
        ));
    }

    #[test]
    fn rejects_invalid_git_revision_duplicate_policy_and_bad_positions() {
        let mut value: Value = serde_json::from_str(SMOLLM2_RETAIN_07).unwrap();
        value["run_repository_revision"] = Value::String("ABC".to_owned());
        assert!(matches!(
            verify_kv_campaign_spec(&canonical_json(&value).unwrap()),
            Err(CampaignSpecError::InvalidField("run_repository_revision"))
        ));

        let mut value: Value = serde_json::from_str(SMOLLM2_RETAIN_07).unwrap();
        value["selections"][1]["policy"] = Value::String("lru".to_owned());
        assert!(matches!(
            verify_kv_campaign_spec(&canonical_json(&value).unwrap()),
            Err(CampaignSpecError::DuplicatePolicy(policy)) if policy == "lru"
        ));

        let mut value: Value = serde_json::from_str(SMOLLM2_RETAIN_07).unwrap();
        value["selections"][0]["retained_positions"] = serde_json::json!([20, 20, 26]);
        assert!(matches!(
            verify_kv_campaign_spec(&canonical_json(&value).unwrap()),
            Err(CampaignSpecError::PositionsNotStrictlyIncreasing(_))
        ));

        let mut value: Value = serde_json::from_str(SMOLLM2_RETAIN_07).unwrap();
        value["selections"][0]["retained_positions"] = serde_json::json!([27]);
        assert!(matches!(
            verify_kv_campaign_spec(&canonical_json(&value).unwrap()),
            Err(CampaignSpecError::PositionOutOfRange(_))
        ));
    }

    #[test]
    fn rejects_candidate_that_duplicates_full_cache_baseline() {
        let mut value: Value = serde_json::from_str(SMOLLM2_RETAIN_07).unwrap();
        value["selections"][0]["retained_positions"] =
            Value::Array((0..27).map(Value::from).collect());
        assert!(matches!(
            verify_kv_campaign_spec(&canonical_json(&value).unwrap()),
            Err(CampaignSpecError::SelectionDuplicatesBaseline(policy)) if policy == "lru"
        ));
    }
}
