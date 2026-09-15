//! Read-only restart preflight. No artifact is executed and no journal is rewritten.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use prospect_dispatch::execution::record::ExecutionRecordError;
use prospect_dispatch::execution::record::journal::recovery::{
    MAX_RESTART_EXPECTATION_BYTES, RestartExpectations, RestartPreflight, preflight_journal_restart,
};
use sha2::{Digest, Sha256};

use crate::input::TextReadBudget;

/// Maximum implementation artifact scanned, with one extra byte to detect overflow.
/// This bounds input I/O volume, not elapsed I/O time or total process memory.
pub const MAX_RESTART_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug)]
pub enum RestartFileError {
    Io { path: PathBuf, source: io::Error },
    Contract(ExecutionRecordError),
}
impl std::fmt::Display for RestartFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "restart input {}: {source}", path.display()),
            Self::Contract(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for RestartFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Contract(e) => Some(e),
        }
    }
}

/// Verify four separately supplied files, never executing the implementation.
///
/// Expectations are read under a dedicated 16 KiB admission bound; journal and
/// bundle share the default text budget. The artifact uses a bounded stream hash
/// with a fixed 64 KiB buffer, not a whole-file allocation. Every file must have a
/// regular, non-symlink final component and trusted unchanged ancestors/content.
///
/// A returned plan can be blocked. `resume_authorized` remains false even when
/// preparation is allowed. Errors return no success summary. Keep the expectation
/// file independently trusted; deriving it from an untrusted journal defeats the
/// external anchor, and this function cannot determine that source's trust.
///
/// ```no_run
/// use prospect_cli::execution_restart::preflight_execution_restart_files;
/// let plan = preflight_execution_restart_files(
///     "run.jsonl", "bundle.json", "trusted-expectations.json", "engine-artifact",
/// )?;
/// assert!(!plan.resume_authorized);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn preflight_execution_restart_files(
    journal: impl AsRef<Path>,
    bundle: impl AsRef<Path>,
    expectations: impl AsRef<Path>,
    artifact: impl AsRef<Path>,
) -> Result<RestartPreflight, RestartFileError> {
    let mut policy_budget =
        TextReadBudget::new(MAX_RESTART_EXPECTATION_BYTES, MAX_RESTART_EXPECTATION_BYTES).map_err(
            |source| RestartFileError::Io {
                path: expectations.as_ref().into(),
                source,
            },
        )?;
    let policy = read(&mut policy_budget, expectations.as_ref())?;
    let policy =
        RestartExpectations::from_canonical_json(&policy).map_err(RestartFileError::Contract)?;
    let mut budget = TextReadBudget::default();
    let journal = read(&mut budget, journal.as_ref())?;
    let bundle = read(&mut budget, bundle.as_ref())?;
    let artifact_sha = hash_implementation_artifact(artifact)?;
    preflight_journal_restart(&journal, &bundle, &policy, &artifact_sha)
        .map_err(RestartFileError::Contract)
}

fn read(budget: &mut TextReadBudget, path: &Path) -> Result<String, RestartFileError> {
    budget
        .read_text(path)
        .map_err(|source| RestartFileError::Io {
            path: path.into(),
            source,
        })
}

/// Stream-hash a nonempty regular implementation artifact without loading/running it.
///
/// The digest describes bytes read, not a Git revision or a running process.
/// Static final links are rejected; concurrent substitution is outside this
/// trusted-file contract. A blocked filesystem can still block a read.
pub fn hash_implementation_artifact(path: impl AsRef<Path>) -> Result<String, RestartFileError> {
    let path = path.as_ref();
    hash_file_with_limit(path, MAX_RESTART_ARTIFACT_BYTES).map_err(|source| RestartFileError::Io {
        path: path.into(),
        source,
    })
}

fn hash_file_with_limit(path: &Path, limit: u64) -> io::Result<String> {
    let link_metadata = fs::symlink_metadata(path)?;
    if !link_metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "implementation artifact must be a regular non-symlink file",
        ));
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "implementation artifact type or size rejected",
        ));
    }
    hash_stream(file, limit)
}

fn hash_stream(reader: impl Read, limit: u64) -> io::Result<String> {
    let cap = limit
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "artifact limit overflow"))?;
    let mut reader = reader.take(cap);
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let n = match reader.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            other => other?,
        };
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "implementation artifact exceeds streaming byte limit",
            ));
        }
        hash.update(&buffer[..n]);
    }
    if total == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty implementation artifact",
        ));
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_stream_exact_boundary_and_binary_bytes() {
        let bytes = [0, 255, 1, 128];
        assert_eq!(
            hash_stream(&bytes[..], 4).unwrap(),
            format!("{:x}", Sha256::digest(bytes))
        );
        assert!(hash_stream(&bytes[..], 3).is_err());
    }

    #[test]
    fn endless_stream_reads_only_limit_plus_sentinel() {
        struct Endless(usize);
        impl Read for Endless {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                self.0 += out.len();
                out.fill(0);
                Ok(out.len())
            }
        }
        let mut stream = Endless(0);
        assert!(hash_stream(&mut stream, 7).is_err());
        assert_eq!(stream.0, 8);
    }

    #[test]
    fn artifact_empty_and_invalid_limit_rejected() {
        assert!(hash_stream(&b""[..], 4).is_err());
        assert!(hash_stream(&b"a"[..], u64::MAX).is_err());
    }

    #[test]
    fn partial_io_failure_never_returns_a_digest() {
        struct Broken(bool);
        impl Read for Broken {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if self.0 {
                    Err(io::Error::other("injected failure"))
                } else {
                    self.0 = true;
                    out[0] = 1;
                    Ok(1)
                }
            }
        }
        assert!(hash_stream(Broken(false), 10).is_err());
    }
}
