#![forbid(unsafe_code)]

mod comparable;
mod evidence;

pub use comparable::{
    ComparableObservedKvSelectionError, ComparableObservedKvSelectionSet,
};
pub use evidence::{
    KVLAB_KV_REAL_MODEL_SELECTION_REVISION, KVLAB_KV_REAL_MODEL_SELECTION_SCHEMA_V1,
    KvRealModelSelectionEvidenceError, KvlabKvRealModelSelectionEvidenceV1,
    ObservedSelectionMetricKind, ObservedSelectionMetricPair,
    ObservedSelectionMetricPreference,
};
