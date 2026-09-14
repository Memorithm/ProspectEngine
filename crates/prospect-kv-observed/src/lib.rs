#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeSet;

use prospect_evidence::{EvidenceError, EvidenceNature, EvidenceSource};
use prospect_kv::{
    KvEvictionContractError, KvEvictionOutcome, KvEvictionProspectiveModel, KvEvictionState,
    KvlabKvEvictionHandoffV1,
};
use serde::Deserialize;

pub const KVLAB_KV_REAL_MODEL_EVIDENCE_SCHEMA_V1: &str = "kvlab.prospect-kv-real-model-eviction/v1";
pub const KVLAB_KV_REAL_MODEL_EVIDENCE_REVISION: &str = "6c1ee30e016827de507e3428387f750931eab5fa";

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
pub struct ObservedKvMetricValue {
    name: String,
    kind: ObservedMetricKind,
    unit: String,
    preference: MetricPreference,
    value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedKvEvictionSignature {
    experiment_id: String,
    model_id: String,
    model_revision: String,
    tokenizer_revision: String,
    runtime_backend: String,
    runtime_revision: String,
    evaluation_id: String,
    trace_sha256: String,
    seed: u64,
    output_sha256: String,
    logical_kv_bytes: u64,
    metrics: Vec<ObservedKvMetricValue>,
    evidence_content_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KvlabKvRealModelEvictionEvidenceV1 {
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
    eviction: KvlabKvEvictionHandoffV1,
    baseline_output_sha256: String,
    candidate_output_sha256: String,
    baseline_logical_kv_bytes: u64,
    candidate_logical_kv_bytes: u64,
    metrics: Vec<ObservedMetricPair>,
    content_fingerprint: String,
}

#[derive(Debug)]
pub enum KvlabKvRealModelEvidenceError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    EmptyText(&'static str),
    InvalidRunRevision,
    InvalidSha256(&'static str),
    EmbeddedEviction(KvEvictionContractError),
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
pub enum ObservedKvEvidenceModelError {
    EmptyEvidence,
    ContextMismatch,
    BaselineMismatch,
    DuplicateOutcome,
    MissingEvidence,
}

#[derive(Clone, Debug)]
pub struct ObservedKvEvictionEvidenceModel {
    records: Vec<KvlabKvRealModelEvictionEvidenceV1>,
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
    eviction: serde_json::Value,
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
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Numerical => "numerical",
            Self::Quality => "quality",
        }
    }

    fn parse(value: &str) -> Result<Self, KvlabKvRealModelEvidenceError> {
        match value {
            "numerical" => Ok(Self::Numerical),
            "quality" => Ok(Self::Quality),
            other => Err(KvlabKvRealModelEvidenceError::UnknownMetricKind(
                other.to_owned(),
            )),
        }
    }
}

impl MetricPreference {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HigherIsBetter => "higher_is_better",
            Self::LowerIsBetter => "lower_is_better",
            Self::None => "none",
        }
    }

    fn parse(value: &str) -> Result<Self, KvlabKvRealModelEvidenceError> {
        match value {
            "higher_is_better" => Ok(Self::HigherIsBetter),
            "lower_is_better" => Ok(Self::LowerIsBetter),
            "none" => Ok(Self::None),
            other => Err(KvlabKvRealModelEvidenceError::UnknownMetricPreference(
                other.to_owned(),
            )),
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

    fn baseline_metric(&self) -> ObservedKvMetricValue {
        ObservedKvMetricValue {
            name: self.name.clone(),
            kind: self.kind,
            unit: self.unit.clone(),
            preference: self.preference,
            value: self.baseline_value,
        }
    }

    fn candidate_metric(&self) -> ObservedKvMetricValue {
        ObservedKvMetricValue {
            name: self.name.clone(),
            kind: self.kind,
            unit: self.unit.clone(),
            preference: self.preference,
            value: self.candidate_value,
        }
    }
}

impl ObservedKvMetricValue {
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

impl ObservedKvEvictionSignature {
    #[must_use]
    pub fn experiment_id(&self) -> &str {
        &self.experiment_id
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
    pub fn output_sha256(&self) -> &str {
        &self.output_sha256
    }

    #[must_use]
    pub const fn logical_kv_bytes(&self) -> u64 {
        self.logical_kv_bytes
    }

    #[must_use]
    pub fn metrics(&self) -> &[ObservedKvMetricValue] {
        &self.metrics
    }

    #[must_use]
    pub fn evidence_content_fingerprint(&self) -> &str {
        &self.evidence_content_fingerprint
    }
}

impl KvlabKvRealModelEvictionEvidenceV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvlabKvRealModelEvidenceError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(KvlabKvRealModelEvidenceError::Json)?;
        if serde_json::to_string(&value).map_err(KvlabKvRealModelEvidenceError::Json)? != json {
            return Err(KvlabKvRealModelEvidenceError::NonCanonicalJson);
        }

        let wire: EvidenceWire =
            serde_json::from_value(value).map_err(KvlabKvRealModelEvidenceError::Json)?;
        if wire.schema != KVLAB_KV_REAL_MODEL_EVIDENCE_SCHEMA_V1 {
            return Err(KvlabKvRealModelEvidenceError::UnsupportedSchema);
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
            return Err(KvlabKvRealModelEvidenceError::InvalidRunRevision);
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
                return Err(KvlabKvRealModelEvidenceError::InvalidSha256(field));
            }
        }

        let eviction_json =
            serde_json::to_string(&wire.eviction).map_err(KvlabKvRealModelEvidenceError::Json)?;
        let eviction = KvlabKvEvictionHandoffV1::from_canonical_json(&eviction_json)
            .map_err(KvlabKvRealModelEvidenceError::EmbeddedEviction)?;
        if wire.baseline_logical_kv_bytes != eviction.outcome().logical_input_bytes() {
            return Err(KvlabKvRealModelEvidenceError::BaselineLogicalBytesMismatch);
        }
        if wire.candidate_logical_kv_bytes != eviction.outcome().logical_retained_bytes() {
            return Err(KvlabKvRealModelEvidenceError::CandidateLogicalBytesMismatch);
        }
        if wire.metrics.is_empty() {
            return Err(KvlabKvRealModelEvidenceError::EmptyMetrics);
        }

        let mut seen = BTreeSet::new();
        let mut metrics = Vec::with_capacity(wire.metrics.len());
        for metric in wire.metrics {
            require_text("metric.name", &metric.name)?;
            require_text("metric.unit", &metric.unit)?;
            if !seen.insert(metric.name.clone()) {
                return Err(KvlabKvRealModelEvidenceError::DuplicateMetric(metric.name));
            }
            if !metric.baseline_value.is_finite()
                || !metric.candidate_value.is_finite()
                || !metric.delta.is_finite()
            {
                return Err(KvlabKvRealModelEvidenceError::NonFiniteMetric(metric.name));
            }
            let expected_delta = metric.candidate_value - metric.baseline_value;
            if !nearly_equal(metric.delta, expected_delta) {
                return Err(KvlabKvRealModelEvidenceError::MetricDeltaMismatch(
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
            eviction,
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
    pub const fn eviction(&self) -> &KvlabKvEvictionHandoffV1 {
        &self.eviction
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
    pub fn metrics(&self) -> &[ObservedMetricPair] {
        &self.metrics
    }

    #[must_use]
    pub fn content_fingerprint(&self) -> &str {
        &self.content_fingerprint
    }

    pub fn evidence_source(&self) -> Result<EvidenceSource, KvlabKvRealModelEvidenceError> {
        EvidenceSource::new_with_nature(
            "KVLab/real-model-kv-eviction",
            KVLAB_KV_REAL_MODEL_EVIDENCE_REVISION,
            EvidenceNature::Observed,
        )
        .map_err(KvlabKvRealModelEvidenceError::Evidence)?
        .with_content_hash(self.content_fingerprint.clone())
        .map_err(KvlabKvRealModelEvidenceError::Evidence)
    }

    fn baseline_signature(&self) -> ObservedKvEvictionSignature {
        ObservedKvEvictionSignature {
            experiment_id: self.experiment_id.clone(),
            model_id: self.model_id.clone(),
            model_revision: self.model_revision.clone(),
            tokenizer_revision: self.tokenizer_revision.clone(),
            runtime_backend: self.runtime_backend.clone(),
            runtime_revision: self.runtime_revision.clone(),
            evaluation_id: self.evaluation_id.clone(),
            trace_sha256: self.trace_sha256.clone(),
            seed: self.seed,
            output_sha256: self.baseline_output_sha256.clone(),
            logical_kv_bytes: self.baseline_logical_kv_bytes,
            metrics: self
                .metrics
                .iter()
                .map(ObservedMetricPair::baseline_metric)
                .collect(),
            evidence_content_fingerprint: self.content_fingerprint.clone(),
        }
    }

    fn candidate_signature(&self) -> ObservedKvEvictionSignature {
        ObservedKvEvictionSignature {
            experiment_id: self.experiment_id.clone(),
            model_id: self.model_id.clone(),
            model_revision: self.model_revision.clone(),
            tokenizer_revision: self.tokenizer_revision.clone(),
            runtime_backend: self.runtime_backend.clone(),
            runtime_revision: self.runtime_revision.clone(),
            evaluation_id: self.evaluation_id.clone(),
            trace_sha256: self.trace_sha256.clone(),
            seed: self.seed,
            output_sha256: self.candidate_output_sha256.clone(),
            logical_kv_bytes: self.candidate_logical_kv_bytes,
            metrics: self
                .metrics
                .iter()
                .map(ObservedMetricPair::candidate_metric)
                .collect(),
            evidence_content_fingerprint: self.content_fingerprint.clone(),
        }
    }
}

impl ObservedKvEvictionEvidenceModel {
    pub fn new(
        records: Vec<KvlabKvRealModelEvictionEvidenceV1>,
    ) -> Result<Self, ObservedKvEvidenceModelError> {
        let Some(first) = records.first() else {
            return Err(ObservedKvEvidenceModelError::EmptyEvidence);
        };

        for (index, record) in records.iter().enumerate() {
            if !same_context(first, record) || first.eviction().state() != record.eviction().state()
            {
                return Err(ObservedKvEvidenceModelError::ContextMismatch);
            }
            if !same_baseline(first, record) {
                return Err(ObservedKvEvidenceModelError::BaselineMismatch);
            }
            if records[..index]
                .iter()
                .any(|prior| prior.eviction().outcome() == record.eviction().outcome())
            {
                return Err(ObservedKvEvidenceModelError::DuplicateOutcome);
            }
        }

        Ok(Self { records })
    }

    #[must_use]
    pub fn records(&self) -> &[KvlabKvRealModelEvictionEvidenceV1] {
        &self.records
    }
}

impl KvEvictionProspectiveModel for ObservedKvEvictionEvidenceModel {
    type Signature = ObservedKvEvictionSignature;
    type Error = ObservedKvEvidenceModelError;

    fn evaluate_eviction(
        &self,
        state: &KvEvictionState,
        outcome: &KvEvictionOutcome,
    ) -> Result<Self::Signature, Self::Error> {
        let first = self
            .records
            .first()
            .ok_or(ObservedKvEvidenceModelError::EmptyEvidence)?;
        if first.eviction().state() != state {
            return Err(ObservedKvEvidenceModelError::MissingEvidence);
        }

        if outcome.evicted_tokens() == 0
            && outcome.retained_token_ids() == outcome.input_token_ids()
            && outcome.logical_evicted_bytes() == 0
        {
            return Ok(first.baseline_signature());
        }

        self.records
            .iter()
            .find(|record| record.eviction().outcome() == outcome)
            .map(KvlabKvRealModelEvictionEvidenceV1::candidate_signature)
            .ok_or(ObservedKvEvidenceModelError::MissingEvidence)
    }
}

fn same_context(
    left: &KvlabKvRealModelEvictionEvidenceV1,
    right: &KvlabKvRealModelEvictionEvidenceV1,
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
    left: &KvlabKvRealModelEvictionEvidenceV1,
    right: &KvlabKvRealModelEvictionEvidenceV1,
) -> bool {
    left.baseline_output_sha256 == right.baseline_output_sha256
        && left.baseline_logical_kv_bytes == right.baseline_logical_kv_bytes
        && metric_baselines_equal(&left.metrics, &right.metrics)
}

fn metric_baselines_equal(left: &[ObservedMetricPair], right: &[ObservedMetricPair]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.name == right.name
                && left.kind == right.kind
                && left.unit == right.unit
                && left.preference == right.preference
                && nearly_equal(left.baseline_value, right.baseline_value)
        })
}

fn require_text(field: &'static str, value: &str) -> Result<(), KvlabKvRealModelEvidenceError> {
    if value.trim().is_empty() {
        return Err(KvlabKvRealModelEvidenceError::EmptyText(field));
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

impl fmt::Display for KvlabKvRealModelEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => {
                write!(formatter, "invalid KVLab real-model evidence JSON: {error}")
            }
            Self::NonCanonicalJson => {
                formatter.write_str("KVLab real-model evidence JSON is not canonical")
            }
            Self::UnsupportedSchema => {
                formatter.write_str("unsupported KVLab real-model evidence schema")
            }
            Self::EmptyText(field) => {
                write!(
                    formatter,
                    "KVLab real-model evidence field {field} must not be empty"
                )
            }
            Self::InvalidRunRevision => formatter
                .write_str("KVLab run repository revision must be a lowercase full Git SHA"),
            Self::InvalidSha256(field) => write!(
                formatter,
                "KVLab real-model evidence field {field} must be a lowercase SHA-256 digest"
            ),
            Self::EmbeddedEviction(error) => {
                write!(formatter, "invalid embedded KV eviction handoff: {error}")
            }
            Self::BaselineLogicalBytesMismatch => {
                formatter.write_str("baseline logical KV bytes do not match the embedded eviction")
            }
            Self::CandidateLogicalBytesMismatch => {
                formatter.write_str("candidate logical KV bytes do not match the embedded eviction")
            }
            Self::EmptyMetrics => {
                formatter.write_str("KVLab real-model evidence must contain at least one metric")
            }
            Self::DuplicateMetric(name) => {
                write!(formatter, "duplicate KVLab observed metric {name}")
            }
            Self::UnknownMetricKind(kind) => {
                write!(formatter, "unknown KVLab observed metric kind {kind}")
            }
            Self::UnknownMetricPreference(preference) => write!(
                formatter,
                "unknown KVLab observed metric preference {preference}"
            ),
            Self::NonFiniteMetric(name) => write!(
                formatter,
                "KVLab observed metric {name} contains a non-finite value"
            ),
            Self::MetricDeltaMismatch(name) => write!(
                formatter,
                "KVLab observed metric {name} delta does not match candidate minus baseline"
            ),
            Self::Evidence(error) => {
                write!(formatter, "invalid ProspectEngine evidence source: {error}")
            }
        }
    }
}

impl std::error::Error for KvlabKvRealModelEvidenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::EmbeddedEviction(error) => Some(error),
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for ObservedKvEvidenceModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyEvidence => "observed KV eviction evidence set must not be empty",
            Self::ContextMismatch => {
                "observed KV eviction records do not share one experimental context"
            }
            Self::BaselineMismatch => {
                "observed KV eviction records do not share one paired full-cache baseline"
            }
            Self::DuplicateOutcome => {
                "observed KV eviction evidence contains duplicate logical outcomes"
            }
            Self::MissingEvidence => {
                "no observed real-model evidence matches this KV eviction outcome"
            }
        })
    }
}

impl std::error::Error for ObservedKvEvidenceModelError {}

#[cfg(test)]
mod tests {
    use prospect_core::ProspectiveEngine;
    use prospect_evidence::EvidenceNature;
    use prospect_kv::{
        KvEvictionEngine, KvEvictionEngineError, KvEvictionIntervention, KvEvictionState,
    };
    use serde_json::json;

    use super::{
        KVLAB_KV_REAL_MODEL_EVIDENCE_REVISION, KvlabKvRealModelEvictionEvidenceV1,
        MetricPreference, ObservedKvEvictionEvidenceModel, ObservedKvEvidenceModelError,
        ObservedMetricKind,
    };

    fn record_json(max_tokens: usize, candidate_hash: char, candidate_accuracy: f64) -> String {
        let retained = if max_tokens == 3 {
            vec![12_u64, 13, 14]
        } else {
            vec![13_u64, 14]
        };
        let evicted = if max_tokens == 3 {
            vec![10_u64, 11]
        } else {
            vec![10_u64, 11, 12]
        };
        let retained_bytes = u64::try_from(retained.len()).unwrap() * 64;
        let evicted_bytes = 320 - retained_bytes;
        serde_json::to_string(&json!({
            "schema":"kvlab.prospect-kv-real-model-eviction/v1",
            "experiment_id":"real-model-c1",
            "run_repository_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "model_id":"example/model",
            "model_revision":"model-r1",
            "tokenizer_revision":"tok-r1",
            "runtime_backend":"fixture-runtime",
            "runtime_revision":"runtime-r1",
            "evaluation_id":"holdout-001",
            "trace_sha256":"1111111111111111111111111111111111111111111111111111111111111111",
            "seed":7,
            "eviction":{
                "schema":"kvlab.prospect-kv-eviction/v1",
                "order":"oldest_first",
                "max_tokens":max_tokens,
                "input_token_ids":[10,11,12,13,14],
                "bytes_per_token":64,
                "retained_token_ids":retained,
                "evicted_token_ids":evicted,
                "logical_input_bytes":320,
                "logical_retained_bytes":retained_bytes,
                "logical_evicted_bytes":evicted_bytes
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
                    "candidate_value":candidate_accuracy,
                    "delta":candidate_accuracy-0.8
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
        }))
        .unwrap()
    }

    #[test]
    fn parses_observed_record_and_preserves_metric_semantics() {
        let record =
            KvlabKvRealModelEvictionEvidenceV1::from_canonical_json(&record_json(3, '3', 0.78))
                .unwrap();
        assert_eq!(record.eviction().outcome().logical_evicted_bytes(), 128);
        assert_eq!(record.metrics()[0].kind(), ObservedMetricKind::Quality);
        assert_eq!(
            record.metrics()[0].preference(),
            MetricPreference::HigherIsBetter
        );
        assert!((record.metrics()[0].delta() + 0.02).abs() < 1.0e-12);

        let source = record.evidence_source().unwrap();
        assert_eq!(source.nature(), EvidenceNature::Observed);
        assert_eq!(source.revision(), KVLAB_KV_REAL_MODEL_EVIDENCE_REVISION);
    }

    #[test]
    fn rejects_tampered_delta_and_logical_bytes() {
        let mut value: serde_json::Value =
            serde_json::from_str(&record_json(3, '3', 0.78)).unwrap();
        value["metrics"][0]["delta"] = json!(0.5);
        let tampered = serde_json::to_string(&value).unwrap();
        assert!(KvlabKvRealModelEvictionEvidenceV1::from_canonical_json(&tampered).is_err());

        let mut value: serde_json::Value =
            serde_json::from_str(&record_json(3, '3', 0.78)).unwrap();
        value["candidate_logical_kv_bytes"] = json!(64);
        let tampered = serde_json::to_string(&value).unwrap();
        assert!(KvlabKvRealModelEvictionEvidenceV1::from_canonical_json(&tampered).is_err());
    }

    #[test]
    fn observed_model_uses_paired_baseline_and_exact_candidate_records() {
        let first =
            KvlabKvRealModelEvictionEvidenceV1::from_canonical_json(&record_json(3, '3', 0.78))
                .unwrap();
        let second =
            KvlabKvRealModelEvictionEvidenceV1::from_canonical_json(&record_json(2, '4', 0.74))
                .unwrap();
        let model = ObservedKvEvictionEvidenceModel::new(vec![first, second]).unwrap();
        let engine = KvEvictionEngine::new(model);
        let state = KvEvictionState::new(vec![10, 11, 12, 13, 14], 64).unwrap();

        let baseline = engine.baseline(&state).unwrap();
        assert_eq!(baseline.output_sha256(), "2".repeat(64));
        assert_eq!(baseline.logical_kv_bytes(), 320);
        assert_eq!(baseline.metrics()[0].value(), 0.8);

        let candidate = engine
            .evaluate(&state, &KvEvictionIntervention::new(3).unwrap())
            .unwrap();
        assert_eq!(candidate.output_sha256(), "3".repeat(64));
        assert_eq!(candidate.logical_kv_bytes(), 192);
        assert_eq!(candidate.metrics()[0].value(), 0.78);

        let missing = engine
            .evaluate(&state, &KvEvictionIntervention::new(1).unwrap())
            .unwrap_err();
        assert!(matches!(
            missing,
            KvEvictionEngineError::Model(ObservedKvEvidenceModelError::MissingEvidence)
        ));
    }

    #[test]
    fn model_rejects_context_or_baseline_drift() {
        let first =
            KvlabKvRealModelEvictionEvidenceV1::from_canonical_json(&record_json(3, '3', 0.78))
                .unwrap();

        let mut context_value: serde_json::Value =
            serde_json::from_str(&record_json(2, '4', 0.74)).unwrap();
        context_value["model_revision"] = json!("model-r2");
        let context_record = KvlabKvRealModelEvictionEvidenceV1::from_canonical_json(
            &serde_json::to_string(&context_value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            ObservedKvEvictionEvidenceModel::new(vec![first.clone(), context_record]),
            Err(ObservedKvEvidenceModelError::ContextMismatch)
        ));

        let mut baseline_value: serde_json::Value =
            serde_json::from_str(&record_json(2, '4', 0.74)).unwrap();
        baseline_value["baseline_output_sha256"] = json!("5".repeat(64));
        let baseline_record = KvlabKvRealModelEvictionEvidenceV1::from_canonical_json(
            &serde_json::to_string(&baseline_value).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            ObservedKvEvictionEvidenceModel::new(vec![first, baseline_record]),
            Err(ObservedKvEvidenceModelError::BaselineMismatch)
        ));
    }
}
