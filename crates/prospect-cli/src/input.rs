//! Bounded UTF-8 input for file-based verification, not a filesystem sandbox.
//!
//! Default CLI policy: 16 MiB per file, 128 MiB of cumulative text per operation,
//! and at most 1,024 entries in a generic campaign directory. Repeated reads count
//! again against the same budget. No truncation or canonicalization is performed.
//!
//! Paths, their parents and the opened files must remain trusted and unmodified.
//! Metadata checks reject static final-component symlinks and non-regular files;
//! they do not provide race-free path isolation, a deadline, or a process-memory
//! limit. JSON parsing and other allocations have their own overhead.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

/// Maximum bytes in a file accepted by the default verification policy.
pub const MAX_JSON_FILE_BYTES: usize = 16 * 1024 * 1024;
/// Cumulative text-read budget of one default verification operation.
pub const MAX_VERIFICATION_TEXT_BYTES: usize = 128 * 1024 * 1024;
/// Maximum entries, including manifests, in one generic campaign directory.
pub const MAX_CAMPAIGN_ENTRIES: usize = 1024;

/// A reusable cumulative budget for composing file-based verifiers.
///
/// Once any read fails, the budget is closed: callers cannot continue an
/// operation after a truncated, unreadable or invalid UTF-8 input. This object
/// does not retain file contents and cannot account for downstream parser memory.
#[derive(Debug)]
pub struct TextReadBudget {
    per_file_bytes: usize,
    remaining_bytes: usize,
    failed: bool,
}

impl Default for TextReadBudget {
    fn default() -> Self {
        Self {
            per_file_bytes: MAX_JSON_FILE_BYTES,
            remaining_bytes: MAX_VERIFICATION_TEXT_BYTES,
            failed: false,
        }
    }
}

impl TextReadBudget {
    /// Construct an explicit byte policy for an application using the Rust API.
    ///
    /// Both limits must be positive and the per-file limit must leave room for
    /// one lookahead byte. CLI commands always use the documented defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use prospect_cli::input::TextReadBudget;
    /// let budget = TextReadBudget::new(1024, 4096)?;
    /// assert_eq!(budget.remaining_bytes(), 4096);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn new(per_file_bytes: usize, total_bytes: usize) -> io::Result<Self> {
        if per_file_bytes == 0 || per_file_bytes == usize::MAX || total_bytes == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid verification input limits"));
        }
        Ok(Self { per_file_bytes, remaining_bytes: total_bytes, failed: false })
    }

    /// Remaining cumulative bytes. Successful repeated reads are charged again.
    #[must_use]
    pub const fn remaining_bytes(&self) -> usize {
        self.remaining_bytes
    }

    /// Read one regular UTF-8 file without changing any accepted input bytes.
    ///
    /// Oversized advertised lengths are rejected before content allocation. The
    /// actual stream is also limited to the allowed bytes plus one sentinel, so
    /// a stale or zero advertised length cannot disable the bound. A sentinel
    /// byte beyond the limit is an error, never a successful truncated prefix.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for unreadable files, static symlinks/special files,
    /// invalid UTF-8, exceeded byte limits, or a previously failed budget. The
    /// caller must keep paths and files unmodified throughout verification.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use prospect_cli::input::TextReadBudget;
    /// let mut budget = TextReadBudget::default();
    /// let manifest = budget.read_text("campaign/manifest.json")?;
    /// let campaign = budget.read_text("campaign/campaign.json")?;
    /// assert!(!manifest.is_empty() && !campaign.is_empty());
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn read_text(&mut self, path: impl AsRef<Path>) -> io::Result<String> {
        let path = path.as_ref();
        let result = (|| {
            self.require_open()?;
            let allowed = self.per_file_bytes.min(self.remaining_bytes);
            self.check_metadata(&fs::symlink_metadata(path)?, allowed)?;
            let file = File::open(path)?;
            self.check_metadata(&file.metadata()?, allowed)?;
            self.read_stream(file)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn require_open(&self) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "verification input budget closed after read failure"));
        }
        Ok(())
    }

    fn limit_error(&self) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, format!(
            "verification input limit exceeded (per-file {} bytes, remaining total {} bytes)",
            self.per_file_bytes, self.remaining_bytes,
        ))
    }

    fn check_metadata(&self, metadata: &fs::Metadata, allowed: usize) -> io::Result<()> {
        if !metadata.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "verification input must be a regular file, not a symlink or special file"));
        }
        if metadata.len() > allowed as u64 {
            return Err(self.limit_error());
        }
        Ok(())
    }

    fn read_stream(&mut self, reader: impl Read) -> io::Result<String> {
        let result = (|| {
            self.require_open()?;
            let allowed = self.per_file_bytes.min(self.remaining_bytes);
            let mut bytes = Vec::new();
            // The constructor reserves space for this sentinel without overflow.
            let read_result = reader.take((allowed + 1) as u64).read_to_end(&mut bytes);
            let exceeds_limit = bytes.len() > allowed;
            let limit_error = exceeds_limit.then(|| self.limit_error());
            self.remaining_bytes = self.remaining_bytes.saturating_sub(bytes.len());
            read_result?;
            if let Some(error) = limit_error {
                return Err(error);
            }
            String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}

/// Read a single regular UTF-8 file with the default byte policy.
///
/// Compose multiple reads with [`TextReadBudget`] when they belong to one
/// operation. Failure never returns a partial or normalized string.
///
/// # Examples
///
/// ```no_run
/// let payload = prospect_cli::input::read_text("experiment.bundle.json")?;
/// assert!(!payload.is_empty());
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn read_text(path: impl AsRef<Path>) -> io::Result<String> {
    TextReadBudget::default().read_text(path)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, ErrorKind};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct TempDirectory(std::path::PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "prospect-input-{}-{}-{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn preserves_exact_utf8_and_exact_limit_without_normalization() {
        let directory = TempDirectory::new();
        let path = directory.0.join("input.json");
        let text = "{\"x\":\"é\"}\n";
        fs::write(&path, text).unwrap();
        let mut budget = TextReadBudget::new(text.len(), text.len()).unwrap();
        assert_eq!(budget.read_text(&path).unwrap().as_bytes(), text.as_bytes());
        assert_eq!(budget.remaining_bytes(), 0);
    }

    #[test]
    fn rejects_oversized_metadata_without_charging_content() {
        let directory = TempDirectory::new();
        let path = directory.0.join("large.json");
        File::create(&path).unwrap().set_len(MAX_JSON_FILE_BYTES as u64 + 1).unwrap();
        let mut budget = TextReadBudget::default();
        let error = budget.read_text(&path).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(error.to_string().contains("limit exceeded"));
        assert_eq!(budget.remaining_bytes(), MAX_VERIFICATION_TEXT_BYTES);
    }

    #[test]
    fn stream_bound_reads_only_limit_plus_sentinel() {
        let mut reader = Cursor::new(vec![b'x'; 100]);
        let mut budget = TextReadBudget::new(8, 100).unwrap();
        assert!(budget.read_stream(&mut reader).is_err());
        assert_eq!(reader.position(), 9);
        assert!(budget.read_stream(Cursor::new(b"{}")).is_err());
    }

    #[test]
    fn never_accepts_a_valid_json_prefix_with_oversized_tail() {
        let mut budget = TextReadBudget::new(2, 20).unwrap();
        let error = budget.read_stream(Cursor::new(b"{}malicious-tail")).unwrap_err();
        assert!(error.to_string().contains("limit exceeded"));
    }

    #[test]
    fn cumulative_budget_counts_repeated_reads_and_exhaustion() {
        let directory = TempDirectory::new();
        let path = directory.0.join("input.json");
        fs::write(&path, "{}").unwrap();
        let mut budget = TextReadBudget::new(4, 4).unwrap();
        assert_eq!(budget.read_text(&path).unwrap(), "{}");
        assert_eq!(budget.read_text(&path).unwrap(), "{}");
        assert_eq!(budget.remaining_bytes(), 0);
        assert!(budget.read_text(&path).is_err());
    }

    #[test]
    fn total_stream_bound_can_be_smaller_than_per_file_bound() {
        let mut reader = Cursor::new(b"123456789");
        let mut budget = TextReadBudget::new(100, 4).unwrap();
        assert!(budget.read_stream(&mut reader).is_err());
        assert_eq!(reader.position(), 5);
        assert_eq!(budget.remaining_bytes(), 0);
    }

    #[test]
    fn invalid_utf8_closes_the_budget_without_partial_success() {
        let directory = TempDirectory::new();
        let path = directory.0.join("bad.json");
        fs::write(&path, [0xff]).unwrap();
        let mut budget = TextReadBudget::new(8, 16).unwrap();
        assert_eq!(budget.read_text(&path).unwrap_err().kind(), ErrorKind::InvalidData);
        fs::write(&path, "{}").unwrap();
        assert!(budget.read_text(&path).unwrap_err().to_string().contains("closed"));
    }

    #[test]
    fn missing_file_and_directory_are_rejected() {
        let directory = TempDirectory::new();
        assert!(read_text(directory.0.join("missing.json")).is_err());
        assert_eq!(read_text(&directory.0).unwrap_err().kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_invalid_policy_and_keeps_default_limits_explicit() {
        for (file, total) in [(0, 1), (1, 0), (usize::MAX, 1)] {
            assert_eq!(TextReadBudget::new(file, total).unwrap_err().kind(), ErrorKind::InvalidInput);
        }
        assert_eq!(TextReadBudget::default().remaining_bytes(), 128 * 1024 * 1024);
    }

    #[test]
    fn io_error_after_partial_read_closes_the_budget() {
        struct FailingReader(bool);
        impl Read for FailingReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.0 { return Err(io::Error::other("injected read failure")); }
                self.0 = true;
                buffer[0] = b'x';
                Ok(1)
            }
        }
        let mut budget = TextReadBudget::new(8, 16).unwrap();
        assert!(budget.read_stream(FailingReader(false)).is_err());
        assert_eq!(budget.remaining_bytes(), 15);
        assert!(budget.read_stream(Cursor::new(b"{}")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_static_symlinks_including_dangling_links() {
        use std::os::unix::fs::symlink;
        let directory = TempDirectory::new();
        let target = directory.0.join("target.json");
        let link = directory.0.join("link.json");
        fs::write(&target, "{}").unwrap();
        symlink(&target, &link).unwrap();
        assert_eq!(read_text(&link).unwrap_err().kind(), ErrorKind::InvalidInput);
        fs::remove_file(&target).unwrap();
        assert_eq!(read_text(&link).unwrap_err().kind(), ErrorKind::InvalidInput);
    }
}
