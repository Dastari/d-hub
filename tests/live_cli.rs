use d_hub::ipc::{Client, Command, Role};
use std::{
    os::unix::fs::PermissionsExt,
    process::{Command as Process, Stdio},
    time::{Duration, Instant},
};
#[test]
fn empty_live_daemon_exposes_real_mode_and_account_profile_settings_over_ipc() {
    let dir = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let socket = dir.path().join("live.sock");
    let mut child = Process::new(env!("CARGO_BIN_EXE_d-hub"))
        .env("XDG_RUNTIME_DIR", dir.path())
        .args(["daemon", "--live", "--foreground", "--data-dir"])
        .arg(dir.path().join("profile"))
        .arg("--socket")
        .arg(&socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut client = loop {
        match Client::connect(&socket, Role::Control) {
            Ok(client) => break client,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    assert!(client.snapshot.live);
    assert!(client.snapshot.session.messages.is_empty());
    let run = |args: &[&str]| {
        let output = Process::new(env!("CARGO_BIN_EXE_d-hub"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    let path = socket.to_str().unwrap();
    run(&["account", "--socket", path, "add", "icloud", "personal"]);
    run(&["profile", "--socket", path, "add", "family"]);
    run(&[
        "config",
        "--socket",
        path,
        "set",
        "accounts.personal.poll_seconds",
        "45",
    ]);
    let snapshot = client.request_snapshot(Command::Poll { after: 0 }).unwrap();
    assert!(!snapshot.settings.accounts[0].enabled);
    assert!(!snapshot.settings.accounts[0].allow_send);
    assert_eq!(snapshot.settings.accounts[0].poll_seconds, 45);
    assert_eq!(snapshot.settings.profiles.last().unwrap().id, "family");
    run(&["service", "--socket", path, "refresh"]);
    let output = run(&["service", "--socket", path, "status", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["mode"], "live");
    assert_eq!(status["messages"], 0);
    assert_eq!(status["protocol"], 2);
    // A second live profile cannot acquire provider ownership, even on another socket.
    let output = Process::new(env!("CARGO_BIN_EXE_d-hub"))
        .env("XDG_RUNTIME_DIR", dir.path())
        .args(["daemon", "--live", "--foreground", "--data-dir"])
        .arg(dir.path().join("second"))
        .arg("--socket")
        .arg(dir.path().join("second.sock"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("another live service"));
    run(&["service", "--socket", path, "stop"]);
    assert!(child.wait().unwrap().success());
}
