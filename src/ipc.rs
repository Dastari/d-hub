//! Versioned, bounded Unix socket RPC. Only the owner thread touches SQLite/state.
use crate::{
    app::App,
    database::Database,
    domain::MessageId,
    service::{Action, Owner, Snapshot},
    storage::{DemoStore, write_private},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

pub const VERSION: u32 = 2;
pub const MAX_FRAME: usize = 8 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Drawer,
    Reader,
    Control,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Hello {
        role: Role,
    },
    Poll {
        after: u64,
    },
    Actions {
        expected_revision: u64,
        actions: Vec<Action>,
    },
    Read {
        id: MessageId,
    },
    Configure {
        expected_revision: u64,
        settings: crate::config::Settings,
    },
    Resolve {
        expected_revision: u64,
        operation: u64,
        accepted: bool,
    },
    Refresh,
    Stop,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub command: Command,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Reply {
    Snapshot {
        snapshot: Box<Snapshot>,
    },
    Unchanged {
        revision: u64,
    },
    Stopped,
    Error {
        code: String,
        message: String,
        snapshot: Option<Box<Snapshot>>,
    },
}
fn error(code: &str, message: impl Into<String>) -> Reply {
    Reply::Error {
        code: code.into(),
        message: message.into(),
        snapshot: None,
    }
}
fn frame_write(stream: &mut UnixStream, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_FRAME {
        bail!("IPC frame too large");
    }
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}
fn frame_read<T: DeserializeOwned>(stream: &mut UnixStream) -> Result<T> {
    let mut size = [0; 4];
    stream.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size == 0 || size > MAX_FRAME {
        bail!("IPC frame size outside limit");
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).context("invalid IPC JSON")
}
fn peer(stream: &UnixStream) -> Result<()> {
    let credentials =
        nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::PeerCredentials)?;
    if credentials.uid() != nix::unistd::geteuid().as_raw() {
        bail!("IPC peer belongs to another user");
    }
    Ok(())
}
pub fn default_live_socket() -> Result<PathBuf> {
    let mut path = default_socket()?;
    path.set_file_name("service-live.sock");
    Ok(path)
}
pub fn default_socket() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|v| !v.is_empty())
            .context(
                "XDG_RUNTIME_DIR is unavailable; provide --socket PATH in a private directory",
            )?,
    )
    .join("d-hub/service-demo.sock"))
}

pub struct Client {
    stream: UnixStream,
    pub snapshot: Snapshot,
}
impl Client {
    pub fn connect(path: &Path, role: Role) -> Result<Self> {
        let metadata = fs::symlink_metadata(path)
            .context("service socket unavailable; launch d-hub to start the live service, or start an explicit demo daemon")?;
        if !metadata.file_type().is_socket()
            || metadata.uid() != nix::unistd::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            bail!("service socket must be owned by this user with mode 0600");
        }
        let stream = UnixStream::connect(path)?;
        peer(&stream)?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(3)))?;
        let mut client = Self {
            stream,
            snapshot: empty_snapshot(),
        };
        client.snapshot = client.request_snapshot(Command::Hello { role })?;
        Ok(client)
    }
    pub fn request(&mut self, command: Command) -> Result<Reply> {
        frame_write(
            &mut self.stream,
            &Request {
                version: VERSION,
                command,
            },
        )?;
        frame_read(&mut self.stream)
    }
    pub fn request_snapshot(&mut self, command: Command) -> Result<Snapshot> {
        match self.request(command)? {
            Reply::Snapshot { snapshot } => {
                self.snapshot = *snapshot;
                Ok(self.snapshot.clone())
            }
            Reply::Error { code, message, .. } => bail!("{code}: {message}"),
            _ => bail!("unexpected service response"),
        }
    }
}
fn empty_snapshot() -> Snapshot {
    Snapshot {
        discord_personal_qr_setup: false,
        live: false,
        provider_status: std::collections::BTreeMap::new(),
        revision: 0,
        session: crate::app::Session::default(),
        settings: crate::config::Settings::default(),
        selected: None,
        focused: true,
        editor_connected: false,
        changes: vec![],
        resync: false,
    }
}

enum Delivery {
    Request {
        id: u64,
        request: Box<Request>,
        respond: SyncSender<Reply>,
        written: Receiver<()>,
    },
    Disconnect(u64),
}
struct SocketGuard {
    path: PathBuf,
    _lock: DemoStore,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn serve(database: Database, path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .context("socket needs a private parent directory")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let meta = fs::symlink_metadata(parent)?;
    if meta.file_type().is_symlink()
        || meta.uid() != nix::unistd::geteuid().as_raw()
        || meta.permissions().mode() & 0o077 != 0
    {
        bail!("socket parent must be owned by this user and private (0700)");
    }
    let name = path
        .file_name()
        .context("missing socket name")?
        .to_str()
        .context("socket name must be UTF-8")?;
    let lock = DemoStore::open(parent.join(format!(".{name}.owner")))?;
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.file_type().is_socket() || meta.uid() != nix::unistd::geteuid().as_raw() {
            bail!("refusing to replace a non-socket or foreign socket");
        }
        if UnixStream::connect(path).is_ok() {
            bail!("another service is listening on this socket");
        }
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    let _guard = SocketGuard {
        path: path.into(),
        _lock: lock,
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let mut owner = Owner::open(database)?;
    let (tx, rx) = mpsc::sync_channel(64);
    let connections = Arc::new(AtomicUsize::new(0));
    let mut next_id = 0_u64;
    let mut roles = BTreeMap::new();
    let mut editor = None;
    eprintln!(
        "Offline demo service ready: {} · no providers or real sends",
        path.display()
    );
    loop {
        owner.tick()?;
        // Bounded accept work leaves time for state commands and mock results.
        for _ in 0..16 {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if connections.load(Ordering::Relaxed) >= 16 || peer(&stream).is_err() {
                        continue;
                    }
                    next_id = next_id.checked_add(1).context("connection IDs exhausted")?;
                    let id = next_id;
                    let tx = tx.clone();
                    let count = connections.clone();
                    count.fetch_add(1, Ordering::Relaxed);
                    std::thread::spawn(move || {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                        while let Ok(request) = frame_read(&mut stream) {
                            let (respond, response) = mpsc::sync_channel(1);
                            let (written_tx, written) = mpsc::sync_channel(1);
                            if tx
                                .try_send(Delivery::Request {
                                    id,
                                    request: Box::new(request),
                                    respond,
                                    written,
                                })
                                .is_err()
                            {
                                let _ = frame_write(
                                    &mut stream,
                                    &error("busy", "service command queue is full"),
                                );
                                break;
                            }
                            let Ok(reply) = response.recv_timeout(Duration::from_secs(3)) else {
                                break;
                            };
                            let result = frame_write(&mut stream, &reply);
                            let _ = written_tx.send(());
                            if result.is_err() || matches!(reply, Reply::Stopped) {
                                break;
                            }
                        }
                        // Disconnects must release a lease even under command pressure.
                        let _ = tx.send(Delivery::Disconnect(id));
                        count.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            }
        }
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(Delivery::Disconnect(id)) => {
                roles.remove(&id);
                if editor == Some(id) {
                    editor = None;
                    owner.actions(owner.revision, &[Action::Focus { focused: false }])?;
                }
            }
            Ok(Delivery::Request {
                id,
                request,
                respond,
                written,
            }) => {
                let request = *request;
                if request.version != VERSION {
                    let _ = respond.send(error("version", format!("protocol {VERSION} required")));
                    continue;
                }
                let mut stop = false;
                let reply = if let Command::Hello { role } = request.command {
                    match roles.entry(id) {
                        std::collections::btree_map::Entry::Occupied(_) => {
                            error("handshake", "already connected")
                        }
                        std::collections::btree_map::Entry::Vacant(_)
                            if role == Role::Drawer && editor.is_some() =>
                        {
                            error(
                                "editor_busy",
                                "another drawer owns the draft editor; open a reader instead",
                            )
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(role);
                            if role == Role::Drawer {
                                editor = Some(id);
                                owner
                                    .actions(owner.revision, &[Action::Focus { focused: true }])?;
                            }
                            Reply::Snapshot {
                                snapshot: Box::new(owner.snapshot(0, editor.is_some())?),
                            }
                        }
                    }
                } else if let Some(role) = roles.get(&id) {
                    match request.command {
                        Command::Poll { after } if after == owner.revision => Reply::Unchanged {
                            revision: owner.revision,
                        },
                        Command::Poll { after } => Reply::Snapshot {
                            snapshot: Box::new(owner.snapshot(after, editor.is_some())?),
                        },
                        Command::Read { id: message } => match owner.read(message) {
                            Ok(()) => Reply::Snapshot {
                                snapshot: Box::new(owner.snapshot(0, editor.is_some())?),
                            },
                            Err(_) => error("not_found", "unknown message"),
                        },
                        Command::Actions {
                            expected_revision,
                            actions,
                        } if *role == Role::Drawer && editor == Some(id) => {
                            if expected_revision != owner.revision {
                                Reply::Error {
                                    code: "conflict".into(),
                                    message: "state changed; rebase against the snapshot".into(),
                                    snapshot: Some(Box::new(
                                        owner.snapshot(expected_revision, true)?,
                                    )),
                                }
                            } else {
                                match owner.actions(expected_revision, &actions) {
                                    Ok(()) => Reply::Snapshot {
                                        snapshot: Box::new(
                                            owner.snapshot(expected_revision, true)?,
                                        ),
                                    },
                                    Err(e) => error("invalid", e.to_string()),
                                }
                            }
                        }
                        Command::Configure {
                            expected_revision,
                            settings,
                        } if *role != Role::Reader => {
                            if expected_revision != owner.revision {
                                error(
                                    "conflict",
                                    "settings changed; read the latest snapshot and retry",
                                )
                            } else {
                                match owner.actions(
                                    expected_revision,
                                    &[Action::Configure {
                                        before: owner.app.settings.clone(),
                                        settings,
                                    }],
                                ) {
                                    Ok(()) => Reply::Snapshot {
                                        snapshot: Box::new(
                                            owner.snapshot(expected_revision, editor.is_some())?,
                                        ),
                                    },
                                    Err(e) => error("invalid", e.to_string()),
                                }
                            }
                        }
                        Command::Resolve {
                            expected_revision,
                            operation,
                            accepted,
                        } if *role == Role::Control => {
                            match owner.actions(
                                expected_revision,
                                &[Action::Resolve {
                                    expected: owner.app.session.composer.clone(),
                                    operation,
                                    accepted,
                                }],
                            ) {
                                Ok(()) => Reply::Snapshot {
                                    snapshot: Box::new(
                                        owner.snapshot(expected_revision, editor.is_some())?,
                                    ),
                                },
                                Err(e) => error("invalid", e.to_string()),
                            }
                        }
                        Command::Refresh if *role != Role::Reader => {
                            owner.refresh();
                            Reply::Snapshot {
                                snapshot: Box::new(owner.snapshot(0, editor.is_some())?),
                            }
                        }
                        Command::Stop if *role == Role::Control => {
                            stop = true;
                            Reply::Stopped
                        }
                        _ => error("forbidden", "this connection does not own that capability"),
                    }
                } else {
                    error("handshake", "hello must be the first request")
                };
                let _ = respond.send(reply);
                if stop {
                    let _ = written.recv_timeout(Duration::from_secs(2));
                    return Ok(());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

/// One bounded worker keeps socket waits out of the terminal render loop.
pub struct Link {
    pub socket: PathBuf,
    pub revision: u64,
    pub connected: bool,
    requests: SyncSender<Command>,
    replies: Receiver<Result<Reply, String>>,
    inflight: Option<Vec<Action>>,
    queued: Vec<Action>,
    last_settings: crate::config::Settings,
    reader: Option<MessageId>,
    pub recovery: PathBuf,
}
impl Link {
    pub fn new(client: Client, socket: PathBuf, reader: Option<MessageId>) -> Self {
        let socket = fs::canonicalize(&socket).unwrap_or(socket);
        let revision = client.snapshot.revision;
        let settings = client.snapshot.settings.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let (results, replies) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut client = client;
            while let Ok(command) = rx.recv() {
                let result = client.request(command).map_err(|e| format!("{e:#}"));
                let failed = result.is_err();
                if results.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let root = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|p| p.join(".local/state"))
            })
            .unwrap_or_else(|| socket.parent().unwrap_or(Path::new(".")).to_owned());
        Self {
            socket,
            revision,
            connected: true,
            requests: tx,
            replies,
            inflight: None,
            queued: vec![],
            last_settings: settings,
            reader,
            recovery: root.join(format!(
                "d-hub/recovery/drawer-{}-{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )),
        }
    }
    fn preserve(&mut self, app: &mut App, reason: &str) -> Result<()> {
        self.connected = false;
        if self.reader.is_none() {
            let saved = (|| -> Result<()> {
                write_private(
                    &self.recovery,
                    &serde_json::to_vec_pretty(
                        &serde_json::json!({"protocol":VERSION,"socket":self.socket,"base_revision":self.revision,"session":app.session,"settings":app.settings,"pending":self.queued,"inflight":self.inflight}),
                    )?,
                )
            })();
            if let Err(error) = saved {
                app.notice = format!(
                    "RECOVERY FAILED · unsaved text stays in this window; copy it before exiting. {error:#}"
                );
                return Ok(());
            }
        }
        app.notice = if self.reader.is_some() {
            format!("Service disconnected · cached reader retained · {reason}")
        } else {
            format!(
                "Service disconnected · unsaved work preserved in {} · reopen the drawer after reconnecting. {reason}",
                self.recovery.display()
            )
        };
        Ok(())
    }
    pub fn pending(&self) -> bool {
        !self.queued.is_empty() || self.inflight.as_ref().is_some_and(|v| !v.is_empty())
    }
    pub fn sync(&mut self, app: &mut App) -> Result<bool> {
        if !self.connected {
            return Ok(false);
        }
        for action in std::mem::take(&mut app.actions) {
            if let Action::Configure { settings, .. } = &action {
                self.last_settings = settings.clone();
            }
            self.queued.push(action);
        }
        if self.reader.is_none() && self.last_settings != app.settings {
            self.queued.push(Action::Configure {
                before: self.last_settings.clone(),
                settings: app.settings.clone(),
            });
            self.last_settings = app.settings.clone();
        }
        let mut dirty = false;
        match self.replies.try_recv() {
            Ok(Ok(reply)) => {
                let sent = self.inflight.take().unwrap_or_default();
                let snapshot = match reply {
                    Reply::Snapshot { snapshot } => Some(*snapshot),
                    Reply::Error {
                        code,
                        snapshot: Some(snapshot),
                        ..
                    } if code == "conflict" => {
                        self.queued.splice(0..0, sent);
                        Some(*snapshot)
                    }
                    Reply::Error { message, .. } => {
                        self.queued.splice(0..0, sent);
                        self.preserve(app, &message)?;
                        return Ok(true);
                    }
                    _ => None,
                };
                if let Some(snapshot) = snapshot {
                    let local = (
                        app.session.clone(),
                        app.settings.clone(),
                        app.selected,
                        app.focused,
                    );
                    let editor = app.editor.clone();
                    let mode = app.mode;
                    let previous_reader = app.reader.as_ref().map(|m| m.id);
                    self.revision = snapshot.revision;
                    app.session = snapshot.session;
                    app.settings = snapshot.settings;
                    app.live_service = snapshot.live;
                    app.service_supports_discord_qr = snapshot.discord_personal_qr_setup;
                    app.provider_status = snapshot.provider_status;
                    app.selected = snapshot.selected;
                    app.focused = snapshot.focused;
                    let shared = app.shared;
                    app.shared = false;
                    for action in &self.queued {
                        if let Err(e) = crate::service::apply(app, action) {
                            app.shared = shared;
                            app.session = local.0;
                            app.settings = local.1;
                            app.selected = local.2;
                            app.focused = local.3;
                            app.editor = editor;
                            self.preserve(app, &e.to_string())?;
                            return Ok(true);
                        }
                    }
                    app.shared = shared;
                    app.mode = mode;
                    if let Some(id) = self.reader.or(previous_reader)
                        && let Some(message) = app.session.messages.iter().find(|m| m.id == id)
                    {
                        app.reader = Some(message.clone());
                    }
                    app.sync_editor();
                    if editor.text == app.editor.text {
                        app.editor = editor;
                    }
                    self.last_settings = app.settings.clone();
                    app.notice = "Connected to offline demo service · sends are simulated and completed by the service.".into();
                    dirty = true;
                }
            }
            Ok(Err(message)) => {
                self.preserve(app, &message)?;
                return Ok(true);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.preserve(app, "IPC worker stopped")?;
                return Ok(true);
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if self.inflight.is_none() {
            let actions = std::mem::take(&mut self.queued);
            let command = if actions.is_empty() {
                Command::Poll {
                    after: self.revision,
                }
            } else {
                Command::Actions {
                    expected_revision: self.revision,
                    actions: actions.clone(),
                }
            };
            self.requests
                .send(command)
                .context("service IPC worker stopped")?;
            self.inflight = Some(actions);
        }
        Ok(dirty)
    }
    /// Exit flushes only local edits; a mock send continues in the owner after the drawer closes.
    pub fn flush(&mut self, app: &mut App) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            self.sync(app)?;
            if !self.connected || (!self.pending() && app.actions.is_empty()) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                self.preserve(app, "Exit flush timed out")?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Restore user-approved draft text only; never replay an unacknowledged send.
pub fn restore(client: &mut Client, path: &Path) -> Result<()> {
    #[derive(Deserialize)]
    struct Recovery {
        protocol: u32,
        session: crate::app::Session,
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_FRAME + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FRAME {
        bail!("recovery file exceeds IPC limit");
    }
    let recovery: Recovery =
        serde_json::from_slice(&bytes).context("invalid drawer recovery file")?;
    if ![1, VERSION].contains(&recovery.protocol) || !recovery.session.composer.editable() {
        bail!(
            "recovery has an incompatible protocol or unresolved send; reconcile the service outbox first"
        );
    }
    let migrated = App::new(recovery.session, client.snapshot.settings.clone())?;
    let action = Action::Restore {
        expected: client.snapshot.session.composer.clone(),
        draft: migrated.session.composer.draft,
        saved: migrated.session.saved,
    };
    client.request_snapshot(Command::Actions {
        expected_revision: client.snapshot.revision,
        actions: vec![action],
    })?;
    Ok(())
}
