use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

use crate::paths::AppPaths;

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
const MAX_PARALLEL_DOWNLOADS: u8 = 32;

/// User-level launcher preferences. Secrets must not be stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    pub max_parallel_downloads: u8,
    pub java_path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            max_parallel_downloads: 8,
            java_path: None,
        }
    }
}

impl Config {
    fn validate(&self) -> Result<(), StoreError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedSchema {
                found: self.schema_version,
                supported: CONFIG_SCHEMA_VERSION,
            });
        }
        if !(1..=MAX_PARALLEL_DOWNLOADS).contains(&self.max_parallel_downloads) {
            return Err(StoreError::InvalidParallelDownloadLimit(
                self.max_parallel_downloads,
            ));
        }
        Ok(())
    }
}

pub fn load_config(paths: &AppPaths) -> Result<Config, StoreError> {
    let Some(config) = read_json::<Config>(&paths.config_file())? else {
        return Ok(Config::default());
    };

    config.validate()?;
    Ok(config)
}

pub fn save_config(paths: &AppPaths, config: &Config) -> Result<(), StoreError> {
    config.validate()?;
    write_json_atomic(&paths.config_file(), config)
}

/// Read a JSON file, returning `None` only when it does not exist.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, StoreError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StoreError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(StoreError::Json)
}

/// Write JSON to a sibling temporary file and atomically replace the target.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    let parent = path.parent().ok_or_else(|| StoreError::MissingParent(path.to_path_buf()))?;
    std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|source| StoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    serde_json::to_writer(&mut temporary, value).map_err(StoreError::Json)?;
    temporary.as_file_mut().write_all(b"\n").map_err(|source| StoreError::Io {
        path: temporary.path().to_path_buf(),
        source,
    })?;
    temporary.as_file().sync_all().map_err(|source| StoreError::Io {
        path: temporary.path().to_path_buf(),
        source,
    })?;

    temporary.persist(path).map_err(|error| StoreError::Io {
        path: path.to_path_buf(),
        source: error.error,
    })?;

    Ok(())
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("unsupported configuration schema {found}; this version supports schema {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("max parallel downloads must be between 1 and 32, got {0}")]
    InvalidParallelDownloadLimit(u8),
    #[error("path has no parent directory: {0}")]
    MissingParent(PathBuf),
    #[error("filesystem error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}
