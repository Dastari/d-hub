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
    about = "A communications drawer · live iCloud, Microsoft mail and Teams",
    long_about = "d-hub: one communications feed with persistent replies. Demo is entirely offline; daemon --live connects explicitly enabled accounts. Personal Discord replies remain unavailable."
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
        help = "Structured output where supported; never starts an interactive TUI"
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
    /// Read a fixture or a stored message through the shared service
    Preview {
        #[arg(
            long,
            required_unless_present = "socket",
            conflicts_with = "socket",
            help = "Fixture-only reader"
        )]
        demo: bool,
        #[arg(
            long,
            value_name = "PATH",
            help = "Connect to a running shared service"
        )]
        socket: Option<PathBuf>,
        message_id: u64,
    },
    /// Read/edit the implemented shared settings schema
    Config {
        #[arg(
            long,
            help = "Read/edit settings owned by the running service instead of a TOML file"
        )]
        socket: Option<PathBuf>,
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
        #[arg(long)]
        socket: Option<PathBuf>,
        #[command(subcommand)]
        action: AccountAction,
    },
    /// Manage configurable focus profiles
    Profile {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Explicitly store an AI provider API key (never pass the key in an argument)
    AiAuth {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        secret_stdin: bool,
    },
    /// Run the persistent service; choose isolated offline demo or opt-in live accounts
    Daemon {
        #[arg(long, required_unless_present = "live", conflicts_with = "live")]
        demo: bool,
        #[arg(long, required_unless_present = "demo", conflicts_with = "demo")]
        live: bool,
        #[arg(long, required = true)]
        foreground: bool,
        #[arg(long, value_name = "DIR")]
        data_dir: PathBuf,
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },
    /// Open the sole draft editor attached to the shared service (defaults to live)
    Drawer {
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Restore unsent drafts from a disconnected drawer; never replay sends"
        )]
        recover: Option<PathBuf>,
    },
    /// Inspect, refresh, reconcile or stop the shared service (defaults to live)
    Service {
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
        #[command(subcommand)]
        action: ServiceAction,
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
enum ServiceAction {
    /// Resolve an uncertain send only after verifying it in the original client
    Resolve {
        operation: u64,
        #[arg(value_parser=["accepted","not-sent"])]
        outcome: String,
    },
    /// Request another sync without restarting the daemon
    Refresh,
    Status,
    Snapshot,
    Stop,
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

fn settings_location(
    config: Option<PathBuf>,
    socket: Option<PathBuf>,
) -> Result<(Settings, PathBuf, Option<d_hub::ipc::Client>)> {
    let path = config.map(Ok).unwrap_or_else(config::default_path)?;
    let client = socket
        .as_deref()
        .map(|p| d_hub::ipc::Client::connect(p, d_hub::ipc::Role::Control))
        .transpose()?;
    let settings = if let Some(client) = &client {
        client.snapshot.settings.clone()
    } else {
        Settings::load(&path)?
    };
    Ok((settings, path, client))
}
fn persist_settings(
    settings: &Settings,
    path: &std::path::Path,
    client: &mut Option<d_hub::ipc::Client>,
) -> Result<()> {
    if let Some(client) = client {
        client.request_snapshot(d_hub::ipc::Command::Configure {
            expected_revision: client.snapshot.revision,
            settings: settings.clone(),
        })?;
        Ok(())
    } else {
        settings.save(path)
    }
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
    let command = cli.command.unwrap_or(Command::Drawer {
        socket: None,
        recover: None,
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
            socket,
            message_id,
        } => {
            if let Some(socket) = socket {
                let mut client = d_hub::ipc::Client::connect(&socket, d_hub::ipc::Role::Reader)?;
                let snapshot = client.request_snapshot(d_hub::ipc::Command::Read {
                    id: MessageId(message_id),
                })?;
                let message = snapshot
                    .session
                    .messages
                    .iter()
                    .find(|m| m.id == MessageId(message_id))
                    .context("unknown message")?
                    .clone();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&message)?);
                } else {
                    let mut app = App::connected(snapshot.session, snapshot.settings)?;
                    app.reader = Some(message);
                    app.mode = d_hub::app::Mode::Reader;
                    app.live_service = snapshot.live;
                    app.provider_status = snapshot.provider_status;
                    app.service_socket = Some(socket.clone());
                    let link = d_hub::ipc::Link::new(client, socket, Some(MessageId(message_id)));
                    tui::run_connected(app, link, true)?;
                }
            } else {
                if cli.json {
                    bail!(
                        "fixture reader is interactive; use preview --socket PATH ID --json for a shared message"
                    );
                }
                let settings = cli
                    .config
                    .as_deref()
                    .map(Settings::load)
                    .transpose()?
                    .unwrap_or_default();
                tui::reader(MessageId(message_id), settings)?;
            }
        }
        Command::Config { action, socket } => {
            let mut service = socket
                .as_deref()
                .map(|p| d_hub::ipc::Client::connect(p, d_hub::ipc::Role::Control))
                .transpose()?;
            let path = cli.config.map(Ok).unwrap_or_else(config::default_path)?;
            let mut settings = service
                .as_ref()
                .map(|c| c.snapshot.settings.clone())
                .map(Ok)
                .unwrap_or_else(|| Settings::load(&path))?;
            let mut output = serde_json::json!({ "config": if service.is_some() { serde_json::Value::Null } else { serde_json::json!(path) }, "socket":socket, "schema_version": 1 });
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
                    if let Some(client) = &mut service {
                        client.request_snapshot(d_hub::ipc::Command::Configure {
                            expected_revision: client.snapshot.revision,
                            settings: settings.clone(),
                        })?;
                    } else {
                        settings.save(&path)?;
                    }
                    output["key"] = key.into();
                    output["value"] = value.into();
                }
                ConfigAction::Reset { key } => {
                    let value = Settings::default().value(&key)?;
                    settings.set(&key, &value)?;
                    if let Some(client) = &mut service {
                        client.request_snapshot(d_hub::ipc::Command::Configure {
                            expected_revision: client.snapshot.revision,
                            settings: settings.clone(),
                        })?;
                    } else {
                        settings.save(&path)?;
                    }
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
                        "origin": if service.is_some() { "service" } else if persisted.contains_key(s.name.split('.').next().unwrap_or("")) { "config" } else { "default" },
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
            app.notice = "Account metadata and prompts are editable here. :account add PROVIDER ID · :auth ID or :auth ai. Enable an account in a separate live service profile to connect.".into();
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
        Command::Account { action, socket } => {
            use d_hub::auth::CredentialStore;
            let (mut settings, path, mut service) = settings_location(cli.config, socket)?;
            match action {
                AccountAction::List => {
                    let rows: Vec<_> = settings.accounts.iter().map(|a| serde_json::json!({"id":a.id,"provider":a.provider,"authentication":a.auth_description(),"allow_ai":a.allow_ai,"enabled":a.enabled,"allow_send":a.allow_send})).collect();
                    println!("{}", serde_json::to_string_pretty(&rows)?);
                }
                AccountAction::Add { provider, id } => {
                    settings.add_account(&id, tui::parse_provider(&provider)?)?;
                    persist_settings(&settings, &path, &mut service)?;
                }
                AccountAction::Remove { id } => {
                    if !settings.accounts.iter().any(|a| a.id == id) {
                        bail!("unknown account");
                    }
                    settings.accounts.retain(|a| a.id != id);
                    persist_settings(&settings, &path, &mut service)?;
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
                        eprintln!(
                            "Credential stored. Enable this account in your live service settings to connect."
                        );
                    }
                }
                AccountAction::Forget { id } => {
                    if !d_hub::policy::identifier(&id) {
                        bail!("invalid account ID");
                    }
                    if let Some(account) = settings.accounts.iter_mut().find(|a| a.id == id) {
                        account.enabled = false;
                        persist_settings(&settings, &path, &mut service)?;
                    }
                    d_hub::auth::SecretService.remove(&format!("account-{id}"))?;
                }
            }
        }
        Command::Profile { action, socket } => {
            let (mut settings, path, mut service) = settings_location(cli.config, socket)?;
            match action {
                ProfileAction::List => {
                    println!("{}", serde_json::to_string_pretty(&settings.profiles)?)
                }
                ProfileAction::Add { id } => {
                    settings.add_profile(&id)?;
                    persist_settings(&settings, &path, &mut service)?;
                }
                ProfileAction::Remove { id } => {
                    settings.remove_profile(&id)?;
                    persist_settings(&settings, &path, &mut service)?;
                }
            }
        }
        Command::AiAuth {
            secret_stdin,
            socket,
        } => {
            let (settings, _, _) = settings_location(cli.config, socket)?;
            store_secret(settings.ai.provider.credential_ref(), secret_stdin)?;
            eprintln!(
                "AI credential stored. External processing still requires ai.enabled and account allow_ai."
            );
        }
        Command::Daemon {
            demo: _,
            live,
            foreground: _,
            data_dir,
            socket,
        } => {
            if cli.json {
                bail!("daemon is a foreground process; use service status --json");
            }
            let path = cli.config.unwrap_or_else(|| data_dir.join("config.toml"));
            let _live_owner = if live {
                let runtime = std::env::var_os("XDG_RUNTIME_DIR")
                    .map(PathBuf::from)
                    .context("XDG_RUNTIME_DIR is required for the live owner lock")?;
                Some(
                    DemoStore::open(runtime.join("d-hub/live-owner"))
                        .context("another live service already owns provider sessions")?,
                )
            } else {
                None
            };
            let database =
                d_hub::database::Database::open_mode(&data_dir, &Settings::load(&path)?, live)?;
            d_hub::ipc::serve(
                database,
                &socket.map(Ok).unwrap_or_else(|| {
                    if live {
                        d_hub::ipc::default_live_socket()
                    } else {
                        d_hub::ipc::default_socket()
                    }
                })?,
            )?;
        }
        Command::Drawer { socket, recover } => {
            if cli.json {
                bail!("drawer is interactive; use service snapshot --json");
            }
            let socket = socket
                .map(Ok)
                .unwrap_or_else(d_hub::ipc::default_live_socket)?;
            let mut client = d_hub::ipc::Client::connect(&socket, d_hub::ipc::Role::Drawer)?;
            if let Some(path) = recover {
                d_hub::ipc::restore(&mut client, &path)?;
            }
            let snapshot = client.snapshot.clone();
            let mut app = App::connected(snapshot.session, snapshot.settings)?;
            app.selected = snapshot.selected;
            app.focused = snapshot.focused;
            app.shared = true;
            app.service_socket = Some(socket.clone());
            app.live_service = snapshot.live;
            app.provider_status = snapshot.provider_status;
            app.notice = if app.live_service {
                "LIVE · replies go to the displayed recipient when account allow_send is enabled."
            } else {
                "Connected to offline service · simulated sends."
            }
            .into();
            let link = d_hub::ipc::Link::new(client, socket, None);
            app.service_socket = Some(link.socket.clone());
            tui::run_connected(app, link, false)?;
        }
        Command::Service { socket, action } => {
            let socket = socket
                .map(Ok)
                .unwrap_or_else(d_hub::ipc::default_live_socket)?;
            let mut client = d_hub::ipc::Client::connect(&socket, d_hub::ipc::Role::Control)?;
            match action {
                ServiceAction::Resolve { operation, outcome } => {
                    client.request_snapshot(d_hub::ipc::Command::Resolve {
                        expected_revision: client.snapshot.revision,
                        operation,
                        accepted: outcome == "accepted",
                    })?;
                    println!(
                        "Owner-reported send outcome recorded; no message was sent by this command."
                    );
                }
                ServiceAction::Refresh => {
                    client.request_snapshot(d_hub::ipc::Command::Refresh)?;
                    println!("Sync requested.");
                }
                ServiceAction::Snapshot => {
                    println!("{}", serde_json::to_string_pretty(&client.snapshot)?)
                }
                ServiceAction::Status => println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"protocol":d_hub::ipc::VERSION,"mode":if client.snapshot.live {"live"}else{"offline-demo"},"provider_status":client.snapshot.provider_status,"revision":client.snapshot.revision,"editor_connected":client.snapshot.editor_connected,"messages":client.snapshot.session.messages.len(),"live_providers":client.snapshot.live})
                    )?
                ),
                ServiceAction::Stop => match client.request(d_hub::ipc::Command::Stop)? {
                    d_hub::ipc::Reply::Stopped => println!("Service stopped."),
                    _ => bail!("service did not acknowledge shutdown"),
                },
            }
        }
        Command::Doctor => {
            let value = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"), "mode": "live-preview", "live_providers": true,
                "discord_personal_history": false, "discord_personal_replies": false,
                "discord_notification_capture": "opt-in, desktop unverified",
                "account_setup": true, "secret_service_helper_available":d_hub::auth::SecretService::available(), "typed_decision_adapters": true, "remote_read_sync": ["icloud","office365"],
                "network_access": true, "demo_network_access": false, "daemon_ipc": true, "daemon_mode": ["live","offline-demo"], "sqlite_storage": true, "shared_reader": true, "omarchy_plugin": false,
                "image_graphics": false, "fixture_reader": true,
                "ghostty_desktop_verified": false,
                "pending": ["Personal Discord DM history and replies", "Provider attachment downloads/uploads", "Live account/AI authentication tests", "Ghostty focus and modified keys", "Wayland clipboard", "terminal graphics", "Omarchy DND and plugin lifecycle"],
            });
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!(
                    "d-hub {} · live preview\n\nImplemented: mixed feed, locked drafts, SQLite service with isolated live/demo profiles, shared reader, local read/dismissal, editable profiles, spam prompts, account setup, typed AI adapters and opt-in Discord notification capture.\nImplemented live transports: iCloud IMAP/SMTP, Microsoft mail delta/reply/read sync, Teams chat sync/replies (local read markers). Pending: personal Discord replies/history, attachments, images, rich clipboard and Omarchy companion plugin.\nActual Ghostty/Omarchy desktop checks have not been run.",
                    env!("CARGO_PKG_VERSION")
                );
            }
        }
    }
    Ok(())
}
