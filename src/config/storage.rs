use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_path")]
    pub path: PathBuf,
}

impl StorageConfig {
    pub fn normalize_paths(&mut self, base_dir: &Path) {
        if self.path.is_relative() {
            self.path = base_dir.join(&self.path);
        }
    }

    pub fn ensure_directories(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.path)?;
        Ok(())
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: default_storage_path(),
        }
    }
}

fn default_storage_path() -> PathBuf {
    PathBuf::from("./gems")
}
