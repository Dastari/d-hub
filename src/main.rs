use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use d_hub::{
    app::{App, Session},
    config::{self, Settings},
    domain::MessageId,
    storage::DemoStore,
    tui,
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    version,
    about = "A communications drawer · offline demo and opt-in notification capture",
    long_about = "d-hub: a communications drawer. Demo is entirely offline. Settings supports explicit account setup; notifications captures matching official Discord desktop notifications only. Personal Discord history and replies are not implemented."
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
    /// Edit all settings and explicitly set up credentials (no message connections)
    Settings,
    /// Opt-in, read-only capture of official Discord notifications; NOT a full personal client
    Notifications {
        #[arg(long, value_name = "DIR")]
        data_dir: Option<PathBuf>,
    },
    /// Manage account metadata and explicitly store credentials in the desktop keyring
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
    /// Manage configurable focus profiles
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Explicitly store an AI provider API key (never pass the key in an argument)
    AiAuth {
        #[arg(long)]
        secret_stdin: bool,
    },
    /// Report implemented capabilities and remaining desktop checks
    Doctor,
}

#[derive(Subcommand)]
enum ConfigAction {
    List,
    Get {
        key: String,
    },
    Set {
        key: String,
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        value: Option<String>,
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
    },
    Reset {
        key: String,
    },
    Validate,
    Export,
}

#[derive(Subcommand)]
enum AccountAction {
    List,
    Add {
        provider: String,
        id: String,
    },
    Remove {
        id: String,
    },
    Auth {
        id: String,
        #[arg(long)]
        secret_stdin: bool,
    },
    Forget {
        id: String,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    List,
    Add { id: String },
    Remove { id: String },
}

fn store_secret(reference: &str, stdin: bool) -> Result<()> {
    use d_hub::auth::CredentialStore;
    if !d_hub::auth::SecretService::available() {
        bail!("install libsecret (secret-tool) and unlock the desktop keyring before setup");
    }
    let secret = d_hub::auth::read_secret(stdin, "Credential (stored in desktop keyring): ")?;
    d_hub::auth::SecretService.store(reference, &secret)
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
                ConfigAction::Set { key, value, file } => {
                    let value = match (value, file) {
                        (Some(value), None) => value,
                        (None, Some(path)) => {
                            let value = std::fs::read_to_string(path)?;
                            if value.len() > 65_536 {
                                bail!("setting input exceeds 65536 bytes");
                            }
                            value
                        }
                        _ => bail!("supply a value or --file PATH"),
                    };
                    settings.set(&key, &value)?;
                    settings.save(&path)?;
                    output["key"] = key.into();
                    output["value"] = value.into();
                }
                ConfigAction::Reset { key } => {
                    let value = Settings::default().value(&key)?;
                    settings.set(&key, &value)?;
                    settings.save(&path)?;
                    output["key"] = key.into();
                    output["value"] = value.into();
                }
                ConfigAction::Get { key } => {
                    output["value"] = settings.value(&key)?.into();
                    output["key"] = key.into();
                }
                ConfigAction::List => {
                    let persisted = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|s| s.parse::<toml::Table>().ok())
                        .unwrap_or_default();
                    output["settings"] = settings.entries().iter().map(|s| serde_json::json!({
                        "key": s.name, "value": s.value, "choices": s.choices, "help": s.help,
                        "origin": if persisted.contains_key(s.name.split('.').next().unwrap_or("")) { "config" } else { "default" },
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
        Command::Settings => {
            if cli.json {
                bail!("settings is interactive; use config list --json");
            }
            let path = cli.config.map(Ok).unwrap_or_else(config::default_path)?;
            let mut app = App::new(
                Session {
                    messages: vec![],
                    ..Session::default()
                },
                Settings::load(&path)?,
            )?;
            app.mode = d_hub::app::Mode::Settings;
            app.live_setup = true;
            app.notice = "Account metadata and prompts are editable here. :account add PROVIDER ID · :auth ID or :auth ai. Live message transports are pending.".into();
            tui::run(app, None, Some(&path), false)?;
        }
        Command::Notifications { data_dir } => {
            if cli.json {
                bail!("notification capture is interactive");
            }
            let path = cli.config.map(Ok).unwrap_or_else(config::default_path)?;
            let settings = Settings::load(&path)?;
            // One live capture owner even when two windows choose different profiles.
            let runtime = std::env::var_os("XDG_RUNTIME_DIR")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .map(|p| p.join(".local/state"))
                })
                .context("XDG_RUNTIME_DIR or HOME is required for the capture ownership lock")?;
            let _capture_owner = DemoStore::open(runtime.join("d-hub/notification-owner"))
                .context("could not acquire the single notification capture owner")?;
            let directory = data_dir
                .or_else(|| {
                    std::env::var_os("XDG_STATE_HOME")
                        .map(PathBuf::from)
                        .map(|p| p.join("d-hub/notifications"))
                })
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .map(|p| p.join(".local/state/d-hub/notifications"))
                })
                .context("set --data-dir or XDG_STATE_HOME")?;
            let store = DemoStore::open(directory)?;
            let session = store.load::<Session>()?.unwrap_or_else(|| Session {
                messages: vec![],
                ..Session::default()
            });
            if session.messages.iter().any(|m| m.target.is_some())
                || session.composer.draft.is_some()
                || !session.composer.sent.is_empty()
            {
                bail!(
                    "notification capture requires its own directory, separate from demo or reply data"
                );
            }
            let mut app = App::new(session, settings)?;
            app.notifications = Some(d_hub::discord_notifications::start(
                app.settings.accounts.clone(),
            )?);
            app.live_setup = true;
            app.live_feed = true;
            app.mode = d_hub::app::Mode::Navigate;
            app.notice = "Starting opt-in Discord notification capture · read-only, no history, no remote read sync, no replies. v requests the official Discord app.".into();
            tui::run(app, Some(&store), Some(&path), false)?;
        }
        Command::Account { action } => {
            use d_hub::auth::CredentialStore;
            let path = cli.config.map(Ok).unwrap_or_else(config::default_path)?;
            let mut settings = Settings::load(&path)?;
            match action {
                AccountAction::List => {
                    let rows: Vec<_> = settings.accounts.iter().map(|a| serde_json::json!({"id":a.id,"provider":a.provider,"authentication":a.auth_description(),"allow_ai":a.allow_ai,"message_transport_connected":false})).collect();
                    println!("{}", serde_json::to_string_pretty(&rows)?);
                }
                AccountAction::Add { provider, id } => {
                    settings.add_account(&id, tui::parse_provider(&provider)?)?;
                    settings.save(&path)?;
                }
                AccountAction::Remove { id } => {
                    if !settings.accounts.iter().any(|a| a.id == id) {
                        bail!("unknown account");
                    }
                    settings.accounts.retain(|a| a.id != id);
                    settings.save(&path)?;
                    eprintln!(
                        "Account metadata removed; use account forget before removal to erase stored credentials."
                    );
                }
                AccountAction::Auth { id, secret_stdin } => {
                    let account = settings
                        .accounts
                        .iter()
                        .find(|a| a.id == id)
                        .context("unknown account")?;
                    if !account.authentication_supported() {
                        bail!("{}", account.auth_description());
                    }
                    if !d_hub::auth::SecretService::available() {
                        bail!(
                            "install libsecret (secret-tool) and unlock the desktop keyring before setup"
                        );
                    }
                    if matches!(
                        account.provider,
                        d_hub::domain::Provider::Office365 | d_hub::domain::Provider::Teams
                    ) {
                        if secret_stdin {
                            bail!("Microsoft uses delegated device login, not a password on stdin");
                        }
                        d_hub::auth::authenticate_graph(
                            account,
                            &d_hub::auth::SecretService,
                            |notice| eprintln!("{notice}"),
                        )?;
                    } else {
                        store_secret(&account.credential_ref(), secret_stdin)?;
                        eprintln!("Credential stored. Message transport is not connected yet.");
                    }
                }
                AccountAction::Forget { id } => {
                    if !d_hub::policy::identifier(&id) {
                        bail!("invalid account ID");
                    }
                    d_hub::auth::SecretService.remove(&format!("account-{id}"))?;
                }
            }
        }
        Command::Profile { action } => {
            let path = cli.config.map(Ok).unwrap_or_else(config::default_path)?;
            let mut settings = Settings::load(&path)?;
            match action {
                ProfileAction::List => {
                    println!("{}", serde_json::to_string_pretty(&settings.profiles)?)
                }
                ProfileAction::Add { id } => {
                    settings.add_profile(&id)?;
                    settings.save(&path)?;
                }
                ProfileAction::Remove { id } => {
                    settings.remove_profile(&id)?;
                    settings.save(&path)?;
                }
            }
        }
        Command::AiAuth { secret_stdin } => {
            let path = cli.config.map(Ok).unwrap_or_else(config::default_path)?;
            let settings = Settings::load(&path)?;
            store_secret(settings.ai.provider.credential_ref(), secret_stdin)?;
            eprintln!(
                "AI credential stored. External processing still requires ai.enabled and account allow_ai."
            );
        }
        Command::Doctor => {
            let value = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"), "mode": "prototype", "live_providers": false,
                "discord_personal_history": false, "discord_personal_replies": false,
                "discord_notification_capture": "opt-in, desktop unverified",
                "account_setup": true, "typed_decision_adapters": true, "remote_read_sync": false,
                "network_access": true, "demo_network_access": false, "daemon_ipc": false, "omarchy_plugin": false,
                "image_graphics": false, "fixture_reader": true,
                "ghostty_desktop_verified": false,
                "pending": ["Personal Discord DM history and replies", "Mail/Teams message transports and remote unread sync", "Live account/AI authentication tests", "Ghostty focus and modified keys", "Wayland clipboard", "terminal graphics", "Omarchy DND and plugin lifecycle"],
            });
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!(
                    "d-hub {} · prototype\n\nImplemented: mixed feed, locked drafts, local read/dismissal, editable profiles, spam prompts, account setup, typed AI adapters and opt-in Discord notification capture.\nPending: personal Discord history/replies, mail/Teams transports, remote read sync, daemon IPC, image graphics, rich clipboard, Omarchy companion plugin.\nActual Ghostty/Omarchy desktop checks have not been run.",
                    env!("CARGO_PKG_VERSION")
                );
            }
        }
    }
    Ok(())
}
