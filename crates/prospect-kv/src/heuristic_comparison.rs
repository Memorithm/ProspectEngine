use core::cmp::Ordering;
use core::fmt;
use std::collections::BTreeSet;

use prospect_evidence::{EvidenceError, EvidenceNature, EvidenceSource};
use serde::Deserialize;

use crate::synthetic_effect::{
    BoundKvRegion, KvlabKvEvictionEffectError, KvlabKvEvictionEffectV1,
};

pub const KVLAB_KV_HEURISTIC_COMPARISON_SCHEMA_V1: &str =
    "kvlab.prospect-kv-heuristic-comparison/v1";
pub const KVLAB_KV_HEURISTIC_COMPARISON_REVISION: &str =
    "407c6f2e15a9cb4b1bcbdfa273e41a930483af39";

const FLOAT_ABS_TOLERANCE: f64 = 1.0e-12;
const FLOAT_REL_TOLERANCE: f64 = 1.0e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeuristicPolicy {
    OldestFirst,
    Lru,
    Magnitude,
    SyntheticSensitivityPerByte,
    Random,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeuristicComparisonResult {
    policy: HeuristicPolicy,
    retained_region_ids: Vec<String>,
    retained_bytes: u64,
    unused_bytes: u64,
    output_l2_delta: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KvlabKvHeuristicComparisonV1 {
    effect: KvlabKvEvictionEffectV1,
    budget_bytes: u64,
    random_seed: u64,
    results: Vec<HeuristicComparisonResult>,
    best_policy: HeuristicPolicy,
    content_fingerprint: String,
}

#[derive(Debug)]
pub enum KvlabKvHeuristicComparisonError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    Effect(KvlabKvEvictionEffectError),
    BudgetMismatch,
    InvalidResultCount,
    UnknownPolicy(String),
    PolicyOrderMismatch,
    DuplicateRetainedRegion,
    UnknownRetainedRegion(String),
    RetainedBytesOverflow,
    RetainedBytesMismatch,
    BudgetExceeded,
    UnusedBytesMismatch,
    L2DeltaMismatch,
    OldestFirstMismatch,
    LruMismatch,
    MagnitudeMismatch,
    SensitivityMismatch,
    BestPolicyMismatch,
    Evidence(EvidenceError),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonWire {
    schema: String,
    effect: serde_json::Value,
    budget_bytes: u64,
    random_seed: u64,
    results: Vec<ResultWire>,
    best_policy: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultWire {
    policy: String,
    retained_region_ids: Vec<String>,
    retained_bytes: u64,
    unused_bytes: u64,
    output_l2_delta: f64,
}

impl HeuristicPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OldestFirst => "oldest_first",
            Self::Lru => "lru",
            Self::Magnitude => "magnitude",
            Self::SyntheticSensitivityPerByte => "synthetic_sensitivity_per_byte",
            Self::Random => "random",
        }
    }

    fn parse(value: &str) -> Result<Self, KvlabKvHeuristicComparisonError> {
        match value {
            "oldest_first" => Ok(Self::OldestFirst),
            "lru" => Ok(Self::Lru),
            "magnitude" => Ok(Self::Magnitude),
            "synthetic_sensitivity_per_byte" => Ok(Self::SyntheticSensitivityPerByte),
            "random" => Ok(Self::Random),
            other => Err(KvlabKvHeuristicComparisonError::UnknownPolicy(
                other.to_owned(),
            )),
        }
    }
}

impl HeuristicComparisonResult {
    #[must_use]
    pub const fn policy(&self) -> HeuristicPolicy {
        self.policy
    }

    #[must_use]
    pub fn retained_region_ids(&self) -> &[String] {
        &self.retained_region_ids
    }

    #[must_use]
    pub const fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }

    #[must_use]
    pub const fn unused_bytes(&self) -> u64 {
        self.unused_bytes
    }

    #[must_use]
    pub const fn output_l2_delta(&self) -> f64 {
        self.output_l2_delta
    }
}

impl KvlabKvHeuristicComparisonV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvlabKvHeuristicComparisonError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(KvlabKvHeuristicComparisonError::Json)?;
        if serde_json::to_string(&value).map_err(KvlabKvHeuristicComparisonError::Json)? != json {
            return Err(KvlabKvHeuristicComparisonError::NonCanonicalJson);
        }

        let wire: ComparisonWire =
            serde_json::from_value(value).map_err(KvlabKvHeuristicComparisonError::Json)?;
        if wire.schema != KVLAB_KV_HEURISTIC_COMPARISON_SCHEMA_V1 {
            return Err(KvlabKvHeuristicComparisonError::UnsupportedSchema);
        }

        let effect_json =
            serde_json::to_string(&wire.effect).map_err(KvlabKvHeuristicComparisonError::Json)?;
        let effect = KvlabKvEvictionEffectV1::from_canonical_json(&effect_json)
            .map_err(KvlabKvHeuristicComparisonError::Effect)?;
        if wire.budget_bytes != effect.eviction().outcome().logical_retained_bytes() {
            return Err(KvlabKvHeuristicComparisonError::BudgetMismatch);
        }

        let expected_policy_order = [
            HeuristicPolicy::OldestFirst,
            HeuristicPolicy::Lru,
            HeuristicPolicy::Magnitude,
            HeuristicPolicy::SyntheticSensitivityPerByte,
            HeuristicPolicy::Random,
        ];
        if wire.results.len() != expected_policy_order.len() {
            return Err(KvlabKvHeuristicComparisonError::InvalidResultCount);
        }

        let mut results = Vec::with_capacity(wire.results.len());
        for (wire_result, expected_policy) in wire.results.iter().zip(expected_policy_order) {
            let policy = HeuristicPolicy::parse(&wire_result.policy)?;
            if policy != expected_policy {
                return Err(KvlabKvHeuristicComparisonError::PolicyOrderMismatch);
            }
            results.push(validate_result(
                wire_result,
                policy,
                wire.budget_bytes,
                effect.regions(),
                effect.signature().full_cache_output(),
            )?);
        }

        validate_deterministic_selections(&effect, wire.budget_bytes, &results)?;

        let best_policy = HeuristicPolicy::parse(&wire.best_policy)?;
        if best_policy != recompute_best_policy(&results) {
            return Err(KvlabKvHeuristicComparisonError::BestPolicyMismatch);
        }

        Ok(Self {
            effect,
            budget_bytes: wire.budget_bytes,
            random_seed: wire.random_seed,
            results,
            best_policy,
            content_fingerprint: format!("fnv1a64:{:016x}", fnv1a64(json.as_bytes())),
        })
    }

    #[must_use]
    pub const fn effect(&self) -> &KvlabKvEvictionEffectV1 {
        &self.effect
    }

    #[must_use]
    pub const fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    #[must_use]
    pub const fn random_seed(&self) -> u64 {
        self.random_seed
    }

    #[must_use]
    pub fn results(&self) -> &[HeuristicComparisonResult] {
        &self.results
    }

    #[must_use]
    pub const fn best_policy(&self) -> HeuristicPolicy {
        self.best_policy
    }

    #[must_use]
    pub fn content_fingerprint(&self) -> &str {
        &self.content_fingerprint
    }

    pub fn evidence_source(&self) -> Result<EvidenceSource, KvlabKvHeuristicComparisonError> {
        EvidenceSource::new_with_nature(
            "KVLab/synthetic-kv-heuristic-comparison",
            KVLAB_KV_HEURISTIC_COMPARISON_REVISION,
            EvidenceNature::Simulated,
        )
        .map_err(KvlabKvHeuristicComparisonError::Evidence)?
        .with_content_hash(self.content_fingerprint.clone())
        .map_err(KvlabKvHeuristicComparisonError::Evidence)
    }
}

fn validate_result(
    wire: &ResultWire,
    policy: HeuristicPolicy,
    budget_bytes: u64,
    regions: &[BoundKvRegion],
    full_cache_output: &[f64],
) -> Result<HeuristicComparisonResult, KvlabKvHeuristicComparisonError> {
    let mut seen = BTreeSet::new();
    let mut retained_bytes = 0_u64;
    let mut selected_output = vec![0.0; full_cache_output.len()];

    for region_id in &wire.retained_region_ids {
        if !seen.insert(region_id.as_str()) {
            return Err(KvlabKvHeuristicComparisonError::DuplicateRetainedRegion);
        }
        let region = regions
            .iter()
            .find(|region| region.region_id() == region_id)
            .ok_or_else(|| {
                KvlabKvHeuristicComparisonError::UnknownRetainedRegion(region_id.clone())
            })?;
        retained_bytes = retained_bytes
            .checked_add(region.storage_bytes())
            .ok_or(KvlabKvHeuristicComparisonError::RetainedBytesOverflow)?;
        for (output, contribution) in selected_output.iter_mut().zip(region.contribution()) {
            *output += contribution;
        }
    }

    if retained_bytes != wire.retained_bytes {
        return Err(KvlabKvHeuristicComparisonError::RetainedBytesMismatch);
    }
    if retained_bytes > budget_bytes {
        return Err(KvlabKvHeuristicComparisonError::BudgetExceeded);
    }
    if wire.unused_bytes != budget_bytes - retained_bytes {
        return Err(KvlabKvHeuristicComparisonError::UnusedBytesMismatch);
    }

    let output_l2_delta = l2_delta(full_cache_output, &selected_output);
    if !wire.output_l2_delta.is_finite()
        || wire.output_l2_delta < 0.0
        || !nearly_equal(wire.output_l2_delta, output_l2_delta)
    {
        return Err(KvlabKvHeuristicComparisonError::L2DeltaMismatch);
    }

    Ok(HeuristicComparisonResult {
        policy,
        retained_region_ids: wire.retained_region_ids.clone(),
        retained_bytes,
        unused_bytes: wire.unused_bytes,
        output_l2_delta,
    })
}

fn validate_deterministic_selections(
    effect: &KvlabKvEvictionEffectV1,
    budget_bytes: u64,
    results: &[HeuristicComparisonResult],
) -> Result<(), KvlabKvHeuristicComparisonError> {
    let oldest = &results[0];
    if oldest.retained_region_ids() != effect.retained_region_ids() {
        return Err(KvlabKvHeuristicComparisonError::OldestFirstMismatch);
    }

    let lru = greedy_lru(effect.regions(), budget_bytes);
    if results[1].retained_region_ids() != lru {
        return Err(KvlabKvHeuristicComparisonError::LruMismatch);
    }

    let magnitude = greedy_ranked(effect.regions(), budget_bytes, |region| {
        contribution_l2(region)
    });
    if results[2].retained_region_ids() != magnitude {
        return Err(KvlabKvHeuristicComparisonError::MagnitudeMismatch);
    }

    let sensitivity = greedy_ranked(effect.regions(), budget_bytes, |region| {
        contribution_l2(region) / region.storage_bytes() as f64
    });
    if results[3].retained_region_ids() != sensitivity {
        return Err(KvlabKvHeuristicComparisonError::SensitivityMismatch);
    }

    // The random result is not regenerated here: Python's Random/shuffle
    // implementation is producer-specific. Its retained set, byte accounting,
    // membership and numerical L2 consequence were independently revalidated
    // by `validate_result`; the explicit seed remains preserved as provenance.
    Ok(())
}

fn greedy_lru(regions: &[BoundKvRegion], budget_bytes: u64) -> Vec<String> {
    let mut retained = BTreeSet::new();
    let mut used = 0_u64;
    for index in (0..regions.len()).rev() {
        let bytes = regions[index].storage_bytes();
        if used.checked_add(bytes).is_some_and(|next| next <= budget_bytes) {
            used += bytes;
            retained.insert(index);
        }
    }
    regions
        .iter()
        .enumerate()
        .filter(|(index, _)| retained.contains(index))
        .map(|(_, region)| region.region_id().to_owned())
        .collect()
}

fn greedy_ranked<F>(
    regions: &[BoundKvRegion],
    budget_bytes: u64,
    score: F,
) -> Vec<String>
where
    F: Fn(&BoundKvRegion) -> f64,
{
    let mut ranked = regions
        .iter()
        .enumerate()
        .map(|(index, region)| (index, score(region)))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(Ordering::Equal)
    });

    let mut retained = BTreeSet::new();
    let mut used = 0_u64;
    for (index, _) in ranked {
        let bytes = regions[index].storage_bytes();
        if used.checked_add(bytes).is_some_and(|next| next <= budget_bytes) {
            used += bytes;
            retained.insert(index);
        }
    }
    regions
        .iter()
        .enumerate()
        .filter(|(index, _)| retained.contains(index))
        .map(|(_, region)| region.region_id().to_owned())
        .collect()
}

fn contribution_l2(region: &BoundKvRegion) -> f64 {
    region
        .contribution()
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt()
}

fn l2_delta(full: &[f64], selected: &[f64]) -> f64 {
    full.iter()
        .zip(selected)
        .map(|(full, selected)| {
            let delta = full - selected;
            delta * delta
        })
        .sum::<f64>()
        .sqrt()
}

fn recompute_best_policy(results: &[HeuristicComparisonResult]) -> HeuristicPolicy {
    results
        .iter()
        .min_by(|left, right| {
            left.output_l2_delta
                .partial_cmp(&right.output_l2_delta)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.policy.as_str().cmp(right.policy.as_str()))
        })
        .expect("schema validation requires five heuristic results")
        .policy
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

impl fmt::Display for KvlabKvHeuristicComparisonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid KV heuristic comparison JSON: {error}"),
            Self::NonCanonicalJson => formatter.write_str("KV heuristic comparison JSON is not canonical"),
            Self::UnsupportedSchema => formatter.write_str("unsupported KV heuristic comparison schema"),
            Self::Effect(error) => write!(formatter, "invalid embedded KV eviction effect: {error}"),
            Self::BudgetMismatch => formatter.write_str("KV heuristic comparison budget does not match the embedded eviction"),
            Self::InvalidResultCount => formatter.write_str("KV heuristic comparison must contain exactly five policy results"),
            Self::UnknownPolicy(policy) => write!(formatter, "unknown KV heuristic policy {policy}"),
            Self::PolicyOrderMismatch => formatter.write_str("KV heuristic comparison policy order does not match schema v1"),
            Self::DuplicateRetainedRegion => formatter.write_str("KV heuristic result contains duplicate retained regions"),
            Self::UnknownRetainedRegion(region) => write!(formatter, "KV heuristic result references unknown region {region}"),
            Self::RetainedBytesOverflow => formatter.write_str("KV heuristic retained-byte accounting overflow"),
            Self::RetainedBytesMismatch => formatter.write_str("KV heuristic retained bytes do not match retained regions"),
            Self::BudgetExceeded => formatter.write_str("KV heuristic retained regions exceed the shared budget"),
            Self::UnusedBytesMismatch => formatter.write_str("KV heuristic unused bytes do not match shared budget accounting"),
            Self::L2DeltaMismatch => formatter.write_str("KV heuristic L2 delta does not match the synthetic oracle replay"),
            Self::OldestFirstMismatch => formatter.write_str("oldest_first result does not match the embedded eviction"),
            Self::LruMismatch => formatter.write_str("LRU result does not match independent Rust replay"),
            Self::MagnitudeMismatch => formatter.write_str("magnitude result does not match independent Rust replay"),
            Self::SensitivityMismatch => formatter.write_str("sensitivity-per-byte result does not match independent Rust replay"),
            Self::BestPolicyMismatch => formatter.write_str("best heuristic policy does not match replayed L2 results"),
            Self::Evidence(error) => write!(formatter, "invalid ProspectEngine evidence source: {error}"),
        }
    }
}

impl std::error::Error for KvlabKvHeuristicComparisonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Effect(error) => Some(error),
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use prospect_evidence::EvidenceNature;
    use serde_json::json;

    use super::{
        HeuristicPolicy, KVLAB_KV_HEURISTIC_COMPARISON_REVISION,
        KvlabKvHeuristicComparisonV1,
    };

    fn effect_value() -> serde_json::Value {
        json!({
            "schema":"kvlab.prospect-kv-eviction-effect/v1",
            "trace_id":"comparison-fixture",
            "regions":[
                {"token_id":10,"region_id":"r10","storage_bytes":64,"contribution":[1.0,0.0]},
                {"token_id":11,"region_id":"r11","storage_bytes":64,"contribution":[0.0,2.0]},
                {"token_id":12,"region_id":"r12","storage_bytes":64,"contribution":[3.0,0.0]},
                {"token_id":13,"region_id":"r13","storage_bytes":64,"contribution":[0.0,4.0]},
                {"token_id":14,"region_id":"r14","storage_bytes":64,"contribution":[-1.0,1.0]}
            ],
            "eviction":{
                "schema":"kvlab.prospect-kv-eviction/v1",
                "order":"oldest_first",
                "max_tokens":3,
                "input_token_ids":[10,11,12,13,14],
                "bytes_per_token":64,
                "retained_token_ids":[12,13,14],
                "evicted_token_ids":[10,11],
                "logical_input_bytes":320,
                "logical_retained_bytes":192,
                "logical_evicted_bytes":128
            },
            "retained_region_ids":["r12","r13","r14"],
            "evicted_region_ids":["r10","r11"],
            "full_cache_output":[3.0,7.0],
            "retained_output":[2.0,5.0],
            "output_l2_delta":5.0_f64.sqrt(),
            "logical_evicted_bytes":128
        })
    }

    fn comparison_json() -> String {
        serde_json::to_string(&json!({
            "schema":"kvlab.prospect-kv-heuristic-comparison/v1",
            "effect":effect_value(),
            "budget_bytes":192,
            "random_seed":7,
            "results":[
                {"policy":"oldest_first","retained_region_ids":["r12","r13","r14"],"retained_bytes":192,"unused_bytes":0,"output_l2_delta":5.0_f64.sqrt()},
                {"policy":"lru","retained_region_ids":["r12","r13","r14"],"retained_bytes":192,"unused_bytes":0,"output_l2_delta":5.0_f64.sqrt()},
                {"policy":"magnitude","retained_region_ids":["r11","r12","r13"],"retained_bytes":192,"unused_bytes":0,"output_l2_delta":1.0},
                {"policy":"synthetic_sensitivity_per_byte","retained_region_ids":["r11","r12","r13"],"retained_bytes":192,"unused_bytes":0,"output_l2_delta":1.0},
                {"policy":"random","retained_region_ids":["r10","r13","r14"],"retained_bytes":192,"unused_bytes":0,"output_l2_delta":13.0_f64.sqrt()}
            ],
            "best_policy":"magnitude"
        }))
        .unwrap()
    }

    #[test]
    fn replays_budget_and_deterministic_heuristics() {
        let comparison =
            KvlabKvHeuristicComparisonV1::from_canonical_json(&comparison_json()).unwrap();
        assert_eq!(comparison.budget_bytes(), 192);
        assert_eq!(comparison.random_seed(), 7);
        assert_eq!(comparison.best_policy(), HeuristicPolicy::Magnitude);
        assert_eq!(comparison.results().len(), 5);
        assert_eq!(comparison.results()[0].policy(), HeuristicPolicy::OldestFirst);
        assert_eq!(comparison.results()[1].policy(), HeuristicPolicy::Lru);
    }

    #[test]
    fn rejects_tampered_deterministic_selection() {
        let mut value: serde_json::Value = serde_json::from_str(&comparison_json()).unwrap();
        value["results"][2]["retained_region_ids"] = json!(["r10", "r12", "r13"]);
        value["results"][2]["output_l2_delta"] = json!(2.0);
        let tampered = serde_json::to_string(&value).unwrap();
        assert!(KvlabKvHeuristicComparisonV1::from_canonical_json(&tampered).is_err());
    }

    #[test]
    fn rejects_tampered_random_oracle_score() {
        let mut value: serde_json::Value = serde_json::from_str(&comparison_json()).unwrap();
        value["results"][4]["output_l2_delta"] = json!(1.0);
        let tampered = serde_json::to_string(&value).unwrap();
        assert!(KvlabKvHeuristicComparisonV1::from_canonical_json(&tampered).is_err());
    }

    #[test]
    fn marks_comparison_as_simulated_evidence() {
        let comparison =
            KvlabKvHeuristicComparisonV1::from_canonical_json(&comparison_json()).unwrap();
        let source = comparison.evidence_source().unwrap();
        assert_eq!(source.nature(), EvidenceNature::Simulated);
        assert_eq!(source.revision(), KVLAB_KV_HEURISTIC_COMPARISON_REVISION);
        assert!(source.content_hash().unwrap().starts_with("fnv1a64:"));
    }
}
