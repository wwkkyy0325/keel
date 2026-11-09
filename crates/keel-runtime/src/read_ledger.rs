//! A per-session record of the files the model has read (ADR 0020, ADR 0031).
//!
//! A write or an edit is allowed only when the path was read and the bytes on
//! disk still match the bytes that were read. `write_file` additionally needs
//! a full read: overwriting a file from a line range would discard content the
//! model never saw. A large file can be read in several ranges; the ranges
//! count as full once together they cover every line. The check is
//! content-based, not mtime-based, and it is best-effort: an external writer
//! can still change the file between the check and the write.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

#[derive(Clone)]
struct Entry {
    digest: String,
    /// The whole file has been seen, whether in one read or several ranges.
    full: bool,
    /// 1-based inclusive line ranges seen while the whole file was loaded.
    covered: Vec<(usize, usize)>,
    /// The line count the ranges refer to.
    total: usize,
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
                entries.insert(
                    path.to_path_buf(),
                    Entry {
                        digest,
                        full,
                        covered: Vec::new(),
                        total: 0,
                    },
                );
            }
        }
    }

    /// Records one line range of a file whose whole content was loaded.
    /// Ranges that together cover every line count as a full read, so a large
    /// file can be read in several calls and then rewritten.
    pub fn record_range(&self, path: &Path, bytes: &[u8], first: usize, last: usize, total: usize) {
        if first == 0 || last < first || total == 0 {
            return;
        }
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let digest = digest(bytes);
        let entry = entries.entry(path.to_path_buf()).or_insert_with(|| Entry {
            digest: digest.clone(),
            full: false,
            covered: Vec::new(),
            total,
        });
        if entry.digest != digest {
            *entry = Entry {
                digest,
                full: false,
                covered: Vec::new(),
                total,
            };
        }
        if entry.total != total {
            entry.covered.clear();
            entry.total = total;
        }
        merge_range(&mut entry.covered, first.min(total), last.min(total));
        if entry.covered == vec![(1, total)] {
            entry.full = true;
        }
    }

    /// Records the result of an edit. The model saw the old bytes and applied
    /// an exact replacement, so a prior full read carries over; line ranges do
    /// not, because an edit can move every line after it.
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
                covered: Vec::new(),
                total: 0,
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
                "read the whole file before overwriting it; read the remaining lines in ranges first"
                    .to_owned(),
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

/// Inserts a 1-based inclusive range, merging overlaps and neighbors.
fn merge_range(ranges: &mut Vec<(usize, usize)>, first: usize, last: usize) {
    ranges.push((first, last));
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges.iter().copied() {
        match merged.last_mut() {
            Some((_, last_end)) if start <= last_end.saturating_add(1) => {
                *last_end = (*last_end).max(end);
            }
            _ => merged.push((start, end)),
        }
    }
    *ranges = merged;
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

    #[test]
    fn ranges_that_cover_the_file_count_as_a_full_read() {
        let ledger = ReadLedger::new();
        let path = Path::new("/workspace/long.txt");

        ledger.record_range(path, b"one", 1, 2, 3);
        assert!(ledger.check_full(path, b"one").is_err());

        ledger.record_range(path, b"one", 3, 3, 3);
        assert_eq!(ledger.check_full(path, b"one"), Ok(()));
    }

    #[test]
    fn overlapping_ranges_merge() {
        let ledger = ReadLedger::new();
        let path = Path::new("/workspace/long.txt");

        ledger.record_range(path, b"one", 1, 5, 10);
        ledger.record_range(path, b"one", 3, 8, 10);
        ledger.record_range(path, b"one", 6, 10, 10);

        assert_eq!(ledger.check_full(path, b"one"), Ok(()));
    }

    #[test]
    fn an_edit_drops_the_range_coverage() {
        let ledger = ReadLedger::new();
        let path = Path::new("/workspace/long.txt");

        ledger.record_range(path, b"one", 1, 2, 3);
        ledger.record_edit(path, b"two");
        ledger.record_range(path, b"two", 3, 3, 3);

        // The first range no longer describes the edited file.
        assert!(ledger.check_full(path, b"two").is_err());
    }

    #[test]
    fn ranges_of_changed_bytes_do_not_count() {
        let ledger = ReadLedger::new();
        let path = Path::new("/workspace/long.txt");

        ledger.record_range(path, b"one", 1, 3, 3);

        assert!(ledger.check_full(path, b"two").is_err());
    }
}
