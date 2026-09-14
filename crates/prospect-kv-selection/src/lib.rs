#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeSet;

use serde::Deserialize;

pub const KVLAB_KV_SELECTION_HANDOFF_SCHEMA_V1: &str = "kvlab.prospect-kv-selection/v1";
pub const KVLAB_KV_SELECTION_HANDOFF_REVISION: &str = "0e7274bf565d9079943845ea4eb0699a525b1db1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvlabKvSelectionHandoffV1 {
    policy: String,
    input_token_ids: Vec<u64>,
    bytes_per_token: u64,
    retained_token_ids: Vec<u64>,
    evicted_token_ids: Vec<u64>,
    logical_input_bytes: u64,
    logical_retained_bytes: u64,
    logical_evicted_bytes: u64,
}

#[derive(Debug)]
pub enum KvSelectionContractError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    EmptyPolicy,
    EmptyInput,
    ZeroBytesPerToken,
    DuplicateInputToken { token_id: u64 },
    DuplicateRetainedToken { token_id: u64 },
    DuplicateEvictedToken { token_id: u64 },
    PartitionOverlap { token_id: u64 },
    PartitionMismatch,
    RetainedOrderMismatch,
    EvictedOrderMismatch,
    LogicalByteOverflow,
    LogicalInputBytesMismatch,
    LogicalRetainedBytesMismatch,
    LogicalEvictedBytesMismatch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffWire {
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

impl KvlabKvSelectionHandoffV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvSelectionContractError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(KvSelectionContractError::Json)?;
        if serde_json::to_string(&value).map_err(KvSelectionContractError::Json)? != json {
            return Err(KvSelectionContractError::NonCanonicalJson);
        }
        let wire: HandoffWire =
            serde_json::from_value(value).map_err(KvSelectionContractError::Json)?;
        if wire.schema != KVLAB_KV_SELECTION_HANDOFF_SCHEMA_V1 {
            return Err(KvSelectionContractError::UnsupportedSchema);
        }
        if wire.policy.trim().is_empty() {
            return Err(KvSelectionContractError::EmptyPolicy);
        }
        if wire.input_token_ids.is_empty() {
            return Err(KvSelectionContractError::EmptyInput);
        }
        if wire.bytes_per_token == 0 {
            return Err(KvSelectionContractError::ZeroBytesPerToken);
        }

        validate_unique(&wire.input_token_ids, |token_id| {
            KvSelectionContractError::DuplicateInputToken { token_id }
        })?;
        validate_unique(&wire.retained_token_ids, |token_id| {
            KvSelectionContractError::DuplicateRetainedToken { token_id }
        })?;
        validate_unique(&wire.evicted_token_ids, |token_id| {
            KvSelectionContractError::DuplicateEvictedToken { token_id }
        })?;

        let input = wire
            .input_token_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let retained = wire
            .retained_token_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let evicted = wire
            .evicted_token_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();

        if let Some(token_id) = retained.intersection(&evicted).next().copied() {
            return Err(KvSelectionContractError::PartitionOverlap { token_id });
        }
        let union = retained.union(&evicted).copied().collect::<BTreeSet<_>>();
        if union != input {
            return Err(KvSelectionContractError::PartitionMismatch);
        }

        let expected_retained = wire
            .input_token_ids
            .iter()
            .copied()
            .filter(|token_id| retained.contains(token_id))
            .collect::<Vec<_>>();
        if wire.retained_token_ids != expected_retained {
            return Err(KvSelectionContractError::RetainedOrderMismatch);
        }
        let expected_evicted = wire
            .input_token_ids
            .iter()
            .copied()
            .filter(|token_id| evicted.contains(token_id))
            .collect::<Vec<_>>();
        if wire.evicted_token_ids != expected_evicted {
            return Err(KvSelectionContractError::EvictedOrderMismatch);
        }

        let logical_input_bytes = checked_bytes(wire.input_token_ids.len(), wire.bytes_per_token)?;
        let logical_retained_bytes =
            checked_bytes(wire.retained_token_ids.len(), wire.bytes_per_token)?;
        let logical_evicted_bytes = logical_input_bytes
            .checked_sub(logical_retained_bytes)
            .ok_or(KvSelectionContractError::LogicalByteOverflow)?;

        if wire.logical_input_bytes != logical_input_bytes {
            return Err(KvSelectionContractError::LogicalInputBytesMismatch);
        }
        if wire.logical_retained_bytes != logical_retained_bytes {
            return Err(KvSelectionContractError::LogicalRetainedBytesMismatch);
        }
        if wire.logical_evicted_bytes != logical_evicted_bytes {
            return Err(KvSelectionContractError::LogicalEvictedBytesMismatch);
        }

        Ok(Self {
            policy: wire.policy,
            input_token_ids: wire.input_token_ids,
            bytes_per_token: wire.bytes_per_token,
            retained_token_ids: wire.retained_token_ids,
            evicted_token_ids: wire.evicted_token_ids,
            logical_input_bytes,
            logical_retained_bytes,
            logical_evicted_bytes,
        })
    }

    #[must_use]
    pub fn policy(&self) -> &str {
        &self.policy
    }

    #[must_use]
    pub fn input_token_ids(&self) -> &[u64] {
        &self.input_token_ids
    }

    #[must_use]
    pub const fn bytes_per_token(&self) -> u64 {
        self.bytes_per_token
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

fn checked_bytes(count: usize, bytes_per_token: u64) -> Result<u64, KvSelectionContractError> {
    u64::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(bytes_per_token))
        .ok_or(KvSelectionContractError::LogicalByteOverflow)
}

fn validate_unique<E>(values: &[u64], duplicate: impl Fn(u64) -> E) -> Result<(), E> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(*value) {
            return Err(duplicate(*value));
        }
    }
    Ok(())
}

impl fmt::Display for KvSelectionContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid KV selection JSON: {error}"),
            Self::NonCanonicalJson => formatter.write_str("KV selection JSON is not canonical"),
            Self::UnsupportedSchema => formatter.write_str("unsupported KV selection schema"),
            Self::EmptyPolicy => formatter.write_str("KV selection policy must not be empty"),
            Self::EmptyInput => formatter.write_str("KV selection input must not be empty"),
            Self::ZeroBytesPerToken => {
                formatter.write_str("KV selection bytes_per_token must be positive")
            }
            Self::DuplicateInputToken { token_id } => {
                write!(formatter, "duplicate KV selection input token {token_id}")
            }
            Self::DuplicateRetainedToken { token_id } => {
                write!(formatter, "duplicate KV retained token {token_id}")
            }
            Self::DuplicateEvictedToken { token_id } => {
                write!(formatter, "duplicate KV evicted token {token_id}")
            }
            Self::PartitionOverlap { token_id } => write!(
                formatter,
                "KV token {token_id} appears in both retained and evicted partitions"
            ),
            Self::PartitionMismatch => formatter.write_str(
                "KV retained and evicted sets do not exactly partition the input tokens",
            ),
            Self::RetainedOrderMismatch => {
                formatter.write_str("KV retained tokens do not preserve input order")
            }
            Self::EvictedOrderMismatch => {
                formatter.write_str("KV evicted tokens do not preserve input order")
            }
            Self::LogicalByteOverflow => formatter.write_str("KV logical byte accounting overflow"),
            Self::LogicalInputBytesMismatch => {
                formatter.write_str("KV logical input byte count does not match the token set")
            }
            Self::LogicalRetainedBytesMismatch => {
                formatter.write_str("KV logical retained byte count does not match the token set")
            }
            Self::LogicalEvictedBytesMismatch => {
                formatter.write_str("KV logical evicted byte count does not match the token set")
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

    use super::{
        KVLAB_KV_SELECTION_HANDOFF_REVISION, KvSelectionContractError,
        KvlabKvSelectionHandoffV1,
    };

    const LRU: &str = concat!(
        "{\"bytes_per_token\":64,\"evicted_token_ids\":[11,13],",
        "\"input_token_ids\":[10,11,12,13,14],\"logical_evicted_bytes\":128,",
        "\"logical_input_bytes\":320,\"logical_retained_bytes\":192,",
        "\"policy\":\"lru\",\"retained_token_ids\":[10,12,14],",
        "\"schema\":\"kvlab.prospect-kv-selection/v1\"}"
    );

    #[test]
    fn consumes_arbitrary_explicit_selection() {
        let selection = KvlabKvSelectionHandoffV1::from_canonical_json(LRU).expect("selection");
        assert_eq!(selection.policy(), "lru");
        assert_eq!(selection.retained_token_ids(), [10, 12, 14]);
        assert_eq!(selection.evicted_token_ids(), [11, 13]);
        assert_eq!(selection.logical_input_bytes(), 320);
        assert_eq!(selection.logical_retained_bytes(), 192);
        assert_eq!(selection.logical_evicted_bytes(), 128);
        assert_eq!(KVLAB_KV_SELECTION_HANDOFF_REVISION.len(), 40);
    }

    #[test]
    fn same_budget_preserves_distinct_policy_membership() {
        let lru = KvlabKvSelectionHandoffV1::from_canonical_json(LRU).expect("lru");
        let magnitude = LRU
            .replace("[11,13]", "[10,12]")
            .replace("\"lru\"", "\"magnitude\"")
            .replace("[10,12,14]", "[11,13,14]");
        let magnitude =
            KvlabKvSelectionHandoffV1::from_canonical_json(&magnitude).expect("magnitude");
        assert_eq!(
            lru.logical_retained_bytes(),
            magnitude.logical_retained_bytes()
        );
        assert_ne!(lru.retained_token_ids(), magnitude.retained_token_ids());
    }

    #[test]
    fn rejects_partition_and_byte_tampering() {
        let mut value: serde_json::Value = serde_json::from_str(LRU).expect("json");
        value["evicted_token_ids"] = json!([11]);
        let tampered = serde_json::to_string(&value).expect("json");
        assert!(matches!(
            KvlabKvSelectionHandoffV1::from_canonical_json(&tampered),
            Err(KvSelectionContractError::PartitionMismatch)
        ));

        let mut value: serde_json::Value = serde_json::from_str(LRU).expect("json");
        value["logical_retained_bytes"] = json!(64);
        let tampered = serde_json::to_string(&value).expect("json");
        assert!(matches!(
            KvlabKvSelectionHandoffV1::from_canonical_json(&tampered),
            Err(KvSelectionContractError::LogicalRetainedBytesMismatch)
        ));
    }

    #[test]
    fn rejects_reordered_membership_and_noncanonical_json() {
        let mut value: serde_json::Value = serde_json::from_str(LRU).expect("json");
        value["retained_token_ids"] = json!([14, 12, 10]);
        let tampered = serde_json::to_string(&value).expect("json");
        assert!(matches!(
            KvlabKvSelectionHandoffV1::from_canonical_json(&tampered),
            Err(KvSelectionContractError::RetainedOrderMismatch)
        ));

        let value: serde_json::Value = serde_json::from_str(LRU).expect("json");
        let pretty = serde_json::to_string_pretty(&value).expect("json");
        assert!(matches!(
            KvlabKvSelectionHandoffV1::from_canonical_json(&pretty),
            Err(KvSelectionContractError::NonCanonicalJson)
        ));
    }
}
