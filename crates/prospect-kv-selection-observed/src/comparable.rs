use core::fmt;
use std::collections::BTreeSet;

use crate::evidence::{
    KvlabKvRealModelSelectionEvidenceV1, ObservedSelectionMetricPair, nearly_equal,
};

#[derive(Clone, Debug)]
pub struct ComparableObservedKvSelectionSet {
    records: Vec<KvlabKvRealModelSelectionEvidenceV1>,
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
    pub fn budget_bytes(&self) -> u64 {
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

impl fmt::Display for ComparableObservedKvSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptySet => "comparable observed KV selection set must not be empty",
            Self::ContextMismatch => "observed KV selections do not share one experimental context",
            Self::InputMismatch => "observed KV selections do not share one logical input",
            Self::BaselineMismatch => "observed KV selections do not share one paired baseline",
            Self::BudgetMismatch => "observed KV selections do not share one candidate byte budget",
            Self::DuplicatePolicy => "observed KV selection set contains a duplicate policy label",
        })
    }
}

impl std::error::Error for ComparableObservedKvSelectionError {}
