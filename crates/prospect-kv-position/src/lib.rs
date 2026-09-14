#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;

pub const KVLAB_KV_POSITION_HANDOFF_SCHEMA_V2: &str = "kvlab.prospect-kv-selection/v2";
pub const KVLAB_KV_POSITION_HANDOFF_REVISION: &str =
    "782dde3304f2da984f6544cc0a49bab7f5977ea9";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvPositionSelectionState {
    token_ids: Vec<u64>,
    bytes_per_token: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvPositionSelectionOutcome {
    policy: String,
    retained_positions: Vec<usize>,
    evicted_positions: Vec<usize>,
    retained_token_ids: Vec<u64>,
    evicted_token_ids: Vec<u64>,
    logical_input_bytes: u64,
    logical_retained_bytes: u64,
    logical_evicted_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvlabKvPositionHandoffV2 {
    state: KvPositionSelectionState,
    outcome: KvPositionSelectionOutcome,
}

#[derive(Debug)]
pub enum KvPositionContractError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    EmptyPolicy,
    EmptyInput,
    ZeroBytesPerToken,
    InvalidPosition {
        field: &'static str,
        position: usize,
        input_len: usize,
    },
    PositionOrderMismatch(&'static str),
    OverlappingPartition(usize),
    IncompletePartition(usize),
    NonCanonicalEvictedPositions,
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
    retained_positions: Vec<usize>,
    evicted_positions: Vec<usize>,
    logical_input_bytes: u64,
    logical_retained_bytes: u64,
    logical_evicted_bytes: u64,
}

impl KvPositionSelectionState {
    fn new(token_ids: Vec<u64>, bytes_per_token: u64) -> Result<Self, KvPositionContractError> {
        if token_ids.is_empty() {
            return Err(KvPositionContractError::EmptyInput);
        }
        if bytes_per_token == 0 {
            return Err(KvPositionContractError::ZeroBytesPerToken);
        }
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

impl KvPositionSelectionOutcome {
    #[must_use]
    pub fn policy(&self) -> &str {
        &self.policy
    }

    #[must_use]
    pub fn retained_positions(&self) -> &[usize] {
        &self.retained_positions
    }

    #[must_use]
    pub fn evicted_positions(&self) -> &[usize] {
        &self.evicted_positions
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

impl KvlabKvPositionHandoffV2 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvPositionContractError> {
        let value: Value = serde_json::from_str(json).map_err(KvPositionContractError::Json)?;
        let canonical = canonical_json(&value).map_err(KvPositionContractError::Json)?;
        if canonical != json {
            return Err(KvPositionContractError::NonCanonicalJson);
        }

        let wire: SelectionWire =
            serde_json::from_value(value).map_err(KvPositionContractError::Json)?;
        if wire.schema != KVLAB_KV_POSITION_HANDOFF_SCHEMA_V2 {
            return Err(KvPositionContractError::UnsupportedSchema);
        }
        if wire.policy.trim().is_empty() {
            return Err(KvPositionContractError::EmptyPolicy);
        }

        let state = KvPositionSelectionState::new(wire.input_token_ids, wire.bytes_per_token)?;
        validate_positions(
            "retained_positions",
            &wire.retained_positions,
            state.token_ids.len(),
        )?;
        validate_positions(
            "evicted_positions",
            &wire.evicted_positions,
            state.token_ids.len(),
        )?;

        let retained = wire
            .retained_positions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let evicted = wire
            .evicted_positions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        for position in retained.iter().copied() {
            if evicted.contains(&position) {
                return Err(KvPositionContractError::OverlappingPartition(position));
            }
        }
        for position in 0..state.token_ids.len() {
            if !retained.contains(&position) && !evicted.contains(&position) {
                return Err(KvPositionContractError::IncompletePartition(position));
            }
        }

        let expected_evicted = (0..state.token_ids.len())
            .filter(|position| !retained.contains(position))
            .collect::<Vec<_>>();
        if wire.evicted_positions != expected_evicted {
            return Err(KvPositionContractError::NonCanonicalEvictedPositions);
        }

        let logical_input_bytes = checked_bytes(state.token_ids.len(), state.bytes_per_token)?;
        let logical_retained_bytes =
            checked_bytes(wire.retained_positions.len(), state.bytes_per_token)?;
        let logical_evicted_bytes = logical_input_bytes - logical_retained_bytes;
        if wire.logical_input_bytes != logical_input_bytes
            || wire.logical_retained_bytes != logical_retained_bytes
            || wire.logical_evicted_bytes != logical_evicted_bytes
        {
            return Err(KvPositionContractError::LogicalAccountingMismatch);
        }

        let retained_token_ids = wire
            .retained_positions
            .iter()
            .map(|&position| state.token_ids[position])
            .collect();
        let evicted_token_ids = wire
            .evicted_positions
            .iter()
            .map(|&position| state.token_ids[position])
            .collect();

        Ok(Self {
            state,
            outcome: KvPositionSelectionOutcome {
                policy: wire.policy,
                retained_positions: wire.retained_positions,
                evicted_positions: wire.evicted_positions,
                retained_token_ids,
                evicted_token_ids,
                logical_input_bytes,
                logical_retained_bytes,
                logical_evicted_bytes,
            },
        })
    }

    #[must_use]
    pub const fn state(&self) -> &KvPositionSelectionState {
        &self.state
    }

    #[must_use]
    pub const fn outcome(&self) -> &KvPositionSelectionOutcome {
        &self.outcome
    }
}

fn checked_bytes(count: usize, bytes_per_token: u64) -> Result<u64, KvPositionContractError> {
    u64::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(bytes_per_token))
        .ok_or(KvPositionContractError::LogicalByteOverflow)
}

fn validate_positions(
    field: &'static str,
    positions: &[usize],
    input_len: usize,
) -> Result<(), KvPositionContractError> {
    let mut previous = None;
    for &position in positions {
        if position >= input_len {
            return Err(KvPositionContractError::InvalidPosition {
                field,
                position,
                input_len,
            });
        }
        if previous.is_some_and(|last| position <= last) {
            return Err(KvPositionContractError::PositionOrderMismatch(field));
        }
        previous = Some(position);
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

impl fmt::Display for KvPositionContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid KV position-selection JSON: {error}"),
            Self::NonCanonicalJson => {
                formatter.write_str("KV position-selection JSON is not canonical")
            }
            Self::UnsupportedSchema => {
                formatter.write_str("unsupported KV position-selection schema")
            }
            Self::EmptyPolicy => formatter.write_str("KV selection policy must not be empty"),
            Self::EmptyInput => formatter.write_str("KV selection input must not be empty"),
            Self::ZeroBytesPerToken => formatter.write_str("KV bytes per token must be positive"),
            Self::InvalidPosition {
                field,
                position,
                input_len,
            } => write!(
                formatter,
                "position {position} in {field} is outside input prefix 0..{input_len}"
            ),
            Self::PositionOrderMismatch(field) => {
                write!(formatter, "{field} must be strictly increasing and unique")
            }
            Self::OverlappingPartition(position) => write!(
                formatter,
                "sequence position {position} is both retained and evicted"
            ),
            Self::IncompletePartition(position) => write!(
                formatter,
                "sequence position {position} is absent from the selection partition"
            ),
            Self::NonCanonicalEvictedPositions => formatter.write_str(
                "evicted_positions is not the canonical complement of retained_positions",
            ),
            Self::LogicalByteOverflow => formatter.write_str("KV logical byte accounting overflow"),
            Self::LogicalAccountingMismatch => formatter
                .write_str("KV position-selection logical byte accounting does not replay"),
        }
    }
}

impl std::error::Error for KvPositionContractError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{KvlabKvPositionHandoffV2, canonical_json};

    fn fixture() -> String {
        canonical_json(&json!({
            "schema":"kvlab.prospect-kv-selection/v2",
            "policy":"fixture",
            "input_token_ids":[7,11,7,7,19],
            "bytes_per_token":64,
            "retained_positions":[0,2,4],
            "evicted_positions":[1,3],
            "logical_input_bytes":320,
            "logical_retained_bytes":192,
            "logical_evicted_bytes":128
        }))
        .unwrap()
    }

    #[test]
    fn replays_duplicate_token_values_by_position() {
        let handoff = KvlabKvPositionHandoffV2::from_canonical_json(&fixture()).unwrap();
        assert_eq!(handoff.state().token_ids(), &[7, 11, 7, 7, 19]);
        assert_eq!(handoff.outcome().retained_positions(), &[0, 2, 4]);
        assert_eq!(handoff.outcome().evicted_positions(), &[1, 3]);
        assert_eq!(handoff.outcome().retained_token_ids(), &[7, 7, 19]);
        assert_eq!(handoff.outcome().evicted_token_ids(), &[11, 7]);
        assert_eq!(handoff.outcome().logical_retained_bytes(), 192);
    }

    #[test]
    fn rejects_position_or_accounting_tampering() {
        let mut value: Value = serde_json::from_str(&fixture()).unwrap();
        value["retained_positions"] = json!([0, 2, 2]);
        let tampered = canonical_json(&value).unwrap();
        assert!(KvlabKvPositionHandoffV2::from_canonical_json(&tampered).is_err());

        let mut value: Value = serde_json::from_str(&fixture()).unwrap();
        value["evicted_positions"] = json!([3, 1]);
        let tampered = canonical_json(&value).unwrap();
        assert!(KvlabKvPositionHandoffV2::from_canonical_json(&tampered).is_err());

        let mut value: Value = serde_json::from_str(&fixture()).unwrap();
        value["logical_retained_bytes"] = json!(128);
        let tampered = canonical_json(&value).unwrap();
        assert!(KvlabKvPositionHandoffV2::from_canonical_json(&tampered).is_err());
    }

    #[test]
    fn rejects_noncanonical_json() {
        let value: Value = serde_json::from_str(&fixture()).unwrap();
        let pretty = serde_json::to_string_pretty(&value).unwrap();
        assert!(KvlabKvPositionHandoffV2::from_canonical_json(&pretty).is_err());
    }
}
