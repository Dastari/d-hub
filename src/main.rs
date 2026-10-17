use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use d_hub::{
    app::{App, Session},
    config::{self, REGISTRY, Settings},
    domain::MessageId,
    storage::DemoStore,
    tui,
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    version,
    about = "A clean communications drawer · offline prototype",
    long_about = "d-hub: an offline communications drawer prototype. All messages are fictional and every send is a local simulation. No accounts or network access are used."
)]
struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Settings file; demo otherwise uses its isolated data directory"
    )]
    config: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Structured output for config and doctor (never starts a TUI)"
    )]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the offline demo; creates a temporary profile unless --data-dir is supplied
    Demo {
        #[arg(
            long,
            value_name = "DIR",
            help = "Isolated demo profile to reopen; never use a live account directory"
        )]
        data_dir: Option<PathBuf>,
        #[arg(
            long,
            help = "Render one plain-text frame without an interactive terminal"
        )]
        snapshot: bool,
        #[arg(long, default_value_t = 66, value_parser = clap::value_parser!(u16).range(1..=300))]
        width: u16,
        #[arg(long, default_value_t = 36, value_parser = clap::value_parser!(u16).range(1..=150))]
        height: u16,
    },
    /// Read a fictional message in this terminal; no daemon or accounts yet
    Preview {
        #[arg(
            long,
            required = true,
            help = "Explicitly select the fixture-only reader"
        )]
        demo: bool,
        message_id: u64,
    },
    /// Read/edit the implemented shared settings schema
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Report implemented capabilities and remaining desktop checks
    Doctor,
}

#[derive(Subcommand)]
enum ConfigAction {
    List,
    Get { key: String },
    Set { key: String, value: String },
    Reset { key: String },
    Validate,
    Export,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("d-hub: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let command = cli.command.unwrap_or(Command::Demo {
        data_dir: None,
        snapshot: false,
        width: 66,
        height: 36,
    });
    match command {
        Command::Demo {
            data_dir,
            snapshot,
            width,
            height,
        } => {
            if cli.json {
                bail!(
                    "--json is available for config and doctor; use demo --snapshot for a text frame"
                );
            }
            let temporary = if data_dir.is_none() {
                Some(tempfile::Builder::new().prefix("d-hub-demo-").tempdir()?)
            } else {
                None
            };
            let directory = data_dir
                .or_else(|| temporary.as_ref().map(|d| d.path().to_owned()))
                .context("missing demo directory")?;
            let store = DemoStore::open(directory.clone())?;
            let config_path = cli.config.unwrap_or_else(|| directory.join("config.toml"));
            let settings = Settings::load(&config_path)?;
            let mut app = App::new(store.load::<Session>()?.unwrap_or_default(), settings)?;
            if snapshot {
                print!("{}", tui::snapshot(&mut app, width, height)?);
            } else {
                eprintln!(
                    "Offline demo profile: {}{}",
                    directory.display(),
                    if temporary.is_some() {
                        " (temporary; removed on exit)"
                    } else {
                        " (persistent)"
                    }
                );
                tui::run(app, Some(&store), Some(&config_path), false)?;
            }
        }
        Command::Preview {
            demo: _,
            message_id,
        } => {
            if cli.json {
                bail!("the reader is interactive; --json cannot start a TUI");
            }
            let settings = cli
                .config
                .as_deref()
                .map(Settings::load)
                .transpose()?
                .unwrap_or_default();
            tui::reader(MessageId(message_id), settings)?;
        }
        Command::Config { action } => {
            let path = cli.config.map(Ok).unwrap_or_else(config::default_path)?;
            let mut settings = Settings::load(&path)?;
            let mut output = serde_json::json!({ "config": path, "schema_version": 1 });
            match action {
                ConfigAction::Set { key, value } => {
                    settings.set(&key, &value)?;
                    settings.save(&path)?;
                    output["key"] = key.into();
                    output["value"] = value.into();
                }
                ConfigAction::Reset { key } => {
                    let setting = config::setting(&key)?;
                    let value = Settings::default().get(setting.key);
                    settings.set(&key, &value)?;
                    settings.save(&path)?;
                    output["key"] = key.into();
                    output["value"] = value.into();
                }
                ConfigAction::Get { key } => {
                    let setting = config::setting(&key)?;
                    output["value"] = settings.get(setting.key).into();
                    output["key"] = key.into();
                }
                ConfigAction::List => {
                    let persisted = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|s| s.parse::<toml::Table>().ok())
                        .unwrap_or_default();
                    output["settings"] = REGISTRY.iter().map(|s| serde_json::json!({
                        "key": s.name, "value": settings.get(s.key), "choices": s.choices, "help": s.help,
                        "origin": if persisted.contains_key(s.name) { "config" } else { "default" },
                    })).collect::<Vec<_>>().into();
                }
                ConfigAction::Validate => output["valid"] = true.into(),
                ConfigAction::Export => {
                    if cli.json {
                        println!("{}", serde_json::to_string_pretty(&settings)?);
                    } else {
                        print!("{}", toml::to_string_pretty(&settings)?);
                    }
                    return Ok(());
                }
            }
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else if let Some(values) = output["settings"].as_array() {
                for value in values {
                    println!(
                        "{} = {}  ({})",
                        value["key"].as_str().unwrap_or(""),
                        value["value"].as_str().unwrap_or(""),
                        value["origin"].as_str().unwrap_or("")
                    );
                }
            } else if let Some(value) = output["value"].as_str() {
                println!("{value}");
            } else {
                println!("Configuration is valid.");
            }
        }
        Command::Doctor => {
            let value = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"), "mode": "offline-prototype", "live_providers": false,
                "network_access": false, "daemon_ipc": false, "omarchy_plugin": false,
                "image_graphics": false, "fixture_reader": true,
                "ghostty_desktop_verified": false,
                "pending": ["Ghostty focus and modified keys", "Wayland clipboard", "terminal graphics", "Omarchy DND and plugin lifecycle"],
            });
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!(
                    "d-hub {} · offline prototype\n\nImplemented: mixed feed, locked drafts, mock sends, fixture reader, shared settings.\nPending: daemon IPC, live providers, image graphics, rich clipboard, Omarchy companion plugin.\nActual Ghostty/Omarchy desktop checks have not been run.",
                    env!("CARGO_PKG_VERSION")
                );
            }
        }
    }
    Ok(())
}
