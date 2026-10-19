use d_hub::{
    app::{App, Session},
    composer::{Event, SendState},
    config::Settings,
    database::Database,
    domain::MessageId,
    ipc::{self, Client, Command, Reply, Role},
    service::{Action, Owner},
    storage::DemoStore,
};
use std::{
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    process::{Child, Command as Process, Stdio},
    time::{Duration, Instant},
};

fn private_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

struct Service {
    root: tempfile::TempDir,
    child: Child,
    socket: PathBuf,
    data: PathBuf,
}
impl Service {
    fn new() -> Self {
        let root = private_dir();
        let socket = root.path().join("demo.sock");
        let data = root.path().join("profile");
        let child = Self::spawn(&data, &socket);
        let mut service = Self {
            root,
            child,
            socket,
            data,
        };
        service.ready();
        service
    }
    fn spawn(data: &std::path::Path, socket: &std::path::Path) -> Child {
        Process::new(env!("CARGO_BIN_EXE_d-hub"))
            .args(["daemon", "--demo", "--foreground", "--data-dir"])
            .arg(data)
            .arg("--socket")
            .arg(socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }
    fn ready(&mut self) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if Client::connect(&self.socket, Role::Reader).is_ok() {
                return;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "service exited during startup"
            );
            assert!(Instant::now() < until, "service startup timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn client(&self, role: Role) -> Client {
        Client::connect(&self.socket, role).unwrap()
    }
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    fn restart(&mut self) {
        self.kill();
        self.child = Self::spawn(&self.data, &self.socket);
        self.ready();
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.kill();
    }
}
fn edit(writer: &mut Client, body: &str) {
    let action = Action::Compose {
        expected: writer.snapshot.session.composer.clone(),
        event: Event::Body(body.into()),
        outcome: d_hub::app::MockOutcome::Success,
    };
    writer
        .request_snapshot(Command::Actions {
            expected_revision: writer.snapshot.revision,
            actions: vec![action],
        })
        .unwrap();
}
fn poll(client: &mut Client) {
    if let Reply::Snapshot { snapshot } = client
        .request(Command::Poll {
            after: client.snapshot.revision,
        })
        .unwrap()
    {
        client.snapshot = *snapshot;
    }
}

#[test]
fn sqlite_import_is_atomic_preserves_json_and_recovers_interrupted_outbox() {
    let dir = private_dir();
    let original;
    {
        let store = DemoStore::open(dir.path().into()).unwrap();
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.editor.insert("Durable fictional reply");
        app.update_body();
        app.submit().unwrap();
        store.save(&app.session).unwrap();
        original = std::fs::read(dir.path().join("demo-state.json")).unwrap();
    }
    let db = Database::open(dir.path(), &Settings::default()).unwrap();
    assert_eq!(db.outbox_status(1).unwrap().as_deref(), Some("uncertain"));
    let (_, session, _) = db.load().unwrap();
    assert!(matches!(session.composer.send, SendState::Uncertain(_)));
    assert_eq!(session.composer.body(), "Durable fictional reply");
    assert_eq!(
        std::fs::read(dir.path().join("demo-state.json")).unwrap(),
        original
    );
    drop(db);
    std::fs::write(
        dir.path().join("demo-state.json"),
        "obsolete malformed legacy backup",
    )
    .unwrap();
    let db = Database::open(dir.path(), &Settings::default()).unwrap();
    assert_eq!(
        db.load().unwrap().1.composer.body(),
        "Durable fictional reply"
    );
    assert_eq!(
        std::fs::metadata(dir.path().join("service.sqlite3"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn failed_or_future_migrations_never_replace_source_data() {
    let dir = private_dir();
    std::fs::write(dir.path().join("demo-state.json"), "invalid legacy input").unwrap();
    assert!(Database::open(dir.path(), &Settings::default()).is_err());
    let connection = rusqlite::Connection::open(dir.path().join("service.sqlite3")).unwrap();
    assert_eq!(
        connection
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("demo-state.json")).unwrap(),
        "invalid legacy input"
    );
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    let original = std::fs::read(dir.path().join("service.sqlite3")).unwrap();
    assert!(Database::open(dir.path(), &Settings::default()).is_err());
    assert_eq!(
        std::fs::read(dir.path().join("service.sqlite3")).unwrap(),
        original
    );
}

#[test]
fn revisions_and_event_gaps_are_explicit_and_batches_roll_back_as_a_unit() {
    let dir = private_dir();
    let db = Database::open(dir.path(), &Settings::default()).unwrap();
    let mut owner = Owner::open(db).unwrap();
    let initial = serde_json::to_vec(&owner.app.session).unwrap();
    let revision = owner.revision;
    let invalid = Action::Compose {
        expected: owner.app.session.composer.clone(),
        event: Event::Body("x".repeat(65_537)),
        outcome: d_hub::app::MockOutcome::Success,
    };
    assert!(
        owner
            .actions(
                revision,
                &[
                    Action::Unread {
                        id: MessageId(2),
                        unread: false
                    },
                    invalid
                ]
            )
            .is_err()
    );
    assert_eq!(serde_json::to_vec(&owner.app.session).unwrap(), initial);
    assert_eq!(owner.revision, revision);
    assert!(owner.actions(0, &[Action::Incoming]).is_err());
    for i in 0..260 {
        owner
            .actions(
                owner.revision,
                &[Action::Unread {
                    id: MessageId(2),
                    unread: i % 2 == 0,
                }],
            )
            .unwrap();
    }
    let snapshot = owner.snapshot(1, false).unwrap();
    assert!(snapshot.resync);
    assert_eq!(snapshot.changes.len(), 256);
    let tail = owner.snapshot(owner.revision - 1, false).unwrap();
    assert!(!tail.resync);
    assert_eq!(tail.changes.len(), 1);
}

#[test]
fn readers_share_seen_state_without_redirecting_draft_and_cannot_edit() {
    let service = Service::new();
    let mut writer = service.client(Role::Drawer);
    edit(&mut writer, "Keep this recipient");
    let draft = writer.snapshot.session.composer.draft.clone();
    let old_revision = writer.snapshot.revision;
    assert!(Client::connect(&service.socket, Role::Drawer).is_err());
    let mut reader = service.client(Role::Reader);
    let read = reader
        .request_snapshot(Command::Read { id: MessageId(2) })
        .unwrap();
    assert_eq!(read.session.composer.draft, draft);
    assert!(
        !read
            .session
            .messages
            .iter()
            .find(|m| m.id == MessageId(2))
            .unwrap()
            .unread
    );
    assert!(
        matches!(reader.request(Command::Actions { expected_revision:read.revision,actions:vec![Action::Incoming] }).unwrap(), Reply::Error { code,.. } if code == "forbidden")
    );
    assert!(
        matches!(writer.request(Command::Actions { expected_revision:old_revision,actions:vec![Action::Incoming] }).unwrap(), Reply::Error { code,snapshot:Some(_),.. } if code == "conflict")
    );
    poll(&mut writer);
    assert_eq!(writer.snapshot.session.composer.draft, draft);
    drop(writer);
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if Client::connect(&service.socket, Role::Drawer).is_ok() {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn service_completes_mock_send_after_drawer_disconnect_and_persists_acceptance() {
    let mut service = Service::new();
    let mut writer = service.client(Role::Drawer);
    edit(&mut writer, "A fictional accepted reply");
    writer
        .request_snapshot(Command::Actions {
            expected_revision: writer.snapshot.revision,
            actions: vec![Action::Compose {
                expected: writer.snapshot.session.composer.clone(),
                event: Event::Submit,
                outcome: d_hub::app::MockOutcome::Success,
            }],
        })
        .unwrap();
    assert!(matches!(
        writer.snapshot.session.composer.send,
        SendState::Submitting(_)
    ));
    drop(writer);
    let mut reader = service.client(Role::Reader);
    let until = Instant::now() + Duration::from_secs(3);
    while reader.snapshot.session.composer.sent.is_empty() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(20));
        poll(&mut reader);
    }
    assert_eq!(reader.snapshot.session.composer.sent.len(), 1);
    assert!(reader.snapshot.session.message_state[&MessageId(5)].dismissed);
    drop(reader);
    service.restart();
    let reader = service.client(Role::Reader);
    assert_eq!(reader.snapshot.session.composer.sent.len(), 1);
    assert!(reader.snapshot.session.composer.draft.is_none());
    let connection = rusqlite::Connection::open_with_flags(
        service.data.join("service.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>("SELECT status FROM outbox WHERE operation=1", [], |r| r
                .get(0))
            .unwrap(),
        "accepted"
    );
}

#[test]
fn crash_during_submission_recovers_uncertain_and_never_retries() {
    let mut service = Service::new();
    let mut writer = service.client(Role::Drawer);
    edit(&mut writer, "Keep an uncertain reply");
    writer
        .request_snapshot(Command::Actions {
            expected_revision: writer.snapshot.revision,
            actions: vec![Action::Compose {
                expected: writer.snapshot.session.composer.clone(),
                event: Event::Submit,
                outcome: d_hub::app::MockOutcome::Success,
            }],
        })
        .unwrap();
    let target = writer
        .snapshot
        .session
        .composer
        .draft
        .as_ref()
        .unwrap()
        .target
        .clone();
    service.restart();
    drop(writer);
    let mut reader = service.client(Role::Reader);
    assert!(matches!(
        reader.snapshot.session.composer.send,
        SendState::Uncertain(_)
    ));
    std::thread::sleep(Duration::from_millis(750));
    poll(&mut reader);
    assert!(reader.snapshot.session.composer.sent.is_empty());
    assert_eq!(
        reader
            .snapshot
            .session
            .composer
            .draft
            .as_ref()
            .unwrap()
            .target,
        target
    );
}

#[test]
fn optimistic_edits_are_replayed_without_losing_text_or_cursor() {
    let service = Service::new();
    let client = service.client(Role::Drawer);
    let mut app = App::connected(
        client.snapshot.session.clone(),
        client.snapshot.settings.clone(),
    )
    .unwrap();
    app.shared = true;
    let mut link = ipc::Link::new(client, service.socket.clone(), None);
    app.editor.insert("first");
    app.update_body();
    link.sync(&mut app).unwrap();
    app.editor.insert(" second 東京");
    app.update_body();
    app.editor.cursor = 2;
    link.flush(&mut app).unwrap();
    assert!(link.connected);
    assert_eq!(app.editor.text, "first second 東京");
    assert_eq!(app.editor.cursor, 2);
    assert_eq!(
        service
            .client(Role::Reader)
            .snapshot
            .session
            .composer
            .body(),
        "first second 東京"
    );
}

#[test]
fn version_and_frame_limits_fail_without_stopping_other_clients() {
    let service = Service::new();
    let mut stream = UnixStream::connect(&service.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let bytes = serde_json::to_vec(&ipc::Request {
        version: 999,
        command: Command::Hello { role: Role::Reader },
    })
    .unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
    let mut length = [0; 4];
    stream.read_exact(&mut length).unwrap();
    let mut reply = vec![0; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut reply).unwrap();
    assert!(
        matches!(serde_json::from_slice::<Reply>(&reply).unwrap(),Reply::Error { code,.. } if code == "version")
    );
    // Upgrade discovery stays available even across protocol/snapshot schema changes.
    let bytes = serde_json::to_vec(&ipc::Request {
        version: 999,
        command: Command::BuildInfo,
    })
    .unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
    stream.read_exact(&mut length).unwrap();
    let mut reply = vec![0; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut reply).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Reply>(&reply).unwrap(),
        Reply::BuildInfo {
            protocol: ipc::VERSION,
            editor_connected: false,
            ..
        }
    ));
    drop(stream);
    let mut oversized = UnixStream::connect(&service.socket).unwrap();
    oversized
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    oversized
        .write_all(&((ipc::MAX_FRAME + 1) as u32).to_be_bytes())
        .unwrap();
    assert!(oversized.read(&mut [0; 1]).is_err() || oversized.read(&mut [0; 1]).unwrap_or(0) == 0);
    assert_eq!(
        service.client(Role::Reader).snapshot.session.messages.len(),
        5
    );
}

#[test]
fn cli_settings_and_reader_use_the_service_and_shutdown_is_acknowledged() {
    let mut service = Service::new();
    let output = Process::new(env!("CARGO_BIN_EXE_d-hub"))
        .args(["config", "--socket"])
        .arg(&service.socket)
        .args(["set", "padding_horizontal", "2"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        service
            .client(Role::Reader)
            .snapshot
            .settings
            .padding_horizontal,
        2
    );
    let output = Process::new(env!("CARGO_BIN_EXE_d-hub"))
        .args(["preview", "--socket"])
        .arg(&service.socket)
        .args(["2", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["unread"],
        false
    );
    let mut client = service.client(Role::Control);
    assert!(matches!(
        client.request(Command::Stop).unwrap(),
        Reply::Stopped
    ));
    let until = Instant::now() + Duration::from_secs(2);
    while service.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!service.socket.exists());
}

#[test]
fn disconnect_preserves_unacknowledged_draft_in_a_private_recovery_file() {
    let mut service = Service::new();
    let client = service.client(Role::Drawer);
    let mut app = App::connected(
        client.snapshot.session.clone(),
        client.snapshot.settings.clone(),
    )
    .unwrap();
    app.shared = true;
    let mut link = ipc::Link::new(client, service.socket.clone(), None);
    link.recovery = service.root.path().join("recovery.json");
    service.kill();
    app.editor.insert("Unsaved fictional reply 東京");
    app.update_body();
    let draft = app.session.composer.draft.clone();
    let until = Instant::now() + Duration::from_secs(3);
    while link.connected {
        link.sync(&mut app).unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.session.composer.draft, draft);
    let recovery: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&link.recovery).unwrap()).unwrap();
    assert_eq!(
        recovery["session"]["composer"]["draft"]["body"],
        "Unsaved fictional reply 東京"
    );
    service.restart();
    let mut resumed = service.client(Role::Drawer);
    ipc::restore(&mut resumed, &link.recovery).unwrap();
    assert_eq!(
        resumed.snapshot.session.composer.body(),
        "Unsaved fictional reply 東京"
    );
    assert!(resumed.snapshot.session.composer.sent.is_empty());
    assert_eq!(
        std::fs::metadata(&link.recovery)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn stale_composer_and_configuration_edits_never_overwrite_newer_work() {
    let service = Service::new();
    let mut writer = service.client(Role::Drawer);
    let stale = writer.snapshot.session.composer.clone();
    edit(&mut writer, "Newer draft");
    let reply = writer
        .request(Command::Actions {
            expected_revision: writer.snapshot.revision,
            actions: vec![Action::Compose {
                expected: stale,
                event: Event::Body("Stale edit".into()),
                outcome: d_hub::app::MockOutcome::Success,
            }],
        })
        .unwrap();
    assert!(matches!(reply,Reply::Error { code,.. } if code=="invalid"));
    let before = writer.snapshot.settings.clone();
    let mut control = service.client(Role::Control);
    let mut changed = before.clone();
    changed.padding_vertical = 2;
    control
        .request_snapshot(Command::Configure {
            expected_revision: control.snapshot.revision,
            settings: Box::new(changed.clone()),
        })
        .unwrap();
    poll(&mut writer);
    let mut stale_settings = before.clone();
    stale_settings.theme = d_hub::config::Theme::Dusk;
    let reply = writer
        .request(Command::Actions {
            expected_revision: writer.snapshot.revision,
            actions: vec![Action::Configure {
                before,
                settings: stale_settings,
            }],
        })
        .unwrap();
    assert!(matches!(reply,Reply::Error { code,.. } if code=="invalid"));
    let reader = service.client(Role::Reader);
    assert_eq!(reader.snapshot.session.composer.body(), "Newer draft");
    assert_eq!(reader.snapshot.settings, changed);
}

#[test]
fn reply_first_message_and_source_switches_share_one_durable_service_transaction() {
    let service = Service::new();
    let client = service.client(Role::Drawer);
    let mut app = App::connected(
        client.snapshot.session.clone(),
        client.snapshot.settings.clone(),
    )
    .unwrap();
    app.shared = true;
    let mut link = ipc::Link::new(client, service.socket.clone(), None);
    link.recovery = service.root.path().join("unexpected-recovery.json");
    app.editor.insert("First message draft");
    app.update_body();
    link.sync(&mut app).unwrap();
    app.move_reply(1, 4);
    app.editor.insert("Second message draft");
    app.update_body();
    app.move_reply(-1, 4);
    app.cycle_reply_source(false);
    app.cycle_reply_source(false);
    app.cycle_reply_source(true);
    link.flush(&mut app).unwrap();
    assert!(link.connected, "{}", app.notice);
    assert_eq!(app.session.composer.body(), "First message draft");
    let reader = service.client(Role::Reader);
    assert_eq!(
        reader.snapshot.settings.source,
        d_hub::domain::SourceFilter::ICloud
    );
    assert_eq!(
        reader.snapshot.session.composer.body(),
        "First message draft"
    );
    assert!(
        reader
            .snapshot
            .session
            .saved
            .values()
            .any(|d| d.body == "Second message draft")
    );
}

#[test]
fn recovery_does_not_overwrite_another_draft_or_replay_an_unresolved_send() {
    let service = Service::new();
    let mut writer = service.client(Role::Drawer);
    edit(&mut writer, "Existing service draft");
    let mut app = App::new(Session::default(), Settings::default()).unwrap();
    app.editor.insert("Different recovery draft");
    app.update_body();
    let path = service.root.path().join("recovery.json");
    d_hub::storage::write_private(
        &path,
        &serde_json::to_vec(&serde_json::json!({"protocol":ipc::VERSION,"session":app.session}))
            .unwrap(),
    )
    .unwrap();
    assert!(ipc::restore(&mut writer, &path).is_err());
    assert_eq!(
        service
            .client(Role::Reader)
            .snapshot
            .session
            .composer
            .body(),
        "Existing service draft"
    );
    app.submit().unwrap();
    d_hub::storage::write_private(
        &path,
        &serde_json::to_vec(&serde_json::json!({"protocol":ipc::VERSION,"session":app.session}))
            .unwrap(),
    )
    .unwrap();
    assert!(ipc::restore(&mut writer, &path).is_err());
    assert!(
        service
            .client(Role::Reader)
            .snapshot
            .session
            .composer
            .sent
            .is_empty()
    );
}

#[test]
fn recovery_write_failure_keeps_the_unsaved_draft_in_the_window() {
    let mut service = Service::new();
    let client = service.client(Role::Drawer);
    let mut app = App::connected(
        client.snapshot.session.clone(),
        client.snapshot.settings.clone(),
    )
    .unwrap();
    app.shared = true;
    let mut link = ipc::Link::new(client, service.socket.clone(), None);
    link.recovery = service.root.path().to_owned();
    service.kill();
    app.editor.insert("Keep this unsaved reply visible");
    app.update_body();
    let until = Instant::now() + Duration::from_secs(3);
    while link.connected {
        link.sync(&mut app).unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(app.notice.starts_with("RECOVERY FAILED"));
    assert_eq!(
        app.session.composer.body(),
        "Keep this unsaved reply visible"
    );
}

#[test]
fn update_waits_for_an_active_send_without_retrying_or_releasing_its_target() {
    let root = private_dir();
    let database = Database::open(root.path(), &Settings::default()).unwrap();
    let mut owner = Owner::open(database).unwrap();
    assert!(owner.update_safe());
    let expected = owner.app.session.composer.clone();
    owner
        .actions(
            owner.revision,
            &[Action::Compose {
                expected,
                event: Event::Body("A fictional send must finish before update".into()),
                outcome: d_hub::app::MockOutcome::Failure,
            }],
        )
        .unwrap();
    let target = owner
        .app
        .session
        .composer
        .draft
        .as_ref()
        .unwrap()
        .target
        .clone();
    let expected = owner.app.session.composer.clone();
    owner
        .actions(
            owner.revision,
            &[Action::Compose {
                expected,
                event: Event::Submit,
                outcome: d_hub::app::MockOutcome::Failure,
            }],
        )
        .unwrap();
    assert!(!owner.update_safe());
    std::thread::sleep(Duration::from_millis(700));
    owner.tick().unwrap();
    assert!(owner.update_safe());
    assert!(matches!(
        owner.app.session.composer.send,
        SendState::Failed(_)
    ));
    assert_eq!(
        owner.app.session.composer.draft.as_ref().unwrap().target,
        target
    );
    assert!(owner.app.session.composer.sent.is_empty());
}
