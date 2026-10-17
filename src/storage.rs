use anyhow::{Context, Result, bail};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending
        .persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("save {}", path.display()))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub struct DemoStore {
    pub directory: PathBuf,
    _lock: File,
}

impl DemoStore {
    pub fn open(directory: PathBuf) -> Result<Self> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(directory.join(".demo.lock"))?;
        if lock.try_lock_exclusive().is_err() {
            bail!("another demo owns this data directory; use a different --data-dir");
        }
        Ok(Self {
            directory,
            _lock: lock,
        })
    }

    pub fn load<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>> {
        let path = self.directory.join("demo-state.json");
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(
                serde_json::from_slice(&bytes)
                    .context("invalid demo state; preserved without overwriting")?,
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save<T: serde::Serialize>(&self, value: &T) -> Result<()> {
        write_private(
            &self.directory.join("demo-state.json"),
            &serde_json::to_vec_pretty(value)?,
        )
    }
}
