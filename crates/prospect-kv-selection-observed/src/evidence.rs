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
    pub(crate) name: String,
    pub(crate) kind: ObservedSelectionMetricKind,
    pub(crate) unit: String,
    pub(crate) preference: ObservedSelectionMetricPreference,
    pub(crate) baseline_value: f64,
    pub(crate) candidate_value: f64,
    pub(crate) delta: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KvlabKvRealModelSelectionEvidenceV1 {
    pub(crate) experiment_id: String,
    pub(crate) run_repository_revision: String,
    pub(crate) model_id: String,
    pub(crate) model_revision: String,
    pub(crate) tokenizer_revision: String,
    pub(crate) runtime_backend: String,
    pub(crate) runtime_revision: String,
    pub(crate) evaluation_id: String,
    pub(crate) trace_sha256: String,
    pub(crate) seed: u64,
    pub(crate) selection: KvlabKvSelectionHandoffV1,
    pub(crate) baseline_output_sha256: String,
    pub(crate) candidate_output_sha256: String,
    pub(crate) baseline_logical_kv_bytes: u64,
    pub(crate) candidate_logical_kv_bytes: u64,
    pub(crate) metrics: Vec<ObservedSelectionMetricPair>,
    content_fingerprint: String,
}

#[derive(Debug)]
pub enum KvRealModelSelectionEvidenceError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    EmptyText(&'static str),
    InvalidRunRevision,
    InvalidSha256(&'static str),
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
            other => Err(KvRealModelSelectionEvidenceError::UnknownMetricPreference(
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
        validate_provenance(&wire)?;

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

fn validate_provenance(wire: &EvidenceWire) -> Result<(), KvRealModelSelectionEvidenceError> {
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
    Ok(())
}

fn parse_metrics(
    wires: Vec<MetricWire>,
) -> Result<Vec<ObservedSelectionMetricPair>, KvRealModelSelectionEvidenceError> {
    if wires.is_empty() {
        return Err(KvRealModelSelectionEvidenceError::EmptyMetrics);
    }
    let mut seen = BTreeSet::new();
    let mut metrics = Vec::with_capacity(wires.len());
    for metric in wires {
        require_text("metric.name", &metric.name)?;
        require_text("metric.unit", &metric.unit)?;
        if !seen.insert(metric.name.clone()) {
            return Err(KvRealModelSelectionEvidenceError::DuplicateMetric(
                metric.name,
            ));
        }
        if !metric.baseline_value.is_finite()
            || !metric.candidate_value.is_finite()
            || !metric.delta.is_finite()
        {
            return Err(KvRealModelSelectionEvidenceError::NonFiniteMetric(
                metric.name,
            ));
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
    Ok(metrics)
}

pub(crate) fn canonical_json(value: &Value) -> Result<String, serde_json::Error> {
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

pub(crate) fn nearly_equal(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs());
    (left - right).abs() <= FLOAT_ABS_TOLERANCE + FLOAT_REL_TOLERANCE * scale
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
            Self::EmptyText(field) => write!(
                formatter,
                "observed KV selection field {field} must not be empty"
            ),
            Self::InvalidRunRevision => formatter
                .write_str("observed KV selection run revision must be a lowercase full Git SHA"),
            Self::InvalidSha256(field) => write!(
                formatter,
                "observed KV selection field {field} must be a lowercase SHA-256 digest"
            ),
            Self::Selection(error) => write!(formatter, "invalid embedded KV selection: {error}"),
            Self::BaselineLogicalBytesMismatch => formatter
                .write_str("observed baseline logical KV bytes do not match selection input bytes"),
            Self::CandidateLogicalBytesMismatch => formatter.write_str(
                "observed candidate logical KV bytes do not match selection retained bytes",
            ),
            Self::EmptyMetrics => formatter
                .write_str("observed KV selection evidence must contain at least one metric"),
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
