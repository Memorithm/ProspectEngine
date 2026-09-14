#![forbid(unsafe_code)]

pub mod heuristic_comparison;
pub mod synthetic_effect;

use core::fmt;
use std::collections::BTreeSet;

use prospect_core::{InvalidScenarioId, ProspectiveEngine, Scenario, ScenarioId};
use serde::Deserialize;

pub const KVLAB_KV_EVICTION_HANDOFF_SCHEMA_V1: &str = "kvlab.prospect-kv-eviction/v1";
pub const KVLAB_KV_EVICTION_HANDOFF_REVISION: &str = "e53a09e9b5923bb95527036d5148735f973eefc9";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvEvictionState {
    token_ids: Vec<u64>,
    bytes_per_token: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KvEvictionIntervention {
    max_tokens: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvEvictionOutcome {
    input_token_ids: Vec<u64>,
    retained_token_ids: Vec<u64>,
    evicted_token_ids: Vec<u64>,
    bytes_per_token: u64,
    logical_input_bytes: u64,
    logical_retained_bytes: u64,
    logical_evicted_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvlabKvEvictionHandoffV1 {
    state: KvEvictionState,
    intervention: KvEvictionIntervention,
    outcome: KvEvictionOutcome,
}

#[derive(Debug)]
pub enum KvEvictionContractError {
    ZeroBytesPerToken,
    DuplicateTokenId { token_id: u64 },
    ZeroMaxTokens,
    LogicalByteOverflow,
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    UnsupportedOrder,
    ReplayMismatch,
    InvalidScenarioId(InvalidScenarioId),
    DuplicateScenarioLimit { max_tokens: usize },
    NoEvictionScenario { max_tokens: usize },
}

#[derive(Debug)]
pub enum KvEvictionEngineError<E> {
    Contract(KvEvictionContractError),
    Model(E),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffWire {
    schema: String,
    order: String,
    max_tokens: usize,
    input_token_ids: Vec<u64>,
    bytes_per_token: u64,
    retained_token_ids: Vec<u64>,
    evicted_token_ids: Vec<u64>,
    logical_input_bytes: u64,
    logical_retained_bytes: u64,
    logical_evicted_bytes: u64,
}

impl KvEvictionState {
    pub fn new(
        token_ids: impl Into<Vec<u64>>,
        bytes_per_token: u64,
    ) -> Result<Self, KvEvictionContractError> {
        if bytes_per_token == 0 {
            return Err(KvEvictionContractError::ZeroBytesPerToken);
        }
        let token_ids = token_ids.into();
        validate_unique_token_ids(&token_ids)?;
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

    #[must_use]
    pub fn token_count(&self) -> usize {
        self.token_ids.len()
    }
}

impl KvEvictionIntervention {
    pub fn new(max_tokens: usize) -> Result<Self, KvEvictionContractError> {
        if max_tokens == 0 {
            return Err(KvEvictionContractError::ZeroMaxTokens);
        }
        Ok(Self { max_tokens })
    }

    #[must_use]
    pub const fn max_tokens(self) -> usize {
        self.max_tokens
    }
}

impl KvEvictionOutcome {
    #[must_use]
    pub fn input_token_ids(&self) -> &[u64] {
        &self.input_token_ids
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
    pub const fn bytes_per_token(&self) -> u64 {
        self.bytes_per_token
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

    #[must_use]
    pub fn retained_tokens(&self) -> usize {
        self.retained_token_ids.len()
    }

    #[must_use]
    pub fn evicted_tokens(&self) -> usize {
        self.evicted_token_ids.len()
    }
}

impl KvlabKvEvictionHandoffV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvEvictionContractError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(KvEvictionContractError::Json)?;
        if serde_json::to_string(&value).map_err(KvEvictionContractError::Json)? != json {
            return Err(KvEvictionContractError::NonCanonicalJson);
        }
        let wire: HandoffWire =
            serde_json::from_value(value).map_err(KvEvictionContractError::Json)?;
        if wire.schema != KVLAB_KV_EVICTION_HANDOFF_SCHEMA_V1 {
            return Err(KvEvictionContractError::UnsupportedSchema);
        }
        if wire.order != "oldest_first" {
            return Err(KvEvictionContractError::UnsupportedOrder);
        }

        let state = KvEvictionState::new(wire.input_token_ids, wire.bytes_per_token)?;
        let intervention = KvEvictionIntervention::new(wire.max_tokens)?;
        let outcome = apply_oldest_first(&state, intervention)?;
        if outcome.retained_token_ids != wire.retained_token_ids
            || outcome.evicted_token_ids != wire.evicted_token_ids
            || outcome.logical_input_bytes != wire.logical_input_bytes
            || outcome.logical_retained_bytes != wire.logical_retained_bytes
            || outcome.logical_evicted_bytes != wire.logical_evicted_bytes
        {
            return Err(KvEvictionContractError::ReplayMismatch);
        }

        Ok(Self {
            state,
            intervention,
            outcome,
        })
    }

    #[must_use]
    pub const fn state(&self) -> &KvEvictionState {
        &self.state
    }

    #[must_use]
    pub const fn intervention(&self) -> KvEvictionIntervention {
        self.intervention
    }

    #[must_use]
    pub const fn outcome(&self) -> &KvEvictionOutcome {
        &self.outcome
    }
}

/// Domain-model boundary for the consequences of a logical KV eviction.
///
/// ProspectEngine supplies the exact retained/evicted token set and logical
/// byte accounting. The model is responsible for any numerical, quality,
/// latency, physical-memory, or task-specific semantics. No such effect is
/// inferred from `logical_evicted_bytes` alone.
pub trait KvEvictionProspectiveModel {
    type Signature;
    type Error;

    fn evaluate_eviction(
        &self,
        state: &KvEvictionState,
        outcome: &KvEvictionOutcome,
    ) -> Result<Self::Signature, Self::Error>;
}

pub struct KvEvictionEngine<M> {
    model: M,
}

impl<M> KvEvictionEngine<M> {
    #[must_use]
    pub const fn new(model: M) -> Self {
        Self { model }
    }

    #[must_use]
    pub const fn model(&self) -> &M {
        &self.model
    }
}

impl<M> ProspectiveEngine<KvEvictionState, KvEvictionIntervention> for KvEvictionEngine<M>
where
    M: KvEvictionProspectiveModel,
{
    type Signature = M::Signature;
    type Error = KvEvictionEngineError<M::Error>;

    fn baseline(&self, state: &KvEvictionState) -> Result<Self::Signature, Self::Error> {
        let outcome = no_eviction_outcome(state).map_err(KvEvictionEngineError::Contract)?;
        self.model
            .evaluate_eviction(state, &outcome)
            .map_err(KvEvictionEngineError::Model)
    }

    fn evaluate(
        &self,
        state: &KvEvictionState,
        intervention: &KvEvictionIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        let outcome =
            apply_oldest_first(state, *intervention).map_err(KvEvictionEngineError::Contract)?;
        self.model
            .evaluate_eviction(state, &outcome)
            .map_err(KvEvictionEngineError::Model)
    }
}

pub fn retention_scenarios(
    state: &KvEvictionState,
    limits: impl IntoIterator<Item = usize>,
) -> Result<Vec<Scenario<KvEvictionIntervention>>, KvEvictionContractError> {
    let mut limits = limits.into_iter().collect::<Vec<_>>();
    if limits.contains(&0) {
        return Err(KvEvictionContractError::ZeroMaxTokens);
    }
    limits.sort_unstable();
    if let Some(pair) = limits.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(KvEvictionContractError::DuplicateScenarioLimit {
            max_tokens: pair[0],
        });
    }

    let mut scenarios = Vec::with_capacity(limits.len());
    for max_tokens in limits {
        if max_tokens >= state.token_count() {
            return Err(KvEvictionContractError::NoEvictionScenario { max_tokens });
        }
        let intervention = KvEvictionIntervention::new(max_tokens)?;
        let id = ScenarioId::new(format!("kv-retain-{max_tokens}"))
            .map_err(KvEvictionContractError::InvalidScenarioId)?;
        scenarios.push(Scenario::new(id, intervention));
    }
    Ok(scenarios)
}

fn no_eviction_outcome(
    state: &KvEvictionState,
) -> Result<KvEvictionOutcome, KvEvictionContractError> {
    logical_outcome(state, 0)
}

fn apply_oldest_first(
    state: &KvEvictionState,
    intervention: KvEvictionIntervention,
) -> Result<KvEvictionOutcome, KvEvictionContractError> {
    let evict_count = state
        .token_count()
        .saturating_sub(intervention.max_tokens());
    logical_outcome(state, evict_count)
}

fn logical_outcome(
    state: &KvEvictionState,
    evict_count: usize,
) -> Result<KvEvictionOutcome, KvEvictionContractError> {
    let logical_input_bytes = u64::try_from(state.token_count())
        .ok()
        .and_then(|count| count.checked_mul(state.bytes_per_token))
        .ok_or(KvEvictionContractError::LogicalByteOverflow)?;
    let retained = &state.token_ids[evict_count..];
    let logical_retained_bytes = u64::try_from(retained.len())
        .ok()
        .and_then(|count| count.checked_mul(state.bytes_per_token))
        .ok_or(KvEvictionContractError::LogicalByteOverflow)?;

    Ok(KvEvictionOutcome {
        input_token_ids: state.token_ids.clone(),
        retained_token_ids: retained.to_vec(),
        evicted_token_ids: state.token_ids[..evict_count].to_vec(),
        bytes_per_token: state.bytes_per_token,
        logical_input_bytes,
        logical_retained_bytes,
        logical_evicted_bytes: logical_input_bytes - logical_retained_bytes,
    })
}

fn validate_unique_token_ids(token_ids: &[u64]) -> Result<(), KvEvictionContractError> {
    let mut seen = BTreeSet::new();
    for token_id in token_ids {
        if !seen.insert(*token_id) {
            return Err(KvEvictionContractError::DuplicateTokenId {
                token_id: *token_id,
            });
        }
    }
    Ok(())
}

impl fmt::Display for KvEvictionContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroBytesPerToken => formatter.write_str("KV bytes per token must be positive"),
            Self::DuplicateTokenId { token_id } => {
                write!(formatter, "duplicate KV token id {token_id}")
            }
            Self::ZeroMaxTokens => formatter.write_str("KV max_tokens must be positive"),
            Self::LogicalByteOverflow => formatter.write_str("KV logical byte accounting overflow"),
            Self::Json(error) => write!(formatter, "invalid KV eviction handoff JSON: {error}"),
            Self::NonCanonicalJson => {
                formatter.write_str("KV eviction handoff JSON is not canonical")
            }
            Self::UnsupportedSchema => {
                formatter.write_str("unsupported KV eviction handoff schema")
            }
            Self::UnsupportedOrder => formatter.write_str("unsupported KV eviction order"),
            Self::ReplayMismatch => {
                formatter.write_str("KV eviction handoff does not match replayed semantics")
            }
            Self::InvalidScenarioId(error) => write!(formatter, "invalid KV scenario id: {error}"),
            Self::DuplicateScenarioLimit { max_tokens } => {
                write!(formatter, "duplicate KV retention limit {max_tokens}")
            }
            Self::NoEvictionScenario { max_tokens } => write!(
                formatter,
                "KV retention limit {max_tokens} does not evict any token and duplicates the baseline"
            ),
        }
    }
}

impl std::error::Error for KvEvictionContractError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::InvalidScenarioId(error) => Some(error),
            _ => None,
        }
    }
}

impl<E> fmt::Display for KvEvictionEngineError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(error) => write!(formatter, "KV eviction contract error: {error}"),
            Self::Model(error) => write!(formatter, "KV eviction model error: {error}"),
        }
    }
}

impl<E> std::error::Error for KvEvictionEngineError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(error) => Some(error),
            Self::Model(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::convert::Infallible;

    use prospect_core::ProspectiveEngine;

    use super::{
        KvEvictionEngine, KvEvictionIntervention, KvEvictionProspectiveModel, KvEvictionState,
        KvlabKvEvictionHandoffV1, retention_scenarios,
    };

    struct ExplicitRetainedIdModel;

    impl KvEvictionProspectiveModel for ExplicitRetainedIdModel {
        type Signature = (Vec<u64>, u64);
        type Error = Infallible;

        fn evaluate_eviction(
            &self,
            _state: &KvEvictionState,
            outcome: &super::KvEvictionOutcome,
        ) -> Result<Self::Signature, Self::Error> {
            Ok((
                outcome.retained_token_ids().to_vec(),
                outcome.logical_evicted_bytes(),
            ))
        }
    }

    #[test]
    fn baseline_and_eviction_are_distinct_and_model_driven() {
        let state = KvEvictionState::new(vec![10, 11, 12, 13, 14], 2048).expect("state");
        let engine = KvEvictionEngine::new(ExplicitRetainedIdModel);

        assert_eq!(
            engine.baseline(&state).expect("baseline"),
            (vec![10, 11, 12, 13, 14], 0)
        );
        assert_eq!(
            engine
                .evaluate(
                    &state,
                    &KvEvictionIntervention::new(3).expect("intervention")
                )
                .expect("candidate"),
            (vec![12, 13, 14], 4096)
        );
    }

    #[test]
    fn consumes_kvlab_canonical_eviction_handoff() {
        let json = "{\"bytes_per_token\":2048,\"evicted_token_ids\":[10,11],\"input_token_ids\":[10,11,12,13,14],\"logical_evicted_bytes\":4096,\"logical_input_bytes\":10240,\"logical_retained_bytes\":6144,\"max_tokens\":3,\"order\":\"oldest_first\",\"retained_token_ids\":[12,13,14],\"schema\":\"kvlab.prospect-kv-eviction/v1\"}";
        let handoff = KvlabKvEvictionHandoffV1::from_canonical_json(json).expect("handoff");

        assert_eq!(handoff.state().token_ids(), &[10, 11, 12, 13, 14]);
        assert_eq!(handoff.intervention().max_tokens(), 3);
        assert_eq!(handoff.outcome().evicted_token_ids(), &[10, 11]);
        assert_eq!(handoff.outcome().logical_evicted_bytes(), 4096);
    }

    #[test]
    fn rejects_tampered_or_noncanonical_handoff() {
        let tampered = "{\"bytes_per_token\":2048,\"evicted_token_ids\":[10],\"input_token_ids\":[10,11,12,13,14],\"logical_evicted_bytes\":4096,\"logical_input_bytes\":10240,\"logical_retained_bytes\":6144,\"max_tokens\":3,\"order\":\"oldest_first\",\"retained_token_ids\":[12,13,14],\"schema\":\"kvlab.prospect-kv-eviction/v1\"}";
        assert!(KvlabKvEvictionHandoffV1::from_canonical_json(tampered).is_err());

        let pretty = "{ \"bytes_per_token\": 2048, \"evicted_token_ids\": [10,11], \"input_token_ids\": [10,11,12,13,14], \"logical_evicted_bytes\": 4096, \"logical_input_bytes\": 10240, \"logical_retained_bytes\": 6144, \"max_tokens\": 3, \"order\": \"oldest_first\", \"retained_token_ids\": [12,13,14], \"schema\": \"kvlab.prospect-kv-eviction/v1\" }";
        assert!(KvlabKvEvictionHandoffV1::from_canonical_json(pretty).is_err());
    }

    #[test]
    fn retention_scenarios_are_sorted_and_exclude_baseline_duplicates() {
        let state = KvEvictionState::new(vec![0, 1, 2, 3, 4], 512).expect("state");
        let scenarios = retention_scenarios(&state, [3, 1, 2]).expect("scenarios");
        let ids = scenarios
            .iter()
            .map(|scenario| scenario.id().as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["kv-retain-1", "kv-retain-2", "kv-retain-3"]);
        assert!(retention_scenarios(&state, [5]).is_err());
    }
}
