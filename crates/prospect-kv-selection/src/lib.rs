#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;

pub const KVLAB_KV_SELECTION_HANDOFF_SCHEMA_V1: &str = "kvlab.prospect-kv-selection/v1";
pub const KVLAB_KV_SELECTION_HANDOFF_REVISION: &str =
    "0e7274bf565d9079943845ea4eb0699a525b1db1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvSelectionState {
    token_ids: Vec<u64>,
    bytes_per_token: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvSelectionOutcome {
    policy: String,
    retained_token_ids: Vec<u64>,
    evicted_token_ids: Vec<u64>,
    logical_input_bytes: u64,
    logical_retained_bytes: u64,
    logical_evicted_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvlabKvSelectionHandoffV1 {
    state: KvSelectionState,
    outcome: KvSelectionOutcome,
}

#[derive(Debug)]
pub enum KvSelectionContractError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    EmptyPolicy,
    EmptyInput,
    ZeroBytesPerToken,
    DuplicateTokenId { field: &'static str, token_id: u64 },
    UnknownTokenId { field: &'static str, token_id: u64 },
    OverlappingPartition { token_id: u64 },
    IncompletePartition { token_id: u64 },
    OrderMismatch(&'static str),
    LogicalByteOverflow,
    LogicalAccountingMismatch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionWire {
    schema: String,
    policy: String,
    input_token_ids: Vec<u64>,
    bytes_per_token: u64,
    retained_token_ids: Vec<u64>,
    evicted_token_ids: Vec<u64>,
    logical_input_bytes: u64,
    logical_retained_bytes: u64,
    logical_evicted_bytes: u64,
}

impl KvSelectionState {
    fn new(token_ids: Vec<u64>, bytes_per_token: u64) -> Result<Self, KvSelectionContractError> {
        if token_ids.is_empty() {
            return Err(KvSelectionContractError::EmptyInput);
        }
        if bytes_per_token == 0 {
            return Err(KvSelectionContractError::ZeroBytesPerToken);
        }
        validate_unique("input_token_ids", &token_ids)?;
        Ok(Self {
            token_ids,
            bytes_per_token,
        })
    }

    #[must_use]
    pub fn token_ids(&self) -> &[u64] {
        &self.token_ids
    }

    #[must_use]
    pub const fn bytes_per_token(&self) -> u64 {
        self.bytes_per_token
    }
}

impl KvSelectionOutcome {
    #[must_use]
    pub fn policy(&self) -> &str {
        &self.policy
    }

    #[must_use]
    pub fn retained_token_ids(&self) -> &[u64] {
        &self.retained_token_ids
    }

    #[must_use]
    pub fn evicted_token_ids(&self) -> &[u64] {
        &self.evicted_token_ids
    }

    #[must_use]
    pub const fn logical_input_bytes(&self) -> u64 {
        self.logical_input_bytes
    }

    #[must_use]
    pub const fn logical_retained_bytes(&self) -> u64 {
        self.logical_retained_bytes
    }

    #[must_use]
    pub const fn logical_evicted_bytes(&self) -> u64 {
        self.logical_evicted_bytes
    }
}

impl KvlabKvSelectionHandoffV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvSelectionContractError> {
        let value: Value = serde_json::from_str(json).map_err(KvSelectionContractError::Json)?;
        let canonical = canonical_json(&value).map_err(KvSelectionContractError::Json)?;
        if canonical != json {
            return Err(KvSelectionContractError::NonCanonicalJson);
        }

        let wire: SelectionWire =
            serde_json::from_value(value).map_err(KvSelectionContractError::Json)?;
        if wire.schema != KVLAB_KV_SELECTION_HANDOFF_SCHEMA_V1 {
            return Err(KvSelectionContractError::UnsupportedSchema);
        }
        if wire.policy.trim().is_empty() {
            return Err(KvSelectionContractError::EmptyPolicy);
        }

        let state = KvSelectionState::new(wire.input_token_ids, wire.bytes_per_token)?;
        validate_unique("retained_token_ids", &wire.retained_token_ids)?;
        validate_unique("evicted_token_ids", &wire.evicted_token_ids)?;

        let input = state.token_ids.iter().copied().collect::<BTreeSet<_>>();
        let retained = wire.retained_token_ids.iter().copied().collect::<BTreeSet<_>>();
        let evicted = wire.evicted_token_ids.iter().copied().collect::<BTreeSet<_>>();

        for token_id in retained.iter().copied() {
            if !input.contains(&token_id) {
                return Err(KvSelectionContractError::UnknownTokenId {
                    field: "retained_token_ids",
                    token_id,
                });
            }
            if evicted.contains(&token_id) {
                return Err(KvSelectionContractError::OverlappingPartition { token_id });
            }
        }
        for token_id in evicted.iter().copied() {
            if !input.contains(&token_id) {
                return Err(KvSelectionContractError::UnknownTokenId {
                    field: "evicted_token_ids",
                    token_id,
                });
            }
        }
        for token_id in state.token_ids.iter().copied() {
            if !retained.contains(&token_id) && !evicted.contains(&token_id) {
                return Err(KvSelectionContractError::IncompletePartition { token_id });
            }
        }

        let expected_retained = state
            .token_ids
            .iter()
            .copied()
            .filter(|token_id| retained.contains(token_id))
            .collect::<Vec<_>>();
        let expected_evicted = state
            .token_ids
            .iter()
            .copied()
            .filter(|token_id| evicted.contains(token_id))
            .collect::<Vec<_>>();
        if wire.retained_token_ids != expected_retained {
            return Err(KvSelectionContractError::OrderMismatch("retained_token_ids"));
        }
        if wire.evicted_token_ids != expected_evicted {
            return Err(KvSelectionContractError::OrderMismatch("evicted_token_ids"));
        }

        let logical_input_bytes = checked_bytes(state.token_ids.len(), state.bytes_per_token)?;
        let logical_retained_bytes =
            checked_bytes(wire.retained_token_ids.len(), state.bytes_per_token)?;
        let logical_evicted_bytes = logical_input_bytes - logical_retained_bytes;
        if wire.logical_input_bytes != logical_input_bytes
            || wire.logical_retained_bytes != logical_retained_bytes
            || wire.logical_evicted_bytes != logical_evicted_bytes
        {
            return Err(KvSelectionContractError::LogicalAccountingMismatch);
        }

        Ok(Self {
            state,
            outcome: KvSelectionOutcome {
                policy: wire.policy,
                retained_token_ids: wire.retained_token_ids,
                evicted_token_ids: wire.evicted_token_ids,
                logical_input_bytes,
                logical_retained_bytes,
                logical_evicted_bytes,
            },
        })
    }

    #[must_use]
    pub const fn state(&self) -> &KvSelectionState {
        &self.state
    }

    #[must_use]
    pub const fn outcome(&self) -> &KvSelectionOutcome {
        &self.outcome
    }
}

fn checked_bytes(count: usize, bytes_per_token: u64) -> Result<u64, KvSelectionContractError> {
    u64::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(bytes_per_token))
        .ok_or(KvSelectionContractError::LogicalByteOverflow)
}

fn validate_unique(
    field: &'static str,
    token_ids: &[u64],
) -> Result<(), KvSelectionContractError> {
    let mut seen = BTreeSet::new();
    for token_id in token_ids.iter().copied() {
        if !seen.insert(token_id) {
            return Err(KvSelectionContractError::DuplicateTokenId { field, token_id });
        }
    }
    Ok(())
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

impl fmt::Display for KvSelectionContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid KV selection JSON: {error}"),
            Self::NonCanonicalJson => formatter.write_str("KV selection JSON is not canonical"),
            Self::UnsupportedSchema => formatter.write_str("unsupported KV selection schema"),
            Self::EmptyPolicy => formatter.write_str("KV selection policy must not be empty"),
            Self::EmptyInput => formatter.write_str("KV selection input must not be empty"),
            Self::ZeroBytesPerToken => formatter.write_str("KV bytes per token must be positive"),
            Self::DuplicateTokenId { field, token_id } => {
                write!(formatter, "duplicate token id {token_id} in {field}")
            }
            Self::UnknownTokenId { field, token_id } => {
                write!(formatter, "unknown token id {token_id} in {field}")
            }
            Self::OverlappingPartition { token_id } => {
                write!(formatter, "token id {token_id} is both retained and evicted")
            }
            Self::IncompletePartition { token_id } => {
                write!(formatter, "token id {token_id} is absent from the selection partition")
            }
            Self::OrderMismatch(field) => write!(formatter, "{field} does not preserve input order"),
            Self::LogicalByteOverflow => formatter.write_str("KV logical byte accounting overflow"),
            Self::LogicalAccountingMismatch => {
                formatter.write_str("KV selection logical byte accounting does not replay")
            }
        }
    }
}

impl std::error::Error for KvSelectionContractError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{KvlabKvSelectionHandoffV1, canonical_json};

    fn fixture() -> String {
        canonical_json(&json!({
            "schema":"kvlab.prospect-kv-selection/v1",
            "policy":"lru",
            "input_token_ids":[10,11,12,13],
            "bytes_per_token":64,
            "retained_token_ids":[10,12],
            "evicted_token_ids":[11,13],
            "logical_input_bytes":256,
            "logical_retained_bytes":128,
            "logical_evicted_bytes":128
        }))
        .unwrap()
    }

    #[test]
    fn replays_explicit_partition_and_accounting() {
        let record = KvlabKvSelectionHandoffV1::from_canonical_json(&fixture()).unwrap();
        assert_eq!(record.outcome().policy(), "lru");
        assert_eq!(record.outcome().retained_token_ids(), &[10, 12]);
        assert_eq!(record.outcome().evicted_token_ids(), &[11, 13]);
        assert_eq!(record.outcome().logical_retained_bytes(), 128);
    }

    #[test]
    fn rejects_order_or_accounting_tampering() {
        let mut value: Value = serde_json::from_str(&fixture()).unwrap();
        value["retained_token_ids"] = json!([12,10]);
        let tampered = canonical_json(&value).unwrap();
        assert!(KvlabKvSelectionHandoffV1::from_canonical_json(&tampered).is_err());

        let mut value: Value = serde_json::from_str(&fixture()).unwrap();
        value["logical_retained_bytes"] = json!(64);
        let tampered = canonical_json(&value).unwrap();
        assert!(KvlabKvSelectionHandoffV1::from_canonical_json(&tampered).is_err());
    }
}
