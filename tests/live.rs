use anyhow::Result;
use d_hub::{
    app::MockOutcome,
    composer::{Event, SendState, Submission},
    config::Settings,
    database::Database,
    domain::Provider,
    policy::AccountConfig,
    providers::{self, Batch, Item, SendResult, Transport},
    read_state::RemoteMessage,
    service::{Action, Owner},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Scenario {
    batches: VecDeque<Batch>,
    sends: usize,
    reads: usize,
    result: Option<SendResult>,
    db: Option<PathBuf>,
}
fn scenarios() -> &'static Mutex<BTreeMap<String, Scenario>> {
    static MAP: OnceLock<Mutex<BTreeMap<String, Scenario>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}
struct Fake;
impl Transport for Fake {
    fn sync(&mut self, account: &AccountConfig, cursor: Value) -> Result<Batch> {
        let mut scenarios = scenarios().lock().unwrap();
        Ok(scenarios
            .get_mut(&account.id)
            .unwrap()
            .batches
            .pop_front()
            .unwrap_or(Batch {
                items: vec![],
                cursor,
                status: "Fake connected".into(),
            }))
    }
    fn unread(&mut self, account: &AccountConfig, _: &RemoteMessage, _: bool) -> Result<()> {
        scenarios()
            .lock()
            .unwrap()
            .get_mut(&account.id)
            .unwrap()
            .reads += 1;
        Ok(())
    }
    fn send(&mut self, account: &AccountConfig, _: &Item, submission: &Submission) -> SendResult {
        let mut scenarios = scenarios().lock().unwrap();
        let scenario = scenarios.get_mut(&account.id).unwrap();
        scenario.sends += 1;
        if let Some(path) = &scenario.db {
            let connection = rusqlite::Connection::open(path).unwrap();
            let status: String = connection
                .query_row(
                    "SELECT status FROM outbox WHERE operation=?1",
                    [submission.operation],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                status, "submitting",
                "network write must follow durable operation"
            );
        }
        scenario.result.clone().unwrap_or(SendResult::Accepted)
    }
}
fn fake() -> Result<Box<dyn Transport>> {
    Ok(Box::new(Fake))
}
fn directory() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}
fn account(id: &str) -> AccountConfig {
    let mut account = AccountConfig::new(id, Provider::ICloud);
    account.username = "owner@example.invalid".into();
    account.enabled = true;
    account.allow_send = true;
    account
}
fn item(account: &AccountConfig, uid: u32) -> Item {
    Item {
        account_scope: providers::scope(account),
        remote: RemoteMessage::Imap {
            account: account.id.clone(),
            mailbox: "INBOX".into(),
            uid_validity: 7,
            uid,
        },
        sender: "Fictional sender".into(),
        recipients: vec!["sender@example.invalid".into()],
        subject: format!("Subject {uid}"),
        body: "Fictional text only".into(),
        timestamp: 1000 + uid as u64,
        unread: Some(true),
        message_id: format!("<{uid}@example.invalid>"),
        references: String::new(),
        deleted: false,
        partial: false,
    }
}
fn batch(account: &AccountConfig, uids: &[u32], cursor: u64) -> Batch {
    Batch {
        items: uids.iter().map(|u| item(account, *u)).collect(),
        cursor: json!({"checkpoint":cursor}),
        status: "Fake connected".into(),
    }
}
fn owner(id: &str) -> (tempfile::TempDir, Owner, AccountConfig) {
    let dir = directory();
    let account = account(id);
    scenarios().lock().unwrap().insert(
        id.into(),
        Scenario {
            db: Some(dir.path().join("service.sqlite3")),
            ..Default::default()
        },
    );
    let settings = Settings {
        accounts: vec![account.clone()],
        ..Default::default()
    };
    let owner = Owner::open(Database::open_mode(dir.path(), &settings, true).unwrap())
        .unwrap()
        .with_transport(fake);
    (dir, owner, account)
}
fn drive(owner: &mut Owner, until: impl Fn(&Owner) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        owner.tick().unwrap();
        if until(owner) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for fake provider"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn compose(owner: &mut Owner, event: Event) {
    owner
        .actions(
            owner.revision,
            &[Action::Compose {
                expected: owner.app.session.composer.clone(),
                event,
                outcome: MockOutcome::Success,
            }],
        )
        .unwrap();
}
fn seed(owner: &mut Owner, account: &AccountConfig, uids: &[u32]) {
    scenarios()
        .lock()
        .unwrap()
        .get_mut(&account.id)
        .unwrap()
        .batches
        .push_back(batch(account, uids, 1));
    owner.refresh();
    drive(owner, |o| o.app.session.messages.len() == uids.len());
}
#[test]
fn live_profiles_start_empty_and_cannot_cross_demo_or_legacy_data() {
    let dir = directory();
    let settings = Settings::default();
    let db = Database::open_mode(dir.path(), &settings, true).unwrap();
    assert!(db.load().unwrap().1.messages.is_empty());
    drop(db);
    assert!(Database::open(dir.path(), &settings).is_err());
    assert!(
        Database::open_mode(dir.path(), &settings, true)
            .unwrap()
            .live
    );
    let demo = directory();
    let db = Database::open(demo.path(), &settings).unwrap();
    drop(db);
    assert!(Database::open_mode(demo.path(), &settings, true).is_err());
    let legacy = directory();
    std::fs::write(
        legacy.path().join("demo-state.json"),
        serde_json::to_vec(&d_hub::app::Session::default()).unwrap(),
    )
    .unwrap();
    assert!(Database::open_mode(legacy.path(), &settings, true).is_err());
}
#[test]
fn arrivals_read_acknowledgements_and_replays_preserve_locked_drafts() {
    let (dir, mut owner, a) = owner("live-replays");
    seed(&mut owner, &a, &[1]);
    let id = owner.app.session.messages[0].id;
    owner
        .actions(
            owner.revision,
            &[Action::Select {
                id,
                prepare_reply: true,
            }],
        )
        .unwrap();
    compose(&mut owner, Event::Body("Keep this recipient 東京".into()));
    let draft = owner.app.session.composer.draft.clone();
    let mut second = batch(&a, &[1, 2], 2);
    second.items[0].unread = Some(false);
    scenarios()
        .lock()
        .unwrap()
        .get_mut(&a.id)
        .unwrap()
        .batches
        .push_back(second);
    owner.refresh();
    drive(&mut owner, |o| {
        o.app.session.messages.len() == 2 && o.app.session.read_changes.is_empty()
    });
    assert_eq!(owner.app.session.composer.draft, draft);
    assert!(
        !owner
            .app
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .unread
    );
    assert_eq!(scenarios().lock().unwrap()[&a.id].reads, 1);
    owner
        .actions(
            owner.revision,
            &[Action::Dismiss {
                id,
                dismissed: true,
            }],
        )
        .unwrap();
    let before = owner
        .app
        .session
        .messages
        .iter()
        .map(|m| m.id)
        .collect::<Vec<_>>();
    drop(owner);
    let mut owner =
        Owner::open(Database::open_mode(dir.path(), &Settings::default(), true).unwrap())
            .unwrap()
            .with_transport(fake);
    scenarios()
        .lock()
        .unwrap()
        .get_mut(&a.id)
        .unwrap()
        .batches
        .push_back(batch(&a, &[1, 2], 2));
    owner.refresh();
    let revision = owner.revision;
    drive(&mut owner, |o| o.revision > revision);
    assert_eq!(
        owner
            .app
            .session
            .messages
            .iter()
            .map(|m| m.id)
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(owner.app.session.composer.draft, draft);
    assert!(owner.app.dismissed(id));
}
#[test]
fn live_send_is_persisted_once_and_acceptance_dismisses_only_original_message() {
    let (dir, mut owner, a) = owner("live-send-once");
    seed(&mut owner, &a, &[1, 2]);
    let id = owner.app.session.messages[0].id;
    owner
        .actions(
            owner.revision,
            &[Action::Select {
                id,
                prepare_reply: true,
            }],
        )
        .unwrap();
    compose(&mut owner, Event::Body("A fictional reply".into()));
    compose(&mut owner, Event::Submit);
    assert!(matches!(
        owner.app.session.composer.send,
        SendState::Submitting(_)
    ));
    // A reader opens the other item while the frozen operation runs.
    owner.read(owner.app.session.messages[1].id).unwrap();
    drive(&mut owner, |o| {
        matches!(o.app.session.composer.send, SendState::Ready)
    });
    assert_eq!(scenarios().lock().unwrap()[&a.id].sends, 1);
    assert!(owner.app.dismissed(id));
    assert!(!owner.app.dismissed(owner.app.session.messages[1].id));
    assert_eq!(owner.app.session.composer.sent.len(), 1);
    drop(owner);
    let mut owner =
        Owner::open(Database::open_mode(dir.path(), &Settings::default(), true).unwrap())
            .unwrap()
            .with_transport(fake);
    owner.tick().unwrap();
    assert_eq!(scenarios().lock().unwrap()[&a.id].sends, 1);
}
#[test]
fn ambiguous_and_interrupted_sends_require_owner_resolution_and_never_retry() {
    let (dir, mut owner, a) = owner("live-uncertain");
    seed(&mut owner, &a, &[1]);
    let id = owner.app.session.messages[0].id;
    owner
        .actions(
            owner.revision,
            &[Action::Select {
                id,
                prepare_reply: true,
            }],
        )
        .unwrap();
    compose(&mut owner, Event::Body("Preserved uncertain text".into()));
    scenarios().lock().unwrap().get_mut(&a.id).unwrap().result = Some(SendResult::Uncertain);
    compose(&mut owner, Event::Submit);
    drive(&mut owner, |o| {
        matches!(o.app.session.composer.send, SendState::Uncertain(_))
    });
    let op = match &owner.app.session.composer.send {
        SendState::Uncertain(op) => op.operation,
        _ => unreachable!(),
    };
    assert!(!owner.app.session.composer.editable());
    assert!(
        owner
            .actions(owner.revision, &[Action::Reconcile { operation: op }])
            .is_err()
    );
    for _ in 0..10 {
        owner.tick().unwrap();
    }
    assert_eq!(scenarios().lock().unwrap()[&a.id].sends, 1);
    drop(owner);
    let mut owner =
        Owner::open(Database::open_mode(dir.path(), &Settings::default(), true).unwrap())
            .unwrap()
            .with_transport(fake);
    owner.tick().unwrap();
    assert_eq!(scenarios().lock().unwrap()[&a.id].sends, 1);
    owner
        .actions(
            owner.revision,
            &[Action::Resolve {
                expected: owner.app.session.composer.clone(),
                operation: op,
                accepted: false,
            }],
        )
        .unwrap();
    assert!(owner.app.session.composer.editable());
    assert!(!owner.app.dismissed(id));
    assert_eq!(
        owner.app.session.composer.draft.as_ref().unwrap().body,
        "Preserved uncertain text"
    );
    // Commit a new submission and crash before any network call.
    compose(&mut owner, Event::Submit);
    drop(owner);
    let mut owner =
        Owner::open(Database::open_mode(dir.path(), &Settings::default(), true).unwrap())
            .unwrap()
            .with_transport(fake);
    assert!(matches!(
        owner.app.session.composer.send,
        SendState::Uncertain(_)
    ));
    owner.tick().unwrap();
    assert_eq!(scenarios().lock().unwrap()[&a.id].sends, 1);
}
#[test]
fn account_identity_changes_reject_old_drafts_and_skip_old_remote_read_work() {
    let (_dir, mut owner, a) = owner("live-identity");
    seed(&mut owner, &a, &[1]);
    let id = owner.app.session.messages[0].id;
    owner
        .actions(
            owner.revision,
            &[Action::Select {
                id,
                prepare_reply: true,
            }],
        )
        .unwrap();
    compose(&mut owner, Event::Body("Do not redirect".into()));
    let mut settings = owner.app.settings.clone();
    settings.accounts[0].username = "replacement@example.invalid".into();
    owner
        .actions(
            owner.revision,
            &[Action::Configure {
                before: owner.app.settings.clone(),
                settings,
            }],
        )
        .unwrap();
    let action = Action::Compose {
        expected: owner.app.session.composer.clone(),
        event: Event::Submit,
        outcome: MockOutcome::Success,
    };
    assert!(owner.actions(owner.revision, &[action]).is_err());
    let revision = owner.revision;
    drive(&mut owner, |o| o.revision > revision);
    assert_eq!(scenarios().lock().unwrap()[&a.id].reads, 0);
    assert_eq!(scenarios().lock().unwrap()[&a.id].sends, 0);
    assert_eq!(
        owner.app.session.composer.draft.as_ref().unwrap().body,
        "Do not redirect"
    );
}
#[test]
fn live_mode_rejects_fixture_injection_and_metadata_only_attachments() {
    let (_dir, mut owner, a) = owner("live-no-fixtures");
    seed(&mut owner, &a, &[1]);
    assert!(owner.actions(owner.revision, &[Action::Incoming]).is_err());
    let action = Action::Compose {
        expected: owner.app.session.composer.clone(),
        event: Event::Attach(d_hub::domain::Attachment {
            name: "fake.png".into(),
            bytes: 5,
            mime: "image/png".into(),
        }),
        outcome: MockOutcome::Success,
    };
    assert!(owner.actions(owner.revision, &[action]).is_err());
}
#[test]
fn remote_registry_replays_uncommitted_imports_and_merges_partial_updates() {
    let dir = directory();
    let a = account("registry");
    let mut db = Database::open_mode(dir.path(), &Settings::default(), true).unwrap();
    let first = item(&a, 4);
    let id = db.store_remote(&first).unwrap();
    assert!(!db.imported(id).unwrap());
    drop(db);
    let mut db = Database::open_mode(dir.path(), &Settings::default(), true).unwrap();
    assert_eq!(db.store_remote(&first).unwrap(), id);
    let mut partial = first.clone();
    partial.partial = true;
    partial.body.clear();
    partial.recipients.clear();
    partial.unread = Some(false);
    db.store_remote(&partial).unwrap();
    assert_eq!(db.remote(id).unwrap().body, first.body);
    assert_eq!(db.remote(id).unwrap().unread, Some(false));
    let mut other = a.clone();
    other.username = "other@example.invalid".into();
    let second = item(&other, 4);
    assert_ne!(db.store_remote(&second).unwrap(), id);
}

#[test]
fn version_one_database_migrates_without_replacing_drafts_or_settings() {
    let dir = directory();
    let settings = Settings::default();
    let mut db = Database::open(dir.path(), &settings).unwrap();
    let (revision, session, settings) = db.load().unwrap();
    let mut app = d_hub::app::App::new(session, settings).unwrap();
    app.editor.insert("Retain this older profile draft");
    app.update_body();
    db.commit(revision, &app.session, &app.settings, "fixture")
        .unwrap();
    let expected = db.load().unwrap();
    drop(db);
    let connection = rusqlite::Connection::open(dir.path().join("service.sqlite3")).unwrap();
    connection
        .execute_batch("DROP TABLE profile; DROP TABLE remote_messages; PRAGMA user_version=1;")
        .unwrap();
    drop(connection);
    assert!(Database::open_mode(dir.path(), &Settings::default(), true).is_err());
    let connection = rusqlite::Connection::open(dir.path().join("service.sqlite3")).unwrap();
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    drop(connection);
    let db = Database::open(dir.path(), &Settings::default()).unwrap();
    let actual = db.load().unwrap();
    assert_eq!(expected.0, actual.0);
    assert_eq!(expected.1.composer, actual.1.composer);
    assert_eq!(expected.2, actual.2);
}

#[test]
fn discord_arrivals_edits_duplicates_deletions_and_replies_preserve_draft_and_arrival_order() {
    let dir = directory();
    let mut a = AccountConfig::new("discord-flow", Provider::Discord);
    a.discord_user_id = "100000000000000001".into();
    a.enabled = true;
    a.allow_send = true;
    let settings = Settings {
        accounts: vec![a.clone()],
        ..Settings::default()
    };
    let mut owner = Owner::open(Database::open_mode(dir.path(), &settings, true).unwrap())
        .unwrap()
        .with_transport(fake);
    let make = |id: &str, body: &str| Item {
        account_scope: providers::scope(&a),
        remote: RemoteMessage::DiscordDm {
            account: a.id.clone(),
            channel: "100000000000000003".into(),
            message: id.into(),
        },
        sender: "Fictional friend".into(),
        recipients: vec!["100000000000000002".into()],
        subject: "Direct message".into(),
        body: body.into(),
        timestamp: 0,
        unread: None,
        message_id: id.into(),
        references: String::new(),
        deleted: false,
        partial: false,
    };
    let first = make("100000000000000004", "First incoming DM");
    let second = make("100000000000000005", "Second incoming DM");
    let enqueue = |items: Vec<Item>| {
        scenarios()
            .lock()
            .unwrap()
            .entry(a.id.clone())
            .or_default()
            .batches
            .push_back(Batch {
                items,
                cursor: Value::Null,
                status: "Fictional Discord connected".into(),
            });
    };
    enqueue(vec![first.clone()]);
    drive(&mut owner, |o| o.app.session.messages.len() == 1);
    let id = owner.app.session.messages[0].id;
    let arrived = owner.app.session.messages[0].timestamp;
    assert!(arrived > 0);
    owner
        .actions(
            owner.revision,
            &[Action::Select {
                id,
                prepare_reply: true,
            }],
        )
        .unwrap();
    compose(
        &mut owner,
        Event::Body("Keep this Discord recipient 東京".into()),
    );
    let draft = owner.app.session.composer.draft.clone();
    assert!(
        owner.app.session.read_changes.is_empty(),
        "Discord seen markers are local"
    );
    let mut edit = first.clone();
    edit.partial = true;
    edit.body = "Edited incoming DM".into();
    enqueue(vec![first.clone(), edit, second]);
    owner.refresh();
    drive(&mut owner, |o| {
        o.app.session.messages.len() == 2
            && o.app
                .session
                .messages
                .iter()
                .any(|m| m.body == "Edited incoming DM")
    });
    assert_eq!(owner.app.session.composer.draft, draft);
    assert_eq!(
        owner
            .app
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .timestamp,
        arrived
    );
    assert_eq!(owner.app.visible()[0].body, "Second incoming DM");
    assert!(
        !owner
            .app
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .unread
    );
    let mut deletion = first.clone();
    deletion.partial = true;
    deletion.deleted = true;
    enqueue(vec![deletion]);
    owner.refresh();
    drive(&mut owner, |o| {
        o.app
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .target
            .is_none()
    });
    assert_eq!(owner.app.session.composer.draft, draft);
    // A deleted original cannot become an automatic reply to a newer arrival.
    assert!(
        owner
            .actions(
                owner.revision,
                &[Action::Compose {
                    expected: owner.app.session.composer.clone(),
                    event: Event::Submit,
                    outcome: MockOutcome::Success,
                }]
            )
            .is_err()
    );
    assert_eq!(owner.app.session.composer.draft, draft);
    let revision = owner.revision;
    enqueue(vec![first.clone()]);
    owner.refresh();
    drive(&mut owner, |o| o.revision > revision);
    assert!(
        owner
            .app
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .target
            .is_none()
    );
    let second_id = owner
        .app
        .session
        .messages
        .iter()
        .find(|m| m.id != id)
        .unwrap()
        .id;
    owner
        .actions(
            owner.revision,
            &[Action::Park {
                expected: owner.app.session.composer.clone(),
            }],
        )
        .unwrap();
    owner
        .actions(
            owner.revision,
            &[Action::Select {
                id: second_id,
                prepare_reply: true,
            }],
        )
        .unwrap();
    scenarios().lock().unwrap().get_mut(&a.id).unwrap().db =
        Some(dir.path().join("service.sqlite3"));
    compose(
        &mut owner,
        Event::Body("Reply only to the second DM".into()),
    );
    compose(&mut owner, Event::Submit);
    drive(&mut owner, |o| {
        !matches!(o.app.session.composer.send, SendState::Submitting(_))
    });
    assert!(owner.app.session.message_state[&second_id].dismissed);
    assert!(!owner.app.session.message_state[&id].dismissed);
    assert!(
        owner
            .app
            .session
            .saved
            .values()
            .any(|saved| Some(saved) == draft.as_ref())
    );
    let mut changed = a.clone();
    changed.discord_user_id = "100000000000000009".into();
    assert_ne!(providers::scope(&a), providers::scope(&changed));
    let mut mail = account("legacy-scope");
    mail.provider = Provider::ICloud;
    use sha2::{Digest, Sha256};
    let legacy = format!(
        "{}:{:x}",
        mail.id,
        Sha256::digest(
            serde_json::to_vec(&(
                mail.provider,
                &mail.username,
                &mail.tenant,
                &mail.client_id,
                &mail.resources
            ))
            .unwrap()
        )
    );
    assert_eq!(
        providers::scope(&mail),
        legacy,
        "existing mail cursor/cache identity must not change"
    );
}

#[test]
fn teams_edits_and_reactions_update_in_place_and_never_resurface_old_messages() {
    let dir = directory();
    let mut a = AccountConfig::new("teams-flow", Provider::Teams);
    a.username = "owner@example.invalid".into();
    a.resources = vec!["chat".into()];
    a.enabled = true;
    a.allow_send = true;
    let settings = Settings {
        accounts: vec![a.clone()],
        ..Settings::default()
    };
    let mut owner = Owner::open(Database::open_mode(dir.path(), &settings, true).unwrap())
        .unwrap()
        .with_transport(fake);
    let make = |id: &str, body: &str, timestamp: u64, partial: bool| Item {
        account_scope: providers::scope(&a),
        remote: RemoteMessage::GraphChat {
            account: a.id.clone(),
            chat: "chat".into(),
            message: id.into(),
        },
        sender: "Fictional colleague".into(),
        recipients: vec!["Teams chat chat".into()],
        subject: "Teams message".into(),
        body: body.into(),
        timestamp,
        unread: None,
        message_id: String::new(),
        references: String::new(),
        deleted: false,
        partial,
    };
    let enqueue = |items: Vec<Item>| {
        scenarios()
            .lock()
            .unwrap()
            .entry(a.id.clone())
            .or_default()
            .batches
            .push_back(Batch {
                items,
                cursor: Value::Null,
                status: "Fictional Teams connected".into(),
            });
    };
    enqueue(vec![make("first", "Lunch at noon?", 2_000, false)]);
    drive(&mut owner, |o| o.app.session.messages.len() == 1);
    let id = owner.app.session.messages[0].id;
    owner
        .actions(
            owner.revision,
            &[Action::Select {
                id,
                prepare_reply: true,
            }],
        )
        .unwrap();
    compose(&mut owner, Event::Body("Yes, see you there".into()));
    let draft = owner.app.session.composer.draft.clone();
    let arrivals = owner.app.new_items;
    // An edit to the captured message, a reaction to a month-old message never offered,
    // and a genuinely new message.
    enqueue(vec![
        make("first", "Lunch at 12:30?", 2_000, true),
        make("month-old", "Old announcement", 1_000, true),
        make("second", "Booked a table", 3_000, false),
    ]);
    owner.refresh();
    drive(&mut owner, |o| o.app.session.messages.len() == 2);
    let messages = &owner.app.session.messages;
    assert!(messages.iter().all(|m| m.body != "Old announcement"));
    let first = messages.iter().find(|m| m.id == id).unwrap();
    assert_eq!(first.body, "Lunch at 12:30?");
    assert_eq!(first.timestamp, 2_000);
    assert_eq!(owner.app.new_items, arrivals + 1);
    assert_eq!(owner.app.session.composer.draft, draft);
    // A deletion of the replied-to message keeps the draft and removes its reply target.
    let mut deleted = make("first", "", 2_000, true);
    deleted.deleted = true;
    enqueue(vec![deleted]);
    owner.refresh();
    drive(&mut owner, |o| {
        o.app
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .is_some_and(|m| m.target.is_none())
    });
    assert_eq!(owner.app.session.composer.draft, draft);
}
