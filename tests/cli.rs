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
    assert_eq!(value["network_access"], true);
    assert_eq!(value["demo_network_access"], false);
    assert_eq!(value["discord_personal_history"], false);
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

#[test]
fn padding_defaults_are_compatible_and_invalid_values_never_replace_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "schema_version = 1\ntheme = 'dusk'\n").unwrap();
    let old = Settings::load(&path).unwrap();
    assert_eq!((old.padding_horizontal, old.padding_vertical), (0, 0));
    for (key, value) in [("padding_horizontal", "3"), ("padding_vertical", "2")] {
        assert!(run(&path, &["config", "set", key, value]).status.success());
    }
    let saved = Settings::load(&path).unwrap();
    assert_eq!((saved.padding_horizontal, saved.padding_vertical), (3, 2));
    let original = std::fs::read(&path).unwrap();
    for key in ["padding_horizontal", "padding_vertical"] {
        for value in ["-1", "17", "65536", "1.5", "wide"] {
            assert!(!run(&path, &["config", "set", key, value]).status.success());
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
        assert!(run(&path, &["config", "reset", key]).status.success());
        // Keep the next invalid-edit checks relative to the restored configuration.
        std::fs::write(&path, &original).unwrap();
    }
    for content in [
        "padding_horizontal = 17\n",
        "padding_vertical = -1\n",
        "padding_vertical = '2'\n",
    ] {
        std::fs::write(&path, content).unwrap();
        assert!(!run(&path, &["config", "validate"]).status.success());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }
}

#[test]
fn profile_account_and_prompt_file_commands_use_the_shared_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for args in [
        vec!["profile", "add", "family"],
        vec!["account", "add", "discord", "home-discord"],
        vec!["config", "set", "focus", "family"],
        vec![
            "config",
            "set",
            "accounts.home-discord.notification_sender",
            "Fictional Son",
        ],
    ] {
        let output = run(&path, &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let prompt = dir.path().join("prompt.txt");
    let text = "Is this about family?\nInclude personal conversations.";
    std::fs::write(&prompt, text).unwrap();
    assert!(
        run(
            &path,
            &[
                "config",
                "set",
                "profiles.family.prompt",
                "--file",
                prompt.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    let settings = Settings::load(&path).unwrap();
    assert_eq!(settings.value("profiles.family.prompt").unwrap(), text);
    assert_eq!(settings.accounts[0].discord_mode, "personal");
    let output = run(&path, &["account", "auth", "home-discord"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal"));
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !run(
            &path,
            &[
                "config",
                "set",
                "accounts.home-discord.token",
                "fictional-not-a-token"
            ]
        )
        .status
        .success()
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(
        run(&path, &["profile", "remove", "family"])
            .status
            .success()
    );
    assert_eq!(Settings::load(&path).unwrap().focus, "all");
}

#[test]
fn a_second_notification_window_cannot_start_another_capture_owner() {
    use d_hub::storage::DemoStore;
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    let _owner = DemoStore::open(runtime.join("d-hub/notification-owner")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_d-hub"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .arg("--config")
        .arg(dir.path().join("config.toml"))
        .args(["notifications", "--data-dir"])
        .arg(dir.path().join("different-profile"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("single notification capture owner"));
    assert!(!dir.path().join("different-profile").exists());
}

#[test]
fn several_accounts_of_one_provider_get_free_ids_and_rename_without_identity_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for args in [
        vec!["account", "add", "discord", "--name", "Son"],
        vec!["account", "add", "discord"],
        vec!["account", "rename", "discord-2", "Gaming friends"],
        vec!["config", "set", "accounts.discord.name", "Family"],
    ] {
        let output = run(&path, &args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let settings = Settings::load(&path).unwrap();
    let accounts: Vec<_> = settings
        .accounts
        .iter()
        .map(|a| (a.id.as_str(), a.label(), a.enabled, a.allow_send))
        .collect();
    assert_eq!(
        accounts,
        [
            ("discord", "Family", false, false),
            ("discord-2", "Gaming friends", false, false)
        ]
    );
    let original = std::fs::read(&path).unwrap();
    assert!(
        !run(&path, &["account", "add", "discord", "discord"])
            .status
            .success()
    );
    assert!(
        !run(&path, &["account", "rename", "missing", "Name"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let listed = run(&path, &["account", "list"]);
    let json: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(json[1]["name"], "Gaming friends");
}
