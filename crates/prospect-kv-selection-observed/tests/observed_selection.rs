use prospect_evidence::EvidenceNature;
use prospect_kv_selection_observed::{
    ComparableObservedKvSelectionError, ComparableObservedKvSelectionSet,
    KVLAB_KV_REAL_MODEL_SELECTION_REVISION, KvlabKvRealModelSelectionEvidenceV1,
    ObservedSelectionMetricKind, ObservedSelectionMetricPreference,
};
use serde_json::{Value, json};

fn record_json(policy: &str, retained: &[u64], candidate_hash: char) -> String {
    let input = [10_u64, 11, 12, 13, 14];
    let retained_set = retained
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let evicted = input
        .iter()
        .copied()
        .filter(|token_id| !retained_set.contains(token_id))
        .collect::<Vec<_>>();
    let retained_bytes = u64::try_from(retained.len()).expect("length") * 64;
    serde_json::to_string(&json!({
        "schema":"kvlab.prospect-kv-real-model-selection/v1",
        "experiment_id":"real-model-selection-c1",
        "run_repository_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "model_id":"example/model",
        "model_revision":"model-r1",
        "tokenizer_revision":"tok-r1",
        "runtime_backend":"fixture-runtime",
        "runtime_revision":"runtime-r1",
        "evaluation_id":"holdout-001",
        "trace_sha256":"1111111111111111111111111111111111111111111111111111111111111111",
        "seed":7,
        "selection":{
            "schema":"kvlab.prospect-kv-selection/v1",
            "policy":policy,
            "input_token_ids":input,
            "bytes_per_token":64,
            "retained_token_ids":retained,
            "evicted_token_ids":evicted,
            "logical_input_bytes":320,
            "logical_retained_bytes":retained_bytes,
            "logical_evicted_bytes":320-retained_bytes
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
                "candidate_value":0.78,
                "delta":-0.02
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
    .expect("canonical JSON")
}

#[test]
fn consumes_observed_explicit_selection() {
    let record = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
        "lru",
        &[10, 12, 14],
        '3',
    ))
    .expect("record");
    assert_eq!(record.selection().policy(), "lru");
    assert_eq!(record.selection().retained_token_ids(), [10, 12, 14]);
    assert_eq!(record.candidate_logical_kv_bytes(), 192);
    assert_eq!(record.metrics()[0].kind(), ObservedSelectionMetricKind::Quality);
    assert_eq!(
        record.metrics()[0].preference(),
        ObservedSelectionMetricPreference::HigherIsBetter
    );
    let source = record.evidence_source().expect("source");
    assert_eq!(source.nature(), EvidenceNature::Observed);
    assert_eq!(source.revision(), KVLAB_KV_REAL_MODEL_SELECTION_REVISION);
}

#[test]
fn comparable_set_accepts_distinct_policies_at_one_budget_without_ranking() {
    let lru = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
        "lru",
        &[10, 12, 14],
        '3',
    ))
    .expect("lru");
    let magnitude = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
        "magnitude",
        &[11, 13, 14],
        '4',
    ))
    .expect("magnitude");
    let set = ComparableObservedKvSelectionSet::new(vec![lru, magnitude]).expect("set");
    assert_eq!(set.budget_bytes(), 192);
    assert_eq!(set.records().len(), 2);
}

#[test]
fn comparable_set_rejects_budget_or_baseline_drift() {
    let lru = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
        "lru",
        &[10, 12, 14],
        '3',
    ))
    .expect("lru");
    let smaller = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&record_json(
        "magnitude",
        &[13, 14],
        '4',
    ))
    .expect("smaller");
    assert!(matches!(
        ComparableObservedKvSelectionSet::new(vec![lru.clone(), smaller]),
        Err(ComparableObservedKvSelectionError::BudgetMismatch)
    ));

    let mut value: Value =
        serde_json::from_str(&record_json("magnitude", &[11, 13, 14], '4')).expect("json");
    value["baseline_output_sha256"] = json!("5".repeat(64));
    let drift_json = serde_json::to_string(&value).expect("json");
    let drift = KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&drift_json)
        .expect("drift");
    assert!(matches!(
        ComparableObservedKvSelectionSet::new(vec![lru, drift]),
        Err(ComparableObservedKvSelectionError::BaselineMismatch)
    ));
}

#[test]
fn rejects_tampered_metric_and_selection() {
    let mut value: Value =
        serde_json::from_str(&record_json("lru", &[10, 12, 14], '3')).expect("json");
    value["metrics"][0]["delta"] = json!(0.5);
    let tampered = serde_json::to_string(&value).expect("json");
    assert!(KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&tampered).is_err());

    let mut value: Value =
        serde_json::from_str(&record_json("lru", &[10, 12, 14], '3')).expect("json");
    value["selection"]["retained_token_ids"] = json!([14, 12, 10]);
    let tampered = serde_json::to_string(&value).expect("json");
    assert!(KvlabKvRealModelSelectionEvidenceV1::from_canonical_json(&tampered).is_err());
}
