//! A per-session record of the files the model has read (ADR 0020).
//!
//! A write or an edit is allowed only when the path was read and the bytes on
//! disk still match the bytes that were read. The check is content-based, not
//! mtime-based, and it is best-effort: an external writer can still change the
//! file between the check and the write.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

/// Files the model has observed in one engine, keyed by resolved path and
/// fingerprinted by their content. Cloning shares the same record.
#[derive(Clone, Default)]
pub struct ReadLedger {
    entries: Arc<Mutex<HashMap<PathBuf, String>>>,
}

impl ReadLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the bytes the model has just seen.
    pub fn record(&self, path: &Path, bytes: &[u8]) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.insert(path.to_path_buf(), digest(bytes));
    }

    /// `Ok(())` when the path was read and the current bytes still match.
    /// The error is a recovery instruction for the model, not a code.
    pub fn check(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match entries.get(path) {
            None => Err("read the file before writing it".to_owned()),
            Some(seen) if seen != &digest(bytes) => {
                Err("the file changed since it was read; read it again".to_owned())
            }
            Some(_) => Ok(()),
        }
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
        ledger.record(path, b"one");
        assert_eq!(ledger.check(path, b"one"), Ok(()));
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
        ledger.record(path, b"one");
        let error = ledger.check(path, b"two").unwrap_err();
        assert!(error.contains("changed since it was read"), "{error}");
    }
}
