//! Explicitly synthetic record fixtures. No engine or model execution evidence.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use prospect_cli::execution_record::{publish_execution_record, verify_execution_record_files};
use prospect_dispatch::execution::record::ExecutionRecord;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static ID: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "prospect-record-tests-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn canonical(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}
fn fixture() -> (ExecutionRecord, String) {
    let bundle = canonical(&json!({
        "schema":"prospect.scenario-bundle/v1", "bundle_id":"experiment.storage_fixture",
        "adapter":{"adapter_id":"prospect.storage_fixture","contract_version":{"major":1,"minor":0},"upstream":null},
        "seed":7,"state":10,
        "scenarios":[{"id":"a","intervention":1},{"id":"b","intervention":2}],"metric":null,"policy":null
    }));
    let payload = canonical(&json!({
        "schema":"prospect.bundle-evaluation-record/v1", "evidence_kind":"software_execution_report",
        "run_id":"storage-fixture", "bundle_sha256":format!("{:x}", Sha256::digest(bundle.as_bytes())),
        "bundle_id":"experiment.storage_fixture", "seed":7,
        "adapter":{"adapter_id":"prospect.storage_fixture","contract_version":{"major":1,"minor":0},"upstream":null,
            "capabilities":[{"id":"fixture.evaluate","version":{"major":1,"minor":0}}]},
        "max_evaluations":1, "deadline_configured":false,
        "codecs":{"signature":"fixture.i32.v1","error":"fixture.text.v1"},
        "baseline":"10", "outcomes":[{"scenario_id":"a","payload":"11"}], "pending":["b"],
        "terminal":{"state":"interrupted","reason":"evaluation_limit_reached"}
    }));
    (
        ExecutionRecord::verify_against_bundle(&payload, &bundle).unwrap(),
        bundle,
    )
}
fn verify_command(record: &Path, bundle: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_prospect"))
        .arg("verify-execution-record")
        .arg(record)
        .arg(bundle)
        .output()
        .unwrap()
}

#[test]
fn published_record_roundtrips_without_byte_normalization_or_implicit_resume() {
    let temp = Temp::new();
    let (record, bundle) = fixture();
    let target = temp.join("record.json");
    let input = temp.join("bundle.json");
    fs::write(&input, &bundle).unwrap();
    publish_execution_record(&record, &target).unwrap();
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        record.canonical_json()
    );
    let summary = verify_execution_record_files(&target, &input).unwrap();
    assert_eq!(summary.state, "interrupted");
    assert_eq!(summary.successful_candidates, 1);
    assert_eq!(summary.never_started_candidates, 1);
    assert_eq!(summary.record_sha256, record.sha256());
    assert!(!summary.resume_authorized);
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 2);
    let output = verify_command(&target, &input);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["state"], "interrupted");
    assert_eq!(value["resume_authorized"], false);
}

#[test]
fn existing_destination_is_never_overwritten_and_temporary_is_removed() {
    let temp = Temp::new();
    let (record, _) = fixture();
    let target = temp.join("record.json");
    fs::write(&target, "keep existing contents").unwrap();
    assert!(publish_execution_record(&record, &target).is_err());
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "keep existing contents"
    );
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
}

#[test]
fn directory_destination_is_preserved() {
    let temp = Temp::new();
    let (record, _) = fixture();
    let target = temp.join("directory");
    fs::create_dir(&target).unwrap();
    assert!(publish_execution_record(&record, &target).is_err());
    assert!(target.is_dir());
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn dangling_destination_symlink_is_not_followed_or_removed() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let (record, _) = fixture();
    let target = temp.join("record.json");
    let missing = temp.join("absent");
    symlink(&missing, &target).unwrap();
    assert!(publish_execution_record(&record, &target).is_err());
    assert_eq!(fs::read_link(&target).unwrap(), missing);
    assert!(!missing.exists());
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn published_records_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new();
    let (record, _) = fixture();
    let target = temp.join("record.json");
    publish_execution_record(&record, &target).unwrap();
    assert_eq!(
        fs::metadata(target).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn wrong_input_and_malformed_records_return_no_success_json() {
    let temp = Temp::new();
    let (record, bundle) = fixture();
    let target = temp.join("record.json");
    let input = temp.join("bundle.json");
    publish_execution_record(&record, &target).unwrap();
    let mut changed: Value = serde_json::from_str(&bundle).unwrap();
    changed["state"] = json!(99);
    fs::write(&input, canonical(&changed)).unwrap();
    let mismatch = verify_command(&target, &input);
    assert_eq!(mismatch.status.code(), Some(1));
    assert!(mismatch.stdout.is_empty());
    fs::write(&input, bundle).unwrap();
    fs::write(&target, "{broken").unwrap();
    let malformed = verify_command(&target, &input);
    assert_eq!(malformed.status.code(), Some(1));
    assert!(malformed.stdout.is_empty());
}

#[test]
fn command_requires_exactly_two_paths() {
    for args in [vec![], vec!["record"], vec!["record", "bundle", "extra"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_prospect"))
            .arg("verify-execution-record")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn both_inputs_share_the_existing_static_symlink_rejection() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let (record, bundle) = fixture();
    let target = temp.join("record.json");
    let input = temp.join("bundle.json");
    fs::write(&target, record.canonical_json()).unwrap();
    fs::write(&input, bundle).unwrap();
    let record_link = temp.join("record.link");
    let bundle_link = temp.join("bundle.link");
    symlink(&target, &record_link).unwrap();
    symlink(&input, &bundle_link).unwrap();
    for (r, b) in [(&record_link, &input), (&target, &bundle_link)] {
        let output = verify_command(r, b);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn concurrent_writers_never_replace_the_winning_file() {
    let temp = Temp::new();
    let (record, _) = fixture();
    let target = temp.join("record.json");
    let successes = std::thread::scope(|scope| {
        let a = scope.spawn(|| publish_execution_record(&record, &target));
        let b = scope.spawn(|| publish_execution_record(&record, &target));
        usize::from(a.join().unwrap().is_ok()) + usize::from(b.join().unwrap().is_ok())
    });
    assert_eq!(successes, 1);
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        record.canonical_json()
    );
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
}

#[test]
fn oversized_files_are_rejected_by_the_shared_reader() {
    let temp = Temp::new();
    let (record, bundle) = fixture();
    let target = temp.join("record.json");
    let input = temp.join("bundle.json");
    fs::write(&target, record.canonical_json()).unwrap();
    fs::write(&input, &bundle).unwrap();
    for large in [&target, &input] {
        fs::OpenOptions::new()
            .write(true)
            .open(large)
            .unwrap()
            .set_len(16 * 1024 * 1024 + 1)
            .unwrap();
        let output = verify_command(&target, &input);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        fs::write(&target, record.canonical_json()).unwrap();
        fs::write(&input, &bundle).unwrap();
    }
}
