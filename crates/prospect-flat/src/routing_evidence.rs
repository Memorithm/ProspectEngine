use core::fmt;

use flat_attention::api::boolean_attention_signature::BooleanAttentionSignature;
use prospect_evidence::{EvidenceError, RunId};
use serde::{Deserialize, Serialize};

use crate::{
    FLAT_ATTENTION_REVISION, FlatBooleanAttentionState, FlatBooleanContractError,
    FlatBooleanIntervention,
};

pub const FLAT_BOOLEAN_ROUTING_EVIDENCE_SCHEMA_V1: &str =
    "prospect.flat-boolean-routing-evidence/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlatBooleanRoutingMode {
    Dense,
    Hamming { max_distance: usize },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatBooleanRoutingEvidenceV1 {
    schema: String,
    run_id: String,
    flat_revision: String,
    query_bits: usize,
    query_words: Vec<u64>,
    key_words: Vec<Vec<u64>>,
    mode: FlatBooleanRoutingMode,
    mask_words: Vec<u64>,
    admitted_blocks: Vec<usize>,
    mask_physical_bytes: usize,
}

#[derive(Debug)]
pub enum FlatBooleanRoutingEvidenceError {
    Evidence(EvidenceError),
    Contract(FlatBooleanContractError),
    Json(serde_json::Error),
    UnsupportedSchema,
    FlatRevisionMismatch,
    DerivedMaskMismatch,
    ReplayMismatch,
}

impl FlatBooleanRoutingEvidenceV1 {
    pub fn capture_dense(
        run_id: &RunId,
        state: &FlatBooleanAttentionState,
    ) -> Result<Self, FlatBooleanRoutingEvidenceError> {
        Self::capture(run_id, state, FlatBooleanRoutingMode::Dense)
    }

    pub fn capture_hamming(
        run_id: &RunId,
        state: &FlatBooleanAttentionState,
        intervention: FlatBooleanIntervention,
    ) -> Result<Self, FlatBooleanRoutingEvidenceError> {
        Self::capture(
            run_id,
            state,
            FlatBooleanRoutingMode::Hamming {
                max_distance: intervention.max_hamming_distance(),
            },
        )
    }

    fn capture(
        run_id: &RunId,
        state: &FlatBooleanAttentionState,
        mode: FlatBooleanRoutingMode,
    ) -> Result<Self, FlatBooleanRoutingEvidenceError> {
        let mask = derive_mask(state, mode)?;
        let evidence = Self {
            schema: FLAT_BOOLEAN_ROUTING_EVIDENCE_SCHEMA_V1.to_owned(),
            run_id: run_id.as_str().to_owned(),
            flat_revision: FLAT_ATTENTION_REVISION.to_owned(),
            query_bits: state.query().bits(),
            query_words: state.query().words().to_vec(),
            key_words: state
                .keys()
                .iter()
                .map(|key| key.words().to_vec())
                .collect(),
            mode,
            mask_words: mask.words().to_vec(),
            admitted_blocks: mask.admitted_blocks(),
            mask_physical_bytes: mask
                .physical_bytes()
                .map_err(|error| FlatBooleanRoutingEvidenceError::Contract(
                    FlatBooleanContractError::Mask(error),
                ))?,
        };
        evidence.validate_contract()?;
        Ok(evidence)
    }

    pub fn canonical_json(&self) -> Result<String, FlatBooleanRoutingEvidenceError> {
        serde_json::to_string(self).map_err(FlatBooleanRoutingEvidenceError::Json)
    }

    pub fn from_canonical_json(json: &str) -> Result<Self, FlatBooleanRoutingEvidenceError> {
        let evidence: Self =
            serde_json::from_str(json).map_err(FlatBooleanRoutingEvidenceError::Json)?;
        evidence.validate_contract()?;
        Ok(evidence)
    }

    fn validate_contract(&self) -> Result<(), FlatBooleanRoutingEvidenceError> {
        if self.schema != FLAT_BOOLEAN_ROUTING_EVIDENCE_SCHEMA_V1 {
            return Err(FlatBooleanRoutingEvidenceError::UnsupportedSchema);
        }
        if self.flat_revision != FLAT_ATTENTION_REVISION {
            return Err(FlatBooleanRoutingEvidenceError::FlatRevisionMismatch);
        }
        RunId::new(self.run_id.clone()).map_err(FlatBooleanRoutingEvidenceError::Evidence)?;

        let query = BooleanAttentionSignature::new(self.query_bits, self.query_words.clone())
            .map_err(|error| {
                FlatBooleanRoutingEvidenceError::Contract(FlatBooleanContractError::Signature(
                    error,
                ))
            })?;
        let keys = self
            .key_words
            .iter()
            .map(|words| {
                BooleanAttentionSignature::new(self.query_bits, words.clone()).map_err(|error| {
                    FlatBooleanRoutingEvidenceError::Contract(
                        FlatBooleanContractError::Signature(error),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let state = FlatBooleanAttentionState::new(query, keys)
            .map_err(FlatBooleanRoutingEvidenceError::Contract)?;
        let expected = derive_mask(&state, self.mode)?;
        let physical_bytes = expected.physical_bytes().map_err(|error| {
            FlatBooleanRoutingEvidenceError::Contract(FlatBooleanContractError::Mask(error))
        })?;

        if self.mask_words != expected.words()
            || self.admitted_blocks != expected.admitted_blocks()
            || self.mask_physical_bytes != physical_bytes
        {
            return Err(FlatBooleanRoutingEvidenceError::DerivedMaskMismatch);
        }
        Ok(())
    }

    pub fn verify_replay(
        &self,
        replayed: &Self,
    ) -> Result<(), FlatBooleanRoutingEvidenceError> {
        if self == replayed {
            Ok(())
        } else {
            Err(FlatBooleanRoutingEvidenceError::ReplayMismatch)
        }
    }

    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    #[must_use]
    pub const fn mode(&self) -> FlatBooleanRoutingMode {
        self.mode
    }

    #[must_use]
    pub fn admitted_blocks(&self) -> &[usize] {
        &self.admitted_blocks
    }

    #[must_use]
    pub fn mask_words(&self) -> &[u64] {
        &self.mask_words
    }

    #[must_use]
    pub const fn mask_physical_bytes(&self) -> usize {
        self.mask_physical_bytes
    }
}

fn derive_mask(
    state: &FlatBooleanAttentionState,
    mode: FlatBooleanRoutingMode,
) -> Result<flat_attention::api::boolean_attention_mask::BooleanAttentionMask, FlatBooleanRoutingEvidenceError>
{
    match mode {
        FlatBooleanRoutingMode::Dense => state.dense_mask(),
        FlatBooleanRoutingMode::Hamming { max_distance } => state.hamming_mask(max_distance),
    }
    .map_err(FlatBooleanRoutingEvidenceError::Contract)
}

impl fmt::Display for FlatBooleanRoutingEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evidence(error) => write!(formatter, "invalid run evidence: {error}"),
            Self::Contract(error) => write!(formatter, "invalid FLAT routing contract: {error}"),
            Self::Json(error) => write!(formatter, "invalid FLAT routing evidence JSON: {error}"),
            Self::UnsupportedSchema => {
                formatter.write_str("unsupported FLAT Boolean routing evidence schema")
            }
            Self::FlatRevisionMismatch => {
                formatter.write_str("FLAT routing evidence revision does not match pinned FLAT")
            }
            Self::DerivedMaskMismatch => formatter.write_str(
                "recorded FLAT Boolean mask does not match signatures and routing mode",
            ),
            Self::ReplayMismatch => {
                formatter.write_str("replayed FLAT Boolean routing evidence differs")
            }
        }
    }
}

impl std::error::Error for FlatBooleanRoutingEvidenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evidence(error) => Some(error),
            Self::Contract(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::UnsupportedSchema
            | Self::FlatRevisionMismatch
            | Self::DerivedMaskMismatch
            | Self::ReplayMismatch => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use flat_attention::api::boolean_attention_signature::BooleanAttentionSignature;
    use prospect_evidence::RunId;

    use super::{
        FlatBooleanRoutingEvidenceError, FlatBooleanRoutingEvidenceV1, FlatBooleanRoutingMode,
    };
    use crate::{FlatBooleanAttentionState, FlatBooleanIntervention};

    fn signature(value: u64) -> BooleanAttentionSignature {
        BooleanAttentionSignature::new(4, vec![value]).expect("4-bit signature")
    }

    fn state() -> FlatBooleanAttentionState {
        FlatBooleanAttentionState::new(
            signature(0b1010),
            vec![signature(0b1010), signature(0b1000), signature(0b0000)],
        )
        .expect("valid state")
    }

    #[test]
    fn canonical_hamming_evidence_replays_exact_mask_derivation() {
        let run_id = RunId::new("flat-routing-1").expect("run id");
        let evidence = FlatBooleanRoutingEvidenceV1::capture_hamming(
            &run_id,
            &state(),
            FlatBooleanIntervention::new(1),
        )
        .expect("routing evidence");

        assert_eq!(
            evidence.mode(),
            FlatBooleanRoutingMode::Hamming { max_distance: 1 }
        );
        assert_eq!(evidence.admitted_blocks(), &[0, 1]);
        assert_eq!(evidence.mask_words(), &[0b0011]);
        assert_eq!(evidence.mask_physical_bytes(), 8);

        let json = evidence.canonical_json().expect("json");
        let decoded = FlatBooleanRoutingEvidenceV1::from_canonical_json(&json).expect("decode");
        evidence.verify_replay(&decoded).expect("exact replay");
    }

    #[test]
    fn dense_evidence_preserves_explicit_all_admitted_baseline() {
        let run_id = RunId::new("flat-dense-1").expect("run id");
        let evidence = FlatBooleanRoutingEvidenceV1::capture_dense(&run_id, &state())
            .expect("dense evidence");

        assert_eq!(evidence.mode(), FlatBooleanRoutingMode::Dense);
        assert_eq!(evidence.admitted_blocks(), &[0, 1, 2]);
        assert_eq!(evidence.mask_words(), &[0b0111]);
    }

    #[test]
    fn decoder_rejects_mask_tampering() {
        let run_id = RunId::new("flat-routing-tamper").expect("run id");
        let evidence = FlatBooleanRoutingEvidenceV1::capture_hamming(
            &run_id,
            &state(),
            FlatBooleanIntervention::new(1),
        )
        .expect("routing evidence");
        let json = evidence.canonical_json().expect("json");
        let tampered = json.replace("\"mask_words\":[3]", "\"mask_words\":[7]");

        assert!(matches!(
            FlatBooleanRoutingEvidenceV1::from_canonical_json(&tampered),
            Err(FlatBooleanRoutingEvidenceError::DerivedMaskMismatch)
        ));
    }
}
