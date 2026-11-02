use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use keel_core::{ErrorCode, ToolError};
use serde::{Deserialize, Serialize};

/// One recovered file in the trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashEntry {
    pub id: String,
    pub original: PathBuf,
    pub stored: PathBuf,
}

/// Host capability for recoverable file operations.
///
/// `backup` copies a file into the trash before an overwrite. It never
/// removes the original and never falls back to a permanent delete. The move
/// variant used by a future `delete_file` tool belongs to the same port.
///
/// `list` and `find` expose the backups so a host can offer an explicit
/// restore instead of asking the model to rebuild the file. They have
/// defaults: a backend written before those methods existed keeps compiling
/// and reports that it cannot list.
#[async_trait]
pub trait TrashStore: Send + Sync {
    async fn backup(&self, path: &Path) -> Result<TrashEntry, ToolError>;

    /// Removes a file while keeping a recoverable copy. The default backs up
    /// and then removes; a local backend can move the file instead, which is
    /// what large deletions need.
    async fn discard(&self, path: &Path) -> Result<TrashEntry, ToolError> {
        let entry = self.backup(path).await?;
        tokio::fs::remove_file(path)
            .await
            .map_err(|error| ToolError::io(error.to_string()))?;
        Ok(entry)
    }

    async fn list(&self, _limit: usize) -> Result<Vec<TrashEntry>, ToolError> {
        Err(ToolError::new(
            ErrorCode::Internal,
            "this trash backend cannot list entries",
            false,
        ))
    }

    async fn find(&self, _id: &str) -> Result<TrashEntry, ToolError> {
        Err(ToolError::new(
            ErrorCode::Internal,
            "this trash backend cannot find entries",
            false,
        ))
    }
}

/// Orders ids by their `millis-counter-name` prefix. A plain string sort
/// misorders counters once they reach two digits.
fn id_order(id: &str) -> (u128, u64) {
    let mut parts = id.splitn(3, '-');
    let millis = parts
        .next()
        .and_then(|part| part.parse::<u128>().ok())
        .unwrap_or(0);
    let counter = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .unwrap_or(0);
    (millis, counter)
}

/// Metadata stored next to a backup so a later process can find its original
/// path. The data file itself keeps the id as its name.
#[derive(Debug, Serialize, Deserialize)]
struct TrashMeta {
    original: PathBuf,
}

/// Local trash. `new` uses `<workspace>/.keel/trash`; `at` uses the given
/// directory directly, so a host can keep undo data in a state directory.
///
/// This is the deterministic backend for tests and headless hosts. An OS
/// recycle-bin backend lands with the delete tool.
pub struct LocalTrashStore {
    trash_root: PathBuf,
    counter: AtomicU64,
}

impl LocalTrashStore {
    pub fn new(root: PathBuf) -> Self {
        Self::at(root.join(".keel").join("trash"))
    }

    /// Uses `trash_root` as the trash directory itself.
    pub fn at(trash_root: PathBuf) -> Self {
        Self {
            trash_root,
            counter: AtomicU64::new(1),
        }
    }

    pub fn trash_dir(&self) -> PathBuf {
        self.trash_root.clone()
    }

    fn meta_dir(&self) -> PathBuf {
        self.trash_dir().join("meta")
    }

    async fn write_meta(&self, id: &str, original: &Path) -> Result<(), ToolError> {
        let dir = self.meta_dir();
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|error| ToolError::io(error.to_string()))?;
        let text = toml::to_string(&TrashMeta {
            original: original.to_path_buf(),
        })
        .map_err(|error| ToolError::io(error.to_string()))?;
        tokio::fs::write(dir.join(format!("{id}.toml")), text)
            .await
            .map_err(|error| ToolError::io(error.to_string()))
    }

    async fn read_meta(&self, id: &str) -> Result<TrashEntry, ToolError> {
        let file_name = Path::new(id);
        if file_name.components().count() != 1
            || !matches!(file_name.components().next(), Some(Component::Normal(_)))
        {
            return Err(ToolError::invalid_arguments("invalid trash entry id"));
        }
        let meta_path = self.meta_dir().join(format!("{id}.toml"));
        let text = tokio::fs::read_to_string(&meta_path)
            .await
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    ToolError::not_found(format!("trash entry `{id}` does not exist"))
                } else {
                    ToolError::io(error.to_string())
                }
            })?;
        let meta: TrashMeta = toml::from_str(&text)
            .map_err(|error| ToolError::io(format!("trash entry `{id}` is malformed: {error}")))?;
        let stored = self.trash_dir().join(id);
        if !stored.is_file() {
            return Err(ToolError::not_found(format!(
                "trash entry `{id}` has no data file"
            )));
        }
        Ok(TrashEntry {
            id: id.to_owned(),
            original: meta.original,
            stored,
        })
    }

    /// Validates the source and allocates the trash location and id.
    async fn prepare(&self, path: &Path) -> Result<(String, PathBuf), ToolError> {
        let trash_dir = self.trash_dir();
        if path.starts_with(&trash_dir) {
            return Err(ToolError::invalid_arguments(
                "refusing to trash a file that is already in the trash",
            ));
        }
        let metadata = tokio::fs::metadata(path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ToolError::not_found(format!("`{}` does not exist", path.display()))
            } else {
                ToolError::io(error.to_string())
            }
        })?;
        if !metadata.is_file() {
            return Err(ToolError::invalid_arguments(format!(
                "`{}` is not a regular file",
                path.display()
            )));
        }
        tokio::fs::create_dir_all(&trash_dir)
            .await
            .map_err(|error| ToolError::io(error.to_string()))?;
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let counter = self.counter.fetch_add(1, Ordering::Relaxed);
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "item".to_owned());
        let id = format!("{millis}-{counter}-{file_name}");
        let stored = trash_dir.join(&id);
        Ok((id, stored))
    }
}

#[async_trait]
impl TrashStore for LocalTrashStore {
    async fn backup(&self, path: &Path) -> Result<TrashEntry, ToolError> {
        let (id, stored) = self.prepare(path).await?;
        tokio::fs::copy(path, &stored)
            .await
            .map_err(|error| ToolError::io(error.to_string()))?;
        self.write_meta(&id, path).await?;

        Ok(TrashEntry {
            id,
            original: path.to_path_buf(),
            stored,
        })
    }

    async fn discard(&self, path: &Path) -> Result<TrashEntry, ToolError> {
        let (id, stored) = self.prepare(path).await?;
        match tokio::fs::rename(path, &stored).await {
            Ok(()) => {
                if let Err(error) = self.write_meta(&id, path).await {
                    // Put the file back rather than leave an unreachable copy.
                    let _ = tokio::fs::rename(&stored, path).await;
                    return Err(error);
                }
            }
            Err(_) => {
                // A volume boundary: copy, record, then remove the source.
                tokio::fs::copy(path, &stored)
                    .await
                    .map_err(|error| ToolError::io(error.to_string()))?;
                self.write_meta(&id, path).await?;
                tokio::fs::remove_file(path)
                    .await
                    .map_err(|error| ToolError::io(error.to_string()))?;
            }
        }

        Ok(TrashEntry {
            id,
            original: path.to_path_buf(),
            stored,
        })
    }

    async fn list(&self, limit: usize) -> Result<Vec<TrashEntry>, ToolError> {
        let dir = self.meta_dir();
        let mut reader = match tokio::fs::read_dir(&dir).await {
            Ok(reader) => reader,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(ToolError::io(error.to_string())),
        };
        let mut entries = Vec::new();
        loop {
            let entry = match reader.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(error) => return Err(ToolError::io(error.to_string())),
            };
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Ok(text) = tokio::fs::read_to_string(&path).await else {
                continue;
            };
            let Ok(meta) = toml::from_str::<TrashMeta>(&text) else {
                continue;
            };
            let stored = self.trash_dir().join(id);
            if !stored.is_file() {
                continue;
            }
            entries.push(TrashEntry {
                id: id.to_owned(),
                original: meta.original,
                stored,
            });
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(id_order(&entry.id)));
        entries.truncate(limit);
        Ok(entries)
    }

    async fn find(&self, id: &str) -> Result<TrashEntry, ToolError> {
        self.read_meta(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn at_uses_the_given_directory_directly() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("notes.txt");
        std::fs::write(&source, "old content").unwrap();
        let store = LocalTrashStore::at(dir.path().join("state").join("trash"));

        let entry = store.backup(&source).await.unwrap();

        assert!(entry
            .stored
            .starts_with(dir.path().join("state").join("trash")));
        assert!(dir.path().join("state").join("trash").join("meta").is_dir());
        assert!(!dir.path().join(".keel").exists());
    }

    #[tokio::test]
    async fn discard_moves_the_file_and_keeps_it_restorable() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("large.bin");
        std::fs::write(&source, "payload").unwrap();
        let store = LocalTrashStore::new(dir.path().to_path_buf());

        let entry = store.discard(&source).await.unwrap();

        assert!(!source.exists());
        assert!(entry.stored.is_file());
        assert_eq!(std::fs::read_to_string(&entry.stored).unwrap(), "payload");
        let found = store.find(&entry.id).await.unwrap();
        assert_eq!(found.original, source);
    }

    #[tokio::test]
    async fn backup_copies_the_file_and_leaves_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("notes.txt");
        std::fs::write(&source, "old content").unwrap();
        let store = LocalTrashStore::new(dir.path().to_path_buf());

        let entry = store.backup(&source).await.unwrap();

        assert_eq!(entry.original, source);
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "old content");
        assert_eq!(
            std::fs::read_to_string(&entry.stored).unwrap(),
            "old content"
        );
        assert!(entry.stored.starts_with(store.trash_dir()));
    }

    #[tokio::test]
    async fn backups_are_listed_newest_first_and_found_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("notes.txt");
        let store = LocalTrashStore::new(dir.path().to_path_buf());

        std::fs::write(&source, "first").unwrap();
        let first = store.backup(&source).await.unwrap();
        std::fs::write(&source, "second").unwrap();
        let second = store.backup(&source).await.unwrap();

        let entries = store.list(10).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, second.id, "newest first");
        assert_eq!(entries[0].original, source);

        let found = store.find(&first.id).await.unwrap();
        assert_eq!(found.stored, first.stored);
        assert_eq!(std::fs::read_to_string(&found.stored).unwrap(), "first");
    }

    #[tokio::test]
    async fn a_trash_id_cannot_escape_the_trash_directory() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalTrashStore::new(dir.path().to_path_buf());

        let error = store.find("../outside").await.unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidArguments);
    }

    #[tokio::test]
    async fn backup_of_a_missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalTrashStore::new(dir.path().to_path_buf());
        let error = store
            .backup(&dir.path().join("missing.txt"))
            .await
            .unwrap_err();
        assert_eq!(error.code, keel_core::ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn backup_rejects_directories() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let store = LocalTrashStore::new(dir.path().to_path_buf());
        assert!(store.backup(&nested).await.is_err());
    }
}
