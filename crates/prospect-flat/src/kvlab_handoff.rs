use core::fmt;

use flat_attention::api::boolean_attention_mask::BooleanAttentionMask;
use flat_attention::api::boolean_attention_signature::BooleanAttentionSignature;
use prospect_evidence::RunId;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    FlatBooleanAttentionState, FlatBooleanContractError, FlatBooleanIntervention,
    FlatBooleanRoutingEvidenceError, FlatBooleanRoutingEvidenceV1,
};

pub const KVLAB_BKV_HANDOFF_SCHEMA_V1: &str = "kvlab.prospect-bkv-handoff/v1";
pub const KVLAB_BKV_HANDOFF_REVISION: &str = "0fb5adc5babfaea9077342db881ce775eacc4442";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvlabBkvHandoffV1 {
    generation: u64,
    state: FlatBooleanAttentionState,
    intervention: FlatBooleanIntervention,
    admitted_pages: Vec<usize>,
}

#[derive(Debug)]
pub enum KvlabBkvHandoffError {
    Json(serde_json::Error),
    NonCanonicalJson,
    UnsupportedSchema,
    InvalidHexWord,
    FlatContract(FlatBooleanContractError),
    AdmittedPagesMismatch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KvlabBkvHandoffWire {
    schema: String,
    signature_bits: usize,
    generation: u64,
    query_words: Vec<String>,
    page_words: Vec<Vec<String>>,
    max_distance: usize,
    admitted_pages: Vec<usize>,
}

impl KvlabBkvHandoffV1 {
    pub fn from_canonical_json(json: &str) -> Result<Self, KvlabBkvHandoffError> {
        let value: Value = serde_json::from_str(json).map_err(KvlabBkvHandoffError::Json)?;
        let canonical = serde_json::to_string(&value).map_err(KvlabBkvHandoffError::Json)?;
        if canonical != json {
            return Err(KvlabBkvHandoffError::NonCanonicalJson);
        }

        let wire: KvlabBkvHandoffWire =
            serde_json::from_value(value).map_err(KvlabBkvHandoffError::Json)?;
        if wire.schema != KVLAB_BKV_HANDOFF_SCHEMA_V1 {
            return Err(KvlabBkvHandoffError::UnsupportedSchema);
        }

        let query =
            BooleanAttentionSignature::new(wire.signature_bits, parse_words(&wire.query_words)?)
                .map_err(|error| {
                    KvlabBkvHandoffError::FlatContract(FlatBooleanContractError::Signature(error))
                })?;
        let keys =
            wire.page_words
                .iter()
                .map(|words| {
                    BooleanAttentionSignature::new(wire.signature_bits, parse_words(words)?)
                        .map_err(|error| {
                            KvlabBkvHandoffError::FlatContract(FlatBooleanContractError::Signature(
                                error,
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
        let state = FlatBooleanAttentionState::new(query, keys)
            .map_err(KvlabBkvHandoffError::FlatContract)?;
        let intervention = FlatBooleanIntervention::new(wire.max_distance);
        let mask = state
            .hamming_mask(wire.max_distance)
            .map_err(KvlabBkvHandoffError::FlatContract)?;
        if mask.admitted_blocks() != wire.admitted_pages {
            return Err(KvlabBkvHandoffError::AdmittedPagesMismatch);
        }

        Ok(Self {
            generation: wire.generation,
            state,
            intervention,
            admitted_pages: wire.admitted_pages,
        })
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn state(&self) -> &FlatBooleanAttentionState {
        &self.state
    }

    #[must_use]
    pub const fn intervention(&self) -> FlatBooleanIntervention {
        self.intervention
    }

    #[must_use]
    pub fn admitted_pages(&self) -> &[usize] {
        &self.admitted_pages
    }

    pub fn mask(&self) -> Result<BooleanAttentionMask, FlatBooleanContractError> {
        self.state
            .hamming_mask(self.intervention.max_hamming_distance())
    }

    pub fn routing_evidence(
        &self,
        run_id: &RunId,
    ) -> Result<FlatBooleanRoutingEvidenceV1, FlatBooleanRoutingEvidenceError> {
        FlatBooleanRoutingEvidenceV1::capture_hamming(run_id, &self.state, self.intervention)
    }
}

fn parse_words(words: &[String]) -> Result<Vec<u64>, KvlabBkvHandoffError> {
    words.iter().map(|word| parse_word(word)).collect()
}

fn parse_word(word: &str) -> Result<u64, KvlabBkvHandoffError> {
    if word.len() != 16
        || word
            .as_bytes()
            .iter()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(byte))
    {
        return Err(KvlabBkvHandoffError::InvalidHexWord);
    }
    let value = u64::from_str_radix(word, 16).map_err(|_| KvlabBkvHandoffError::InvalidHexWord)?;
    if format!("{value:016x}") != word {
        return Err(KvlabBkvHandoffError::InvalidHexWord);
    }
    Ok(value)
}

impl fmt::Display for KvlabBkvHandoffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid KVLab BKV handoff JSON: {error}"),
            Self::NonCanonicalJson => {
                formatter.write_str("KVLab BKV handoff JSON is not canonical")
            }
            Self::UnsupportedSchema => formatter.write_str("unsupported KVLab BKV handoff schema"),
            Self::InvalidHexWord => formatter.write_str(
                "KVLab BKV words must be canonical 16-digit lowercase hexadecimal u64 values",
            ),
            Self::FlatContract(error) => {
                write!(formatter, "invalid FLAT Boolean contract: {error}")
            }
            Self::AdmittedPagesMismatch => formatter
                .write_str("KVLab admitted pages do not match FLAT exact Hamming mask derivation"),
        }
    }
}

impl std::error::Error for KvlabBkvHandoffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::FlatContract(error) => Some(error),
            Self::NonCanonicalJson
            | Self::UnsupportedSchema
            | Self::InvalidHexWord
            | Self::AdmittedPagesMismatch => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use prospect_evidence::RunId;

    use super::{KvlabBkvHandoffError, KvlabBkvHandoffV1};

    const FIXTURE: &str = concat!(
        "{\"admitted_pages\":[0,1,3],",
        "\"generation\":0,",
        "\"max_distance\":1,",
        "\"page_words\":[[\"000000000000000a\"],[\"000000000000000e\"],",
        "[\"0000000000000000\"],[\"000000000000000b\"]],",
        "\"query_words\":[\"000000000000000a\"],",
        "\"schema\":\"kvlab.prospect-bkv-handoff/v1\",",
        "\"signature_bits\":4}"
    );

    #[test]
    fn imports_kvlab_fixture_into_exact_flat_mask() {
        let handoff = KvlabBkvHandoffV1::from_canonical_json(FIXTURE).expect("valid handoff");

        assert_eq!(handoff.generation(), 0);
        assert_eq!(handoff.admitted_pages(), &[0, 1, 3]);
        assert_eq!(
            handoff.mask().expect("mask").admitted_blocks(),
            vec![0, 1, 3]
        );

        let run_id = RunId::new("kvlab-flat-1").expect("run id");
        let evidence = handoff.routing_evidence(&run_id).expect("routing evidence");
        assert_eq!(evidence.admitted_blocks(), &[0, 1, 3]);
    }

    #[test]
    fn rejects_candidate_set_drift_across_projects() {
        let tampered =
            FIXTURE.replace("\"admitted_pages\":[0,1,3]", "\"admitted_pages\":[0,1,2,3]");
        assert!(matches!(
            KvlabBkvHandoffV1::from_canonical_json(&tampered),
            Err(KvlabBkvHandoffError::AdmittedPagesMismatch)
        ));
    }

    #[test]
    fn rejects_noncanonical_json_and_word_encoding() {
        let pretty = FIXTURE.replace(",\"generation\"", ", \"generation\"");
        assert!(matches!(
            KvlabBkvHandoffV1::from_canonical_json(&pretty),
            Err(KvlabBkvHandoffError::NonCanonicalJson)
        ));

        let uppercase = FIXTURE.replace("000000000000000a", "000000000000000A");
        assert!(matches!(
            KvlabBkvHandoffV1::from_canonical_json(&uppercase),
            Err(KvlabBkvHandoffError::InvalidHexWord)
        ));
    }
}
