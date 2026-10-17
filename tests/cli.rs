use d_hub::config::{REGISTRY, Settings};
use std::{
    path::Path,
    process::{Command, Output},
};

fn run(path: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_d-hub"))
        .arg("--config")
        .arg(path)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn shared_registry_round_trips_and_rejects_invalid_edits_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for setting in REGISTRY {
        let value = setting.choices.last().unwrap();
        let output = run(&path, &["config", "set", setting.name, value, "--json"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(Settings::load(&path).unwrap().get(setting.key), *value);
        let output = run(&path, &["config", "get", setting.name, "--json"]);
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["value"], *value);
    }
    let original = std::fs::read(&path).unwrap();
    assert!(
        !run(&path, &["config", "set", "theme", "invalid"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(
        !run(&path, &["config", "set", "mystery", "true"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn unknown_schema_and_unknown_fields_fail_without_rewriting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for content in ["schema_version = 9\n", "mystery = true\n"] {
        std::fs::write(&path, content).unwrap();
        let output = run(&path, &["config", "validate", "--json"]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }
}

#[test]
fn snapshot_needs_neither_credentials_nor_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let output = run(&path, &["demo", "--snapshot"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let frame = String::from_utf8(output.stdout).unwrap();
    assert!(frame.contains("OFFLINE DEMO"));
    assert!(frame.contains("Office 365"));
    assert!(frame.contains("Teams"));
    assert!(frame.contains("LOCKED") || frame.contains("Reply"));
    assert!(!path.exists());
}

#[test]
fn json_never_opens_an_interactive_surface_and_doctor_reports_limits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let output = run(&path, &["demo", "--json"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let output = run(&path, &["doctor", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["network_access"], false);
    assert_eq!(value["omarchy_plugin"], false);
    assert_eq!(value["ghostty_desktop_verified"], false);
}

#[test]
fn export_reset_and_effective_origin_are_available_without_tui() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let output = run(&path, &["config", "list", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["settings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["origin"] == "default")
    );
    assert!(
        run(&path, &["config", "set", "theme", "dusk"])
            .status
            .success()
    );
    assert!(run(&path, &["config", "reset", "theme"]).status.success());
    assert_eq!(Settings::load(&path).unwrap(), Settings::default());
    let output = run(&path, &["config", "export"]);
    let exported: Settings = toml::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap();
    assert_eq!(exported, Settings::default());
}
