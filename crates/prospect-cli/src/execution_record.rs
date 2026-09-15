//! Bounded record loading and no-clobber publication. No model execution.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use prospect_dispatch::execution::record::{ExecutionRecord, ExecutionRecordError, ExecutionRecordSummary};
use crate::input::TextReadBudget;

#[derive(Debug)]
pub enum ExecutionRecordFileError {
    Io { path: PathBuf, source: io::Error },
    Record(ExecutionRecordError),
}
impl fmt::Display for ExecutionRecordFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "execution record input {}: {source}", path.display()),
            Self::Record(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for ExecutionRecordFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self { Self::Io { source, .. } => Some(source), Self::Record(e) => Some(e) }
    }
}

/// Read a stored report and compare it with a separately supplied canonical bundle.
///
/// Both reads share the existing bounded CLI reader. Verification checks file
/// consistency and input identity, not execution authenticity or codec semantics.
/// Interrupted/failed records can be valid records without being completed runs.
///
/// ```no_run
/// use prospect_cli::execution_record::verify_execution_record_files;
/// let summary = verify_execution_record_files("run.record.json", "input.bundle.json")?;
/// assert!(!summary.resume_authorized);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn verify_execution_record_files(
    record_path: impl AsRef<Path>,
    bundle_path: impl AsRef<Path>,
) -> Result<ExecutionRecordSummary, ExecutionRecordFileError> {
    let mut budget = TextReadBudget::default();
    let mut read = |path: &Path| budget.read_text(path).map_err(|source| ExecutionRecordFileError::Io {
        path: path.to_path_buf(), source,
    });
    let record = read(record_path.as_ref())?;
    let bundle = read(bundle_path.as_ref())?;
    Ok(ExecutionRecord::verify_against_bundle(&record, &bundle)
        .map_err(ExecutionRecordFileError::Record)?.summary())
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) { let _ = fs::remove_file(&self.0); }
}

/// Publish a verified terminal record to a NEW path, without replacing anything.
///
/// Write/sync a same-directory temporary regular file, close it, then create a
/// hard link at the destination. Existing files, directories and dangling links
/// cause failure. Filesystems without hard-link support fail closed; no weakening
/// copy/rename fallback is used. Temporary cleanup is best effort on return/panic.
///
/// The trusted parent directory must exist and remain unmodified. This is neither
/// hostile-filesystem isolation nor a power-loss/crash recovery guarantee; directory
/// entries are not fsynced. A crash may leave a temporary file. On Unix, temporary
/// and published files are created with owner-only permissions (0600).
///
/// Encoding/write failure does not consume the record, so callers retain the
/// report and can explicitly choose another new destination without rerunning it.
///
/// ```no_run
/// use prospect_dispatch::execution::record::ExecutionRecord;
/// use prospect_cli::execution_record::publish_execution_record;
/// fn save(record: &ExecutionRecord) -> std::io::Result<()> {
///     publish_execution_record(record, "new-run.record.json")
/// }
/// ```
pub fn publish_execution_record(record: &ExecutionRecord, destination: impl AsRef<Path>) -> io::Result<()> {
    let destination = destination.as_ref();
    let parent = destination.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if destination.file_name().is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "record destination must name a file"));
    }
    for _ in 0..32 {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".prospect-record-{}-{id}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let temporary = Temporary(path);
        file.write_all(record.canonical_json().as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::hard_link(&temporary.0, destination)?;
        return Ok(());
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "temporary record name collisions"))
}
