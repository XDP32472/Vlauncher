use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// User-owned paths used by launcher-core.
#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// Resolve the data directory without requiring administrator privileges.
    ///
    /// If `portable.flag` sits next to the executable, data is stored in a
    /// sibling `data/` directory. Otherwise, the platform's per-user local
    /// data directory is used.
    pub fn resolve() -> Result<Self, PathError> {
        let executable = std::env::current_exe().map_err(|source| PathError::Io {
            path: PathBuf::from("<current executable>"),
            source,
        })?;
        let executable_dir = executable
            .parent()
            .ok_or_else(|| PathError::MissingParent(executable.clone()))?;

        let root = if executable_dir.join("portable.flag").is_file() {
            executable_dir.join("data")
        } else {
            directories::ProjectDirs::from("com", "XDP32472", "Vlauncher")
                .ok_or(PathError::NoUserDataDirectory)?
                .data_local_dir()
                .to_path_buf()
        };

        Ok(Self { root })
    }

    /// Construct paths under a caller-provided root, primarily for embedding.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn shared(&self) -> PathBuf {
        self.root.join("shared")
    }

    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }

    pub fn instance_dir(&self, id: &InstanceId) -> PathBuf {
        self.instances().join(id.as_str())
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Create the launcher's directory layout under the resolved user root.
    pub fn ensure_layout(&self) -> Result<(), PathError> {
        let directories = [
            self.shared().join("versions"),
            self.shared().join("libraries"),
            self.shared().join("assets").join("indexes"),
            self.shared().join("assets").join("objects"),
            self.shared().join("runtimes"),
            self.instances(),
            self.cache(),
            self.logs(),
        ];

        for path in directories {
            fs::create_dir_all(&path).map_err(|source| PathError::Io { path, source })?;
        }

        Ok(())
    }
}

/// A validated, filesystem-safe instance identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstanceId(String);

impl InstanceId {
    pub fn parse(value: impl Into<String>) -> Result<Self, PathError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
            });

        if !valid {
            return Err(PathError::InvalidInstanceId(value));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Join a manifest-provided relative path beneath `base`.
///
/// This is lexical validation; callers handling untrusted archives must also
/// avoid following attacker-controlled symlinks inside the destination tree.
pub fn safe_join(base: &Path, relative: &str) -> Result<PathBuf, PathError> {
    if relative.is_empty()
        || relative.contains('\\')
        || relative.contains('\0')
        || relative.contains(':')
        || relative.starts_with('/')
    {
        return Err(PathError::UnsafeRelativePath(relative.to_owned()));
    }

    let mut result = base.to_path_buf();
    for component in relative.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(PathError::UnsafeRelativePath(relative.to_owned()));
        }
        result.push(component);
    }

    if !result.starts_with(base) {
        return Err(PathError::UnsafeRelativePath(relative.to_owned()));
    }

    Ok(result)
}

#[derive(Debug, Error)]
pub enum PathError {
    #[error("could not determine a per-user data directory")]
    NoUserDataDirectory,
    #[error("path has no parent directory: {0}")]
    MissingParent(PathBuf),
    #[error("invalid instance identifier: {0}")]
    InvalidInstanceId(String),
    #[error("unsafe relative path: {0}")]
    UnsafeRelativePath(String),
    #[error("filesystem error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
