//! A per-session record of the files the model has read (ADR 0020).
//!
//! A write or an edit is allowed only when the path was read and the bytes on
//! disk still match the bytes that were read. `write_file` additionally needs
//! a full read: overwriting a file from a single line range would discard
//! content the model never saw. The check is content-based, not mtime-based,
//! and it is best-effort: an external writer can still change the file between
//! the check and the write.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

#[derive(Clone)]
struct Entry {
    digest: String,
    full: bool,
}

/// Files the model has observed in one engine, keyed by resolved path and
/// fingerprinted by their content. Cloning shares the same record.
#[derive(Clone, Default)]
pub struct ReadLedger {
    entries: Arc<Mutex<HashMap<PathBuf, Entry>>>,
}

impl ReadLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records bytes the model has seen. `full` says the model saw the whole
    /// file, not a line range or a truncated prefix. A later partial read does
    /// not weaken an earlier full read of the same bytes.
    pub fn record(&self, path: &Path, bytes: &[u8], full: bool) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let digest = digest(bytes);
        match entries.get_mut(path) {
            Some(entry) if entry.digest == digest => entry.full |= full,
            _ => {
                entries.insert(path.to_path_buf(), Entry { digest, full });
            }
        }
    }

    /// Records the result of an edit. The model saw the old bytes and applied
    /// an exact replacement, so the coverage of the prior read carries over.
    pub fn record_edit(&self, path: &Path, bytes: &[u8]) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let full = entries.get(path).is_some_and(|entry| entry.full);
        entries.insert(
            path.to_path_buf(),
            Entry {
                digest: digest(bytes),
                full,
            },
        );
    }

    /// `Ok(())` when the path was read and the current bytes still match.
    /// The error is a recovery instruction for the model, not a code.
    pub fn check(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let entry = self.entry(path);
        check_entry(entry.as_ref(), bytes)
    }

    /// Like [`Self::check`], and the read must have covered the whole file.
    pub fn check_full(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let entry = self.entry(path);
        check_entry(entry.as_ref(), bytes)?;
        if entry.is_some_and(|entry| !entry.full) {
            return Err(
                "read the whole file before overwriting it; a line range is not enough".to_owned(),
            );
        }
        Ok(())
    }

    fn entry(&self, path: &Path) -> Option<Entry> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.get(path).cloned()
    }
}

fn check_entry(entry: Option<&Entry>, bytes: &[u8]) -> Result<(), String> {
    match entry {
        None => Err("read the file before writing it".to_owned()),
        Some(entry) if entry.digest != digest(bytes) => {
            Err("the file changed since it was read; read it again".to_owned())
        }
        Some(_) => Ok(()),
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_file_can_be_written() {
        let ledger = ReadLedger::new();
        let path = Path::new("/workspace/notes.txt");
        ledger.record(path, b"one", true);
        assert_eq!(ledger.check(path, b"one"), Ok(()));
        assert_eq!(ledger.check_full(path, b"one"), Ok(()));
    }

    #[test]
    fn an_unread_file_is_refused() {
        let ledger = ReadLedger::new();
        let error = ledger
            .check(Path::new("/workspace/notes.txt"), b"one")
            .unwrap_err();
        assert!(error.contains("read the file"), "{error}");
    }

    #[test]
    fn a_changed_file_is_stale() {
        let ledger = ReadLedger::new();
        let path = Path::new("/workspace/notes.txt");
        ledger.record(path, b"one", true);
        let error = ledger.check(path, b"two").unwrap_err();
        assert!(error.contains("changed since it was read"), "{error}");
    }

    #[test]
    fn a_ranged_read_cannot_overwrite() {
        let ledger = ReadLedger::new();
        let path = Path::new("/workspace/notes.txt");
        ledger.record(path, b"one", false);

        assert_eq!(ledger.check(path, b"one"), Ok(()));
        let error = ledger.check_full(path, b"one").unwrap_err();
        assert!(error.contains("whole file"), "{error}");
    }

    #[test]
    fn a_full_read_survives_a_ranged_read() {
        let ledger = ReadLedger::new();
        let path = Path::new("/workspace/notes.txt");
        ledger.record(path, b"one", true);
        ledger.record(path, b"one", false);

        assert_eq!(ledger.check_full(path, b"one"), Ok(()));
    }

    #[test]
    fn an_edit_keeps_the_read_coverage() {
        let ledger = ReadLedger::new();
        let path = Path::new("/workspace/notes.txt");

        ledger.record(path, b"one", false);
        ledger.record_edit(path, b"two");
        assert_eq!(ledger.check(path, b"two"), Ok(()));
        assert!(ledger
            .check_full(path, b"two")
            .unwrap_err()
            .contains("whole file"));

        ledger.record(path, b"two", true);
        ledger.record_edit(path, b"three");
        assert_eq!(ledger.check_full(path, b"three"), Ok(()));
    }
}
