//! Filesystem and executable tests; no domain or model execution.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use prospect_cli::execution_journal::FileJournal;
use prospect_dispatch::execution::record::journal::{JournalSink, MAX_JOURNAL_ENTRY_BYTES};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "prospect-live-journal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn file_sink_preserves_exact_entries_and_acknowledges_after_write() {
    let temp = Temp::new();
    let path = temp.path().join("run.jsonl");
    let mut sink = FileJournal::create(&path).unwrap();
    sink.append_record(b"{\"a\":1}\n").unwrap();
    sink.append_record(b"{\"b\":2}\n").unwrap();
    assert_eq!(sink.acknowledged_bytes(), 16);
    assert!(!sink.is_poisoned());
    assert_eq!(fs::read(&path).unwrap(), b"{\"a\":1}\n{\"b\":2}\n");
    drop(sink);
    assert!(path.is_file());
}

#[test]
fn existing_files_and_directories_are_never_overwritten() {
    let temp = Temp::new();
    let path = temp.path().join("existing");
    fs::write(&path, "preserve this").unwrap();
    assert!(FileJournal::create(&path).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "preserve this");
    assert!(FileJournal::create(temp.path()).is_err());
}

#[cfg(unix)]
#[test]
fn dangling_and_live_symlink_destinations_are_not_followed() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let target = temp.path().join("target");
    let link = temp.path().join("link");
    symlink(&target, &link).unwrap();
    assert!(FileJournal::create(&link).is_err());
    assert!(!target.exists());
    fs::write(&target, "keep").unwrap();
    assert!(FileJournal::create(&link).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
}

#[cfg(unix)]
#[test]
fn unix_journal_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new();
    let path = temp.path().join("private");
    let _sink = FileJournal::create(&path).unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn invalid_entry_poisons_sink_and_preserves_acknowledged_prefix() {
    let temp = Temp::new();
    let path = temp.path().join("run");
    let mut sink = FileJournal::create(&path).unwrap();
    sink.append_record(b"{}\n").unwrap();
    assert!(sink.append_record(b"not terminated").is_err());
    assert!(sink.is_poisoned());
    assert!(sink.append_record(b"{}\n").is_err());
    assert_eq!(sink.acknowledged_bytes(), 3);
    assert_eq!(fs::read(path).unwrap(), b"{}\n");
}

#[test]
fn multiple_lines_and_oversized_entry_fail_before_writing() {
    for bytes in [b"{}\n{}\n".to_vec(), {
        let mut value = vec![b'x'; MAX_JOURNAL_ENTRY_BYTES];
        value.push(b'\n');
        value
    }] {
        let temp = Temp::new();
        let path = temp.path().join("run");
        let mut sink = FileJournal::create(&path).unwrap();
        assert!(sink.append_record(&bytes).is_err());
        assert!(sink.is_poisoned());
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
    }
}

#[test]
fn cli_usage_errors_have_no_success_json() {
    for args in [
        vec!["inspect-execution-journal"],
        vec!["inspect-execution-journal", "a"],
        vec!["inspect-execution-journal", "a", "b", "c"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_prospect"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn cli_rejects_missing_invalid_utf8_and_oversized_input_without_json() {
    let temp = Temp::new();
    let journal = temp.path().join("run");
    let bundle = temp.path().join("bundle");
    fs::write(&bundle, "{}").unwrap();
    for variant in 0..3 {
        match variant {
            0 => {}
            1 => fs::write(&journal, [0xff]).unwrap(),
            _ => {
                fs::File::create(&journal)
                    .unwrap()
                    .set_len(16 * 1024 * 1024 + 1)
                    .unwrap();
            }
        }
        let output = Command::new(env!("CARGO_BIN_EXE_prospect"))
            .arg("inspect-execution-journal")
            .arg(&journal)
            .arg(&bundle)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
    }
}
