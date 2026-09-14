#![forbid(unsafe_code)]

mod bikv_executed_evidence;
mod kvlab_handoff;
mod routing_evidence;
pub mod scenarios;

use core::fmt;

use flat_attention::api::boolean_attention_mask::{
    BooleanAttentionMask, BooleanAttentionMaskError,
};
use flat_attention::api::boolean_attention_signature::{
    BooleanAttentionSignature, BooleanAttentionSignatureError, HammingAdmissionRule,
};
use prospect_core::ProspectiveEngine;

pub use bikv_executed_evidence::{
    FLAT_BIKV_EVIDENCE_CONTRACT_REVISION, FLAT_BIKV_EVIDENCE_SCHEMA_VERSION,
    FlatBikvExecutedEvidenceError, FlatBikvExecutedEvidenceV1, ObservedBikvDecision,
    ObservedBikvSignature,
};
pub use kvlab_handoff::{
    KVLAB_BKV_HANDOFF_REVISION, KVLAB_BKV_HANDOFF_SCHEMA_V1, KvlabBkvHandoffError,
    KvlabBkvHandoffV1,
};
pub use routing_evidence::{
    FLAT_BOOLEAN_ROUTING_EVIDENCE_SCHEMA_V1, FlatBooleanRoutingEvidenceError,
    FlatBooleanRoutingEvidenceV1, FlatBooleanRoutingMode,
};

pub const FLAT_ATTENTION_REVISION: &str = "a5b6598ffe475c74c938f45feb86b009d0e4ad0a";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatBooleanAttentionState {
    query: BooleanAttentionSignature,
    keys: Vec<BooleanAttentionSignature>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatBooleanIntervention {
    max_hamming_distance: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlatBooleanContractError {
    EmptyKeys,
    WidthMismatch {
        query_bits: usize,
        key_index: usize,
        key_bits: usize,
    },
    Signature(BooleanAttentionSignatureError),
    Mask(BooleanAttentionMaskError),
}

#[derive(Debug)]
pub enum FlatBooleanEngineError<E> {
    Contract(FlatBooleanContractError),
    Model(E),
}

impl FlatBooleanAttentionState {
    pub fn new(
        query: BooleanAttentionSignature,
        keys: Vec<BooleanAttentionSignature>,
    ) -> Result<Self, FlatBooleanContractError> {
        if keys.is_empty() {
            return Err(FlatBooleanContractError::EmptyKeys);
        }

        for (key_index, key) in keys.iter().enumerate() {
            if key.bits() != query.bits() {
                return Err(FlatBooleanContractError::WidthMismatch {
                    query_bits: query.bits(),
                    key_index,
                    key_bits: key.bits(),
                });
            }
        }

        Ok(Self { query, keys })
    }

    #[must_use]
    pub const fn query(&self) -> &BooleanAttentionSignature {
        &self.query
    }

    #[must_use]
    pub fn keys(&self) -> &[BooleanAttentionSignature] {
        &self.keys
    }

    #[must_use]
    pub fn block_count(&self) -> usize {
        self.keys.len()
    }

    pub fn dense_mask(&self) -> Result<BooleanAttentionMask, FlatBooleanContractError> {
        BooleanAttentionMask::from_admissions(&vec![true; self.keys.len()])
            .map_err(FlatBooleanContractError::Mask)
    }

    pub fn hamming_mask(
        &self,
        max_hamming_distance: usize,
    ) -> Result<BooleanAttentionMask, FlatBooleanContractError> {
        let rule = HammingAdmissionRule::new(max_hamming_distance, self.query.bits())
            .map_err(FlatBooleanContractError::Signature)?;
        let admissions = self
            .keys
            .iter()
            .map(|key| {
                rule.admits(&self.query, key)
                    .map_err(FlatBooleanContractError::Signature)
            })
            .collect::<Result<Vec<_>, _>>()?;

        BooleanAttentionMask::from_admissions(&admissions).map_err(FlatBooleanContractError::Mask)
    }
}

impl FlatBooleanIntervention {
    #[must_use]
    pub const fn new(max_hamming_distance: usize) -> Self {
        Self {
            max_hamming_distance,
        }
    }

    #[must_use]
    pub const fn max_hamming_distance(self) -> usize {
        self.max_hamming_distance
    }
}

/// Domain-model boundary for prospective evaluation of FLAT Boolean routing.
///
/// The adapter only derives a mask from FLAT's public exact Hamming contract.
/// It does not infer that a sparser mask is faster, safer, or more accurate.
/// Those properties belong to a separately validated model or benchmark.
pub trait FlatBooleanProspectiveModel {
    type Signature;
    type Error;

    fn evaluate_mask(
        &self,
        state: &FlatBooleanAttentionState,
        mask: &BooleanAttentionMask,
    ) -> Result<Self::Signature, Self::Error>;
}

pub struct FlatBooleanAttentionEngine<M> {
    model: M,
}

impl<M> FlatBooleanAttentionEngine<M> {
    #[must_use]
    pub const fn new(model: M) -> Self {
        Self { model }
    }

    #[must_use]
    pub const fn model(&self) -> &M {
        &self.model
    }
}

impl<M> ProspectiveEngine<FlatBooleanAttentionState, FlatBooleanIntervention>
    for FlatBooleanAttentionEngine<M>
where
    M: FlatBooleanProspectiveModel,
{
    type Signature = M::Signature;
    type Error = FlatBooleanEngineError<M::Error>;

    fn baseline(&self, state: &FlatBooleanAttentionState) -> Result<Self::Signature, Self::Error> {
        let mask = state
            .dense_mask()
            .map_err(FlatBooleanEngineError::Contract)?;
        self.model
            .evaluate_mask(state, &mask)
            .map_err(FlatBooleanEngineError::Model)
    }

    fn evaluate(
        &self,
        state: &FlatBooleanAttentionState,
        intervention: &FlatBooleanIntervention,
    ) -> Result<Self::Signature, Self::Error> {
        let mask = state
            .hamming_mask(intervention.max_hamming_distance())
            .map_err(FlatBooleanEngineError::Contract)?;
        self.model
            .evaluate_mask(state, &mask)
            .map_err(FlatBooleanEngineError::Model)
    }
}

impl fmt::Display for FlatBooleanContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKeys => formatter.write_str("FLAT Boolean attention state requires keys"),
            Self::WidthMismatch {
                query_bits,
                key_index,
                key_bits,
            } => write!(
                formatter,
                "FLAT Boolean key {key_index} has {key_bits} bits but query has {query_bits} bits"
            ),
            Self::Signature(error) => write!(formatter, "invalid FLAT Boolean signature: {error}"),
            Self::Mask(error) => write!(formatter, "invalid FLAT Boolean mask: {error}"),
        }
    }
}

impl std::error::Error for FlatBooleanContractError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Signature(error) => Some(error),
            Self::Mask(error) => Some(error),
            Self::EmptyKeys | Self::WidthMismatch { .. } => None,
        }
    }
}

impl<E> fmt::Display for FlatBooleanEngineError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(error) => write!(formatter, "FLAT Boolean contract error: {error}"),
            Self::Model(error) => write!(formatter, "FLAT prospective model error: {error}"),
        }
    }
}

impl<E> std::error::Error for FlatBooleanEngineError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(error) => Some(error),
            Self::Model(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::convert::Infallible;

    use flat_attention::api::boolean_attention_mask::BooleanAttentionMask;
    use flat_attention::api::boolean_attention_signature::BooleanAttentionSignature;
    use prospect_core::ProspectiveEngine;

    use super::{
        FlatBooleanAttentionEngine, FlatBooleanAttentionState, FlatBooleanContractError,
        FlatBooleanIntervention, FlatBooleanProspectiveModel,
    };

    struct CountAdmitted;

    impl FlatBooleanProspectiveModel for CountAdmitted {
        type Signature = usize;
        type Error = Infallible;

        fn evaluate_mask(
            &self,
            _state: &FlatBooleanAttentionState,
            mask: &BooleanAttentionMask,
        ) -> Result<Self::Signature, Self::Error> {
            Ok(mask.admitted_count())
        }
    }

    fn signature(value: u64) -> BooleanAttentionSignature {
        BooleanAttentionSignature::new(4, vec![value]).expect("4-bit signature")
    }

    #[test]
    fn dense_baseline_and_hamming_candidate_use_public_flat_contracts() {
        let state = FlatBooleanAttentionState::new(
            signature(0b1010),
            vec![signature(0b1010), signature(0b1000), signature(0b0000)],
        )
        .expect("valid state");
        let engine = FlatBooleanAttentionEngine::new(CountAdmitted);

        assert_eq!(engine.baseline(&state).expect("baseline"), 3);
        assert_eq!(
            engine
                .evaluate(&state, &FlatBooleanIntervention::new(1))
                .expect("candidate"),
            2
        );

        let mask = state.hamming_mask(1).expect("mask");
        assert_eq!(mask.admitted_blocks(), vec![0, 1]);
    }

    #[test]
    fn rejects_empty_or_mixed_width_state_and_invalid_threshold() {
        assert_eq!(
            FlatBooleanAttentionState::new(signature(0), Vec::new()),
            Err(FlatBooleanContractError::EmptyKeys)
        );

        let wide = BooleanAttentionSignature::new(5, vec![0]).expect("5-bit signature");
        assert!(matches!(
            FlatBooleanAttentionState::new(signature(0), vec![wide]),
            Err(FlatBooleanContractError::WidthMismatch { .. })
        ));

        let state =
            FlatBooleanAttentionState::new(signature(0), vec![signature(0)]).expect("valid state");
        assert!(matches!(
            state.hamming_mask(5),
            Err(FlatBooleanContractError::Signature(_))
        ));
    }
}
