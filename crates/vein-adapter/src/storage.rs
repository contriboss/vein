use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use tokio::{
    fs::{self, File, OpenOptions},
    io::AsyncWriteExt,
};

// Retry configuration constants
const MAX_ATTEMPTS: u32 = 3;
const BACKOFF_MS: u64 = 100;

#[derive(Clone)]
pub struct FilesystemStorage {
    root: PathBuf,
}

impl FilesystemStorage {
    pub fn new(root: PathBuf) -> Self {
        FilesystemStorage { root }
    }

    pub async fn prepare(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("creating storage root {}", self.root.display()))
    }

    pub fn resolve(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub async fn open_read(&self, relative: &str) -> Result<Option<FileHandle>> {
        let path = self.resolve(relative);

        // Retry logic for file open
        let mut attempt = 0;
        let file = loop {
            attempt += 1;
            match File::open(&path).await {
                Ok(file) => break file,
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
                Err(e) if should_retry(&e) && attempt < MAX_ATTEMPTS => {
                    tracing::debug!(
                        "open_read attempt {}/{} failed with {:?}, retrying in {}ms: {}",
                        attempt,
                        MAX_ATTEMPTS,
                        e.kind(),
                        BACKOFF_MS,
                        path.display()
                    );
                    tokio::time::sleep(Duration::from_millis(BACKOFF_MS)).await;
                    continue;
                }
                Err(e) => {
                    return Err(anyhow::Error::from(e).context(format!(
                        "opening cached asset {} (after {} attempts)",
                        path.display(),
                        attempt
                    )));
                }
            }
        };

        let metadata = file
            .metadata()
            .await
            .with_context(|| format!("reading metadata {}", path.display()))?;

        Ok(Some(FileHandle {
            file,
            size: metadata.len(),
            path,
        }))
    }

    pub async fn create_temp_writer(&self, relative: &str) -> Result<TempFile> {
        let final_path = self.resolve(relative);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating storage dir {}", parent.display()))?;
        }

        let tmp_path = temp_path_for(&final_path);

        // Retry logic for temp file creation
        let mut attempt = 0;
        let file = loop {
            attempt += 1;
            match OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)
                .await
            {
                Ok(file) => break file,
                Err(e) if should_retry(&e) && attempt < MAX_ATTEMPTS => {
                    tracing::debug!(
                        "create_temp_writer attempt {}/{} failed with {:?}, retrying in {}ms: {}",
                        attempt,
                        MAX_ATTEMPTS,
                        e.kind(),
                        BACKOFF_MS,
                        tmp_path.display()
                    );
                    tokio::time::sleep(Duration::from_millis(BACKOFF_MS)).await;
                    continue;
                }
                Err(e) => {
                    return Err(anyhow::Error::from(e).context(format!(
                        "creating temp file {} (after {} attempts)",
                        tmp_path.display(),
                        attempt
                    )));
                }
            }
        };

        Ok(TempFile {
            tmp_path,
            final_path,
            file,
        })
    }
}

pub struct FileHandle {
    pub file: File,
    pub size: u64,
    pub path: PathBuf,
}

pub struct TempFile {
    tmp_path: PathBuf,
    final_path: PathBuf,
    file: File,
}

impl TempFile {
    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    pub async fn commit(self) -> Result<()> {
        let Self {
            tmp_path,
            final_path,
            mut file,
        } = self;

        file.flush()
            .await
            .with_context(|| format!("flushing {}", tmp_path.display()))?;
        drop(file);

        // Retry logic for atomic rename
        let mut attempt = 0;
        loop {
            attempt += 1;
            match fs::rename(&tmp_path, &final_path).await {
                Ok(()) => return Ok(()),
                Err(e) if should_retry(&e) && attempt < MAX_ATTEMPTS => {
                    tracing::debug!(
                        "commit (rename) attempt {}/{} failed with {:?}, \
                         retrying in {}ms: {} -> {}",
                        attempt,
                        MAX_ATTEMPTS,
                        e.kind(),
                        BACKOFF_MS,
                        tmp_path.display(),
                        final_path.display()
                    );
                    tokio::time::sleep(Duration::from_millis(BACKOFF_MS)).await;
                    continue;
                }
                Err(e) => {
                    return Err(anyhow::Error::from(e).context(format!(
                        "moving {} to {} (after {} attempts)",
                        tmp_path.display(),
                        final_path.display(),
                        attempt
                    )));
                }
            }
        }
    }

    pub async fn rollback(self) -> Result<()> {
        let Self {
            tmp_path, mut file, ..
        } = self;
        file.flush()
            .await
            .with_context(|| format!("flushing {}", tmp_path.display()))?;
        drop(file);
        match fs::remove_file(&tmp_path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow::Error::from(e)
                .context(format!("removing temp file {}", tmp_path.display()))),
        }
    }
}

fn temp_path_for(final_path: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let tmp_name = match final_path.file_name().and_then(|s| s.to_str()) {
        Some(name) => format!("{name}.tmp-{pid}-{timestamp}"),
        None => format!("tmp-{pid}-{timestamp}"),
    };
    final_path.with_file_name(tmp_name)
}

/// Determines if an I/O error should be retried
fn should_retry(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted)
        || matches!(error.raw_os_error(), Some(16) | Some(11))
    // 16 = EBUSY (Device or resource busy)
    // 11 = EAGAIN (Resource temporarily unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_new_creates_storage() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
        assert_eq!(storage.root, temp_dir.path());
    }

    #[tokio::test]
    async fn test_prepare_creates_root_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_path = temp_dir.path().join("storage_root");
        let storage = FilesystemStorage::new(storage_path.clone());

        assert!(!storage_path.exists());
        storage.prepare().await.unwrap();
        assert!(storage_path.exists());
        assert!(storage_path.is_dir());
    }

    #[tokio::test]
    async fn test_prepare_succeeds_if_directory_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());

        storage.prepare().await.unwrap();
        storage.prepare().await.unwrap();
    }

    #[tokio::test]
    async fn test_resolve_joins_paths() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());

        let resolved = storage.resolve("gems/rack-3.0.0.gem");
        assert_eq!(resolved, temp_dir.path().join("gems/rack-3.0.0.gem"));
    }

    #[tokio::test]
    async fn test_resolve_handles_nested_paths() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());

        let resolved = storage.resolve("a/b/c/file.gem");
        assert_eq!(resolved, temp_dir.path().join("a/b/c/file.gem"));
    }

    #[tokio::test]
    async fn test_open_read_returns_none_for_missing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());

        let result = storage.open_read("missing.gem").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_open_read_returns_handle_for_existing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
        storage.prepare().await.unwrap();

        let path = storage.resolve("test.gem");
        fs::write(&path, b"content").await.unwrap();

        let handle = storage.open_read("test.gem").await.unwrap();
        assert!(handle.is_some());

        let mut handle = handle.unwrap();
        let mut buf = Vec::new();
        handle.file.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"content");
    }

    #[tokio::test]
    async fn test_create_temp_writer_creates_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
        storage.prepare().await.unwrap();

        let mut temp_file = storage.create_temp_writer("test/file.gem").await.unwrap();
        temp_file.file_mut().write_all(b"data").await.unwrap();
        temp_file.commit().await.unwrap();

        let final_path = storage.resolve("test/file.gem");
        let data = fs::read(final_path).await.unwrap();
        assert_eq!(data, b"data");
    }

    #[tokio::test]
    async fn test_commit_moves_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
        storage.prepare().await.unwrap();

        let mut temp_file = storage.create_temp_writer("test/file.gem").await.unwrap();
        temp_file.file_mut().write_all(b"data").await.unwrap();
        let tmp_path = temp_file.tmp_path.clone();
        let final_path = temp_file.final_path.clone();
        temp_file.commit().await.unwrap();

        assert!(!tmp_path.exists());
        assert!(final_path.exists());
    }

    #[tokio::test]
    async fn test_rollback_removes_temp_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
        storage.prepare().await.unwrap();

        let temp_file = storage.create_temp_writer("test/file.gem").await.unwrap();
        let tmp_path = temp_file.tmp_path.clone();
        temp_file.rollback().await.unwrap();

        assert!(!tmp_path.exists());
        assert!(!storage.resolve("test/file.gem").exists());
    }

    #[tokio::test]
    async fn test_temp_path_generation() {
        let final_path = PathBuf::from("foo/bar.gem");
        let tmp_path = temp_path_for(&final_path);
        assert!(
            tmp_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("bar.gem.tmp-")
        );
    }

    #[test]
    fn test_should_retry_logic() {
        // Should retry these errors
        let would_block = std::io::Error::from(ErrorKind::WouldBlock);
        assert!(should_retry(&would_block));

        let interrupted = std::io::Error::from(ErrorKind::Interrupted);
        assert!(should_retry(&interrupted));

        // Should NOT retry these errors
        let not_found = std::io::Error::from(ErrorKind::NotFound);
        assert!(!should_retry(&not_found));

        let permission_denied = std::io::Error::from(ErrorKind::PermissionDenied);
        assert!(!should_retry(&permission_denied));
    }
}
