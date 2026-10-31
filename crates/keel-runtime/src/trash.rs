use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use keel_core::ToolError;

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
#[async_trait]
pub trait TrashStore: Send + Sync {
    async fn backup(&self, path: &Path) -> Result<TrashEntry, ToolError>;
}

/// Workspace-local trash at `.keel/trash`.
///
/// This is the deterministic backend for tests and headless hosts. An OS
/// recycle-bin backend lands with the delete tool.
pub struct LocalTrashStore {
    root: PathBuf,
    counter: AtomicU64,
}

impl LocalTrashStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            counter: AtomicU64::new(1),
        }
    }

    pub fn trash_dir(&self) -> PathBuf {
        self.root.join(".keel").join("trash")
    }
}

#[async_trait]
impl TrashStore for LocalTrashStore {
    async fn backup(&self, path: &Path) -> Result<TrashEntry, ToolError> {
        let trash_dir = self.trash_dir();
        if path.starts_with(&trash_dir) {
            return Err(ToolError::invalid_arguments(
                "refusing to back up a file that is already in the trash",
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

        tokio::fs::copy(path, &stored)
            .await
            .map_err(|error| ToolError::io(error.to_string()))?;

        Ok(TrashEntry {
            id,
            original: path.to_path_buf(),
            stored,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
