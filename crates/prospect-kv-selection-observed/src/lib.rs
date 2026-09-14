#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeSet;

use prospect_evidence::{EvidenceError, EvidenceNature, EvidenceSource};
use prospect_kv_selection::{KvSelectionContractError, KvlabKvSelectionHandoffV1};
use serde::Deserialize;
use serde_json::Value;

pub const KVLAB_KV_REAL_MODEL_SELECTION_SCHEMA_V1: &str =
    "kvlab.prospect-kv-real-model-selection/v1";
pub const KVLAB_KV_REAL_MODEL_SELECTION_REVISION: &str = "0e7274bf565d9079943845ea4eb0699a525b1db1";

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
pub struct ObservedKvSelectionSignature {
    policy: Option<String>,
    output_sha256: String,
    logical_kv_bytes: u64,
    retained_token_ids: Vec<u64>,
    metrics: Vec<ObservedMetricValue>,
    evidence_content_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KvlabKvRealModelSelectionEvidenceV1 {
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
    selection: KvlabKvSelectionHandoffV1,
    baseline_output_sha256: String,
    candidate_output_sha256: String,
    baseline_logical_kv_bytes: u64,
    candidate_logical_kv_bytes: u64,
    metrics: Vec<ObservedMetricPair>,
    content_fingerprint: String,
}

#[derive(Clone, Debug)]
pub struct ObservedKvSelectionComparison {
    records: Vec<KvlabKvRealModelSelectionEvidenceV1>,
}

#[derive(Debug)]
pub enum KvlabKvRealModelSelectionError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    EmptyText(&'static str),
    InvalidRunRevision,
    InvalidSha256(&'static str),
    EmbeddedSelection(KvSelectionContractError),
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
pub enum ObservedKvSelectionComparisonError {
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
    fn parse(value: &str) -> Result<Self, KvlabKvRealModelSelectionError> {
        match value {
            "numerical" => Ok(Self::Numerical),
            "quality" => Ok(Self::Quality),
            other => Err(KvlabKvRealModelSelectionError::UnknownMetricKind(
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
    fn parse(value: &str) -> Result<Self, KvlabKvRealModelSelectionError> {
        match value {
            "higher_is_better" => Ok(Self::HigherIsBetter),
            "lower_is_better" => Ok(Self::LowerIsBetter),
            "none" => Ok(Self::None),
            other => Err(KvlabKvRealModelSelectionError::UnknownMetricPreference(
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

impl ObservedKvSelectionSignature {
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

impl KvlabKvRealModelSelectionEvidenceV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvlabKvRealModelSelectionError> {
        let value: Value =
            serde_json::from_str(json).map_err(KvlabKvRealModelSelectionError::Json)?;
        let canonical = canonical_json(&value).map_err(KvlabKvRealModelSelectionError::Json)?;
        if canonical != json {
            return Err(KvlabKvRealModelSelectionError::NonCanonicalJson);
        }

        let wire: EvidenceWire =
            serde_json::from_value(value).map_err(KvlabKvRealModelSelectionError::Json)?;
        if wire.schema != KVLAB_KV_REAL_MODEL_SELECTION_SCHEMA_V1 {
            return Err(KvlabKvRealModelSelectionError::UnsupportedSchema);
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
            return Err(KvlabKvRealModelSelectionError::InvalidRunRevision);
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
                return Err(KvlabKvRealModelSelectionError::InvalidSha256(field));
            }
        }

        let selection_json =
            canonical_json(&wire.selection).map_err(KvlabKvRealModelSelectionError::Json)?;
        let selection = KvlabKvSelectionHandoffV1::from_canonical_json(&selection_json)
            .map_err(KvlabKvRealModelSelectionError::EmbeddedSelection)?;
        if wire.baseline_logical_kv_bytes != selection.outcome().logical_input_bytes() {
            return Err(KvlabKvRealModelSelectionError::BaselineLogicalBytesMismatch);
        }
        if wire.candidate_logical_kv_bytes != selection.outcome().logical_retained_bytes() {
            return Err(KvlabKvRealModelSelectionError::CandidateLogicalBytesMismatch);
        }
        if wire.metrics.is_empty() {
            return Err(KvlabKvRealModelSelectionError::EmptyMetrics);
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
    pub const fn selection(&self) -> &KvlabKvSelectionHandoffV1 {
        &self.selection
    }

    #[must_use]
    pub fn metrics(&self) -> &[ObservedMetricPair] {
        &self.metrics
    }

    pub fn evidence_source(&self) -> Result<EvidenceSource, KvlabKvRealModelSelectionError> {
        EvidenceSource::new_with_nature(
            "KVLab/real-model-kv-selection",
            KVLAB_KV_REAL_MODEL_SELECTION_REVISION,
            EvidenceNature::Observed,
        )
        .map_err(KvlabKvRealModelSelectionError::Evidence)?
        .with_content_hash(self.content_fingerprint.clone())
        .map_err(KvlabKvRealModelSelectionError::Evidence)
    }

    fn baseline_signature(&self) -> ObservedKvSelectionSignature {
        ObservedKvSelectionSignature {
            policy: None,
            output_sha256: self.baseline_output_sha256.clone(),
            logical_kv_bytes: self.baseline_logical_kv_bytes,
            retained_token_ids: self.selection.state().token_ids().to_vec(),
            metrics: self
                .metrics
                .iter()
                .map(ObservedMetricPair::baseline_metric)
                .collect(),
            evidence_content_fingerprint: self.content_fingerprint.clone(),
        }
    }

    fn candidate_signature(&self) -> ObservedKvSelectionSignature {
        ObservedKvSelectionSignature {
            policy: Some(self.selection.outcome().policy().to_owned()),
            output_sha256: self.candidate_output_sha256.clone(),
            logical_kv_bytes: self.candidate_logical_kv_bytes,
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

impl ObservedKvSelectionComparison {
    pub fn new(
        records: Vec<KvlabKvRealModelSelectionEvidenceV1>,
    ) -> Result<Self, ObservedKvSelectionComparisonError> {
        let Some(first) = records.first() else {
            return Err(ObservedKvSelectionComparisonError::EmptyEvidence);
        };
        let mut policies = BTreeSet::new();
        for record in &records {
            if !same_context(first, record) || first.selection.state() != record.selection.state() {
                return Err(ObservedKvSelectionComparisonError::ContextMismatch);
            }
            if !same_baseline(first, record) {
                return Err(ObservedKvSelectionComparisonError::BaselineMismatch);
            }
            if first.candidate_logical_kv_bytes != record.candidate_logical_kv_bytes {
                return Err(ObservedKvSelectionComparisonError::BudgetMismatch);
            }
            let policy = record.selection.outcome().policy().to_owned();
            if !policies.insert(policy.clone()) {
                return Err(ObservedKvSelectionComparisonError::DuplicatePolicy(policy));
            }
        }
        Ok(Self { records })
    }

    #[must_use]
    pub fn records(&self) -> &[KvlabKvRealModelSelectionEvidenceV1] {
        &self.records
    }

    #[must_use]
    pub fn logical_budget_bytes(&self) -> u64 {
        self.records[0].candidate_logical_kv_bytes
    }

    #[must_use]
    pub fn baseline(&self) -> ObservedKvSelectionSignature {
        self.records[0].baseline_signature()
    }

    pub fn candidate(
        &self,
        policy: &str,
    ) -> Result<ObservedKvSelectionSignature, ObservedKvSelectionComparisonError> {
        self.records
            .iter()
            .find(|record| record.selection.outcome().policy() == policy)
            .map(KvlabKvRealModelSelectionEvidenceV1::candidate_signature)
            .ok_or_else(|| ObservedKvSelectionComparisonError::MissingPolicy(policy.to_owned()))
    }
}

fn parse_metrics(
    wires: Vec<MetricWire>,
) -> Result<Vec<ObservedMetricPair>, KvlabKvRealModelSelectionError> {
    let mut seen = BTreeSet::new();
    let mut metrics = Vec::with_capacity(wires.len());
    for metric in wires {
        require_text("metric.name", &metric.name)?;
        require_text("metric.unit", &metric.unit)?;
        if !seen.insert(metric.name.clone()) {
            return Err(KvlabKvRealModelSelectionError::DuplicateMetric(metric.name));
        }
        if !metric.baseline_value.is_finite()
            || !metric.candidate_value.is_finite()
            || !metric.delta.is_finite()
        {
            return Err(KvlabKvRealModelSelectionError::NonFiniteMetric(metric.name));
        }
        if !nearly_equal(metric.delta, metric.candidate_value - metric.baseline_value) {
            return Err(KvlabKvRealModelSelectionError::MetricDeltaMismatch(
                metric.name,
            ));
        }
        metrics.push(ObservedMetricPair {
            name: metric.name,
            kind: ObservedMetricKind::parse(&metric.kind)?,
            unit: metric.unit,
            preference: MetricPreference::parse(&metric.preference)?,
            baseline_value: metric.baseline_value,
            candidate_value: metric.candidate_value,
            delta: metric.delta,
        });
    }
    Ok(metrics)
}

fn same_context(
    left: &KvlabKvRealModelSelectionEvidenceV1,
    right: &KvlabKvRealModelSelectionEvidenceV1,
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
    left: &KvlabKvRealModelSelectionEvidenceV1,
    right: &KvlabKvRealModelSelectionEvidenceV1,
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
                    && nearly_equal(left.baseline_value, right.baseline_value)
            })
}

fn require_text(field: &'static str, value: &str) -> Result<(), KvlabKvRealModelSelectionError> {
    if value.trim().is_empty() {
        return Err(KvlabKvRealModelSelectionError::EmptyText(field));
    }
    Ok(())
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

impl fmt::Display for KvlabKvRealModelSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => {
                write!(formatter, "invalid KVLab selection evidence JSON: {error}")
            }
            Self::NonCanonicalJson => {
                formatter.write_str("KVLab selection evidence JSON is not canonical")
            }
            Self::UnsupportedSchema => {
                formatter.write_str("unsupported KVLab selection evidence schema")
            }
            Self::EmptyText(field) => write!(
                formatter,
                "KVLab selection evidence field {field} must not be empty"
            ),
            Self::InvalidRunRevision => {
                formatter.write_str("KVLab run revision must be a lowercase full Git SHA")
            }
            Self::InvalidSha256(field) => write!(
                formatter,
                "KVLab selection evidence field {field} must be a lowercase SHA-256 digest"
            ),
            Self::EmbeddedSelection(error) => {
                write!(formatter, "invalid embedded KV selection: {error}")
            }
            Self::BaselineLogicalBytesMismatch => {
                formatter.write_str("baseline logical KV bytes do not match selection input")
            }
            Self::CandidateLogicalBytesMismatch => {
                formatter.write_str("candidate logical KV bytes do not match selection retention")
            }
            Self::EmptyMetrics => {
                formatter.write_str("selection evidence must contain at least one metric")
            }
            Self::DuplicateMetric(name) => write!(formatter, "duplicate observed metric {name}"),
            Self::UnknownMetricKind(kind) => {
                write!(formatter, "unknown observed metric kind {kind}")
            }
            Self::UnknownMetricPreference(preference) => {
                write!(formatter, "unknown observed metric preference {preference}")
            }
            Self::NonFiniteMetric(name) => write!(
                formatter,
                "observed metric {name} contains a non-finite value"
            ),
            Self::MetricDeltaMismatch(name) => {
                write!(formatter, "observed metric {name} delta is inconsistent")
            }
            Self::Evidence(error) => write!(formatter, "invalid evidence source: {error}"),
        }
    }
}

impl std::error::Error for KvlabKvRealModelSelectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::EmbeddedSelection(error) => Some(error),
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for ObservedKvSelectionComparisonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEvidence => formatter.write_str("observed KV selection comparison is empty"),
            Self::ContextMismatch => {
                formatter.write_str("observed KV selections do not share one experimental context")
            }
            Self::BaselineMismatch => {
                formatter.write_str("observed KV selections do not share one paired baseline")
            }
            Self::BudgetMismatch => {
                formatter.write_str("observed KV selections do not share one logical byte budget")
            }
            Self::DuplicatePolicy(policy) => {
                write!(formatter, "duplicate observed KV selection policy {policy}")
            }
            Self::MissingPolicy(policy) => write!(
                formatter,
                "no observed KV selection evidence exists for policy {policy}"
            ),
        }
    }
}

impl std::error::Error for ObservedKvSelectionComparisonError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use prospect_evidence::EvidenceNature;
    use serde_json::{Value, json};

    use super::{
        KvlabKvRealModelSelectionEvidenceV1, ObservedKvSelectionComparison,
        ObservedKvSelectionComparisonError, canonical_json,
    };

    fn record_json(policy: &str, retained: &[u64], candidate_hash: char, accuracy: f64) -> String {
        let input = [10_u64, 11, 12, 13];
        let retained_set = retained.iter().copied().collect::<BTreeSet<_>>();
        let evicted = input
            .iter()
            .copied()
            .filter(|token_id| !retained_set.contains(token_id))
            .collect::<Vec<_>>();
        let retained_bytes = u64::try_from(retained.len()).unwrap() * 64;
        canonical_json(&json!({
            "schema":"kvlab.prospect-kv-real-model-selection/v1",
            "experiment_id":"real-selection-c1",
            "run_repository_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "model_id":"example/model",
            "model_revision":"model-r1",
            "tokenizer_revision":"tok-r1",
            "runtime_backend":"fixture-runtime",
            "runtime_revision":"runtime-r1",
            "evaluation_id":"holdout-001",
            "trace_sha256":"1111111111111111111111111111111111111111111111111111111111111111",
            "seed":7,
            "selection":{
                "schema":"kvlab.prospect-kv-selection/v1",
                "policy":policy,
                "input_token_ids":input,
                "bytes_per_token":64,
                "retained_token_ids":retained,
                "evicted_token_ids":evicted,
                "logical_input_bytes":256,
                "logical_retained_bytes":retained_bytes,
                "logical_evicted_bytes":256-retained_bytes
            },
            "baseline_output_sha256":"2222222222222222222222222222222222222222222222222222222222222222",
            "candidate_output_sha256":candidate_hash.to_string().repeat(64),
            "baseline_logical_kv_bytes":256,
            "candidate_logical_kv_bytes":retained_bytes,
            "metrics":[{
                "name":"token_accuracy",
                "kind":"quality",
                "unit":"ratio",
                "preference":"higher_is_better",
                "baseline_value":0.8,
                "candidate_value":accuracy,
                "delta":accuracy-0.8
            }]
        }))
        .unwrap()
    }

    #[test]
    fn parses_explicit_observed_selection_and_marks_source_observed() {
        let record = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "lru",
            &[10, 12],
            '3',
            0.78,
        ))
        .unwrap();
        assert_eq!(record.selection().outcome().policy(), "lru");
        assert_eq!(record.selection().outcome().retained_token_ids(), &[10, 12]);
        assert_eq!(
            record.evidence_source().unwrap().nature(),
            EvidenceNature::Observed
        );
    }

    #[test]
    fn builds_budget_matched_policy_comparison_without_ranking() {
        let lru = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "lru",
            &[10, 12],
            '3',
            0.78,
        ))
        .unwrap();
        let magnitude = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "magnitude",
            &[11, 13],
            '4',
            0.76,
        ))
        .unwrap();
        let comparison = ObservedKvSelectionComparison::new(vec![lru, magnitude]).unwrap();
        assert_eq!(comparison.logical_budget_bytes(), 128);
        assert_eq!(comparison.baseline().policy(), None);
        assert_eq!(comparison.candidate("lru").unwrap().policy(), Some("lru"));
        assert!(matches!(
            comparison.candidate("random"),
            Err(ObservedKvSelectionComparisonError::MissingPolicy(policy)) if policy == "random"
        ));
    }

    #[test]
    fn rejects_budget_or_context_drift() {
        let lru = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "lru",
            &[10, 12],
            '3',
            0.78,
        ))
        .unwrap();
        let smaller = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "magnitude",
            &[13],
            '4',
            0.70,
        ))
        .unwrap();
        assert!(matches!(
            ObservedKvSelectionComparison::new(vec![lru.clone(), smaller]),
            Err(ObservedKvSelectionComparisonError::BudgetMismatch)
        ));

        let mut value: Value =
            serde_json::from_str(&record_json("magnitude", &[11, 13], '4', 0.76)).unwrap();
        value["model_revision"] = json!("model-r2");
        let drifted = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(
            &canonical_json(&value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            ObservedKvSelectionComparison::new(vec![lru, drifted]),
            Err(ObservedKvSelectionComparisonError::ContextMismatch)
        ));
    }
}
