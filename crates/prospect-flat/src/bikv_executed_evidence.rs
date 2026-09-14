use core::fmt;

use prospect_evidence::{EvidenceError, EvidenceSource};
use serde::Deserialize;

pub const FLAT_BIKV_EVIDENCE_SCHEMA_VERSION: u16 = 1;
pub const FLAT_BIKV_EVIDENCE_CONTRACT_REVISION: &str =
    "a5b6598ffe475c74c938f45feb86b009d0e4ad0a";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservedBikvDecision {
    Promote,
    FallbackQualityGate,
    FallbackCorrectnessGate,
    FallbackNoLatencyWin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedBikvSignature {
    candidate_median_latency_ns: u64,
    dense_median_latency_ns: u64,
    live_tokens: usize,
    selected_live_tokens: usize,
    mapped_pages: usize,
    selected_pages: usize,
    boolean_index_bytes_read: u64,
    dense_numerical_kv_bytes: u64,
    selected_numerical_kv_bytes: u64,
    avoided_numerical_kv_bytes: u64,
    correctness_gate_passed: bool,
    quality_gate_passed: bool,
    physical_dram_traffic_claim: bool,
    model_quality_claim: bool,
    decision: ObservedBikvDecision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatBikvExecutedEvidenceV1 {
    measured_commit: String,
    evidence_checksum: String,
    signature_bits: usize,
    max_distance: usize,
    timing_scope: String,
    selection_policy: String,
    signature: ObservedBikvSignature,
}

#[derive(Debug)]
pub enum FlatBikvExecutedEvidenceError {
    Json(serde_json::Error),
    UnsupportedSchema,
    UnsupportedChecksumAlgorithm,
    InvalidChecksumEncoding,
    EvidenceChecksumMismatch,
    CandidateChecksumMismatch,
    DenseChecksumMismatch,
    InvalidCommitSha,
    EmptyField(&'static str),
    InvalidProblem(&'static str),
    ProvenanceMismatch(&'static str),
    DuplicateBenchmarkId,
    InvalidSelection,
    InvalidScope,
    InvalidAccounting(&'static str),
    InvalidPhaseMedians,
    InvalidGates,
    InvalidPromotionDecision,
    Evidence(EvidenceError),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkEnvironmentWire {
    device: String,
    backend: String,
    driver: String,
    os: String,
    arch: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkProblemWire {
    precision: String,
    batch: usize,
    q_heads: usize,
    kv_heads: usize,
    query_len: usize,
    kv_len: usize,
    head_dim: usize,
    causal: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkProtocolWire {
    warmup_iterations: u32,
    measured_iterations: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkResultWire {
    median_latency_ns: u64,
    p95_latency_ns: u64,
    tokens_per_second_milli: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChecksumWire {
    algorithm: String,
    value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkManifestWire {
    schema_version: u16,
    commit_sha: String,
    benchmark_id: String,
    command: String,
    environment: BenchmarkEnvironmentWire,
    problem: BenchmarkProblemWire,
    protocol: BenchmarkProtocolWire,
    result: BenchmarkResultWire,
    result_checksum: ChecksumWire,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionWire {
    signature_bits: usize,
    max_distance: usize,
    policy: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeWire {
    timing: String,
    q_device_resident: bool,
    q_host_mirror_retained: bool,
    kv_device_resident: bool,
    uploads_readbacks_excluded: bool,
    resident_only_production_claim: bool,
    gpu_timestamp_claim: bool,
    physical_dram_traffic_claim: bool,
    model_quality_claim: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountingWire {
    live_tokens: usize,
    selected_live_tokens: usize,
    mapped_pages: usize,
    selected_pages: usize,
    page_size: usize,
    kv_heads: usize,
    head_dim: usize,
    scalar_bytes: usize,
    boolean_index_bytes_read: u64,
    kv_bytes_per_token: u64,
    dense_numerical_kv_bytes: u64,
    selected_numerical_kv_bytes: u64,
    avoided_numerical_kv_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhaseMediansWire {
    signature_generation: u64,
    boolean_search: u64,
    selected_attention: u64,
    synchronization: u64,
    dense_attention: u64,
    diagnostic_sum: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct GatesWire {
    all_accept_k6_vs_m16: bool,
    sparse_k6_vs_restricted_oracle: bool,
    correctness_gate_passed: bool,
    quality_gate_passed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct BikvEvidenceWire {
    schema_version: u16,
    candidate: BenchmarkManifestWire,
    dense_baseline: BenchmarkManifestWire,
    selection: SelectionWire,
    scope: ScopeWire,
    accounting: AccountingWire,
    phase_medians_ns: PhaseMediansWire,
    gates: GatesWire,
    promotion_decision: String,
    evidence_checksum: ChecksumWire,
}

impl FlatBikvExecutedEvidenceV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, FlatBikvExecutedEvidenceError> {
        let wire: BikvEvidenceWire =
            serde_json::from_str(json).map_err(FlatBikvExecutedEvidenceError::Json)?;
        validate_checksum(&wire.evidence_checksum)?;
        validate_evidence_checksum(json, &wire.evidence_checksum.value)?;

        if wire.schema_version != FLAT_BIKV_EVIDENCE_SCHEMA_VERSION
            || wire.candidate.schema_version != FLAT_BIKV_EVIDENCE_SCHEMA_VERSION
            || wire.dense_baseline.schema_version != FLAT_BIKV_EVIDENCE_SCHEMA_VERSION
        {
            return Err(FlatBikvExecutedEvidenceError::UnsupportedSchema);
        }

        validate_benchmark(&wire.candidate, false)?;
        validate_benchmark(&wire.dense_baseline, true)?;
        validate_pair(&wire.candidate, &wire.dense_baseline)?;
        validate_selection(&wire.selection)?;
        validate_scope(&wire.scope)?;
        validate_accounting(&wire.accounting, &wire.candidate.problem)?;
        validate_phase_medians(
            &wire.phase_medians_ns,
            wire.dense_baseline.result.median_latency_ns,
        )?;
        validate_gates(&wire.gates)?;

        let decision = expected_decision(
            wire.gates,
            wire.candidate.result.median_latency_ns,
            wire.dense_baseline.result.median_latency_ns,
        );
        if decision_name(decision) != wire.promotion_decision {
            return Err(FlatBikvExecutedEvidenceError::InvalidPromotionDecision);
        }

        let signature = ObservedBikvSignature {
            candidate_median_latency_ns: wire.candidate.result.median_latency_ns,
            dense_median_latency_ns: wire.dense_baseline.result.median_latency_ns,
            live_tokens: wire.accounting.live_tokens,
            selected_live_tokens: wire.accounting.selected_live_tokens,
            mapped_pages: wire.accounting.mapped_pages,
            selected_pages: wire.accounting.selected_pages,
            boolean_index_bytes_read: wire.accounting.boolean_index_bytes_read,
            dense_numerical_kv_bytes: wire.accounting.dense_numerical_kv_bytes,
            selected_numerical_kv_bytes: wire.accounting.selected_numerical_kv_bytes,
            avoided_numerical_kv_bytes: wire.accounting.avoided_numerical_kv_bytes,
            correctness_gate_passed: wire.gates.correctness_gate_passed,
            quality_gate_passed: wire.gates.quality_gate_passed,
            physical_dram_traffic_claim: wire.scope.physical_dram_traffic_claim,
            model_quality_claim: wire.scope.model_quality_claim,
            decision,
        };

        Ok(Self {
            measured_commit: wire.candidate.commit_sha,
            evidence_checksum: wire.evidence_checksum.value,
            signature_bits: wire.selection.signature_bits,
            max_distance: wire.selection.max_distance,
            timing_scope: wire.scope.timing,
            selection_policy: wire.selection.policy,
            signature,
        })
    }

    #[must_use]
    pub fn measured_commit(&self) -> &str {
        &self.measured_commit
    }

    #[must_use]
    pub fn evidence_checksum(&self) -> &str {
        &self.evidence_checksum
    }

    #[must_use]
    pub const fn signature_bits(&self) -> usize {
        self.signature_bits
    }

    #[must_use]
    pub const fn max_distance(&self) -> usize {
        self.max_distance
    }

    #[must_use]
    pub fn timing_scope(&self) -> &str {
        &self.timing_scope
    }

    #[must_use]
    pub fn selection_policy(&self) -> &str {
        &self.selection_policy
    }

    #[must_use]
    pub const fn signature(&self) -> &ObservedBikvSignature {
        &self.signature
    }

    pub fn evidence_source(&self) -> Result<EvidenceSource, FlatBikvExecutedEvidenceError> {
        EvidenceSource::new("FLAT-ATTENTION/BKV-K6", self.measured_commit.clone())
            .map_err(FlatBikvExecutedEvidenceError::Evidence)?
            .with_content_hash(format!("fnv1a64:{}", self.evidence_checksum))
            .map_err(FlatBikvExecutedEvidenceError::Evidence)
    }
}

impl ObservedBikvSignature {
    #[must_use]
    pub const fn candidate_median_latency_ns(&self) -> u64 {
        self.candidate_median_latency_ns
    }

    #[must_use]
    pub const fn dense_median_latency_ns(&self) -> u64 {
        self.dense_median_latency_ns
    }

    #[must_use]
    pub const fn latency_advantage_ns(&self) -> i128 {
        self.dense_median_latency_ns as i128 - self.candidate_median_latency_ns as i128
    }

    #[must_use]
    pub const fn live_tokens(&self) -> usize {
        self.live_tokens
    }

    #[must_use]
    pub const fn selected_live_tokens(&self) -> usize {
        self.selected_live_tokens
    }

    #[must_use]
    pub const fn mapped_pages(&self) -> usize {
        self.mapped_pages
    }

    #[must_use]
    pub const fn selected_pages(&self) -> usize {
        self.selected_pages
    }

    #[must_use]
    pub const fn boolean_index_bytes_read(&self) -> u64 {
        self.boolean_index_bytes_read
    }

    #[must_use]
    pub const fn dense_numerical_kv_bytes(&self) -> u64 {
        self.dense_numerical_kv_bytes
    }

    #[must_use]
    pub const fn selected_numerical_kv_bytes(&self) -> u64 {
        self.selected_numerical_kv_bytes
    }

    #[must_use]
    pub const fn avoided_numerical_kv_bytes(&self) -> u64 {
        self.avoided_numerical_kv_bytes
    }

    #[must_use]
    pub const fn correctness_gate_passed(&self) -> bool {
        self.correctness_gate_passed
    }

    #[must_use]
    pub const fn quality_gate_passed(&self) -> bool {
        self.quality_gate_passed
    }

    #[must_use]
    pub const fn physical_dram_traffic_claim(&self) -> bool {
        self.physical_dram_traffic_claim
    }

    #[must_use]
    pub const fn model_quality_claim(&self) -> bool {
        self.model_quality_claim
    }

    #[must_use]
    pub const fn decision(&self) -> ObservedBikvDecision {
        self.decision
    }
}

fn validate_benchmark(
    manifest: &BenchmarkManifestWire,
    dense: bool,
) -> Result<(), FlatBikvExecutedEvidenceError> {
    if manifest.commit_sha.len() != 40
        || !manifest
            .commit_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(FlatBikvExecutedEvidenceError::InvalidCommitSha);
    }
    for (field, value) in [
        ("benchmark_id", manifest.benchmark_id.as_str()),
        ("command", manifest.command.as_str()),
        ("environment.device", manifest.environment.device.as_str()),
        ("environment.backend", manifest.environment.backend.as_str()),
        ("environment.driver", manifest.environment.driver.as_str()),
        ("environment.os", manifest.environment.os.as_str()),
        ("environment.arch", manifest.environment.arch.as_str()),
        ("problem.precision", manifest.problem.precision.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(FlatBikvExecutedEvidenceError::EmptyField(field));
        }
    }
    for (field, value) in [
        ("batch", manifest.problem.batch),
        ("q_heads", manifest.problem.q_heads),
        ("kv_heads", manifest.problem.kv_heads),
        ("query_len", manifest.problem.query_len),
        ("kv_len", manifest.problem.kv_len),
        ("head_dim", manifest.problem.head_dim),
    ] {
        if value == 0 {
            return Err(FlatBikvExecutedEvidenceError::InvalidProblem(field));
        }
    }
    if !manifest.problem.q_heads.is_multiple_of(manifest.problem.kv_heads) {
        return Err(FlatBikvExecutedEvidenceError::InvalidProblem(
            "head_grouping",
        ));
    }
    if manifest.protocol.measured_iterations == 0 {
        return Err(FlatBikvExecutedEvidenceError::InvalidProblem(
            "measured_iterations",
        ));
    }
    if manifest.result.p95_latency_ns < manifest.result.median_latency_ns {
        return Err(FlatBikvExecutedEvidenceError::InvalidProblem(
            "latency_percentiles",
        ));
    }
    if manifest.result.median_latency_ns == 0 {
        return Err(FlatBikvExecutedEvidenceError::InvalidProblem(if dense {
            "dense_median_latency"
        } else {
            "candidate_median_latency"
        }));
    }
    validate_result_checksum(manifest, dense)
}

fn validate_result_checksum(
    manifest: &BenchmarkManifestWire,
    dense: bool,
) -> Result<(), FlatBikvExecutedEvidenceError> {
    validate_checksum(&manifest.result_checksum)?;
    let result_json = format!(
        "{{\"median_latency_ns\":{},\"p95_latency_ns\":{},\"tokens_per_second_milli\":{}}}",
        manifest.result.median_latency_ns,
        manifest.result.p95_latency_ns,
        manifest.result.tokens_per_second_milli
    );
    let expected = format!("{:016x}", fnv1a64(result_json.as_bytes()));
    if manifest.result_checksum.value != expected {
        return Err(if dense {
            FlatBikvExecutedEvidenceError::DenseChecksumMismatch
        } else {
            FlatBikvExecutedEvidenceError::CandidateChecksumMismatch
        });
    }
    Ok(())
}

fn validate_pair(
    candidate: &BenchmarkManifestWire,
    dense: &BenchmarkManifestWire,
) -> Result<(), FlatBikvExecutedEvidenceError> {
    if candidate.commit_sha != dense.commit_sha {
        return Err(FlatBikvExecutedEvidenceError::ProvenanceMismatch(
            "commit",
        ));
    }
    if candidate.environment != dense.environment {
        return Err(FlatBikvExecutedEvidenceError::ProvenanceMismatch(
            "environment",
        ));
    }
    if candidate.problem != dense.problem {
        return Err(FlatBikvExecutedEvidenceError::ProvenanceMismatch(
            "problem",
        ));
    }
    if candidate.protocol != dense.protocol {
        return Err(FlatBikvExecutedEvidenceError::ProvenanceMismatch(
            "protocol",
        ));
    }
    if candidate.benchmark_id == dense.benchmark_id {
        return Err(FlatBikvExecutedEvidenceError::DuplicateBenchmarkId);
    }
    Ok(())
}

fn validate_selection(selection: &SelectionWire) -> Result<(), FlatBikvExecutedEvidenceError> {
    if selection.signature_bits == 0
        || selection.max_distance > selection.signature_bits
        || selection.policy.trim().is_empty()
    {
        return Err(FlatBikvExecutedEvidenceError::InvalidSelection);
    }
    Ok(())
}

fn validate_scope(scope: &ScopeWire) -> Result<(), FlatBikvExecutedEvidenceError> {
    if scope.timing.trim().is_empty() || scope.q_host_mirror_retained && scope.resident_only_production_claim
    {
        return Err(FlatBikvExecutedEvidenceError::InvalidScope);
    }
    Ok(())
}

fn validate_accounting(
    accounting: &AccountingWire,
    problem: &BenchmarkProblemWire,
) -> Result<(), FlatBikvExecutedEvidenceError> {
    for (field, value) in [
        ("live_tokens", accounting.live_tokens),
        ("mapped_pages", accounting.mapped_pages),
        ("page_size", accounting.page_size),
        ("kv_heads", accounting.kv_heads),
        ("head_dim", accounting.head_dim),
        ("scalar_bytes", accounting.scalar_bytes),
    ] {
        if value == 0 {
            return Err(FlatBikvExecutedEvidenceError::InvalidAccounting(field));
        }
    }
    if accounting.live_tokens != problem.kv_len
        || accounting.kv_heads != problem.kv_heads
        || accounting.head_dim != problem.head_dim
    {
        return Err(FlatBikvExecutedEvidenceError::InvalidAccounting(
            "problem_geometry",
        ));
    }
    if accounting.selected_pages > accounting.mapped_pages
        || accounting.selected_live_tokens > accounting.live_tokens
        || accounting.boolean_index_bytes_read == 0
    {
        return Err(FlatBikvExecutedEvidenceError::InvalidAccounting(
            "selection_bounds",
        ));
    }
    let required_pages = accounting
        .live_tokens
        .checked_add(accounting.page_size - 1)
        .ok_or(FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))?
        / accounting.page_size;
    if accounting.mapped_pages != required_pages {
        return Err(FlatBikvExecutedEvidenceError::InvalidAccounting(
            "mapped_pages",
        ));
    }
    if (accounting.selected_pages == 0) != (accounting.selected_live_tokens == 0) {
        return Err(FlatBikvExecutedEvidenceError::InvalidAccounting(
            "empty_selection",
        ));
    }
    if accounting.selected_pages > 0 {
        let selected_capacity = accounting
            .selected_pages
            .checked_mul(accounting.page_size)
            .ok_or(FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))?;
        let final_page_tokens = match accounting.live_tokens % accounting.page_size {
            0 => accounting.page_size,
            value => value,
        };
        let selected_with_final = accounting
            .selected_pages
            .checked_sub(1)
            .and_then(|pages| pages.checked_mul(accounting.page_size))
            .and_then(|tokens| tokens.checked_add(final_page_tokens))
            .ok_or(FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))?;
        if accounting.selected_live_tokens != selected_capacity
            && accounting.selected_live_tokens != selected_with_final
        {
            return Err(FlatBikvExecutedEvidenceError::InvalidAccounting(
                "selected_live_tokens",
            ));
        }
    }

    let kv_bytes_per_token = checked_product(&[
        2,
        u64::try_from(accounting.kv_heads)
            .map_err(|_| FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))?,
        u64::try_from(accounting.head_dim)
            .map_err(|_| FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))?,
        u64::try_from(accounting.scalar_bytes)
            .map_err(|_| FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))?,
    ])?;
    let dense_bytes = u64::try_from(accounting.live_tokens)
        .ok()
        .and_then(|tokens| tokens.checked_mul(kv_bytes_per_token))
        .ok_or(FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))?;
    let selected_bytes = u64::try_from(accounting.selected_live_tokens)
        .ok()
        .and_then(|tokens| tokens.checked_mul(kv_bytes_per_token))
        .ok_or(FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))?;
    let avoided_bytes = dense_bytes
        .checked_sub(selected_bytes)
        .ok_or(FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))?;
    if accounting.kv_bytes_per_token != kv_bytes_per_token
        || accounting.dense_numerical_kv_bytes != dense_bytes
        || accounting.selected_numerical_kv_bytes != selected_bytes
        || accounting.avoided_numerical_kv_bytes != avoided_bytes
    {
        return Err(FlatBikvExecutedEvidenceError::InvalidAccounting(
            "derived_bytes",
        ));
    }
    Ok(())
}

fn validate_phase_medians(
    phase: &PhaseMediansWire,
    dense_median_latency_ns: u64,
) -> Result<(), FlatBikvExecutedEvidenceError> {
    if phase.dense_attention != dense_median_latency_ns {
        return Err(FlatBikvExecutedEvidenceError::InvalidPhaseMedians);
    }
    let diagnostic_sum = phase
        .signature_generation
        .checked_add(phase.boolean_search)
        .and_then(|value| value.checked_add(phase.selected_attention))
        .and_then(|value| value.checked_add(phase.synchronization))
        .ok_or(FlatBikvExecutedEvidenceError::InvalidPhaseMedians)?;
    if diagnostic_sum == 0 || diagnostic_sum != phase.diagnostic_sum {
        return Err(FlatBikvExecutedEvidenceError::InvalidPhaseMedians);
    }
    Ok(())
}

fn validate_gates(gates: &GatesWire) -> Result<(), FlatBikvExecutedEvidenceError> {
    if gates.correctness_gate_passed
        != (gates.all_accept_k6_vs_m16 && gates.sparse_k6_vs_restricted_oracle)
    {
        return Err(FlatBikvExecutedEvidenceError::InvalidGates);
    }
    Ok(())
}

fn expected_decision(
    gates: GatesWire,
    candidate_median_latency_ns: u64,
    dense_median_latency_ns: u64,
) -> ObservedBikvDecision {
    if !gates.correctness_gate_passed {
        return ObservedBikvDecision::FallbackCorrectnessGate;
    }
    if !gates.quality_gate_passed {
        return ObservedBikvDecision::FallbackQualityGate;
    }
    if candidate_median_latency_ns >= dense_median_latency_ns {
        return ObservedBikvDecision::FallbackNoLatencyWin;
    }
    ObservedBikvDecision::Promote
}

const fn decision_name(decision: ObservedBikvDecision) -> &'static str {
    match decision {
        ObservedBikvDecision::Promote => "promote",
        ObservedBikvDecision::FallbackQualityGate => "fallback_quality_gate",
        ObservedBikvDecision::FallbackCorrectnessGate => "fallback_correctness_gate",
        ObservedBikvDecision::FallbackNoLatencyWin => "fallback_no_latency_win",
    }
}

fn validate_evidence_checksum(
    json: &str,
    recorded: &str,
) -> Result<(), FlatBikvExecutedEvidenceError> {
    const MARKER: &str = ",\"evidence_checksum\":";
    let index = json
        .rfind(MARKER)
        .ok_or(FlatBikvExecutedEvidenceError::EvidenceChecksumMismatch)?;
    let payload = &json[..index];
    let expected = format!("{:016x}", fnv1a64(payload.as_bytes()));
    if recorded != expected {
        return Err(FlatBikvExecutedEvidenceError::EvidenceChecksumMismatch);
    }
    Ok(())
}

fn validate_checksum(checksum: &ChecksumWire) -> Result<(), FlatBikvExecutedEvidenceError> {
    if checksum.algorithm != "fnv1a64" {
        return Err(FlatBikvExecutedEvidenceError::UnsupportedChecksumAlgorithm);
    }
    if checksum.value.len() != 16
        || !checksum
            .value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(FlatBikvExecutedEvidenceError::InvalidChecksumEncoding);
    }
    Ok(())
}

fn checked_product(values: &[u64]) -> Result<u64, FlatBikvExecutedEvidenceError> {
    values.iter().try_fold(1_u64, |accumulator, value| {
        accumulator
            .checked_mul(*value)
            .ok_or(FlatBikvExecutedEvidenceError::InvalidAccounting("overflow"))
    })
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

impl fmt::Display for FlatBikvExecutedEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid FLAT BIKV evidence JSON: {error}"),
            Self::UnsupportedSchema => formatter.write_str("unsupported FLAT BIKV evidence schema"),
            Self::UnsupportedChecksumAlgorithm => {
                formatter.write_str("unsupported FLAT BIKV checksum algorithm")
            }
            Self::InvalidChecksumEncoding => {
                formatter.write_str("invalid FLAT BIKV checksum encoding")
            }
            Self::EvidenceChecksumMismatch => {
                formatter.write_str("FLAT BIKV evidence checksum does not match canonical payload")
            }
            Self::CandidateChecksumMismatch => {
                formatter.write_str("FLAT BIKV candidate result checksum mismatch")
            }
            Self::DenseChecksumMismatch => {
                formatter.write_str("FLAT BIKV dense result checksum mismatch")
            }
            Self::InvalidCommitSha => formatter.write_str("invalid FLAT BIKV measured commit SHA"),
            Self::EmptyField(field) => write!(formatter, "empty FLAT BIKV field {field}"),
            Self::InvalidProblem(field) => write!(formatter, "invalid FLAT BIKV problem field {field}"),
            Self::ProvenanceMismatch(field) => {
                write!(formatter, "FLAT BIKV candidate/dense {field} mismatch")
            }
            Self::DuplicateBenchmarkId => {
                formatter.write_str("FLAT BIKV candidate and dense benchmark IDs must differ")
            }
            Self::InvalidSelection => formatter.write_str("invalid FLAT BIKV selection contract"),
            Self::InvalidScope => formatter.write_str("invalid FLAT BIKV measurement scope"),
            Self::InvalidAccounting(field) => {
                write!(formatter, "invalid FLAT BIKV accounting field {field}")
            }
            Self::InvalidPhaseMedians => formatter.write_str("invalid FLAT BIKV phase medians"),
            Self::InvalidGates => formatter.write_str("invalid FLAT BIKV correctness gates"),
            Self::InvalidPromotionDecision => {
                formatter.write_str("FLAT BIKV promotion decision does not match measured gates/latency")
            }
            Self::Evidence(error) => write!(formatter, "invalid ProspectEngine evidence source: {error}"),
        }
    }
}

impl std::error::Error for FlatBikvExecutedEvidenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Evidence(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        fnv1a64, FlatBikvExecutedEvidenceError, FlatBikvExecutedEvidenceV1,
        ObservedBikvDecision,
    };

    fn result_json(median: u64, p95: u64, throughput: u64) -> String {
        format!(
            "{{\"median_latency_ns\":{median},\"p95_latency_ns\":{p95},\"tokens_per_second_milli\":{throughput}}}"
        )
    }

    fn result_checksum(median: u64, p95: u64, throughput: u64) -> String {
        format!("{:016x}", fnv1a64(result_json(median, p95, throughput).as_bytes()))
    }

    fn fixture() -> String {
        let commit = "7a4eec7dbb90627dde800bc1c3c90dbdd890d6d1";
        let candidate_checksum = result_checksum(900, 1_000, 2_000);
        let dense_checksum = result_checksum(1_200, 1_300, 1_500);
        let candidate = json!({
            "schema_version": 1,
            "commit_sha": commit,
            "benchmark_id": "bikv-k6-candidate",
            "command": "cargo run --release --features wgpu --example bikv",
            "environment": {"device":"test-gpu","backend":"Vulkan","driver":"test-driver","os":"Linux","arch":"x86_64"},
            "problem": {"precision":"f32","batch":1,"q_heads":4,"kv_heads":4,"query_len":1,"kv_len":230,"head_dim":64,"causal":true},
            "protocol": {"warmup_iterations":3,"measured_iterations":11},
            "result": {"median_latency_ns":900,"p95_latency_ns":1000,"tokens_per_second_milli":2000},
            "result_checksum": {"algorithm":"fnv1a64","value":candidate_checksum}
        });
        let dense = json!({
            "schema_version": 1,
            "commit_sha": commit,
            "benchmark_id": "m16-dense",
            "command": "cargo run --release --features wgpu --example bikv",
            "environment": {"device":"test-gpu","backend":"Vulkan","driver":"test-driver","os":"Linux","arch":"x86_64"},
            "problem": {"precision":"f32","batch":1,"q_heads":4,"kv_heads":4,"query_len":1,"kv_len":230,"head_dim":64,"causal":true},
            "protocol": {"warmup_iterations":3,"measured_iterations":11},
            "result": {"median_latency_ns":1200,"p95_latency_ns":1300,"tokens_per_second_milli":1500},
            "result_checksum": {"algorithm":"fnv1a64","value":dense_checksum}
        });
        let mut payload = format!(
            "{{\"schema_version\":1,\"candidate\":{},\"dense_baseline\":{},\"selection\":{{\"signature_bits\":256,\"max_distance\":64,\"policy\":\"hamming\"}},\"scope\":{{\"timing\":\"host_wall_clock\",\"q_device_resident\":true,\"q_host_mirror_retained\":true,\"kv_device_resident\":true,\"uploads_readbacks_excluded\":true,\"resident_only_production_claim\":false,\"gpu_timestamp_claim\":false,\"physical_dram_traffic_claim\":false,\"model_quality_claim\":false}},\"accounting\":{{\"live_tokens\":230,\"selected_live_tokens\":128,\"mapped_pages\":4,\"selected_pages\":2,\"page_size\":64,\"kv_heads\":4,\"head_dim\":64,\"scalar_bytes\":4,\"boolean_index_bytes_read\":128,\"kv_bytes_per_token\":2048,\"dense_numerical_kv_bytes\":471040,\"selected_numerical_kv_bytes\":262144,\"avoided_numerical_kv_bytes\":208896}},\"phase_medians_ns\":{{\"signature_generation\":100,\"boolean_search\":200,\"selected_attention\":600,\"synchronization\":50,\"dense_attention\":1200,\"diagnostic_sum\":950}},\"gates\":{{\"all_accept_k6_vs_m16\":true,\"sparse_k6_vs_restricted_oracle\":true,\"correctness_gate_passed\":true,\"quality_gate_passed\":true}},\"promotion_decision\":\"promote\"",
            serde_json::to_string(&candidate).expect("candidate"),
            serde_json::to_string(&dense).expect("dense")
        );
        let checksum = fnv1a64(payload.as_bytes());
        payload.push_str(&format!(
            ",\"evidence_checksum\":{{\"algorithm\":\"fnv1a64\",\"value\":\"{checksum:016x}\"}}}}"
        ));
        payload
    }

    #[test]
    fn ingests_executed_bikv_evidence_without_using_phase_sum_as_end_to_end_latency() {
        let evidence = FlatBikvExecutedEvidenceV1::from_canonical_json(&fixture()).expect("evidence");
        let signature = evidence.signature();

        assert_eq!(signature.candidate_median_latency_ns(), 900);
        assert_eq!(signature.dense_median_latency_ns(), 1_200);
        assert_eq!(signature.latency_advantage_ns(), 300);
        assert_eq!(signature.avoided_numerical_kv_bytes(), 208_896);
        assert_eq!(signature.decision(), ObservedBikvDecision::Promote);
        assert!(!signature.physical_dram_traffic_claim());
        assert!(!signature.model_quality_claim());
        assert_eq!(evidence.max_distance(), 64);

        let source = evidence.evidence_source().expect("source");
        assert_eq!(source.revision(), evidence.measured_commit());
        assert_eq!(source.nature(), prospect_evidence::EvidenceNature::Observed);
    }

    #[test]
    fn rejects_tampered_top_level_checksum() {
        let tampered = fixture().replace("\"median_latency_ns\":900", "\"median_latency_ns\":899");
        assert!(matches!(
            FlatBikvExecutedEvidenceV1::from_canonical_json(&tampered),
            Err(FlatBikvExecutedEvidenceError::EvidenceChecksumMismatch)
        ));
    }

    #[test]
    fn rejects_promotion_decision_that_disagrees_with_observed_latency() {
        let original = fixture();
        let marker = ",\"evidence_checksum\":";
        let index = original.rfind(marker).expect("marker");
        let mut payload = original[..index]
            .replace("\"promotion_decision\":\"promote\"", "\"promotion_decision\":\"fallback_no_latency_win\"");
        let checksum = fnv1a64(payload.as_bytes());
        payload.push_str(&format!(
            ",\"evidence_checksum\":{{\"algorithm\":\"fnv1a64\",\"value\":\"{checksum:016x}\"}}}}"
        ));
        assert!(matches!(
            FlatBikvExecutedEvidenceV1::from_canonical_json(&payload),
            Err(FlatBikvExecutedEvidenceError::InvalidPromotionDecision)
        ));
    }
}
