//! Input-bound terminal execution records. No automatic replay or resume.
//!
//! The bundle's canonical serialization is captured BEFORE any engine call.
//! Signature/error payloads use explicit caller codecs and remain opaque text:
//! this module never silently converts a floating-point NaN to JSON null.
//! A checksum binds bytes; it does not authenticate hardware or engine execution.

use core::fmt;

use prospect_adapter::{
    AdapterCapability, AdapterMetadata, AdapterUpstream, ContractVersion, NamespacedId,
};
use prospect_bundle::ScenarioBundle;
use prospect_evidence::RunId;
use prospect_registry::{DecisionPolicyRegistry, MetricRegistry};
use prospect_scenario::controlled::{
    BatchStatus, EvaluationControl, InterruptionReason, ProgressUpdate,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    BundleExecutionError, ExecutableAdapterRegistry, RegisteredBatchExecution,
    evaluate_registered_bundle_controlled,
};

pub const EXECUTION_RECORD_SCHEMA_V1: &str = "prospect.bundle-evaluation-record/v1";
pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RECORD_SCENARIOS: usize = 4096;
pub const MAX_ENCODED_PAYLOAD_BYTES: usize = 1024 * 1024;
const KIND: &str = "software_execution_report";

/// Contract, encoding, or admission failure. No partial record is returned.
#[derive(Debug)]
pub enum ExecutionRecordError {
    Json(serde_json::Error),
    Invalid(&'static str),
    Encoding(String),
}

impl fmt::Display for ExecutionRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "execution record JSON: {error}"),
            Self::Invalid(reason) => write!(f, "execution record rejected: {reason}"),
            Self::Encoding(error) => write!(f, "execution payload encoder failed: {error}"),
        }
    }
}
impl std::error::Error for ExecutionRecordError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}
impl From<serde_json::Error> for ExecutionRecordError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Failure before evaluation; domain-call failures are retained in the report.
#[derive(Debug)]
pub enum BoundEvaluationError<E> {
    Input(ExecutionRecordError),
    Dispatch(BundleExecutionError<E>),
}
impl<E: fmt::Display> fmt::Display for BoundEvaluationError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(e) => e.fmt(f),
            Self::Dispatch(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for BoundEvaluationError<E> {}

/// Private binding between the pre-execution input snapshot and actual report.
///
/// Retain this value when a codec or disk write fails: capture only borrows it.
/// It is NOT a resumable engine snapshot and it cannot restart an intervention.
#[derive(Debug)]
pub struct BoundBundleEvaluation<I, S, E> {
    input_json: String,
    max_evaluations: usize,
    deadline_configured: bool,
    report: RegisteredBatchExecution<I, S, E>,
}

impl<I, S, E> BoundBundleEvaluation<I, S, E> {
    /// Exact canonical input captured before evaluation. Persist it with the record.
    #[must_use]
    pub fn input_json(&self) -> &str {
        &self.input_json
    }

    /// Actual in-memory execution, including any successful prefix or engine error.
    pub const fn report(&self) -> &RegisteredBatchExecution<I, S, E> {
        &self.report
    }

    /// Capture a terminal record using named, explicitly supplied text codecs.
    ///
    /// Codecs own numerical validity and lossless encoding. A codec name is a
    /// declaration, not an installed decoder. Payloads are preserved byte-for-byte
    /// as UTF-8 strings; empty payloads are allowed. Encoder errors leave `self`
    /// intact and never fabricate a substitute signature or error.
    pub fn capture_record<FS, FE>(
        &self,
        run_id: &RunId,
        codecs: PayloadCodecs,
        mut encode_signature: FS,
        mut encode_error: FE,
    ) -> Result<ExecutionRecord, ExecutionRecordError>
    where
        FS: FnMut(&S) -> Result<String, String>,
        FE: FnMut(&E) -> Result<String, String>,
    {
        let evaluation = self.report.evaluation();
        let mut remaining_payload_bytes = MAX_RECORD_BYTES;
        let mut encode_checked = |signature: &S| -> Result<String, ExecutionRecordError> {
            let payload = encode_signature(signature).map_err(ExecutionRecordError::Encoding)?;
            check_payload(&payload)?;
            remaining_payload_bytes = remaining_payload_bytes.checked_sub(payload.len()).ok_or(
                ExecutionRecordError::Invalid("cumulative encoded payload limit"),
            )?;
            Ok(payload)
        };
        let baseline = evaluation.baseline().map(&mut encode_checked).transpose()?;
        let outcomes = evaluation
            .outcomes()
            .iter()
            .map(|outcome| {
                Ok(StoredOutcome {
                    scenario_id: outcome.scenario().id().as_str().to_owned(),
                    payload: encode_checked(outcome.signature())?,
                })
            })
            .collect::<Result<Vec<_>, ExecutionRecordError>>()?;
        let terminal = match evaluation.status() {
            BatchStatus::Completed => RecordTerminal::Completed,
            BatchStatus::Interrupted(reason) => RecordTerminal::Interrupted {
                reason: match reason {
                    InterruptionReason::Cancelled => RecordInterruption::Cancelled,
                    InterruptionReason::DeadlineReached => RecordInterruption::DeadlineReached,
                    InterruptionReason::EvaluationLimitReached => {
                        RecordInterruption::EvaluationLimitReached
                    }
                },
            },
            BatchStatus::EngineFailed { scenario, error } => RecordTerminal::Failed {
                scenario_id: scenario.as_ref().map(|s| s.id().as_str().to_owned()),
                error_payload: encode_error(error).map_err(ExecutionRecordError::Encoding)?,
            },
            BatchStatus::DuplicateScenarioId(_) => {
                return invalid("a validated bundle cannot contain duplicate scenarios");
            }
        };
        let wire = RecordWire {
            schema: EXECUTION_RECORD_SCHEMA_V1.to_owned(),
            evidence_kind: KIND.to_owned(),
            run_id: run_id.as_str().to_owned(),
            bundle_sha256: digest(self.input_json.as_bytes()),
            bundle_id: self.report.bundle_id().to_owned(),
            seed: self.report.seed(),
            adapter: serde_json::to_value(self.report.adapter())?,
            max_evaluations: self.max_evaluations,
            deadline_configured: self.deadline_configured,
            codecs,
            baseline,
            outcomes,
            pending: evaluation
                .pending()
                .iter()
                .map(|s| s.id().as_str().to_owned())
                .collect(),
            terminal,
        };
        // Use the same independent verifier used for records read from disk.
        ExecutionRecord::verify_against_bundle(&canonical(&wire)?, &self.input_json)
    }
}

/// Resolve/evaluate the existing typed bundle while capturing its input identity.
///
/// Canonical serialization and admission precede all engine calls. The declared
/// bundle serialization (including state, interventions, metric/policy requirements
/// and seed) is the binding, not a claim that arbitrary custom serializers are
/// injective or that interior-mutable inputs cannot change. Callers must provide
/// deterministic serialization and stable inputs for the duration of the run.
///
/// This API retains the controlled evaluator's cooperative/no-retry semantics.
/// It does not write to disk; encode and persist the returned terminal report
/// explicitly. Input and serialization errors occur before engine invocation.
pub fn evaluate_registered_bundle_bound<State, I, S, E, M, P, F>(
    bundle: &ScenarioBundle<State, I>,
    adapters: &ExecutableAdapterRegistry<State, I, S, E>,
    metrics: &MetricRegistry<S, M>,
    policies: &DecisionPolicyRegistry<S, P>,
    control: &EvaluationControl,
    progress: F,
) -> Result<BoundBundleEvaluation<I, S, E>, BoundEvaluationError<E>>
where
    State: Serialize,
    I: Clone + Serialize,
    P: Ord,
    F: FnMut(ProgressUpdate<'_>),
{
    if bundle.scenarios().len() > MAX_RECORD_SCENARIOS {
        return Err(BoundEvaluationError::Input(ExecutionRecordError::Invalid(
            "too many scenarios",
        )));
    }
    let input_json = bundle.canonical_json().map_err(|_| {
        BoundEvaluationError::Input(ExecutionRecordError::Invalid("bundle serialization failed"))
    })?;
    parse_bundle(&input_json).map_err(BoundEvaluationError::Input)?;
    let max_evaluations = control.max_evaluations();
    let deadline_configured = control.deadline().is_some();
    let report = evaluate_registered_bundle_controlled(
        bundle, adapters, metrics, policies, control, progress,
    )
    .map_err(BoundEvaluationError::Dispatch)?;
    Ok(BoundBundleEvaluation {
        input_json,
        max_evaluations,
        deadline_configured,
        report,
    })
}

/// Versioned application codec identifiers; no dynamic loading is performed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadCodecs {
    signature: String,
    error: String,
}
impl PayloadCodecs {
    /// Names must satisfy the existing adapter namespaced-ID contract.
    ///
    /// ```
    /// use prospect_dispatch::execution::record::PayloadCodecs;
    /// let codecs = PayloadCodecs::new("example.i32.v1", "example.error_text.v1")?;
    /// assert_eq!(codecs.signature(), "example.i32.v1");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(signature: &str, error: &str) -> Result<Self, ExecutionRecordError> {
        let value = Self {
            signature: signature.to_owned(),
            error: error.to_owned(),
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), ExecutionRecordError> {
        for id in [&self.signature, &self.error] {
            NamespacedId::new(id.as_str())
                .map_err(|_| ExecutionRecordError::Invalid("invalid codec ID"))?;
        }
        Ok(())
    }
    #[must_use]
    pub fn signature(&self) -> &str {
        &self.signature
    }
    #[must_use]
    pub fn error(&self) -> &str {
        &self.error
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordInterruption {
    Cancelled,
    DeadlineReached,
    EvaluationLimitReached,
}

/// Terminal report. A failed candidate is separate from the never-started suffix.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordTerminal {
    Completed,
    Interrupted {
        reason: RecordInterruption,
    },
    Failed {
        scenario_id: Option<String>,
        error_payload: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredOutcome {
    scenario_id: String,
    payload: String,
}
impl StoredOutcome {
    #[must_use]
    pub fn scenario_id(&self) -> &str {
        &self.scenario_id
    }
    #[must_use]
    pub fn payload(&self) -> &str {
        &self.payload
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordWire {
    schema: String,
    evidence_kind: String,
    run_id: String,
    bundle_sha256: String,
    bundle_id: String,
    seed: Option<u64>,
    adapter: Value,
    max_evaluations: usize,
    deadline_configured: bool,
    codecs: PayloadCodecs,
    baseline: Option<String>,
    outcomes: Vec<StoredOutcome>,
    pending: Vec<String>,
    terminal: RecordTerminal,
}

/// Verified file consistency and input binding, NOT an authenticated observation.
///
/// There is deliberately no conversion to BatchResult, engine state or a resume
/// queue. A caller may decode opaque payloads only under its own codec contract.
#[derive(Debug)]
pub struct ExecutionRecord {
    wire: RecordWire,
    canonical: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecutionRecordSummary {
    pub schema: &'static str,
    pub record_sha256: String,
    pub bundle_sha256: String,
    pub run_id: String,
    pub state: &'static str,
    pub successful_candidates: usize,
    pub failed_candidates: usize,
    pub never_started_candidates: usize,
    pub evidence_kind: &'static str,
    pub resume_authorized: bool,
}

impl ExecutionRecord {
    /// Verify canonical bytes, expected input SHA-256, adapter contract and lifecycle.
    ///
    /// A structurally valid, coherently forged record can still pass: the input
    /// bundle is a trusted external comparison, not a signature over run outputs.
    /// Payload codec semantics and scientific correctness are not checked here.
    pub fn verify_against_bundle(
        payload: &str,
        expected_bundle: &str,
    ) -> Result<Self, ExecutionRecordError> {
        if payload.len() > MAX_RECORD_BYTES {
            return invalid("record exceeds byte limit");
        }
        let bundle = parse_bundle(expected_bundle)?;
        let wire: RecordWire = serde_json::from_str(payload)?;
        if canonical(&wire)? != payload {
            return invalid("noncanonical record or duplicate/missing fields");
        }
        if wire.schema != EXECUTION_RECORD_SCHEMA_V1 || wire.evidence_kind != KIND {
            return invalid("unsupported schema or evidence kind");
        }
        RunId::new(&wire.run_id).map_err(|_| ExecutionRecordError::Invalid("invalid run ID"))?;
        if wire.bundle_sha256 != digest(expected_bundle.as_bytes())
            || wire.bundle_id != bundle.bundle_id().as_str()
            || wire.seed != bundle.seed()
        {
            return invalid("input bundle binding mismatch");
        }
        let metadata = validate_metadata(&wire.adapter)?;
        let required = bundle.adapter();
        if metadata.adapter_id().as_str() != required.adapter_id().as_str()
            || !metadata
                .contract_version()
                .supports(required.contract_version())
        {
            return invalid("adapter contract mismatch");
        }
        if let Some(required) = required.upstream() {
            let actual = metadata
                .upstream()
                .ok_or(ExecutionRecordError::Invalid("missing upstream"))?;
            if actual.component() != required.component()
                || actual.revision() != required.revision()
            {
                return invalid("adapter upstream mismatch");
            }
        }
        wire.codecs.validate()?;
        let mut ids = bundle.scenarios().iter().map(|s| s.id().as_str());
        if wire.outcomes.len() > wire.max_evaluations {
            return invalid("candidate quota exceeded");
        }
        if wire.baseline.is_none() && !wire.outcomes.is_empty() {
            return invalid("outcomes without baseline");
        }
        if let Some(value) = &wire.baseline {
            check_payload(value)?;
        }
        for outcome in &wire.outcomes {
            if ids.next() != Some(outcome.scenario_id.as_str()) {
                return invalid("successful prefix mismatch");
            }
            check_payload(&outcome.payload)?;
        }
        match &wire.terminal {
            RecordTerminal::Completed => {
                if wire.baseline.is_none()
                    || !wire.pending.is_empty()
                    || ids.clone().next().is_some()
                {
                    return invalid("incomplete work relabelled completed");
                }
            }
            RecordTerminal::Failed {
                scenario_id,
                error_payload,
            } => {
                check_payload(error_payload)?;
                match scenario_id {
                    Some(id) => {
                        if wire.baseline.is_none()
                            || ids.next() != Some(id.as_str())
                            || wire.outcomes.len() >= wire.max_evaluations
                        {
                            return invalid("invalid failed candidate");
                        }
                    }
                    None => {
                        if wire.baseline.is_some()
                            || !wire.outcomes.is_empty()
                            || wire.max_evaluations == 0
                        {
                            return invalid("invalid baseline failure");
                        }
                    }
                }
            }
            RecordTerminal::Interrupted { reason } => match reason {
                RecordInterruption::DeadlineReached if !wire.deadline_configured => {
                    return invalid("deadline interruption without deadline");
                }
                RecordInterruption::EvaluationLimitReached => {
                    if wire.pending.is_empty() || wire.outcomes.len() != wire.max_evaluations {
                        return invalid("invalid quota interruption");
                    }
                }
                _ => {}
            },
        }
        if wire.max_evaluations == 0 && wire.baseline.is_some() {
            return invalid("zero budget baseline call");
        }
        if !ids.eq(wire.pending.iter().map(String::as_str)) {
            return invalid("never-started suffix mismatch");
        }
        Ok(Self {
            wire,
            canonical: payload.to_owned(),
        })
    }

    /// Exact bytes for persistence. No newline is appended or normalized.
    #[must_use]
    pub fn canonical_json(&self) -> &str {
        &self.canonical
    }
    #[must_use]
    pub fn sha256(&self) -> String {
        digest(self.canonical.as_bytes())
    }
    #[must_use]
    pub fn codecs(&self) -> &PayloadCodecs {
        &self.wire.codecs
    }
    #[must_use]
    pub fn baseline_payload(&self) -> Option<&str> {
        self.wire.baseline.as_deref()
    }
    #[must_use]
    pub fn outcomes(&self) -> &[StoredOutcome] {
        &self.wire.outcomes
    }
    #[must_use]
    pub fn pending(&self) -> &[String] {
        &self.wire.pending
    }
    #[must_use]
    pub const fn terminal(&self) -> &RecordTerminal {
        &self.wire.terminal
    }
    #[must_use]
    pub fn summary(&self) -> ExecutionRecordSummary {
        ExecutionRecordSummary {
            schema: "prospect.execution-record-verification/v1",
            record_sha256: self.sha256(),
            bundle_sha256: self.wire.bundle_sha256.clone(),
            run_id: self.wire.run_id.clone(),
            state: match self.wire.terminal {
                RecordTerminal::Completed => "completed",
                RecordTerminal::Interrupted { .. } => "interrupted",
                RecordTerminal::Failed { .. } => "failed",
            },
            successful_candidates: self.wire.outcomes.len(),
            failed_candidates: usize::from(matches!(
                self.wire.terminal,
                RecordTerminal::Failed {
                    scenario_id: Some(_),
                    ..
                }
            )),
            never_started_candidates: self.wire.pending.len(),
            evidence_kind: KIND,
            resume_authorized: false,
        }
    }
}

fn invalid<T>(reason: &'static str) -> Result<T, ExecutionRecordError> {
    Err(ExecutionRecordError::Invalid(reason))
}
fn check_payload(payload: &str) -> Result<(), ExecutionRecordError> {
    if payload.len() > MAX_ENCODED_PAYLOAD_BYTES {
        return invalid("encoded payload exceeds byte limit");
    }
    Ok(())
}
fn parse_bundle(payload: &str) -> Result<ScenarioBundle<Value, Value>, ExecutionRecordError> {
    if payload.len() > MAX_RECORD_BYTES {
        return invalid("bundle exceeds byte limit");
    }
    let bundle = ScenarioBundle::<Value, Value>::from_canonical_json(payload)
        .map_err(|_| ExecutionRecordError::Invalid("invalid canonical input bundle"))?;
    if bundle.scenarios().len() > MAX_RECORD_SCENARIOS {
        return invalid("too many scenarios");
    }
    Ok(bundle)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn canonical<T: Serialize>(value: &T) -> Result<String, ExecutionRecordError> {
    // Explicit recursion makes key ordering independent of serde_json features.
    fn write(value: &Value, out: &mut String) -> Result<(), serde_json::Error> {
        match value {
            Value::Object(map) => {
                out.push('{');
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                for (i, key) in keys.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key)?);
                    out.push(':');
                    write(&map[*key], out)?;
                }
                out.push('}');
            }
            Value::Array(values) => {
                out.push('[');
                for (i, value) in values.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    write(value, out)?;
                }
                out.push(']');
            }
            _ => out.push_str(&serde_json::to_string(value)?),
        }
        Ok(())
    }
    let mut out = String::new();
    write(&serde_json::to_value(value)?, &mut out)?;
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionWire {
    major: u16,
    minor: u16,
}
impl VersionWire {
    fn version(&self) -> Result<ContractVersion, ExecutionRecordError> {
        ContractVersion::new(self.major, self.minor)
            .map_err(|_| ExecutionRecordError::Invalid("invalid adapter version"))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpstreamWire {
    component: String,
    revision: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityWire {
    id: String,
    version: VersionWire,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataWire {
    adapter_id: String,
    contract_version: VersionWire,
    upstream: Option<UpstreamWire>,
    capabilities: Vec<CapabilityWire>,
}
fn validate_metadata(value: &Value) -> Result<AdapterMetadata, ExecutionRecordError> {
    let wire: MetadataWire = serde_json::from_value(value.clone())?;
    let invalid = |_| ExecutionRecordError::Invalid("invalid adapter metadata");
    let upstream = wire
        .upstream
        .map(|v| AdapterUpstream::new(v.component, v.revision).map_err(invalid))
        .transpose()?;
    let capabilities = wire
        .capabilities
        .into_iter()
        .map(|v| AdapterCapability::new(v.id, v.version.version()?).map_err(invalid))
        .collect::<Result<Vec<_>, _>>()?;
    let metadata = AdapterMetadata::new(
        wire.adapter_id,
        wire.contract_version.version()?,
        upstream,
        capabilities,
    )
    .map_err(invalid)?;
    if canonical(&metadata)? != canonical(value)? {
        return Err(ExecutionRecordError::Invalid(
            "noncanonical adapter metadata",
        ));
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests;
