use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use prospect_evidence::{EvidenceError, EvidenceNature, EvidenceSource};
use serde::Deserialize;

use crate::{
    KvEvictionContractError, KvEvictionOutcome, KvEvictionProspectiveModel, KvEvictionState,
    KvlabKvEvictionHandoffV1,
};

pub const KVLAB_KV_EVICTION_EFFECT_SCHEMA_V1: &str =
    "kvlab.prospect-kv-eviction-effect/v1";
pub const KVLAB_KV_EVICTION_EFFECT_REVISION: &str =
    "e9c10e38a57657e8910b42f42e3625ca0e4f1bbc";

const FLOAT_ABS_TOLERANCE: f64 = 1.0e-12;
const FLOAT_REL_TOLERANCE: f64 = 1.0e-12;

#[derive(Clone, Debug, PartialEq)]
pub struct BoundKvRegion {
    token_id: u64,
    region_id: String,
    storage_bytes: u64,
    contribution: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SyntheticKvEvictionSignature {
    trace_id: String,
    full_cache_output: Vec<f64>,
    retained_output: Vec<f64>,
    output_l2_delta: f64,
    logical_evicted_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KvlabKvEvictionEffectV1 {
    regions: Vec<BoundKvRegion>,
    eviction: KvlabKvEvictionHandoffV1,
    retained_region_ids: Vec<String>,
    evicted_region_ids: Vec<String>,
    signature: SyntheticKvEvictionSignature,
    content_fingerprint: String,
}

#[derive(Debug)]
pub enum KvlabKvEvictionEffectError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    EmptyTraceId,
    EmptyRegions,
    EmptyRegionId,
    DuplicateRegionId,
    RegionTokenOrderMismatch,
    InvalidRegionStorage,
    EmptyContribution,
    ContributionWidthMismatch,
    NonFiniteValue,
    RetainedRegionsMismatch,
    EvictedRegionsMismatch,
    FullOutputMismatch,
    RetainedOutputMismatch,
    L2DeltaMismatch,
    LogicalEvictedBytesMismatch,
    EmbeddedEviction(KvEvictionContractError),
    Evidence(EvidenceError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntheticKvEvidenceModelError {
    EmptyEvidence,
    DuplicateOutcome,
    MissingEvidence,
}

#[derive(Clone, Debug)]
pub struct SyntheticKvEvictionEvidenceModel {
    records: Vec<KvlabKvEvictionEffectV1>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionWire {
    token_id: u64,
    region_id: String,
    storage_bytes: u64,
    contribution: Vec<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectWire {
    schema: String,
    trace_id: String,
    regions: Vec<RegionWire>,
    eviction: serde_json::Value,
    retained_region_ids: Vec<String>,
    evicted_region_ids: Vec<String>,
    full_cache_output: Vec<f64>,
    retained_output: Vec<f64>,
    output_l2_delta: f64,
    logical_evicted_bytes: u64,
}

impl BoundKvRegion {
    #[must_use]
    pub const fn token_id(&self) -> u64 {
        self.token_id
    }

    #[must_use]
    pub fn region_id(&self) -> &str {
        &self.region_id
    }

    #[must_use]
    pub const fn storage_bytes(&self) -> u64 {
        self.storage_bytes
    }

    #[must_use]
    pub fn contribution(&self) -> &[f64] {
        &self.contribution
    }
}

impl SyntheticKvEvictionSignature {
    #[must_use]
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    #[must_use]
    pub fn full_cache_output(&self) -> &[f64] {
        &self.full_cache_output
    }

    #[must_use]
    pub fn retained_output(&self) -> &[f64] {
        &self.retained_output
    }

    #[must_use]
    pub const fn output_l2_delta(&self) -> f64 {
        self.output_l2_delta
    }

    #[must_use]
    pub const fn logical_evicted_bytes(&self) -> u64 {
        self.logical_evicted_bytes
    }
}

impl KvlabKvEvictionEffectV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvlabKvEvictionEffectError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(KvlabKvEvictionEffectError::Json)?;
        if serde_json::to_string(&value).map_err(KvlabKvEvictionEffectError::Json)? != json {
            return Err(KvlabKvEvictionEffectError::NonCanonicalJson);
        }

        let wire: EffectWire =
            serde_json::from_value(value).map_err(KvlabKvEvictionEffectError::Json)?;
        if wire.schema != KVLAB_KV_EVICTION_EFFECT_SCHEMA_V1 {
            return Err(KvlabKvEvictionEffectError::UnsupportedSchema);
        }
        if wire.trace_id.trim().is_empty() {
            return Err(KvlabKvEvictionEffectError::EmptyTraceId);
        }
        if wire.regions.is_empty() {
            return Err(KvlabKvEvictionEffectError::EmptyRegions);
        }

        let eviction_json =
            serde_json::to_string(&wire.eviction).map_err(KvlabKvEvictionEffectError::Json)?;
        let eviction = KvlabKvEvictionHandoffV1::from_canonical_json(&eviction_json)
            .map_err(KvlabKvEvictionEffectError::EmbeddedEviction)?;

        let regions = validate_regions(&wire.regions, &eviction)?;
        let replay = replay_effect(&regions, &eviction)?;

        if wire.retained_region_ids != replay.retained_region_ids {
            return Err(KvlabKvEvictionEffectError::RetainedRegionsMismatch);
        }
        if wire.evicted_region_ids != replay.evicted_region_ids {
            return Err(KvlabKvEvictionEffectError::EvictedRegionsMismatch);
        }
        if !vector_nearly_equal(&wire.full_cache_output, &replay.full_cache_output) {
            return Err(KvlabKvEvictionEffectError::FullOutputMismatch);
        }
        if !vector_nearly_equal(&wire.retained_output, &replay.retained_output) {
            return Err(KvlabKvEvictionEffectError::RetainedOutputMismatch);
        }
        if !wire.output_l2_delta.is_finite()
            || wire.output_l2_delta < 0.0
            || !nearly_equal(wire.output_l2_delta, replay.output_l2_delta)
        {
            return Err(KvlabKvEvictionEffectError::L2DeltaMismatch);
        }
        if wire.logical_evicted_bytes != eviction.outcome().logical_evicted_bytes() {
            return Err(KvlabKvEvictionEffectError::LogicalEvictedBytesMismatch);
        }

        let content_fingerprint = format!("fnv1a64:{:016x}", fnv1a64(json.as_bytes()));
        let signature = SyntheticKvEvictionSignature {
            trace_id: wire.trace_id,
            full_cache_output: replay.full_cache_output,
            retained_output: replay.retained_output,
            output_l2_delta: replay.output_l2_delta,
            logical_evicted_bytes: eviction.outcome().logical_evicted_bytes(),
        };

        Ok(Self {
            regions,
            eviction,
            retained_region_ids: replay.retained_region_ids,
            evicted_region_ids: replay.evicted_region_ids,
            signature,
            content_fingerprint,
        })
    }

    #[must_use]
    pub fn regions(&self) -> &[BoundKvRegion] {
        &self.regions
    }

    #[must_use]
    pub const fn eviction(&self) -> &KvlabKvEvictionHandoffV1 {
        &self.eviction
    }

    #[must_use]
    pub fn retained_region_ids(&self) -> &[String] {
        &self.retained_region_ids
    }

    #[must_use]
    pub fn evicted_region_ids(&self) -> &[String] {
        &self.evicted_region_ids
    }

    #[must_use]
    pub const fn signature(&self) -> &SyntheticKvEvictionSignature {
        &self.signature
    }

    #[must_use]
    pub fn content_fingerprint(&self) -> &str {
        &self.content_fingerprint
    }

    pub fn evidence_source(&self) -> Result<EvidenceSource, KvlabKvEvictionEffectError> {
        EvidenceSource::new_with_nature(
            "KVLab/synthetic-kv-eviction-effect",
            KVLAB_KV_EVICTION_EFFECT_REVISION,
            EvidenceNature::Simulated,
        )
        .map_err(KvlabKvEvictionEffectError::Evidence)?
        .with_content_hash(self.content_fingerprint.clone())
        .map_err(KvlabKvEvictionEffectError::Evidence)
    }
}

impl SyntheticKvEvictionEvidenceModel {
    pub fn new(
        records: Vec<KvlabKvEvictionEffectV1>,
    ) -> Result<Self, SyntheticKvEvidenceModelError> {
        if records.is_empty() {
            return Err(SyntheticKvEvidenceModelError::EmptyEvidence);
        }

        for (index, record) in records.iter().enumerate() {
            if records[..index]
                .iter()
                .any(|prior| same_outcome_identity(prior, record))
            {
                return Err(SyntheticKvEvidenceModelError::DuplicateOutcome);
            }
        }

        Ok(Self { records })
    }

    #[must_use]
    pub fn records(&self) -> &[KvlabKvEvictionEffectV1] {
        &self.records
    }
}

impl KvEvictionProspectiveModel for SyntheticKvEvictionEvidenceModel {
    type Signature = SyntheticKvEvictionSignature;
    type Error = SyntheticKvEvidenceModelError;

    fn evaluate_eviction(
        &self,
        state: &KvEvictionState,
        outcome: &KvEvictionOutcome,
    ) -> Result<Self::Signature, Self::Error> {
        self.records
            .iter()
            .find(|record| {
                record.eviction().state() == state && record.eviction().outcome() == outcome
            })
            .map(|record| record.signature().clone())
            .ok_or(SyntheticKvEvidenceModelError::MissingEvidence)
    }
}

#[derive(Debug)]
struct ReplayedEffect {
    retained_region_ids: Vec<String>,
    evicted_region_ids: Vec<String>,
    full_cache_output: Vec<f64>,
    retained_output: Vec<f64>,
    output_l2_delta: f64,
}

fn validate_regions(
    regions: &[RegionWire],
    eviction: &KvlabKvEvictionHandoffV1,
) -> Result<Vec<BoundKvRegion>, KvlabKvEvictionEffectError> {
    if regions.len() != eviction.state().token_ids().len() {
        return Err(KvlabKvEvictionEffectError::RegionTokenOrderMismatch);
    }

    let expected_width = regions
        .first()
        .map(|region| region.contribution.len())
        .ok_or(KvlabKvEvictionEffectError::EmptyRegions)?;
    if expected_width == 0 {
        return Err(KvlabKvEvictionEffectError::EmptyContribution);
    }

    let mut seen_region_ids = BTreeSet::new();
    let mut validated = Vec::with_capacity(regions.len());
    for (region, expected_token_id) in regions.iter().zip(eviction.state().token_ids()) {
        if region.token_id != *expected_token_id {
            return Err(KvlabKvEvictionEffectError::RegionTokenOrderMismatch);
        }
        if region.region_id.trim().is_empty() {
            return Err(KvlabKvEvictionEffectError::EmptyRegionId);
        }
        if !seen_region_ids.insert(region.region_id.as_str()) {
            return Err(KvlabKvEvictionEffectError::DuplicateRegionId);
        }
        if region.storage_bytes == 0
            || region.storage_bytes != eviction.state().bytes_per_token()
        {
            return Err(KvlabKvEvictionEffectError::InvalidRegionStorage);
        }
        if region.contribution.is_empty() {
            return Err(KvlabKvEvictionEffectError::EmptyContribution);
        }
        if region.contribution.len() != expected_width {
            return Err(KvlabKvEvictionEffectError::ContributionWidthMismatch);
        }
        if region.contribution.iter().any(|value| !value.is_finite()) {
            return Err(KvlabKvEvictionEffectError::NonFiniteValue);
        }

        validated.push(BoundKvRegion {
            token_id: region.token_id,
            region_id: region.region_id.clone(),
            storage_bytes: region.storage_bytes,
            contribution: region.contribution.clone(),
        });
    }
    Ok(validated)
}

fn replay_effect(
    regions: &[BoundKvRegion],
    eviction: &KvlabKvEvictionHandoffV1,
) -> Result<ReplayedEffect, KvlabKvEvictionEffectError> {
    let width = regions
        .first()
        .map(|region| region.contribution.len())
        .ok_or(KvlabKvEvictionEffectError::EmptyRegions)?;
    let mut by_token = BTreeMap::new();
    for region in regions {
        by_token.insert(region.token_id, region);
    }

    let retained_regions = eviction
        .outcome()
        .retained_token_ids()
        .iter()
        .map(|token_id| {
            by_token
                .get(token_id)
                .copied()
                .ok_or(KvlabKvEvictionEffectError::RegionTokenOrderMismatch)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let evicted_regions = eviction
        .outcome()
        .evicted_token_ids()
        .iter()
        .map(|token_id| {
            by_token
                .get(token_id)
                .copied()
                .ok_or(KvlabKvEvictionEffectError::RegionTokenOrderMismatch)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut full_cache_output = vec![0.0; width];
    for region in regions {
        for (output, contribution) in full_cache_output.iter_mut().zip(&region.contribution) {
            *output += contribution;
        }
    }

    let mut retained_output = vec![0.0; width];
    for region in &retained_regions {
        for (output, contribution) in retained_output.iter_mut().zip(&region.contribution) {
            *output += contribution;
        }
    }

    let output_l2_delta = full_cache_output
        .iter()
        .zip(&retained_output)
        .map(|(full, retained)| {
            let delta = full - retained;
            delta * delta
        })
        .sum::<f64>()
        .sqrt();
    if !output_l2_delta.is_finite() {
        return Err(KvlabKvEvictionEffectError::NonFiniteValue);
    }

    Ok(ReplayedEffect {
        retained_region_ids: retained_regions
            .iter()
            .map(|region| region.region_id.clone())
            .collect(),
        evicted_region_ids: evicted_regions
            .iter()
            .map(|region| region.region_id.clone())
            .collect(),
        full_cache_output,
        retained_output,
        output_l2_delta,
    })
}

fn same_outcome_identity(
    left: &KvlabKvEvictionEffectV1,
    right: &KvlabKvEvictionEffectV1,
) -> bool {
    left.eviction().state() == right.eviction().state()
        && left.eviction().outcome() == right.eviction().outcome()
}

fn vector_nearly_equal(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(&left, &right)| left.is_finite() && nearly_equal(left, right))
}

fn nearly_equal(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs());
    (left - right).abs() <= FLOAT_ABS_TOLERANCE + FLOAT_REL_TOLERANCE * scale
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

impl fmt::Display for KvlabKvEvictionEffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid KVLab eviction-effect JSON: {error}"),
            Self::NonCanonicalJson => formatter.write_str("KVLab eviction-effect JSON is not canonical"),
            Self::UnsupportedSchema => formatter.write_str("unsupported KVLab eviction-effect schema"),
            Self::EmptyTraceId => formatter.write_str("KVLab eviction-effect trace id must not be empty"),
            Self::EmptyRegions => formatter.write_str("KVLab eviction-effect regions must not be empty"),
            Self::EmptyRegionId => formatter.write_str("KVLab eviction-effect region id must not be empty"),
            Self::DuplicateRegionId => formatter.write_str("KVLab eviction-effect region ids must be unique"),
            Self::RegionTokenOrderMismatch => formatter.write_str("KVLab eviction-effect region/token binding does not match the embedded eviction"),
            Self::InvalidRegionStorage => formatter.write_str("KVLab eviction-effect region storage does not match bytes_per_token"),
            Self::EmptyContribution => formatter.write_str("KVLab eviction-effect contributions must not be empty"),
            Self::ContributionWidthMismatch => formatter.write_str("KVLab eviction-effect contribution widths differ"),
            Self::NonFiniteValue => formatter.write_str("KVLab eviction-effect contains a non-finite numerical value"),
            Self::RetainedRegionsMismatch => formatter.write_str("KVLab eviction-effect retained regions do not match replay"),
            Self::EvictedRegionsMismatch => formatter.write_str("KVLab eviction-effect evicted regions do not match replay"),
            Self::FullOutputMismatch => formatter.write_str("KVLab eviction-effect full-cache output does not match replay"),
            Self::RetainedOutputMismatch => formatter.write_str("KVLab eviction-effect retained output does not match replay"),
            Self::L2DeltaMismatch => formatter.write_str("KVLab eviction-effect L2 delta does not match replay"),
            Self::LogicalEvictedBytesMismatch => formatter.write_str("KVLab eviction-effect logical evicted bytes do not match the embedded eviction"),
            Self::EmbeddedEviction(error) => write!(formatter, "invalid embedded KV eviction handoff: {error}"),
            Self::Evidence(error) => write!(formatter, "invalid ProspectEngine evidence source: {error}"),
        }
    }
}

impl std::error::Error for KvlabKvEvictionEffectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::EmbeddedEviction(error) => Some(error),
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for SyntheticKvEvidenceModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyEvidence => "synthetic KV eviction evidence set must not be empty",
            Self::DuplicateOutcome => "synthetic KV eviction evidence contains duplicate logical outcomes",
            Self::MissingEvidence => "no synthetic numerical evidence matches this KV eviction outcome",
        })
    }
}

impl std::error::Error for SyntheticKvEvidenceModelError {}

#[cfg(test)]
mod tests {
    use prospect_core::ProspectiveEngine;
    use prospect_evidence::EvidenceNature;
    use serde_json::json;

    use super::{
        KVLAB_KV_EVICTION_EFFECT_REVISION, KvlabKvEvictionEffectV1,
        SyntheticKvEvidenceModelError, SyntheticKvEvictionEvidenceModel,
    };
    use crate::{KvEvictionEngine, KvEvictionEngineError, KvEvictionIntervention, KvEvictionState};

    fn effect_json(max_tokens: usize) -> String {
        let (retained, evicted, retained_regions, evicted_regions, retained_output, delta) =
            match max_tokens {
                5 => (
                    vec![10, 11, 12, 13, 14],
                    Vec::<u64>::new(),
                    vec!["r10", "r11", "r12", "r13", "r14"],
                    Vec::<&str>::new(),
                    vec![3.0, 7.0],
                    0.0,
                ),
                3 => (
                    vec![12, 13, 14],
                    vec![10, 11],
                    vec!["r12", "r13", "r14"],
                    vec!["r10", "r11"],
                    vec![2.0, 5.0],
                    5.0_f64.sqrt(),
                ),
                _ => panic!("unsupported fixture"),
            };
        let logical_retained_bytes = u64::try_from(retained.len()).unwrap() * 64;
        let logical_evicted_bytes = u64::try_from(evicted.len()).unwrap() * 64;

        serde_json::to_string(&json!({
            "schema": "kvlab.prospect-kv-eviction-effect/v1",
            "trace_id": "eviction-fixture",
            "regions": [
                {"token_id":10,"region_id":"r10","storage_bytes":64,"contribution":[1.0,0.0]},
                {"token_id":11,"region_id":"r11","storage_bytes":64,"contribution":[0.0,2.0]},
                {"token_id":12,"region_id":"r12","storage_bytes":64,"contribution":[3.0,0.0]},
                {"token_id":13,"region_id":"r13","storage_bytes":64,"contribution":[0.0,4.0]},
                {"token_id":14,"region_id":"r14","storage_bytes":64,"contribution":[-1.0,1.0]}
            ],
            "eviction": {
                "schema":"kvlab.prospect-kv-eviction/v1",
                "order":"oldest_first",
                "max_tokens":max_tokens,
                "input_token_ids":[10,11,12,13,14],
                "bytes_per_token":64,
                "retained_token_ids":retained,
                "evicted_token_ids":evicted,
                "logical_input_bytes":320,
                "logical_retained_bytes":logical_retained_bytes,
                "logical_evicted_bytes":logical_evicted_bytes
            },
            "retained_region_ids":retained_regions,
            "evicted_region_ids":evicted_regions,
            "full_cache_output":[3.0,7.0],
            "retained_output":retained_output,
            "output_l2_delta":delta,
            "logical_evicted_bytes":logical_evicted_bytes
        }))
        .unwrap()
    }

    #[test]
    fn consumes_and_replays_synthetic_effect() {
        let record = KvlabKvEvictionEffectV1::from_canonical_json(&effect_json(3)).unwrap();
        assert_eq!(record.retained_region_ids(), &["r12", "r13", "r14"]);
        assert_eq!(record.evicted_region_ids(), &["r10", "r11"]);
        assert_eq!(record.signature().full_cache_output(), &[3.0, 7.0]);
        assert_eq!(record.signature().retained_output(), &[2.0, 5.0]);
        assert!((record.signature().output_l2_delta() - 5.0_f64.sqrt()).abs() < 1.0e-12);
        assert_eq!(record.signature().logical_evicted_bytes(), 128);
    }

    #[test]
    fn rejects_tampered_numerical_effect() {
        let mut value: serde_json::Value = serde_json::from_str(&effect_json(3)).unwrap();
        value["retained_output"][0] = json!(3.0);
        let tampered = serde_json::to_string(&value).unwrap();
        assert!(KvlabKvEvictionEffectV1::from_canonical_json(&tampered).is_err());
    }

    #[test]
    fn marks_synthetic_oracle_as_simulated_evidence() {
        let record = KvlabKvEvictionEffectV1::from_canonical_json(&effect_json(3)).unwrap();
        let source = record.evidence_source().unwrap();
        assert_eq!(source.nature(), EvidenceNature::Simulated);
        assert_eq!(source.revision(), KVLAB_KV_EVICTION_EFFECT_REVISION);
        assert!(source.content_hash().unwrap().starts_with("fnv1a64:"));
    }

    #[test]
    fn evidence_model_drives_baseline_and_measured_candidate_only() {
        let baseline = KvlabKvEvictionEffectV1::from_canonical_json(&effect_json(5)).unwrap();
        let candidate = KvlabKvEvictionEffectV1::from_canonical_json(&effect_json(3)).unwrap();
        let model = SyntheticKvEvictionEvidenceModel::new(vec![baseline, candidate]).unwrap();
        let engine = KvEvictionEngine::new(model);
        let state = KvEvictionState::new(vec![10, 11, 12, 13, 14], 64).unwrap();

        let baseline_signature = engine.baseline(&state).unwrap();
        assert_eq!(baseline_signature.output_l2_delta(), 0.0);
        assert_eq!(baseline_signature.logical_evicted_bytes(), 0);

        let candidate_signature = engine
            .evaluate(&state, &KvEvictionIntervention::new(3).unwrap())
            .unwrap();
        assert!((candidate_signature.output_l2_delta() - 5.0_f64.sqrt()).abs() < 1.0e-12);
        assert_eq!(candidate_signature.logical_evicted_bytes(), 128);

        assert!(matches!(
            engine.evaluate(&state, &KvEvictionIntervention::new(2).unwrap()),
            Err(KvEvictionEngineError::Model(
                SyntheticKvEvidenceModelError::MissingEvidence
            ))
        ));
    }

    #[test]
    fn duplicate_outcomes_are_rejected() {
        let first = KvlabKvEvictionEffectV1::from_canonical_json(&effect_json(3)).unwrap();
        let second = first.clone();
        assert!(matches!(
            SyntheticKvEvictionEvidenceModel::new(vec![first, second]),
            Err(SyntheticKvEvidenceModelError::DuplicateOutcome)
        ));
    }
}
