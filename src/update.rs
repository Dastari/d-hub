//! Stable GitHub releases, managed binary replacement and a bounded background monitor.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const API: &str = "https://api.github.com/repos/dastari/d-hub/releases/latest";
const RELEASES: &str = "https://github.com/dastari/d-hub/releases/download/";
const LIMIT: u64 = 100 * 1024 * 1024;
const INTERVAL: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub running_version: String,
    pub available_version: Option<String>,
    pub managed: bool,
    pub checking: bool,
    pub restart_pending: bool,
    pub message: String,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
    size: u64,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
fn version(value: &str) -> Result<(u64, u64, u64)> {
    let parts: Vec<_> = value
        .strip_prefix('v')
        .unwrap_or(value)
        .split('.')
        .collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|s| s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()))
    {
        bail!("Release version must have three numeric components");
    }
    Ok((parts[0].parse()?, parts[1].parse()?, parts[2].parse()?))
}
pub fn asset_name() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("d-hub-x86_64-unknown-linux-gnu"),
        "aarch64" => Ok("d-hub-aarch64-unknown-linux-gnu"),
        _ => bail!("No production build is available for this architecture"),
    }
}
/// Capture before an atomic replacement: /proc/self/exe subsequently names a deleted inode.
pub fn executable() -> Result<PathBuf> {
    let path = std::env::current_exe()?;
    let value = path.as_os_str().as_encoded_bytes();
    let suffix = b" (deleted)";
    if let Some(value) = value.strip_suffix(suffix) {
        use std::os::unix::ffi::OsStrExt;
        return Ok(PathBuf::from(std::ffi::OsStr::from_bytes(value)));
    }
    Ok(path)
}
pub fn managed(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let marker = parent.join(".d-hub-managed");
    [parent.to_owned(), path.to_owned(), marker.clone()]
        .iter()
        .all(|p| {
            fs::symlink_metadata(p).is_ok_and(|m| {
                !m.file_type().is_symlink()
                    && m.uid() == nix::unistd::geteuid().as_raw()
                    && m.permissions().mode() & 0o022 == 0
            })
        })
        && fs::read_to_string(marker).is_ok_and(|s| s == "d-hub-managed-v1\n")
}
fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("d-hub/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let host = attempt.url().host_str().unwrap_or_default();
            if attempt.previous().len() < 5
                && attempt.url().scheme() == "https"
                && [
                    "github.com",
                    "api.github.com",
                    "release-assets.githubusercontent.com",
                    "objects.githubusercontent.com",
                ]
                .contains(&host)
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()?)
}
fn latest() -> Result<Release> {
    let response = client()?
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        bail!("Release metadata exceeds size limit");
    }
    let release: Release = serde_json::from_slice(&bytes)?;
    if release.draft || release.prerelease || !release.tag_name.starts_with('v') {
        bail!("Latest release is not a production version");
    }
    version(&release.tag_name)?;
    Ok(release)
}
fn choose(release: &Release, name: &str) -> Result<(String, String, u64)> {
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .context("Production binary missing from release")?;
    let expected = format!("{RELEASES}{}/{name}", release.tag_name);
    // GitHub normalizes owner casing; no other repository, host or path is accepted.
    if !asset.browser_download_url.eq_ignore_ascii_case(&expected)
        || asset.size == 0
        || asset.size > LIMIT
    {
        bail!("Invalid production asset URL or size");
    }
    let digest = asset
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .context("Release has no SHA-256 digest")?;
    if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("Invalid release digest");
    }
    Ok((
        asset.browser_download_url.clone(),
        digest.to_ascii_lowercase(),
        asset.size,
    ))
}
fn verify(bytes: &[u8], digest: &str, size: u64) -> Result<()> {
    if bytes.len() as u64 != size || format!("{:x}", Sha256::digest(bytes)) != digest {
        bail!("Production binary checksum/size verification failed");
    }
    if !bytes.starts_with(b"\x7fELF") {
        bail!("Production binary is not a Linux executable");
    }
    Ok(())
}
fn binary_version(path: &Path) -> Result<String> {
    let output = Command::new(path)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()?;
    if !output.status.success() {
        bail!("Production binary failed its version check");
    }
    let value = String::from_utf8(output.stdout)?;
    let value = value
        .trim()
        .strip_prefix("d-hub ")
        .context("Unexpected binary version response")?;
    version(value)?;
    Ok(value.into())
}
fn install(path: &Path, release: &Release) -> Result<()> {
    if !managed(path) {
        bail!("Automatic replacement requires the managed curl installation");
    }
    let parent = path.parent().context("Executable has no directory")?;
    let _lock = crate::storage::DemoStore::open(parent.join(".update-lock"))?;
    if version(&binary_version(path)?)? >= version(&release.tag_name)? {
        return Ok(());
    }
    let (url, digest, size) = choose(release, asset_name()?)?;
    let response = client()?.get(url).send()?.error_for_status()?;
    let mut bytes = Vec::new();
    response.take(LIMIT + 1).read_to_end(&mut bytes)?;
    verify(&bytes, &digest, size)?;
    let mut candidate = tempfile::NamedTempFile::new_in(parent)?;
    candidate.write_all(&bytes)?;
    candidate
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))?;
    candidate.as_file().sync_all()?;
    // Close the write descriptor before executing (Linux ETXTBSY).
    let staged = candidate.into_temp_path();
    if binary_version(&staged)? != release.tag_name.trim_start_matches('v') {
        bail!("Production binary version does not match the release");
    }
    let previous = parent.join("d-hub.previous");
    // Retain the previous executable without ever removing the current path.
    let backup = tempfile::NamedTempFile::new_in(parent)?;
    fs::copy(path, backup.path())?;
    backup.as_file().sync_all()?;
    backup.persist(&previous)?;
    staged.persist(path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
fn check(path: &Path, install_update: bool) -> Result<Status> {
    let mut status = Status {
        running_version: VERSION.into(),
        managed: managed(path),
        ..Status::default()
    };
    // A curl reinstall or newer frontend may have replaced the executable already.
    if status.managed {
        let disk = binary_version(path)?;
        if version(&disk)? > version(VERSION)? {
            status.available_version = Some(disk);
            status.restart_pending = true;
            status.message = "Installed update ready · waiting for drafts and active sends".into();
            return Ok(status);
        }
    }
    let release = latest()?;
    if version(&release.tag_name)? <= version(VERSION)? {
        status.message = format!("d-hub {VERSION} is up to date");
        return Ok(status);
    }
    status.available_version = Some(release.tag_name.trim_start_matches('v').into());
    if install_update && status.managed {
        install(path, &release)?;
        status.restart_pending = true;
        status.message = "Update installed · waiting for drafts and active sends".into();
    } else {
        status.message =
            "New production release available · install with the README curl command".into();
    }
    Ok(status)
}

pub struct Monitor {
    pub status: Status,
    path: PathBuf,
    job: Option<Receiver<Result<Status, String>>>,
    next: Instant,
    last: Option<Instant>,
}
impl Monitor {
    pub fn new(live: bool) -> Result<Self> {
        let path = executable()?;
        Ok(Self {
            status: Status {
                running_version: VERSION.into(),
                managed: live && managed(&path),
                message: "Production updates idle".into(),
                ..Status::default()
            },
            path,
            job: None,
            next: Instant::now(),
            last: None,
        })
    }
    pub fn request(&mut self, manual: bool, enabled: bool) {
        if self.job.is_some()
            || self.status.restart_pending
            || (!manual && (!enabled || !self.status.managed))
            || self
                .last
                .is_some_and(|t| t.elapsed() < Duration::from_secs(30))
        {
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let path = self.path.clone();
        self.status.checking = true;
        self.status.message = "Checking production releases…".into();
        self.last = Some(Instant::now());
        self.next = Instant::now() + INTERVAL;
        self.job = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(check(&path, true).map_err(|e| format!("Update check failed: {e:#}")));
        });
    }
    pub fn tick(&mut self, enabled: bool) {
        if let Some(job) = &self.job {
            match job.try_recv() {
                Ok(result) => {
                    self.job = None;
                    match result {
                        Ok(status) => self.status = status,
                        Err(message) => {
                            self.status.checking = false;
                            self.status.message = crate::domain::safe_text(&message);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.job = None;
                    self.status.checking = false;
                    self.status.message = "Update worker stopped; retry :update".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if Instant::now() >= self.next {
            self.request(false, enabled);
        }
    }
    pub fn restart_path(&self) -> Option<&Path> {
        self.status.restart_pending.then_some(self.path.as_path())
    }
}

/// Replace only after all state owners and terminal guards have been dropped by the caller.
pub fn reexec(path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let mut args = Vec::new();
    let mut skip = false;
    for arg in std::env::args_os().skip(1) {
        if skip {
            skip = false;
            continue;
        }
        if arg == "--recover" {
            skip = true;
            continue;
        }
        if arg.to_string_lossy().starts_with("--recover=") {
            continue;
        }
        args.push(arg);
    }
    Err(Command::new(path).args(args).exec().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_versions_and_asset_authority() {
        assert!(version("v1.10.0").unwrap() > version("1.9.9").unwrap());
        for bad in ["v1.2.3-beta", "1.2", "1.2.3/evil", "-1.2.3"] {
            assert!(version(bad).is_err());
        }
        let mut release = Release {
            tag_name: "v0.2.0".into(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "binary".into(),
                browser_download_url: format!("{RELEASES}v0.2.0/binary"),
                digest: Some(format!("sha256:{}", "a".repeat(64))),
                size: 100,
            }],
        };
        assert!(choose(&release, "binary").is_ok());
        release.assets[0].browser_download_url = "https://evil.invalid/binary".into();
        assert!(choose(&release, "binary").is_err());
    }
    #[test]
    fn corrupt_or_truncated_download_never_verifies() {
        let bytes = b"\x7fELFfake-binary";
        let digest = format!("{:x}", Sha256::digest(bytes));
        assert!(verify(bytes, &digest, bytes.len() as u64).is_ok());
        assert!(verify(bytes, &digest, 1).is_err());
        assert!(verify(b"corrupt", &digest, 7).is_err());
    }
    #[test]
    fn managed_install_requires_private_owner_marker() {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let binary = dir.path().join("d-hub");
        fs::write(&binary, "fake").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!managed(&binary));
        fs::write(dir.path().join(".d-hub-managed"), "d-hub-managed-v1\n").unwrap();
        fs::set_permissions(
            dir.path().join(".d-hub-managed"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert!(managed(&binary));
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(!managed(&binary));
    }
}
