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
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn prepare(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("creating storage root {}", self.root.display()))
    }

    #[must_use]
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
    pub const fn file_mut(&mut self) -> &mut File {
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
        || matches!(error.raw_os_error(), Some(16 | 11))
    // 16 = EBUSY (Device or resource busy)
    // 11 = EAGAIN (Resource temporarily unavailable)
}

#[cfg(test)]
mod tests;
