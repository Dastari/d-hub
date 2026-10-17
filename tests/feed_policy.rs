use d_hub::{
    app::{App, Session},
    composer::{Event, SendState},
    config::Settings,
    decisions::{DecisionJob, DecisionTransport, evaluate},
    discord_notifications::{self, Notification},
    domain::{MessageId, Provider, SourceFilter},
    policy::{AccountConfig, DecisionProvider},
    read_state::{ReadTransport, RemoteMessage},
    storage::DemoStore,
};
use serde_json::{Value, json};

fn app() -> App {
    App::new(Session::default(), Settings::default()).unwrap()
}
fn job_app(provider: DecisionProvider) -> App {
    let mut app = app();
    let message = app
        .session
        .messages
        .iter()
        .find(|m| m.id == MessageId(5))
        .unwrap();
    let mut account = AccountConfig::new(message.account().unwrap(), message.provider().unwrap());
    account.allow_ai = true;
    app.settings.accounts.push(account);
    app.settings.ai.enabled = true;
    app.settings.ai.provider = provider;
    app.settings.ai.model = provider.default_model().into();
    app
}
fn job(app: &App) -> DecisionJob {
    DecisionJob::build(
        app.session
            .messages
            .iter()
            .find(|m| m.id == MessageId(5))
            .unwrap(),
        &app.settings,
    )
    .unwrap()
}
fn answers(job: &DecisionJob, spam: f64) -> Value {
    match job.provider {
        DecisionProvider::Jev => {
            json!({"answers": job.questions.keys().map(|name| (name.clone(), json!({"type":"noul", "noul":if name == "spam" {spam} else {0.9}}))).collect::<serde_json::Map<_,_>>()})
        }
        DecisionProvider::OpenAI => {
            json!({"answers":job.questions.keys().map(|name| json!({"name":name,"type":"predicate","probability":if name == "spam" {spam} else {0.9}})).collect::<Vec<_>>() })
        }
    }
}

#[test]
fn only_accepted_reply_dismisses_its_frozen_target() {
    let mut app = app();
    app.editor.insert("Reply to original recipient");
    app.update_body();
    let operation = app.submit().unwrap();
    app.select(MessageId(2));
    let draft = app.session.composer.draft.clone();
    app.session
        .composer
        .apply(Event::Failed(operation.operation, "offline".into()));
    assert!(!app.dismissed(MessageId(5)));
    assert_eq!(app.session.composer.draft, draft);
    let operation = app.submit().unwrap();
    app.session
        .composer
        .apply(Event::Uncertain(operation.operation));
    assert!(!app.dismissed(MessageId(5)));
    assert!(matches!(app.session.composer.send, SendState::Uncertain(_)));
    app.accepted(operation.operation + 1);
    assert!(!app.dismissed(MessageId(5)));
    app.accepted(operation.operation);
    assert!(app.dismissed(MessageId(5)));
    assert!(!app.dismissed(MessageId(2)));
    assert!(!app.visible().iter().any(|m| m.id == MessageId(5)));
    app.toggle_show_dismissed();
    app.select(MessageId(5));
    app.toggle_dismissed();
    assert!(!app.dismissed(MessageId(5)));
    assert!(app.visible().iter().any(|m| m.id == MessageId(5)));
}

#[test]
fn dismissal_and_seen_state_persist_without_redirecting_a_draft() {
    let mut app = app();
    app.attach();
    let draft = app.session.composer.draft.clone();
    app.select(MessageId(2));
    app.toggle_dismissed();
    assert_eq!(app.session.composer.draft, draft);
    assert!(
        !app.session
            .messages
            .iter()
            .find(|m| m.id == MessageId(2))
            .unwrap()
            .unread
    );
    let dir = tempfile::tempdir().unwrap();
    let store = DemoStore::open(dir.path().into()).unwrap();
    store.save(&app.session).unwrap();
    let mut reopened = App::new(store.load().unwrap().unwrap(), app.settings).unwrap();
    assert!(reopened.dismissed(MessageId(2)));
    assert_eq!(reopened.session.composer.draft, draft);
    reopened.toggle_show_dismissed();
    reopened.select(MessageId(2));
    reopened.toggle_dismissed();
    let reopened = App::new(reopened.session, reopened.settings).unwrap();
    assert!(!reopened.dismissed(MessageId(2)));
}

struct FailingRead;
impl ReadTransport for FailingRead {
    fn set_unread(&mut self, _: &RemoteMessage, _: bool) -> Result<(), String> {
        Err("temporary outage".into())
    }
}
#[test]
fn read_queue_coalesces_rejects_stale_ack_and_preserves_failures_across_restart() {
    let mut app = app();
    app.session
        .messages
        .iter_mut()
        .find(|m| m.id == MessageId(5))
        .unwrap()
        .remote = Some(RemoteMessage::Imap {
        account: "personal".into(),
        mailbox: "INBOX".into(),
        uid_validity: 7,
        uid: 55,
    });
    app.select(MessageId(5));
    let first = app.session.read_changes[&MessageId(5)].clone();
    app.set_unread(MessageId(5), true);
    let revision = app.session.read_changes[&MessageId(5)].revision;
    app.acknowledge_read(MessageId(5), first.revision, Ok(()));
    assert_eq!(app.session.read_changes[&MessageId(5)].revision, revision);
    app.receive_unread(MessageId(5), false);
    assert!(
        app.session
            .messages
            .iter()
            .find(|m| m.id == MessageId(5))
            .unwrap()
            .unread
    );
    app.sync_read_changes(&mut FailingRead);
    assert_eq!(
        app.session.read_changes[&MessageId(5)].error.as_deref(),
        Some("temporary outage")
    );
    let state = serde_json::to_vec(&app.session).unwrap();
    let mut app = App::new(serde_json::from_slice(&state).unwrap(), app.settings).unwrap();
    assert_eq!(app.session.read_changes.len(), 1);
    app.acknowledge_read(MessageId(5), revision, Ok(()));
    app.receive_unread(MessageId(5), false);
    assert!(app.session.read_changes.is_empty());
    assert!(
        !app.session
            .messages
            .iter()
            .find(|m| m.id == MessageId(5))
            .unwrap()
            .unread
    );
}

#[test]
fn old_snapshots_and_old_config_remain_readable() {
    let mut state = serde_json::to_value(Session::default()).unwrap();
    for key in ["message_state", "read_changes", "notification_ids"] {
        state.as_object_mut().unwrap().remove(key);
    }
    for message in state["messages"].as_array_mut().unwrap() {
        for key in ["remote", "provider", "account"] {
            message.as_object_mut().unwrap().remove(key);
        }
    }
    let settings: Settings = toml::from_str("schema_version = 1\nfocus = 'personal'\n").unwrap();
    let app = App::new(serde_json::from_value(state).unwrap(), settings).unwrap();
    assert_eq!(app.settings.profile_label(), "Home");
    assert_eq!(app.visible().len(), 2);
    assert!(
        app.session
            .messages
            .iter()
            .any(|m| m.provider() == Some(Provider::Discord))
    );
}

#[test]
fn profiles_and_global_spam_share_atomic_settings_and_preserve_drafts() {
    let mut app = app();
    app.attach();
    let draft = app.session.composer.draft.clone();
    app.settings.add_profile("family").unwrap();
    app.settings
        .set(
            "profiles.family.prompt",
            "Is this about my family?\nInclude personal DMs.",
        )
        .unwrap();
    app.settings
        .set("profiles.family.terms", "Kai\nMira")
        .unwrap();
    app.set_profile("family");
    assert_eq!(
        app.visible().iter().map(|m| m.id).collect::<Vec<_>>(),
        [MessageId(5), MessageId(1)]
    );
    app.settings.set("spam.local_terms", "Kai").unwrap();
    app.refresh_filters();
    assert_eq!(
        app.visible().iter().map(|m| m.id).collect::<Vec<_>>(),
        [MessageId(1)]
    );
    app.settings.show_spam = true;
    assert_eq!(app.visible().len(), 2);
    assert_eq!(app.session.composer.draft, draft);
    let before = app.settings.clone();
    assert!(app.settings.set("ai.threshold", "NaN").is_err());
    assert_eq!(app.settings, before);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    app.settings.save(&path).unwrap();
    let settings = Settings::load(&path).unwrap();
    assert_eq!(settings, app.settings);
    for row in settings.entries() {
        assert_eq!(settings.value(&row.name).unwrap(), row.value);
    }
    app.settings.remove_profile("family").unwrap();
    assert_eq!(app.settings.focus, "all");
}

struct FakeDecision(Value);
impl DecisionTransport for FakeDecision {
    fn evaluate(&self, _: &DecisionJob) -> anyhow::Result<Value> {
        Ok(self.0.clone())
    }
}
#[test]
fn both_decision_contracts_are_typed_and_stale_results_cannot_filter_or_redirect() {
    for provider in [DecisionProvider::Jev, DecisionProvider::OpenAI] {
        let mut app = job_app(provider);
        app.attach();
        let draft = app.session.composer.draft.clone();
        let job = job(&app);
        let request = job.request();
        assert!(job.endpoint().starts_with("https://"));
        assert!(request.get("tools").is_none());
        match provider {
            DecisionProvider::Jev => {
                assert_eq!(request["questions"]["spam"]["type"], "noul");
                assert!(request["state"].is_string());
            }
            DecisionProvider::OpenAI => {
                assert!(request["input"].is_string());
                assert!(request["questions"].is_array());
            }
        }
        let result = evaluate(&job, &FakeDecision(answers(&job, 0.99))).unwrap();
        assert!(app.apply_classification(MessageId(5), result.clone()));
        assert!(!app.visible().iter().any(|m| m.id == MessageId(5)));
        assert_eq!(app.session.composer.draft, draft);
        app.settings
            .set("spam.prompt", "Exclude only unsolicited sales.")
            .unwrap();
        assert!(!app.apply_classification(MessageId(5), result));
        assert!(app.visible().iter().any(|m| m.id == MessageId(5)));
        for invalid in [
            json!({}),
            json!({"answers":[]}),
            json!({"answers":{"spam":{"type":"noul","noul":1.5}}}),
        ] {
            assert!(job.parse(&invalid).is_err());
        }
    }
}

#[test]
fn ai_requires_global_and_account_consent_and_failure_releases_pending_item() {
    let mut app = app();
    assert!(DecisionJob::build(&app.session.messages[0], &app.settings).is_err());
    let mut configured = job_app(DecisionProvider::Jev);
    configured.settings.accounts[0].allow_ai = false;
    assert!(DecisionJob::build(&configured.session.messages[0], &configured.settings).is_err());
    app = job_app(DecisionProvider::Jev);
    app.settings.ai.hold_pending = true;
    app.live_feed = true;
    let job = job(&app);
    let id = job.message;
    assert!(!app.visible().iter().any(|m| m.id == id));
    for message in &app.session.messages {
        if let Ok(job) = DecisionJob::build(message, &app.settings) {
            app.decision_attempted.insert(job.fingerprint);
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(Err("synthetic timeout".into())).unwrap();
    app.decision_attempted.insert(job.fingerprint.clone());
    app.decision_job = Some((id, job.fingerprint, rx));
    assert!(app.poll_background());
    assert!(app.visible().iter().any(|m| m.id == id));
    assert!(app.decision_job.is_none());
}

fn notification(serial: usize, sender: &str) -> Notification {
    Notification {
        event_id: format!("fake-bus:{serial}"),
        app: "Discord".into(),
        summary: sender.into(),
        body: "Hello from a fictional contact.\u{1b}[31m".into(),
    }
}
#[test]
fn discord_personal_requirement_is_not_silently_substituted_and_notifications_are_read_only() {
    let account = AccountConfig::new("discord-home", Provider::Discord);
    assert_eq!(account.discord_mode, "personal");
    assert!(!account.authentication_supported());
    assert!(!discord_notifications::matches(
        &account,
        &notification(1, "Fictional Son")
    ));
    let mut settings = Settings::default();
    settings.accounts.push(account);
    settings
        .set("accounts.discord-home.discord_mode", "notifications")
        .unwrap();
    settings
        .set("accounts.discord-home.notification_sender", "Fictional Son")
        .unwrap();
    let mut app = App::new(
        Session {
            messages: vec![],
            ..Session::default()
        },
        settings,
    )
    .unwrap();
    assert!(!app.ingest_notification(&notification(1, "Someone Else")));
    for serial in 1..=3 {
        assert!(app.ingest_notification(&notification(serial, "Fictional Son")));
    }
    assert!(!app.ingest_notification(&notification(1, "Fictional Son")));
    app.set_source(SourceFilter::Discord);
    let frame = d_hub::tui::snapshot(&mut app, 100, 50).unwrap();
    assert!(frame.contains("Discord notification"));
    assert!(!frame.contains("Discord bot"));
    let messages = app.visible();
    assert_eq!(messages.len(), 3);
    assert!(messages[0].id > messages[1].id && messages[1].id > messages[2].id);
    assert!(
        messages
            .iter()
            .all(|m| m.target.is_none() && m.remote.is_none() && !m.body.contains('\u{1b}'))
    );
    let id = messages[0].id;
    app.select(id);
    app.open_reader();
    assert!(app.reader.is_some());
    assert!(app.session.read_changes.is_empty());
    assert!(app.submit().is_none());
    let reopened = App::new(app.session, app.settings).unwrap();
    assert!(
        !reopened
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .unread
    );
}

#[test]
fn disconnected_workers_leave_usable_content_and_report_capture_loss() {
    let mut app = job_app(DecisionProvider::Jev);
    app.settings.ai.hold_pending = true;
    app.live_feed = true;
    let pending = job(&app);
    for message in &app.session.messages {
        if let Ok(job) = DecisionJob::build(message, &app.settings) {
            app.decision_attempted.insert(job.fingerprint);
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    drop(tx);
    app.decision_job = Some((pending.message, pending.fingerprint, rx));
    assert!(app.poll_background());
    assert!(app.visible().iter().any(|m| m.id == MessageId(5)));
    let (tx, rx) = std::sync::mpsc::channel();
    drop(tx);
    app.notifications = Some(rx);
    app.bridge_ready = true;
    assert!(app.poll_background());
    assert!(!app.bridge_ready);
    assert!(app.notifications.is_none());
    assert!(app.notice.contains("disconnected"));
}
