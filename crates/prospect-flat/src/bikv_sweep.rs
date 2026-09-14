use core::cmp::Ordering;
use core::fmt;

use serde::Deserialize;

use crate::{
    FlatBikvExecutedEvidenceError, FlatBikvExecutedEvidenceV1, ObservedBikvDecision,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedBikvSweep {
    comparison: ComparisonKey,
    records: Vec<FlatBikvExecutedEvidenceV1>,
}

#[derive(Debug)]
pub enum ObservedBikvSweepError {
    Empty,
    Evidence {
        index: usize,
        source: FlatBikvExecutedEvidenceError,
    },
    MetadataJson {
        index: usize,
        source: serde_json::Error,
    },
    ComparisonMismatch {
        index: usize,
    },
    DuplicateThreshold {
        threshold: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ComparisonKey {
    commit_sha: String,
    candidate_benchmark_id: String,
    dense_benchmark_id: String,
    environment: EnvironmentKey,
    problem: ProblemKey,
    protocol: ProtocolKey,
    signature_bits: usize,
    selection_policy: String,
    scope: ScopeKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct EnvironmentKey {
    device: String,
    backend: String,
    driver: String,
    os: String,
    arch: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct ProblemKey {
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
struct ProtocolKey {
    warmup_iterations: u32,
    measured_iterations: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct ScopeKey {
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

#[derive(Deserialize)]
struct SweepManifestWire {
    commit_sha: String,
    benchmark_id: String,
    environment: EnvironmentKey,
    problem: ProblemKey,
    protocol: ProtocolKey,
}

#[derive(Deserialize)]
struct SweepSelectionWire {
    signature_bits: usize,
    policy: String,
}

#[derive(Deserialize)]
struct SweepWire {
    candidate: SweepManifestWire,
    dense_baseline: SweepManifestWire,
    selection: SweepSelectionWire,
    scope: ScopeKey,
}

impl ObservedBikvSweep {
    /// Build a measured multi-threshold sweep without interpolation.
    ///
    /// Every envelope is first fully revalidated by
    /// [`FlatBikvExecutedEvidenceV1::from_canonical_json`]. The sweep then
    /// requires the same measured commit, benchmark identities, environment,
    /// attention problem, protocol, signature width, selection policy, and
    /// measurement scope. Commands are intentionally excluded from this key
    /// because K6.4 encodes the Hamming threshold in the command line.
    pub fn from_canonical_json_records<I, S>(records: I) -> Result<Self, ObservedBikvSweepError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut parsed = Vec::new();
        let mut comparison = None;

        for (index, json) in records.into_iter().enumerate() {
            let json = json.as_ref();
            let evidence = FlatBikvExecutedEvidenceV1::from_canonical_json(json).map_err(|source| {
                ObservedBikvSweepError::Evidence { index, source }
            })?;
            let wire: SweepWire = serde_json::from_str(json)
                .map_err(|source| ObservedBikvSweepError::MetadataJson { index, source })?;
            let key = ComparisonKey {
                commit_sha: wire.candidate.commit_sha,
                candidate_benchmark_id: wire.candidate.benchmark_id,
                dense_benchmark_id: wire.dense_baseline.benchmark_id,
                environment: wire.candidate.environment,
                problem: wire.candidate.problem,
                protocol: wire.candidate.protocol,
                signature_bits: wire.selection.signature_bits,
                selection_policy: wire.selection.policy,
                scope: wire.scope,
            };

            if let Some(reference) = &comparison {
                if reference != &key {
                    return Err(ObservedBikvSweepError::ComparisonMismatch { index });
                }
            } else {
                comparison = Some(key);
            }
            parsed.push(evidence);
        }

        let Some(comparison) = comparison else {
            return Err(ObservedBikvSweepError::Empty);
        };

        parsed.sort_by_key(FlatBikvExecutedEvidenceV1::max_distance);
        if let Some(pair) = parsed
            .windows(2)
            .find(|pair| pair[0].max_distance() == pair[1].max_distance())
        {
            return Err(ObservedBikvSweepError::DuplicateThreshold {
                threshold: pair[0].max_distance(),
            });
        }

        Ok(Self {
            comparison,
            records: parsed,
        })
    }

    #[must_use]
    pub fn records(&self) -> &[FlatBikvExecutedEvidenceV1] {
        &self.records
    }

    #[must_use]
    pub fn measured_commit(&self) -> &str {
        &self.comparison.commit_sha
    }

    #[must_use]
    pub fn record_for_threshold(&self, threshold: usize) -> Option<&FlatBikvExecutedEvidenceV1> {
        self.records
            .binary_search_by_key(&threshold, FlatBikvExecutedEvidenceV1::max_distance)
            .ok()
            .map(|index| &self.records[index])
    }

    /// Select the best measured threshold that passed every promotion gate.
    ///
    /// Candidate medians from separate runs are normalized by their paired
    /// dense medians. Ratios are compared exactly by cross multiplication in
    /// `u128`, avoiding floating-point ordering and avoiding the false
    /// assumption that the dense baseline latency is identical across runs.
    #[must_use]
    pub fn best_promotable_latency(&self) -> Option<&FlatBikvExecutedEvidenceV1> {
        self.records
            .iter()
            .filter(|record| record.signature().decision() == ObservedBikvDecision::Promote)
            .min_by(|left, right| compare_relative_latency(left, right))
    }
}

fn compare_relative_latency(
    left: &FlatBikvExecutedEvidenceV1,
    right: &FlatBikvExecutedEvidenceV1,
) -> Ordering {
    let left_signature = left.signature();
    let right_signature = right.signature();
    let left_cross = u128::from(left_signature.candidate_median_latency_ns())
        * u128::from(right_signature.dense_median_latency_ns());
    let right_cross = u128::from(right_signature.candidate_median_latency_ns())
        * u128::from(left_signature.dense_median_latency_ns());

    left_cross
        .cmp(&right_cross)
        .then_with(|| {
            left_signature
                .candidate_median_latency_ns()
                .cmp(&right_signature.candidate_median_latency_ns())
        })
        .then_with(|| left.max_distance().cmp(&right.max_distance()))
}

impl fmt::Display for ObservedBikvSweepError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("observed BIKV sweep requires at least one record"),
            Self::Evidence { index, source } => {
                write!(formatter, "invalid BIKV evidence record {index}: {source}")
            }
            Self::MetadataJson { index, source } => {
                write!(formatter, "invalid BIKV sweep metadata record {index}: {source}")
            }
            Self::ComparisonMismatch { index } => write!(
                formatter,
                "BIKV sweep record {index} is not comparable with the reference record"
            ),
            Self::DuplicateThreshold { threshold } => {
                write!(formatter, "duplicate BIKV Hamming threshold {threshold}")
            }
        }
    }
}

impl std::error::Error for ObservedBikvSweepError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evidence { source, .. } => Some(source),
            Self::MetadataJson { source, .. } => Some(source),
            Self::Empty | Self::ComparisonMismatch { .. } | Self::DuplicateThreshold { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ObservedBikvSweep, ObservedBikvSweepError};

    fn fnv1a64(bytes: &[u8]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    fn result_checksum(median: u64, p95: u64, throughput: u64) -> String {
        let result = format!(
            "{{\"median_latency_ns\":{median},\"p95_latency_ns\":{p95},\"tokens_per_second_milli\":{throughput}}}"
        );
        format!("{:016x}", fnv1a64(result.as_bytes()))
    }

    fn fixture(threshold: usize, candidate_ns: u64, dense_ns: u64, device: &str) -> String {
        let commit = "7a4eec7dbb90627dde800bc1c3c90dbdd890d6d1";
        let candidate_p95 = candidate_ns + 100;
        let dense_p95 = dense_ns + 100;
        let candidate_checksum = result_checksum(candidate_ns, candidate_p95, 2_000);
        let dense_checksum = result_checksum(dense_ns, dense_p95, 1_500);
        let candidate = json!({
            "schema_version": 1,
            "commit_sha": commit,
            "benchmark_id": "bkv-k6-selected-candidate",
            "command": format!("FLAT_BKV_QUAL_MAX_DISTANCE={threshold} cargo test --features wgpu --test bkv6_m16_qualification -- --nocapture"),
            "environment": {"device":device,"backend":"Vulkan","driver":"test-driver","os":"Linux","arch":"x86_64"},
            "problem": {"precision":"f32","batch":1,"q_heads":4,"kv_heads":4,"query_len":1,"kv_len":230,"head_dim":64,"causal":true},
            "protocol": {"warmup_iterations":3,"measured_iterations":11},
            "result": {"median_latency_ns":candidate_ns,"p95_latency_ns":candidate_p95,"tokens_per_second_milli":2000},
            "result_checksum": {"algorithm":"fnv1a64","value":candidate_checksum}
        });
        let dense = json!({
            "schema_version": 1,
            "commit_sha": commit,
            "benchmark_id": "m16-dense-baseline",
            "command": format!("FLAT_BKV_QUAL_MAX_DISTANCE={threshold} cargo test --features wgpu --test bkv6_m16_qualification -- --nocapture"),
            "environment": {"device":device,"backend":"Vulkan","driver":"test-driver","os":"Linux","arch":"x86_64"},
            "problem": {"precision":"f32","batch":1,"q_heads":4,"kv_heads":4,"query_len":1,"kv_len":230,"head_dim":64,"causal":true},
            "protocol": {"warmup_iterations":3,"measured_iterations":11},
            "result": {"median_latency_ns":dense_ns,"p95_latency_ns":dense_p95,"tokens_per_second_milli":1500},
            "result_checksum": {"algorithm":"fnv1a64","value":dense_checksum}
        });
        let decision = if candidate_ns < dense_ns {
            "promote"
        } else {
            "fallback_no_latency_win"
        };
        let mut payload = format!(
            "{{\"schema_version\":1,\"candidate\":{},\"dense_baseline\":{},\"selection\":{{\"signature_bits\":256,\"max_distance\":{threshold},\"policy\":\"hamming\"}},\"scope\":{{\"timing\":\"host_wall_clock\",\"q_device_resident\":true,\"q_host_mirror_retained\":true,\"kv_device_resident\":true,\"uploads_readbacks_excluded\":true,\"resident_only_production_claim\":false,\"gpu_timestamp_claim\":false,\"physical_dram_traffic_claim\":false,\"model_quality_claim\":false}},\"accounting\":{{\"live_tokens\":230,\"selected_live_tokens\":128,\"mapped_pages\":4,\"selected_pages\":2,\"page_size\":64,\"kv_heads\":4,\"head_dim\":64,\"scalar_bytes\":4,\"boolean_index_bytes_read\":128,\"kv_bytes_per_token\":2048,\"dense_numerical_kv_bytes\":471040,\"selected_numerical_kv_bytes\":262144,\"avoided_numerical_kv_bytes\":208896}},\"phase_medians_ns\":{{\"signature_generation\":100,\"boolean_search\":200,\"selected_attention\":600,\"synchronization\":50,\"dense_attention\":{dense_ns},\"diagnostic_sum\":950}},\"gates\":{{\"all_accept_k6_vs_m16\":true,\"sparse_k6_vs_restricted_oracle\":true,\"correctness_gate_passed\":true,\"quality_gate_passed\":true}},\"promotion_decision\":\"{decision}\"",
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
    fn sorts_thresholds_and_selects_best_paired_dense_ratio() {
        let sweep = ObservedBikvSweep::from_canonical_json_records([
            fixture(2, 70, 80, "gpu-a"),
            fixture(0, 95, 100, "gpu-a"),
            fixture(1, 80, 100, "gpu-a"),
        ])
        .expect("comparable sweep");

        let thresholds = sweep
            .records()
            .iter()
            .map(|record| record.max_distance())
            .collect::<Vec<_>>();
        assert_eq!(thresholds, vec![0, 1, 2]);

        let best = sweep.best_promotable_latency().expect("promotable record");
        assert_eq!(best.max_distance(), 1);
        assert_eq!(best.signature().candidate_median_latency_ns(), 80);
        assert_eq!(best.signature().dense_median_latency_ns(), 100);
    }

    #[test]
    fn rejects_cross_environment_sweep() {
        let result = ObservedBikvSweep::from_canonical_json_records([
            fixture(0, 90, 100, "gpu-a"),
            fixture(1, 80, 100, "gpu-b"),
        ]);
        assert!(matches!(
            result,
            Err(ObservedBikvSweepError::ComparisonMismatch { index: 1 })
        ));
    }

    #[test]
    fn rejects_duplicate_threshold_and_never_interpolates_missing_thresholds() {
        let duplicate = ObservedBikvSweep::from_canonical_json_records([
            fixture(1, 90, 100, "gpu-a"),
            fixture(1, 80, 100, "gpu-a"),
        ]);
        assert!(matches!(
            duplicate,
            Err(ObservedBikvSweepError::DuplicateThreshold { threshold: 1 })
        ));

        let sweep = ObservedBikvSweep::from_canonical_json_records([
            fixture(0, 90, 100, "gpu-a"),
            fixture(2, 70, 100, "gpu-a"),
        ])
        .expect("sweep");
        assert!(sweep.record_for_threshold(1).is_none());
    }
}
