//! Launch the per-user live owner on demand; no shell, desktop config or service install.
use crate::{
    config::Settings,
    ipc::{Client, Role},
    storage::DemoStore,
};
use anyhow::{Context, Result, bail};
use std::os::unix::process::CommandExt;
use std::{
    io::{Read, Seek},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub fn data_directory() -> Result<PathBuf> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(state).join("d-hub/live"));
    }
    Ok(
        PathBuf::from(std::env::var_os("HOME").context("HOME or XDG_STATE_HOME is required")?)
            .join(".local/state/d-hub/live"),
    )
}
fn unavailable(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|e| e.downcast_ref::<std::io::Error>())
        .any(|e| {
            matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            )
        })
}
fn reap(mut child: Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}
/// Explicit paths also permit isolated process tests without reading owner configuration.
pub fn connect_or_start(socket: &Path, data: &Path, config: &Path) -> Result<Client> {
    match Client::connect(socket, Role::Drawer) {
        Ok(client) => return Ok(client),
        Err(e) if unavailable(&e) => {}
        Err(e) => return Err(e),
    }
    let parent = socket.parent().context("Service socket has no parent")?;
    let until = Instant::now() + Duration::from_secs(10);
    let lock = loop {
        match DemoStore::open(parent.join("startup-owner")) {
            Ok(lock) => break lock,
            Err(error) => {
                match Client::connect(socket, Role::Drawer) {
                    Ok(client) => return Ok(client),
                    Err(e) if unavailable(&e) => {}
                    Err(e) => return Err(e),
                }
                if Instant::now() >= until {
                    return Err(error.context("Timed out waiting for another service startup"));
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        }
    };
    // Another launcher may have completed before this lock became available.
    match Client::connect(socket, Role::Drawer) {
        Ok(client) => return Ok(client),
        Err(e) if unavailable(&e) => {}
        Err(e) => return Err(e),
    }
    Settings::load(config)?.validate()?;
    let mut diagnostic = tempfile::NamedTempFile::new_in(parent)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--config")
        .arg(config)
        .args(["daemon", "--live", "--foreground", "--data-dir"])
        .arg(data)
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(diagnostic.as_file().try_clone()?))
        .process_group(0);
    let mut child = command
        .spawn()
        .context("Could not launch the d-hub background service")?;
    loop {
        if let Some(status) = child.try_wait()? {
            diagnostic.as_file_mut().rewind()?;
            let mut detail = String::new();
            diagnostic
                .as_file_mut()
                .take(4096)
                .read_to_string(&mut detail)?;
            bail!(
                "Background service exited ({status}): {}",
                crate::domain::safe_text(detail.trim())
            );
        }
        match Client::connect(socket, Role::Drawer) {
            Ok(client) => {
                reap(child);
                drop(lock);
                return Ok(client);
            }
            Err(e) if unavailable(&e) => {}
            Err(error) => {
                reap(child);
                return Err(error);
            }
        }
        if Instant::now() >= until {
            // Do not leave an unresponsive child whose ownership was never established.
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "Background service did not become ready within 10 seconds; run daemon --live --foreground to inspect startup"
            );
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}
