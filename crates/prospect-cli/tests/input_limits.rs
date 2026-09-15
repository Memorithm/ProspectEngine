//! Black-box input-boundary tests. No model, GPU or observed experiment is run.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use prospect_cli::input::MAX_JSON_FILE_BYTES;

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "prospect-cli-input-boundary-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn require_read_failure(command: &str, paths: &[&Path], message: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_prospect"))
        .arg(command)
        .args(paths)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{command}: {output:?}");
    assert!(
        output.stdout.is_empty(),
        "partial success emitted by {command}"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(message), "{command}: {stderr}");
}

#[test]
fn oversized_inputs_fail_before_json_decoding_for_every_file_command() {
    let directory = TempDirectory::new();
    let huge = directory.0.join("huge.json");
    let tiny = directory.0.join("tiny.json");
    File::create(&huge)
        .unwrap()
        .set_len(MAX_JSON_FILE_BYTES as u64 + 1)
        .unwrap();
    fs::write(&tiny, "{}").unwrap();
    for command in ["verify-kv-campaign-spec", "verify-scenario-bundle"] {
        require_read_failure(command, &[&huge], "input limit exceeded");
    }
    require_read_failure(
        "preflight-scenario-bundle",
        &[&huge, &tiny],
        "input limit exceeded",
    );
    require_read_failure(
        "preflight-scenario-bundle",
        &[&tiny, &huge],
        "input limit exceeded",
    );
    for (command, manifest_name) in [
        ("verify-kv-campaign", "manifest.json"),
        ("verify-kv-campaign-suite", "suite-manifest.json"),
        ("verify-kv-campaign-suite-r2", "suite-manifest.json"),
    ] {
        let input = directory.0.join(command);
        fs::create_dir(&input).unwrap();
        File::create(input.join(manifest_name))
            .unwrap()
            .set_len(MAX_JSON_FILE_BYTES as u64 + 1)
            .unwrap();
        fs::write(input.join("campaign.json"), "{}").unwrap();
        require_read_failure(command, &[&input], "input limit exceeded");
    }
}

#[cfg(unix)]
#[test]
fn standalone_and_dispatch_commands_reject_static_input_symlinks() {
    use std::os::unix::fs::symlink;
    let directory = TempDirectory::new();
    let input = directory.0.join("input.json");
    let link = directory.0.join("link.json");
    fs::write(&input, "{}").unwrap();
    symlink(&input, &link).unwrap();
    for command in ["verify-kv-campaign-spec", "verify-scenario-bundle"] {
        require_read_failure(command, &[&link], "regular file");
    }
    require_read_failure(
        "preflight-scenario-bundle",
        &[&link, &input],
        "regular file",
    );
    require_read_failure(
        "preflight-scenario-bundle",
        &[&input, &link],
        "regular file",
    );
}

#[cfg(unix)]
#[test]
fn standalone_commands_reject_socket_entries_without_reading_them() {
    use std::os::unix::net::UnixListener;
    let directory = TempDirectory::new();
    let socket = directory.0.join("socket");
    let _listener = UnixListener::bind(&socket).unwrap();
    for command in ["verify-kv-campaign-spec", "verify-scenario-bundle"] {
        require_read_failure(command, &[&socket], "regular file");
    }
}
