//! Single-writer journal files and bounded, non-executing prefix inspection.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use prospect_dispatch::execution::record::journal::{
    JournalSink, JournalSummary, MAX_JOURNAL_BYTES, MAX_JOURNAL_ENTRY_BYTES,
    inspect_execution_journal,
};

use crate::input::TextReadBudget;

/// New owner-only journal file; every append is followed by `File::sync_all`.
///
/// Existing paths are never reopened, truncated, repaired or overwritten. The
/// file is deliberately retained on failure or drop for inspection. A failed
/// append poisons the handle permanently, even if it wrote a full or partial line.
///
/// This requests file synchronization but does not sync the parent directory or
/// guarantee survival of a power loss. Parents/files must remain trusted and
/// unmodified. There is no cross-process writer coordination or hard I/O deadline.
///
/// ```no_run
/// use prospect_cli::execution_journal::FileJournal;
/// let journal = FileJournal::create("new-run.journal.jsonl")?;
/// assert_eq!(journal.acknowledged_bytes(), 0);
/// assert!(!journal.is_poisoned());
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct FileJournal {
    file: File,
    acknowledged_bytes: usize,
    poisoned: bool,
}
impl FileJournal {
    /// Atomically create a fresh file. No pre-existing entry, including a link, is replaced.
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        Ok(Self {
            file: options.open(path)?,
            acknowledged_bytes: 0,
            poisoned: false,
        })
    }
    /// Bytes whose writes and synchronization both returned success to this handle.
    #[must_use]
    pub const fn acknowledged_bytes(&self) -> usize {
        self.acknowledged_bytes
    }
    /// A failed write or admission permanently blocks further writes on this handle.
    #[must_use]
    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }
}
impl JournalSink for FileJournal {
    fn append_record(&mut self, record: &[u8]) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other(
                "journal sink is poisoned; inspect without retry",
            ));
        }
        // Set the latch before ANY fallible operation; success alone releases it.
        self.poisoned = true;
        let total = self
            .acknowledged_bytes
            .checked_add(record.len())
            .ok_or_else(|| io::Error::other("journal byte overflow"))?;
        if record.len() > MAX_JOURNAL_ENTRY_BYTES
            || total > MAX_JOURNAL_BYTES
            || !record.ends_with(b"\n")
            || record[..record.len() - 1].contains(&b'\n')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid or oversized journal entry",
            ));
        }
        self.file.write_all(record)?;
        self.file.sync_all()?;
        self.acknowledged_bytes = total;
        self.poisoned = false;
        Ok(())
    }
}

/// Inspect a journal and separately supplied canonical input under one read budget.
///
/// No engine or decoder is called. A successful inspection may describe an open,
/// interrupted, failed or explicitly torn-tail log; it is not a successful run.
/// Fully malformed entries, invalid UTF-8 and oversized inputs return errors.
pub fn inspect_execution_journal_files(
    journal_path: impl AsRef<Path>,
    bundle_path: impl AsRef<Path>,
) -> Result<JournalSummary, Box<dyn std::error::Error>> {
    let mut budget = TextReadBudget::default();
    let journal = budget.read_text(journal_path.as_ref())?;
    let bundle = budget.read_text(bundle_path.as_ref())?;
    Ok(inspect_execution_journal(&journal, &bundle)?)
}
