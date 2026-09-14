#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeSet;

use prospect_evidence::{EvidenceError, EvidenceNature, EvidenceSource};
use prospect_kv_selection::{
    KvSelectionContractError, KvlabKvSelectionHandoffV1,
};
use serde::Deserialize;
use serde_json::Value;

pub const KVLAB_KV_REAL_MODEL_SELECTION_SCHEMA_V1: &str =
    "kvlab.prospect-kv-real-model-selection/v1";
pub const KVLAB_KV_REAL_MODEL_SELECTION_REVISION: &str =
    "0e7274bf565d9079943845ea4eb0699a525b1db1";

const FLOAT_ABS_TOLERANCE: f64 = 1.0e-12;
const FLOAT_REL_TOLERANCE: f64 = 1.0e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObservedSelectionMetricKind {
    Numerical,
    Quality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObservedSelectionMetricPreference {
    HigherIsBetter,
    LowerIsBetter,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedSelectionMetricPair {
    name: String,
    kind: ObservedSelectionMetricKind,
    unit: String,
    preference: ObservedSelectionMetricPreference,
    baseline_value: f64,
    candidate_value: f64,
    delta: f64,
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
    metrics: Vec<ObservedSelectionMetricPair>,
    content_fingerprint: String,
}

#[derive(Clone, Debug)]
pub struct ComparableObservedKvSelectionSet {
    records: Vec<KvlabKvRealModelSelectionEvidenceV1>,
}

#[derive(Debug)]
pub enum KvRealModelSelectionEvidenceError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    EmptyText(&'static str),
    InvalidRunRevision,
    InvalidSha256(&'static str),
    InvalidSeed,
    Selection(KvSelectionContractError),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComparableObservedKvSelectionError {
    EmptySet,
    ContextMismatch,
    InputMismatch,
    BaselineMismatch,
    BudgetMismatch,
    DuplicatePolicy,
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

impl ObservedSelectionMetricKind {
    fn parse(value: &str) -> Result<Self, KvRealModelSelectionEvidenceError> {
        match value {
            "numerical" => Ok(Self::Numerical),
            "quality" => Ok(Self::Quality),
            other => Err(KvRealModelSelectionEvidenceError::UnknownMetricKind(
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

impl ObservedSelectionMetricPreference {
    fn parse(value: &str) -> Result<Self, KvRealModelSelectionEvidenceError> {
        match value {
            "higher_is_better" => Ok(Self::HigherIsBetter),
            "lower_is_better" => Ok(Self::LowerIsBetter),
            "none" => Ok(Self::None),
            other => Err(
                KvRealModelSelectionEvidenceError::UnknownMetricPreference(other.to_owned()),
            ),
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

impl ObservedSelectionMetricPair {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> ObservedSelectionMetricKind {
        self.kind
    }

    #[must_use]
    pub fn unit(&self) -> &str {
        &self.unit
    }

    #[must_use]
    pub const fn preference(&self) -> ObservedSelectionMetricPreference {
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
}

impl KvlabKvRealModelSelectionEvidenceV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvRealModelSelectionEvidenceError> {
        let value: Value =
            serde_json::from_str(json).map_err(KvRealModelSelectionEvidenceError::Json)?;
        let canonical = canonical_json(&value).map_err(KvRealModelSelectionEvidenceError::Json)?;
        if canonical != json {
            return Err(KvRealModelSelectionEvidenceError::NonCanonicalJson);
        }

        let wire: EvidenceWire =
            serde_json::from_value(value).map_err(KvRealModelSelectionEvidenceError::Json)?;
        if wire.schema != KVLAB_KV_REAL_MODEL_SELECTION_SCHEMA_V1 {
            return Err(KvRealModelSelectionEvidenceError::UnsupportedSchema);
        }
        for (field, text) in [
            ("experiment_id", wire.experiment_id.as_str()),
            ("model_id", wire.model_id.as_str()),
            ("model_revision", wire.model_revision.as_str()),
            ("tokenizer_revision", wire.tokenizer_revision.as_str()),
            ("runtime_backend", wire.runtime_backend.as_str()),
            ("runtime_revision", wire.runtime_revision.as_str()),
            ("evaluation_id", wire.evaluation_id.as_str()),
        ] {
            require_text(field, text)?;
        }
        if !is_lower_hex(&wire.run_repository_revision, 40) {
            return Err(KvRealModelSelectionEvidenceError::InvalidRunRevision);
        }
        if wire.seed > u64::MAX {
            return Err(KvRealModelSelectionEvidenceError::InvalidSeed);
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
                return Err(KvRealModelSelectionEvidenceError::InvalidSha256(field));
            }
        }

        let selection_json =
            canonical_json(&wire.selection).map_err(KvRealModelSelectionEvidenceError::Json)?;
        let selection = KvlabKvSelectionHandoffV1::from_canonical_json(&selection_json)
            .map_err(KvRealModelSelectionEvidenceError::Selection)?;
        if wire.baseline_logical_kv_bytes != selection.logical_input_bytes() {
            return Err(KvRealModelSelectionEvidenceError::BaselineLogicalBytesMismatch);
        }
        if wire.candidate_logical_kv_bytes != selection.logical_retained_bytes() {
            return Err(KvRealModelSelectionEvidenceError::CandidateLogicalBytesMismatch);
        }
        if wire.metrics.is_empty() {
            return Err(KvRealModelSelectionEvidenceError::EmptyMetrics);
        }

        let mut seen = BTreeSet::new();
        let mut metrics = Vec::with_capacity(wire.metrics.len());
        for metric in wire.metrics {
            require_text("metric.name", &metric.name)?;
            require_text("metric.unit", &metric.unit)?;
            if !seen.insert(metric.name.clone()) {
                return Err(KvRealModelSelectionEvidenceError::DuplicateMetric(metric.name));
            }
            if !metric.baseline_value.is_finite()
                || !metric.candidate_value.is_finite()
                || !metric.delta.is_finite()
            {
                return Err(KvRealModelSelectionEvidenceError::NonFiniteMetric(metric.name));
            }
            let expected_delta = metric.candidate_value - metric.baseline_value;
            if !nearly_equal(metric.delta, expected_delta) {
                return Err(KvRealModelSelectionEvidenceError::MetricDeltaMismatch(
                    metric.name,
                ));
            }
            metrics.push(ObservedSelectionMetricPair {
                name: metric.name,
                kind: ObservedSelectionMetricKind::parse(&metric.kind)?,
                unit: metric.unit,
                preference: ObservedSelectionMetricPreference::parse(&metric.preference)?,
                baseline_value: metric.baseline_value,
                candidate_value: metric.candidate_value,
                delta: metric.delta,
            });
        }

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
    pub fn experiment_id(&self) -> &str {
        &self.experiment_id
    }

    #[must_use]
    pub fn run_repository_revision(&self) -> &str {
        &self.run_repository_revision
    }

    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    #[must_use]
    pub fn model_revision(&self) -> &str {
        &self.model_revision
    }

    #[must_use]
    pub fn tokenizer_revision(&self) -> &str {
        &self.tokenizer_revision
    }

    #[must_use]
    pub fn runtime_backend(&self) -> &str {
        &self.runtime_backend
    }

    #[must_use]
    pub fn runtime_revision(&self) -> &str {
        &self.runtime_revision
    }

    #[must_use]
    pub fn evaluation_id(&self) -> &str {
        &self.evaluation_id
    }

    #[must_use]
    pub fn trace_sha256(&self) -> &str {
        &self.trace_sha256
    }

    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    #[must_use]
    pub const fn selection(&self) -> &KvlabKvSelectionHandoffV1 {
        &self.selection
    }

    #[must_use]
    pub fn baseline_output_sha256(&self) -> &str {
        &self.baseline_output_sha256
    }

    #[must_use]
    pub fn candidate_output_sha256(&self) -> &str {
        &self.candidate_output_sha256
    }

    #[must_use]
    pub const fn baseline_logical_kv_bytes(&self) -> u64 {
        self.baseline_logical_kv_bytes
    }

    #[must_use]
    pub const fn candidate_logical_kv_bytes(&self) -> u64 {
        self.candidate_logical_kv_bytes
    }

    #[must_use]
    pub fn metrics(&self) -> &[ObservedSelectionMetricPair] {
        &self.metrics
    }

    #[must_use]
    pub fn content_fingerprint(&self) -> &str {
        &self.content_fingerprint
    }

    pub fn evidence_source(&self) -> Result<EvidenceSource, KvRealModelSelectionEvidenceError> {
        EvidenceSource::new_with_nature(
            "KVLab/real-model-kv-selection",
            KVLAB_KV_REAL_MODEL_SELECTION_REVISION,
            EvidenceNature::Observed,
        )
        .map_err(KvRealModelSelectionEvidenceError::Evidence)?
        .with_content_hash(self.content_fingerprint.clone())
        .map_err(KvRealModelSelectionEvidenceError::Evidence)
    }
}

impl ComparableObservedKvSelectionSet {
    pub fn new(
        records: Vec<KvlabKvRealModelSelectionEvidenceV1>,
    ) -> Result<Self, ComparableObservedKvSelectionError> {
        let Some(first) = records.first() else {
            return Err(ComparableObservedKvSelectionError::EmptySet);
        };
        let mut policies = BTreeSet::new();
        for record in &records {
            if !same_context(first, record) {
                return Err(ComparableObservedKvSelectionError::ContextMismatch);
            }
            if !same_input(first, record) {
                return Err(ComparableObservedKvSelectionError::InputMismatch);
            }
            if !same_baseline(first, record) {
                return Err(ComparableObservedKvSelectionError::BaselineMismatch);
            }
            if record.candidate_logical_kv_bytes != first.candidate_logical_kv_bytes {
                return Err(ComparableObservedKvSelectionError::BudgetMismatch);
            }
            if !policies.insert(record.selection.policy()) {
                return Err(ComparableObservedKvSelectionError::DuplicatePolicy);
            }
        }
        Ok(Self { records })
    }

    #[must_use]
    pub fn records(&self) -> &[KvlabKvRealModelSelectionEvidenceV1] {
        &self.records
    }

    #[must_use]
    pub const fn budget_bytes(&self) -> u64 {
        self.records[0].candidate_logical_kv_bytes
    }
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

fn same_input(
    left: &KvlabKvRealModelSelectionEvidenceV1,
    right: &KvlabKvRealModelSelectionEvidenceV1,
) -> bool {
    left.selection.input_token_ids() == right.selection.input_token_ids()
        && left.selection.bytes_per_token() == right.selection.bytes_per_token()
}

fn same_baseline(
    left: &KvlabKvRealModelSelectionEvidenceV1,
    right: &KvlabKvRealModelSelectionEvidenceV1,
) -> bool {
    left.baseline_output_sha256 == right.baseline_output_sha256
        && left.baseline_logical_kv_bytes == right.baseline_logical_kv_bytes
        && metric_baselines_equal(&left.metrics, &right.metrics)
}

fn metric_baselines_equal(
    left: &[ObservedSelectionMetricPair],
    right: &[ObservedSelectionMetricPair],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.name == right.name
                && left.kind == right.kind
                && left.unit == right.unit
                && left.preference == right.preference
                && nearly_equal(left.baseline_value, right.baseline_value)
        })
}

fn canonical_json(value: &Value) -> Result<String, serde_json::Error> {
    fn write_value(value: &Value, output: &mut String) -> Result<(), serde_json::Error> {
        match value {
            Value::Object(map) => {
                output.push('{');
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    output.push_str(&serde_json::to_string(key)?);
                    output.push(':');
                    write_value(&map[key], output)?;
                }
                output.push('}');
                Ok(())
            }
            Value::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    write_value(value, output)?;
                }
                output.push(']');
                Ok(())
            }
            _ => {
                output.push_str(&serde_json::to_string(value)?);
                Ok(())
            }
        }
    }

    let mut output = String::new();
    write_value(value, &mut output)?;
    Ok(output)
}

fn require_text(
    field: &'static str,
    value: &str,
) -> Result<(), KvRealModelSelectionEvidenceError> {
    if value.trim().is_empty() {
        return Err(KvRealModelSelectionEvidenceError::EmptyText(field));
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

impl fmt::Display for KvRealModelSelectionEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid observed KV selection JSON: {error}"),
            Self::NonCanonicalJson => {
                formatter.write_str("observed KV selection JSON is not canonical")
            }
            Self::UnsupportedSchema => {
                formatter.write_str("unsupported observed KV selection schema")
            }
            Self::EmptyText(field) => {
                write!(formatter, "observed KV selection field {field} must not be empty")
            }
            Self::InvalidRunRevision => formatter
                .write_str("observed KV selection run revision must be a lowercase full Git SHA"),
            Self::InvalidSha256(field) => write!(
                formatter,
                "observed KV selection field {field} must be a lowercase SHA-256 digest"
            ),
            Self::InvalidSeed => {
                formatter.write_str("observed KV selection seed must fit an unsigned 64-bit integer")
            }
            Self::Selection(error) => write!(formatter, "invalid embedded KV selection: {error}"),
            Self::BaselineLogicalBytesMismatch => formatter.write_str(
                "observed baseline logical KV bytes do not match selection input bytes",
            ),
            Self::CandidateLogicalBytesMismatch => formatter.write_str(
                "observed candidate logical KV bytes do not match selection retained bytes",
            ),
            Self::EmptyMetrics => formatter.write_str(
                "observed KV selection evidence must contain at least one metric",
            ),
            Self::DuplicateMetric(name) => {
                write!(formatter, "duplicate observed KV selection metric {name}")
            }
            Self::UnknownMetricKind(kind) => {
                write!(formatter, "unknown observed KV selection metric kind {kind}")
            }
            Self::UnknownMetricPreference(preference) => write!(
                formatter,
                "unknown observed KV selection metric preference {preference}"
            ),
            Self::NonFiniteMetric(name) => write!(
                formatter,
                "observed KV selection metric {name} contains a non-finite value"
            ),
            Self::MetricDeltaMismatch(name) => write!(
                formatter,
                "observed KV selection metric {name} delta does not match candidate minus baseline"
            ),
            Self::Evidence(error) => {
                write!(formatter, "invalid ProspectEngine evidence source: {error}")
            }
        }
    }
}

impl std::error::Error for KvRealModelSelectionEvidenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Selection(error) => Some(error),
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for ComparableObservedKvSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptySet => "comparable observed KV selection set must not be empty",
            Self::ContextMismatch => {
                "observed KV selections do not share one experimental context"
            }
            Self::InputMismatch => "observed KV selections do not share one logical input",
            Self::BaselineMismatch => "observed KV selections do not share one paired baseline",
            Self::BudgetMismatch => "observed KV selections do not share one candidate byte budget",
            Self::DuplicatePolicy => "observed KV selection set contains a duplicate policy label",
        })
    }
}

impl std::error::Error for ComparableObservedKvSelectionError {}

#[cfg(test)]
mod tests {
    use prospect_evidence::EvidenceNature;
    use serde_json::json;

    use super::{
        ComparableObservedKvSelectionError, ComparableObservedKvSelectionSet,
        KVLAB_KV_REAL_MODEL_SELECTION_REVISION, KvlabKvRealModelSelectionEvidenceV1,
        ObservedSelectionMetricKind, ObservedSelectionMetricPreference, canonical_json,
    };

    fn record_json(policy: &str, retained: &[u64], candidate_hash: char) -> String {
        let input = [10_u64, 11, 12, 13, 14];
        let retained_set = retained.iter().copied().collect::<std::collections::BTreeSet<_>>();
        let evicted = input
            .iter()
            .copied()
            .filter(|token_id| !retained_set.contains(token_id))
            .collect::<Vec<_>>();
        let retained_bytes = u64::try_from(retained.len()).expect("len") * 64;
        let value = json!({
            "schema":"kvlab.prospect-kv-real-model-selection/v1",
            "experiment_id":"real-model-selection-c1",
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
                    "name":"token_accuracy",
                    "kind":"quality",
                    "unit":"ratio",
                    "preference":"higher_is_better",
                    "baseline_value":0.8,
                    "candidate_value":0.78,
                    "delta":-0.02
                },
                {
                    "name":"logit_l2",
                    "kind":"numerical",
                    "unit":"l2",
                    "preference":"lower_is_better",
                    "baseline_value":0.0,
                    "candidate_value":0.125,
                    "delta":0.125
                }
            ]
        });
        canonical_json(&value).expect("canonical")
    }

    #[test]
    fn consumes_observed_explicit_selection() {
        let record = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "lru",
            &[10, 12, 14],
            '3',
        ))
        .expect("record");
        assert_eq!(record.selection().policy(), "lru");
        assert_eq!(record.selection().retained_token_ids(), [10, 12, 14]);
        assert_eq!(record.candidate_logical_kv_bytes(), 192);
        assert_eq!(record.metrics()[0].kind(), ObservedSelectionMetricKind::Quality);
        assert_eq!(
            record.metrics()[0].preference(),
            ObservedSelectionMetricPreference::HigherIsBetter
        );
        let source = record.evidence_source().expect("source");
        assert_eq!(source.nature(), EvidenceNature::Observed);
        assert_eq!(source.revision(), KVLAB_KV_REAL_MODEL_SELECTION_REVISION);
    }

    #[test]
    fn comparable_set_accepts_distinct_policies_at_one_budget_without_ranking() {
        let lru = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "lru",
            &[10, 12, 14],
            '3',
        ))
        .expect("lru");
        let magnitude = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "magnitude",
            &[11, 13, 14],
            '4',
        ))
        .expect("magnitude");
        let set = ComparableObservedKvSelectionSet::new(vec![lru, magnitude]).expect("set");
        assert_eq!(set.budget_bytes(), 192);
        assert_eq!(set.records().len(), 2);
    }

    #[test]
    fn comparable_set_rejects_budget_or_baseline_drift() {
        let lru = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "lru",
            &[10, 12, 14],
            '3',
        ))
        .expect("lru");
        let smaller = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
            "magnitude",
            &[13, 14],
            '4',
        ))
        .expect("smaller");
        assert!(matches!(
            ComparableObservedKvSelectionSet::new(vec![lru.clone(), smaller]),
            Err(ComparableObservedKvSelectionError::BudgetMismatch)
        ));

        let mut value: Value = serde_json::from_str(&record_json(
            "magnitude",
            &[11, 13, 14],
            '4',
        ))
        .expect("json");
        value["baseline_output_sha256"] = json!("5".repeat(64));
        let drift = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(
            &canonical_json(&value).expect("canonical"),
        )
        .expect("drift");
        assert!(matches!(
            ComparableObservedKvSelectionSet::new(vec![lru, drift]),
            Err(ComparableObservedKvSelectionError::BaselineMismatch)
        ));
    }

    #[test]
    fn rejects_tampered_metric_and_selection() {
        let mut value: Value = serde_json::from_str(&record_json(
            "lru",
            &[10, 12, 14],
            '3',
        ))
        .expect("json");
        value["metrics"][0]["delta"] = json!(0.5);
        let tampered = canonical_json(&value).expect("canonical");
        assert!(KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&tampered).is_err());

        let mut value: Value = serde_json::from_str(&record_json(
            "lru",
            &[10, 12, 14],
            '3',
        ))
        .expect("json");
        value["selection"]["retained_token_ids"] = json!([14, 12, 10]);
        let tampered = canonical_json(&value).expect("canonical");
        assert!(KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&tampered).is_err());
    }

    use serde_json::Value;
}
