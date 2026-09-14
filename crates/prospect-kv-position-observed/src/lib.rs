#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeSet;

use prospect_evidence::{EvidenceError, EvidenceNature, EvidenceSource};
use prospect_kv_position::{KvPositionContractError, KvlabKvPositionHandoffV2};
use serde::Deserialize;
use serde_json::Value;

pub const KVLAB_KV_REAL_MODEL_POSITION_SCHEMA_V2: &str =
    "kvlab.prospect-kv-real-model-selection/v2";
pub const KVLAB_KV_REAL_MODEL_POSITION_REVISION: &str = "782dde3304f2da984f6544cc0a49bab7f5977ea9";

const FLOAT_ABS_TOLERANCE: f64 = 1.0e-12;
const FLOAT_REL_TOLERANCE: f64 = 1.0e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObservedMetricKind {
    Numerical,
    Quality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetricPreference {
    HigherIsBetter,
    LowerIsBetter,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedMetricPair {
    name: String,
    kind: ObservedMetricKind,
    unit: String,
    preference: MetricPreference,
    baseline_value: f64,
    candidate_value: f64,
    delta: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedMetricValue {
    name: String,
    kind: ObservedMetricKind,
    unit: String,
    preference: MetricPreference,
    value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedKvPositionSignature {
    policy: Option<String>,
    output_sha256: String,
    logical_kv_bytes: u64,
    retained_positions: Vec<usize>,
    retained_token_ids: Vec<u64>,
    metrics: Vec<ObservedMetricValue>,
    evidence_content_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KvlabKvRealModelPositionEvidenceV2 {
    experiment_id: String,
    run_repository_revision: String,
    model_id: String,
    model_revision: String,
    tokenizer_revision: String,
    runtime_backend: String,
    runtime_revision: String,
    evaluation_id: String,
    trace_sha256: String,
    seed: u64,
    selection: KvlabKvPositionHandoffV2,
    baseline_output_sha256: String,
    candidate_output_sha256: String,
    baseline_logical_kv_bytes: u64,
    candidate_logical_kv_bytes: u64,
    metrics: Vec<ObservedMetricPair>,
    content_fingerprint: String,
}

#[derive(Clone, Debug)]
pub struct ObservedKvPositionComparison {
    records: Vec<KvlabKvRealModelPositionEvidenceV2>,
}

#[derive(Debug)]
pub enum KvlabKvRealModelPositionError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    EmptyText(&'static str),
    InvalidRunRevision,
    InvalidSha256(&'static str),
    EmbeddedSelection(KvPositionContractError),
    BaselineLogicalBytesMismatch,
    CandidateLogicalBytesMismatch,
    EmptyMetrics,
    DuplicateMetric(String),
    UnknownMetricKind(String),
    UnknownMetricPreference(String),
    NonFiniteMetric(String),
    MetricDeltaMismatch(String),
    Evidence(EvidenceError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservedKvPositionComparisonError {
    EmptyEvidence,
    ContextMismatch,
    BaselineMismatch,
    BudgetMismatch,
    DuplicatePolicy(String),
    MissingPolicy(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceWire {
    schema: String,
    experiment_id: String,
    run_repository_revision: String,
    model_id: String,
    model_revision: String,
    tokenizer_revision: String,
    runtime_backend: String,
    runtime_revision: String,
    evaluation_id: String,
    trace_sha256: String,
    seed: u64,
    selection: Value,
    baseline_output_sha256: String,
    candidate_output_sha256: String,
    baseline_logical_kv_bytes: u64,
    candidate_logical_kv_bytes: u64,
    metrics: Vec<MetricWire>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricWire {
    name: String,
    kind: String,
    unit: String,
    preference: String,
    baseline_value: f64,
    candidate_value: f64,
    delta: f64,
}

impl ObservedMetricKind {
    fn parse(value: &str) -> Result<Self, KvlabKvRealModelPositionError> {
        match value {
            "numerical" => Ok(Self::Numerical),
            "quality" => Ok(Self::Quality),
            other => Err(KvlabKvRealModelPositionError::UnknownMetricKind(
                other.to_owned(),
            )),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Numerical => "numerical",
            Self::Quality => "quality",
        }
    }
}

impl MetricPreference {
    fn parse(value: &str) -> Result<Self, KvlabKvRealModelPositionError> {
        match value {
            "higher_is_better" => Ok(Self::HigherIsBetter),
            "lower_is_better" => Ok(Self::LowerIsBetter),
            "none" => Ok(Self::None),
            other => Err(KvlabKvRealModelPositionError::UnknownMetricPreference(
                other.to_owned(),
            )),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HigherIsBetter => "higher_is_better",
            Self::LowerIsBetter => "lower_is_better",
            Self::None => "none",
        }
    }
}

impl ObservedMetricPair {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> ObservedMetricKind {
        self.kind
    }

    #[must_use]
    pub fn unit(&self) -> &str {
        &self.unit
    }

    #[must_use]
    pub const fn preference(&self) -> MetricPreference {
        self.preference
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

    fn baseline_metric(&self) -> ObservedMetricValue {
        ObservedMetricValue {
            name: self.name.clone(),
            kind: self.kind,
            unit: self.unit.clone(),
            preference: self.preference,
            value: self.baseline_value,
        }
    }

    fn candidate_metric(&self) -> ObservedMetricValue {
        ObservedMetricValue {
            name: self.name.clone(),
            kind: self.kind,
            unit: self.unit.clone(),
            preference: self.preference,
            value: self.candidate_value,
        }
    }
}

impl ObservedMetricValue {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> ObservedMetricKind {
        self.kind
    }

    #[must_use]
    pub fn unit(&self) -> &str {
        &self.unit
    }

    #[must_use]
    pub const fn preference(&self) -> MetricPreference {
        self.preference
    }

    #[must_use]
    pub const fn value(&self) -> f64 {
        self.value
    }
}

impl ObservedKvPositionSignature {
    #[must_use]
    pub fn policy(&self) -> Option<&str> {
        self.policy.as_deref()
    }

    #[must_use]
    pub fn output_sha256(&self) -> &str {
        &self.output_sha256
    }

    #[must_use]
    pub const fn logical_kv_bytes(&self) -> u64 {
        self.logical_kv_bytes
    }

    #[must_use]
    pub fn retained_positions(&self) -> &[usize] {
        &self.retained_positions
    }

    #[must_use]
    pub fn retained_token_ids(&self) -> &[u64] {
        &self.retained_token_ids
    }

    #[must_use]
    pub fn metrics(&self) -> &[ObservedMetricValue] {
        &self.metrics
    }

    #[must_use]
    pub fn evidence_content_fingerprint(&self) -> &str {
        &self.evidence_content_fingerprint
    }
}

impl KvlabKvRealModelPositionEvidenceV2 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvlabKvRealModelPositionError> {
        let value: Value =
            serde_json::from_str(json).map_err(KvlabKvRealModelPositionError::Json)?;
        let canonical = canonical_json(&value).map_err(KvlabKvRealModelPositionError::Json)?;
        if canonical != json {
            return Err(KvlabKvRealModelPositionError::NonCanonicalJson);
        }

        let wire: EvidenceWire =
            serde_json::from_value(value).map_err(KvlabKvRealModelPositionError::Json)?;
        if wire.schema != KVLAB_KV_REAL_MODEL_POSITION_SCHEMA_V2 {
            return Err(KvlabKvRealModelPositionError::UnsupportedSchema);
        }
        for (field, value) in [
            ("experiment_id", wire.experiment_id.as_str()),
            ("model_id", wire.model_id.as_str()),
            ("model_revision", wire.model_revision.as_str()),
            ("tokenizer_revision", wire.tokenizer_revision.as_str()),
            ("runtime_backend", wire.runtime_backend.as_str()),
            ("runtime_revision", wire.runtime_revision.as_str()),
            ("evaluation_id", wire.evaluation_id.as_str()),
        ] {
            require_text(field, value)?;
        }
        if !is_lower_hex(&wire.run_repository_revision, 40) {
            return Err(KvlabKvRealModelPositionError::InvalidRunRevision);
        }
        for (field, digest) in [
            ("trace_sha256", wire.trace_sha256.as_str()),
            (
                "baseline_output_sha256",
                wire.baseline_output_sha256.as_str(),
            ),
            (
                "candidate_output_sha256",
                wire.candidate_output_sha256.as_str(),
            ),
        ] {
            if !is_lower_hex(digest, 64) {
                return Err(KvlabKvRealModelPositionError::InvalidSha256(field));
            }
        }

        let selection_json =
            canonical_json(&wire.selection).map_err(KvlabKvRealModelPositionError::Json)?;
        let selection = KvlabKvPositionHandoffV2::from_canonical_json(&selection_json)
            .map_err(KvlabKvRealModelPositionError::EmbeddedSelection)?;
        if wire.baseline_logical_kv_bytes != selection.outcome().logical_input_bytes() {
            return Err(KvlabKvRealModelPositionError::BaselineLogicalBytesMismatch);
        }
        if wire.candidate_logical_kv_bytes != selection.outcome().logical_retained_bytes() {
            return Err(KvlabKvRealModelPositionError::CandidateLogicalBytesMismatch);
        }
        if wire.metrics.is_empty() {
            return Err(KvlabKvRealModelPositionError::EmptyMetrics);
        }

        let metrics = parse_metrics(wire.metrics)?;
        Ok(Self {
            experiment_id: wire.experiment_id,
            run_repository_revision: wire.run_repository_revision,
            model_id: wire.model_id,
            model_revision: wire.model_revision,
            tokenizer_revision: wire.tokenizer_revision,
            runtime_backend: wire.runtime_backend,
            runtime_revision: wire.runtime_revision,
            evaluation_id: wire.evaluation_id,
            trace_sha256: wire.trace_sha256,
            seed: wire.seed,
            selection,
            baseline_output_sha256: wire.baseline_output_sha256,
            candidate_output_sha256: wire.candidate_output_sha256,
            baseline_logical_kv_bytes: wire.baseline_logical_kv_bytes,
            candidate_logical_kv_bytes: wire.candidate_logical_kv_bytes,
            metrics,
            content_fingerprint: format!("fnv1a64:{:016x}", fnv1a64(json.as_bytes())),
        })
    }

    #[must_use]
    pub const fn selection(&self) -> &KvlabKvPositionHandoffV2 {
        &self.selection
    }

    #[must_use]
    pub fn metrics(&self) -> &[ObservedMetricPair] {
        &self.metrics
    }

    pub fn evidence_source(&self) -> Result<EvidenceSource, KvlabKvRealModelPositionError> {
        EvidenceSource::new_with_nature(
            "KVLab/real-model-kv-position-selection",
            KVLAB_KV_REAL_MODEL_POSITION_REVISION,
            EvidenceNature::Observed,
        )
        .map_err(KvlabKvRealModelPositionError::Evidence)?
        .with_content_hash(self.content_fingerprint.clone())
        .map_err(KvlabKvRealModelPositionError::Evidence)
    }

    fn baseline_signature(&self) -> ObservedKvPositionSignature {
        ObservedKvPositionSignature {
            policy: None,
            output_sha256: self.baseline_output_sha256.clone(),
            logical_kv_bytes: self.baseline_logical_kv_bytes,
            retained_positions: (0..self.selection.state().token_ids().len()).collect(),
            retained_token_ids: self.selection.state().token_ids().to_vec(),
            metrics: self
                .metrics
                .iter()
                .map(ObservedMetricPair::baseline_metric)
                .collect(),
            evidence_content_fingerprint: self.content_fingerprint.clone(),
        }
    }

    fn candidate_signature(&self) -> ObservedKvPositionSignature {
        ObservedKvPositionSignature {
            policy: Some(self.selection.outcome().policy().to_owned()),
            output_sha256: self.candidate_output_sha256.clone(),
            logical_kv_bytes: self.candidate_logical_kv_bytes,
            retained_positions: self.selection.outcome().retained_positions().to_vec(),
            retained_token_ids: self.selection.outcome().retained_token_ids().to_vec(),
            metrics: self
                .metrics
                .iter()
                .map(ObservedMetricPair::candidate_metric)
                .collect(),
            evidence_content_fingerprint: self.content_fingerprint.clone(),
        }
    }
}

impl ObservedKvPositionComparison {
    pub fn new(
        records: Vec<KvlabKvRealModelPositionEvidenceV2>,
    ) -> Result<Self, ObservedKvPositionComparisonError> {
        let Some(first) = records.first() else {
            return Err(ObservedKvPositionComparisonError::EmptyEvidence);
        };
        let mut policies = BTreeSet::new();
        for record in &records {
            if !same_context(first, record) || first.selection.state() != record.selection.state() {
                return Err(ObservedKvPositionComparisonError::ContextMismatch);
            }
            if !same_baseline(first, record) {
                return Err(ObservedKvPositionComparisonError::BaselineMismatch);
            }
            if first.candidate_logical_kv_bytes != record.candidate_logical_kv_bytes {
                return Err(ObservedKvPositionComparisonError::BudgetMismatch);
            }
            let policy = record.selection.outcome().policy().to_owned();
            if !policies.insert(policy.clone()) {
                return Err(ObservedKvPositionComparisonError::DuplicatePolicy(policy));
            }
        }
        Ok(Self { records })
    }

    #[must_use]
    pub fn records(&self) -> &[KvlabKvRealModelPositionEvidenceV2] {
        &self.records
    }

    #[must_use]
    pub fn logical_budget_bytes(&self) -> u64 {
        self.records[0].candidate_logical_kv_bytes
    }

    #[must_use]
    pub fn baseline(&self) -> ObservedKvPositionSignature {
        self.records[0].baseline_signature()
    }

    pub fn candidate(
        &self,
        policy: &str,
    ) -> Result<ObservedKvPositionSignature, ObservedKvPositionComparisonError> {
        self.records
            .iter()
            .find(|record| record.selection.outcome().policy() == policy)
            .map(KvlabKvRealModelPositionEvidenceV2::candidate_signature)
            .ok_or_else(|| ObservedKvPositionComparisonError::MissingPolicy(policy.to_owned()))
    }
}

fn parse_metrics(
    wires: Vec<MetricWire>,
) -> Result<Vec<ObservedMetricPair>, KvlabKvRealModelPositionError> {
    let mut seen = BTreeSet::new();
    let mut metrics = Vec::with_capacity(wires.len());
    for wire in wires {
        require_text("metric.name", &wire.name)?;
        require_text("metric.unit", &wire.unit)?;
        if !seen.insert(wire.name.clone()) {
            return Err(KvlabKvRealModelPositionError::DuplicateMetric(wire.name));
        }
        let kind = ObservedMetricKind::parse(&wire.kind)?;
        let preference = MetricPreference::parse(&wire.preference)?;
        for (label, value) in [
            ("baseline_value", wire.baseline_value),
            ("candidate_value", wire.candidate_value),
            ("delta", wire.delta),
        ] {
            if !value.is_finite() {
                return Err(KvlabKvRealModelPositionError::NonFiniteMetric(format!(
                    "{}.{}",
                    wire.name, label
                )));
            }
        }
        let expected_delta = wire.candidate_value - wire.baseline_value;
        if !float_close(wire.delta, expected_delta) {
            return Err(KvlabKvRealModelPositionError::MetricDeltaMismatch(
                wire.name,
            ));
        }
        metrics.push(ObservedMetricPair {
            name: wire.name,
            kind,
            unit: wire.unit,
            preference,
            baseline_value: wire.baseline_value,
            candidate_value: wire.candidate_value,
            delta: wire.delta,
        });
    }
    metrics.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(metrics)
}

fn same_context(
    left: &KvlabKvRealModelPositionEvidenceV2,
    right: &KvlabKvRealModelPositionEvidenceV2,
) -> bool {
    left.experiment_id == right.experiment_id
        && left.run_repository_revision == right.run_repository_revision
        && left.model_id == right.model_id
        && left.model_revision == right.model_revision
        && left.tokenizer_revision == right.tokenizer_revision
        && left.runtime_backend == right.runtime_backend
        && left.runtime_revision == right.runtime_revision
        && left.evaluation_id == right.evaluation_id
        && left.trace_sha256 == right.trace_sha256
        && left.seed == right.seed
}

fn same_baseline(
    left: &KvlabKvRealModelPositionEvidenceV2,
    right: &KvlabKvRealModelPositionEvidenceV2,
) -> bool {
    left.baseline_output_sha256 == right.baseline_output_sha256
        && left.baseline_logical_kv_bytes == right.baseline_logical_kv_bytes
        && left.metrics.len() == right.metrics.len()
        && left
            .metrics
            .iter()
            .zip(&right.metrics)
            .all(|(left, right)| {
                left.name == right.name
                    && left.kind == right.kind
                    && left.unit == right.unit
                    && left.preference == right.preference
                    && left.baseline_value.to_bits() == right.baseline_value.to_bits()
            })
}

fn float_close(left: f64, right: f64) -> bool {
    // Finite inputs can still produce an infinite derived delta. Without this
    // guard the relative comparison can become infinity <= infinity.
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
    let difference = (left - right).abs();
    difference <= FLOAT_ABS_TOLERANCE
        || difference <= FLOAT_REL_TOLERANCE * left.abs().max(right.abs())
}

fn require_text(field: &'static str, value: &str) -> Result<(), KvlabKvRealModelPositionError> {
    if value.trim().is_empty() {
        return Err(KvlabKvRealModelPositionError::EmptyText(field));
    }
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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

impl fmt::Display for KvlabKvRealModelPositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid observed KV position JSON: {error}"),
            Self::NonCanonicalJson => {
                formatter.write_str("observed KV position JSON is not canonical")
            }
            Self::UnsupportedSchema => {
                formatter.write_str("unsupported observed KV position schema")
            }
            Self::EmptyText(field) => write!(formatter, "{field} must not be empty"),
            Self::InvalidRunRevision => {
                formatter.write_str("run_repository_revision must be a lowercase full Git SHA")
            }
            Self::InvalidSha256(field) => {
                write!(formatter, "{field} must be a lowercase SHA-256 digest")
            }
            Self::EmbeddedSelection(error) => {
                write!(formatter, "invalid embedded selection: {error}")
            }
            Self::BaselineLogicalBytesMismatch => formatter
                .write_str("baseline logical KV bytes do not match position-selection input"),
            Self::CandidateLogicalBytesMismatch => formatter
                .write_str("candidate logical KV bytes do not match position-selection retention"),
            Self::EmptyMetrics => formatter.write_str("observed evidence must contain metrics"),
            Self::DuplicateMetric(name) => write!(formatter, "duplicate observed metric {name}"),
            Self::UnknownMetricKind(kind) => {
                write!(formatter, "unknown observed metric kind {kind}")
            }
            Self::UnknownMetricPreference(preference) => {
                write!(formatter, "unknown observed metric preference {preference}")
            }
            Self::NonFiniteMetric(name) => write!(formatter, "non-finite observed metric {name}"),
            Self::MetricDeltaMismatch(name) => {
                write!(
                    formatter,
                    "observed metric delta does not replay for {name}"
                )
            }
            Self::Evidence(error) => write!(formatter, "invalid evidence source: {error}"),
        }
    }
}

impl std::error::Error for KvlabKvRealModelPositionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::EmbeddedSelection(error) => Some(error),
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for ObservedKvPositionComparisonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEvidence => formatter.write_str("observed comparison requires evidence"),
            Self::ContextMismatch => formatter.write_str("observed comparison context mismatch"),
            Self::BaselineMismatch => formatter.write_str("observed comparison baseline mismatch"),
            Self::BudgetMismatch => formatter.write_str("observed comparison budget mismatch"),
            Self::DuplicatePolicy(policy) => {
                write!(formatter, "duplicate observed comparison policy {policy}")
            }
            Self::MissingPolicy(policy) => {
                write!(formatter, "missing observed comparison policy {policy}")
            }
        }
    }
}

impl std::error::Error for ObservedKvPositionComparisonError {}

#[cfg(test)]
mod tests {
    use prospect_evidence::EvidenceNature;
    use serde_json::{Value, json};

    use super::{KvlabKvRealModelPositionEvidenceV2, ObservedKvPositionComparison, canonical_json};

    fn fixture(policy: &str, retained: &[usize], candidate_hash: char, accuracy: f64) -> String {
        let input = [7_u64, 11, 7, 7, 19];
        let retained_set = retained
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let evicted = (0..input.len())
            .filter(|position| !retained_set.contains(position))
            .collect::<Vec<_>>();
        let retained_bytes = u64::try_from(retained.len()).unwrap() * 64;
        canonical_json(&json!({
            "schema":"kvlab.prospect-kv-real-model-selection/v2",
            "experiment_id":"position-campaign",
            "run_repository_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "model_id":"example/model",
            "model_revision":"model-r1",
            "tokenizer_revision":"tok-r1",
            "runtime_backend":"nnis-kvlab-v4",
            "runtime_revision":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "evaluation_id":"eval-001",
            "trace_sha256":"1111111111111111111111111111111111111111111111111111111111111111",
            "seed":7,
            "selection":{
                "schema":"kvlab.prospect-kv-selection/v2",
                "policy":policy,
                "input_token_ids":input,
                "bytes_per_token":64,
                "retained_positions":retained,
                "evicted_positions":evicted,
                "logical_input_bytes":320,
                "logical_retained_bytes":retained_bytes,
                "logical_evicted_bytes":320-retained_bytes
            },
            "baseline_output_sha256":"2222222222222222222222222222222222222222222222222222222222222222",
            "candidate_output_sha256":candidate_hash.to_string().repeat(64),
            "baseline_logical_kv_bytes":320,
            "candidate_logical_kv_bytes":retained_bytes,
            "metrics":[
                {
                    "name":"mean_nll",
                    "kind":"quality",
                    "unit":"nat_per_token",
                    "preference":"lower_is_better",
                    "baseline_value":1.0,
                    "candidate_value":1.25,
                    "delta":0.25
                },
                {
                    "name":"token_accuracy",
                    "kind":"quality",
                    "unit":"ratio",
                    "preference":"higher_is_better",
                    "baseline_value":1.0,
                    "candidate_value":accuracy,
                    "delta":accuracy-1.0
                }
            ]
        }))
        .unwrap()
    }

    #[test]
    fn delta_guard_rejects_overflow_through_public_evidence_parser() {
        for (baseline, candidate) in [(-1.0e308_f64, 1.0e308_f64), (1.0e308_f64, -1.0e308_f64)] {
            assert!(baseline.is_finite() && candidate.is_finite());
            assert!(!(candidate - baseline).is_finite());
            let mut value: Value =
                serde_json::from_str(&fixture("fixture", &[0, 2, 4], '3', 0.75)).unwrap();
            value["metrics"][0]["kind"] = json!("numerical");
            value["metrics"][0]["baseline_value"] = json!(baseline);
            value["metrics"][0]["candidate_value"] = json!(candidate);
            value["metrics"][0]["delta"] = json!(0.0);
            let result = KvlabKvRealModelPositionEvidenceV2::from_canonical_json(
                &canonical_json(&value).unwrap(),
            );
            assert!(
                matches!(
                    result,
                    Err(super::KvlabKvRealModelPositionError::MetricDeltaMismatch(_))
                ),
                "non-finite derived delta admitted: {result:?}"
            );
        }
    }

    #[test]
    fn delta_guard_rejects_nonfinite_comparison_operands() {
        for invalid in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            for finite in [0.0, 1.0, -1.0, 1.0e308] {
                assert!(!super::float_close(finite, invalid));
                assert!(!super::float_close(invalid, finite));
            }
            assert!(!super::float_close(invalid, invalid));
        }
    }

    #[test]
    fn delta_guard_preserves_large_finite_differences() {
        let mut value: Value =
            serde_json::from_str(&fixture("fixture", &[0, 2, 4], '3', 0.75)).unwrap();
        value["metrics"][0]["baseline_value"] = json!(1.0e307);
        value["metrics"][0]["candidate_value"] = json!(2.0e307);
        value["metrics"][0]["delta"] = json!(1.0e307);
        assert!(
            KvlabKvRealModelPositionEvidenceV2::from_canonical_json(
                &canonical_json(&value).unwrap()
            )
            .is_ok()
        );
    }

    #[test]
    fn delta_guard_preserves_existing_finite_tolerances() {
        assert!(super::float_close(0.0, 5.0e-13));
        assert!(super::float_close(1.0e6, 1.0e6 + 5.0e-7));
        assert!(!super::float_close(0.0, 1.0e-6));
        assert!(!super::float_close(1.0e6, 1.0e6 + 1.0));
    }

    #[test]
    fn parses_observed_position_evidence_with_repeated_tokens() {
        let record = KvlabKvRealModelPositionEvidenceV2::from_canonical_json(&fixture(
            "fixture",
            &[0, 2, 4],
            '3',
            0.75,
        ))
        .unwrap();
        assert_eq!(record.selection().state().token_ids(), &[7, 11, 7, 7, 19]);
        assert_eq!(
            record.selection().outcome().retained_positions(),
            &[0, 2, 4]
        );
        assert_eq!(
            record.selection().outcome().retained_token_ids(),
            &[7, 7, 19]
        );
        assert_eq!(
            record.evidence_source().unwrap().nature(),
            EvidenceNature::Observed
        );
        assert_eq!(record.metrics().len(), 2);
    }

    #[test]
    fn rejects_accounting_or_delta_tampering() {
        let original = fixture("fixture", &[0, 2, 4], '3', 0.75);
        let mut value: Value = serde_json::from_str(&original).unwrap();
        value["candidate_logical_kv_bytes"] = json!(128);
        assert!(
            KvlabKvRealModelPositionEvidenceV2::from_canonical_json(
                &canonical_json(&value).unwrap()
            )
            .is_err()
        );

        let mut value: Value = serde_json::from_str(&original).unwrap();
        value["metrics"][0]["delta"] = json!(0.5);
        assert!(
            KvlabKvRealModelPositionEvidenceV2::from_canonical_json(
                &canonical_json(&value).unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn compares_budget_matched_policies_without_ranking_them() {
        let left = KvlabKvRealModelPositionEvidenceV2::from_canonical_json(&fixture(
            "lru",
            &[0, 2, 4],
            '3',
            0.75,
        ))
        .unwrap();
        let right = KvlabKvRealModelPositionEvidenceV2::from_canonical_json(&fixture(
            "magnitude",
            &[1, 2, 4],
            '4',
            0.8,
        ))
        .unwrap();
        let comparison = ObservedKvPositionComparison::new(vec![left, right]).unwrap();
        assert_eq!(comparison.logical_budget_bytes(), 192);
        assert_eq!(comparison.baseline().retained_positions(), &[0, 1, 2, 3, 4]);
        assert_eq!(
            comparison.candidate("lru").unwrap().retained_positions(),
            &[0, 2, 4]
        );
        assert_eq!(
            comparison
                .candidate("magnitude")
                .unwrap()
                .retained_token_ids(),
            &[11, 7, 19]
        );
    }

    #[test]
    fn comparison_rejects_budget_or_baseline_drift() {
        let left = KvlabKvRealModelPositionEvidenceV2::from_canonical_json(&fixture(
            "lru",
            &[0, 2, 4],
            '3',
            0.75,
        ))
        .unwrap();
        let smaller = KvlabKvRealModelPositionEvidenceV2::from_canonical_json(&fixture(
            "magnitude",
            &[0, 4],
            '4',
            0.8,
        ))
        .unwrap();
        assert!(ObservedKvPositionComparison::new(vec![left.clone(), smaller]).is_err());

        let mut value: Value =
            serde_json::from_str(&fixture("magnitude", &[1, 2, 4], '4', 0.8)).unwrap();
        value["baseline_output_sha256"] = json!("5".repeat(64));
        let drifted = KvlabKvRealModelPositionEvidenceV2::from_canonical_json(
            &canonical_json(&value).unwrap(),
        )
        .unwrap();
        assert!(ObservedKvPositionComparison::new(vec![left, drifted]).is_err());
    }
}
