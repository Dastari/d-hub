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
                // Structured state, and the for-you questions carry their yes/no boundary.
                assert_eq!(request["state"]["communication"]["from"], "Kai");
                let person = &request["questions"]["attention__person"];
                assert_eq!(person["type"], "noul");
                assert!(person["criteria"]["true"].is_string());
                assert!(person["criteria"]["false"].is_string());
                assert!(
                    request["questions"]["attention__for_you"]["instructions"]
                        .as_str()
                        .unwrap()
                        .contains("`owner`")
                );
            }
            DecisionProvider::OpenAI => {
                assert!(request["input"].is_string());
                assert!(request["questions"].is_array());
                let for_you = request["questions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|q| q["name"] == "attention__for_you")
                    .unwrap();
                assert!(
                    for_you["instructions"]
                        .as_str()
                        .unwrap()
                        .contains("No: A broadcast")
                );
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
    assert!(account.authentication_supported());
    assert!(account.auth_description().contains("QR sign-in"));
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

/// A fictional live-style email with a reply address, so filtering applies as it would live.
fn mail(app: &mut App, id: u64, reply_to: &str) -> MessageId {
    let mut message = app
        .session
        .messages
        .iter()
        .find(|m| m.id == MessageId(5))
        .unwrap()
        .clone();
    message.id = MessageId(id);
    message.timestamp = 5_000 + id;
    message.sender = format!("Sender {id} <{reply_to}>");
    let target = message.target.as_mut().unwrap();
    target.incoming = message.id;
    target.conversation = format!("thread-{id}");
    target.recipients = vec![reply_to.into()];
    app.session.messages.push(message);
    MessageId(id)
}

#[test]
fn no_reply_and_automated_mail_is_hidden_without_ai_and_shown_again_with_s() {
    let mut app = app();
    let noreply = mail(&mut app, 40, "no-reply@example.invalid");
    let donot = mail(&mut app, 41, "DoNotReply@example.invalid");
    let person = mail(&mut app, 42, "mira@example.invalid");
    // From no-reply with a reply address elsewhere (as Apple's account mail does).
    let apple = mail(&mut app, 44, "appleid_x@example.invalid");
    app.session.messages.last_mut().unwrap().sender = "Apple <noreply@example.invalid>".into();
    let bot = mail(&mut app, 43, "updates@example.invalid");
    app.session.messages.last_mut().unwrap().signals.automated = true;
    let visible = |app: &App, id| app.visible().iter().any(|m| m.id == id);
    for hidden in [noreply, donot, apple, bot] {
        assert!(!visible(&app, hidden));
    }
    assert!(visible(&app, person));
    assert_eq!(app.filtered_count(), 4);
    let frame = d_hub::tui::snapshot(&mut app, 120, 50).unwrap();
    assert!(frame.contains("4 hidden"), "{frame}");
    // A hidden message never becomes the automatic reply target.
    assert!(
        app.session
            .composer
            .reply_target()
            .is_none_or(|t| ![noreply, donot, apple, bot].contains(&t.incoming))
    );
    // S shows them, labelled with the reason.
    app.settings.show_spam = true;
    app.refresh_filters();
    assert!(visible(&app, noreply));
    let frame = d_hub::tui::snapshot(&mut app, 120, 60).unwrap();
    assert!(
        frame.contains("no-reply") && frame.contains("automated"),
        "{frame}"
    );
    // The rule has its own setting, through the shared schema.
    app.settings.show_spam = false;
    app.settings.set("spam.no_reply", "false").unwrap();
    app.refresh_filters();
    assert!(visible(&app, noreply) && visible(&app, bot));
}

#[test]
fn decisions_hide_what_a_person_did_not_write_to_you_and_never_hide_unanswered_mail() {
    let mut app = job_app(DecisionProvider::Jev);
    let job = job(&app);
    let answer = |person: f64, for_you: f64, spam: f64| {
        json!({"answers": job.questions.keys().map(|name| (name.clone(), json!({"type":"noul", "noul": match name.as_str() {
            "spam" => spam, "attention__person" => person, "attention__for_you" => for_you, _ => 0.9
        }}))).collect::<serde_json::Map<_,_>>()})
    };
    let visible = |app: &App| app.visible().iter().any(|m| m.id == MessageId(5));
    assert!(visible(&app), "pending decisions do not hide mail");
    for (person, for_you, spam, shown, reason) in [
        (0.95, 0.9, 0.05, true, None),
        (0.10, 0.9, 0.05, false, Some("not from a person")),
        (0.95, 0.10, 0.05, false, Some("not for you")),
        (0.95, 0.9, 0.95, false, Some("promotional")),
        // Uncertain answers lean towards showing.
        (0.40, 0.40, 0.50, true, None),
    ] {
        let result = evaluate(&job, &FakeDecision(answer(person, for_you, spam))).unwrap();
        assert_eq!(
            (result.person, result.for_you),
            (Some(person), Some(for_you))
        );
        assert!(app.apply_classification(MessageId(5), result));
        assert_eq!(visible(&app), shown, "{person} {for_you} {spam}");
        let message = app
            .session
            .messages
            .iter()
            .find(|m| m.id == MessageId(5))
            .unwrap();
        assert_eq!(app.filtered(message).map(|f| f.label()), reason);
    }
    // Turning the for-you questions off stops asking them; a promotion answer still counts.
    app.settings.set("spam.for_you", "false").unwrap();
    let job = DecisionJob::build(
        app.session
            .messages
            .iter()
            .find(|m| m.id == MessageId(5))
            .unwrap(),
        &app.settings,
    )
    .unwrap();
    assert!(!job.questions.contains_key("attention__person"));
    assert!(!job.questions.contains_key("attention__for_you"));
    let result = evaluate(&job, &FakeDecision(answers(&job, 0.1))).unwrap();
    assert_eq!((result.person, result.for_you), (None, None));
    assert!(app.apply_classification(MessageId(5), result));
    assert!(visible(&app));
    // Out-of-range answers are rejected.
    let mut bad = evaluate(&job, &FakeDecision(answers(&job, 0.1))).unwrap();
    bad.for_you = Some(1.5);
    assert!(!app.apply_classification(MessageId(5), bad));
}

#[test]
fn decision_state_carries_addressing_and_a_bounded_body_and_skips_locally_hidden_mail() {
    let mut app = job_app(DecisionProvider::Jev);
    app.settings.accounts[0].username = "owner@example.invalid".into();
    {
        let message = app
            .session
            .messages
            .iter_mut()
            .find(|m| m.id == MessageId(5))
            .unwrap();
        message.body = "x".repeat(20_000);
        message.signals = d_hub::domain::Signals {
            automated: false,
            bulk: true,
            addressed: d_hub::domain::Addressed::NotListed,
            audience: Some(400),
        };
    }
    let request = job(&app).request();
    let state = &request["state"];
    assert_eq!(state["owner"]["address"], "owner@example.invalid");
    assert_eq!(state["communication"]["mailing_list_or_bulk_headers"], true);
    assert_eq!(state["communication"]["visible_recipients"], 400);
    assert!(
        state["communication"]["addressing"]
            .as_str()
            .unwrap()
            .contains("not in To or Cc")
    );
    assert_eq!(
        state["communication"]["body"].as_str().unwrap().len(),
        4_000
    );
    // No request is spent on mail the local rules already hide.
    let message = app
        .session
        .messages
        .iter_mut()
        .find(|m| m.id == MessageId(5))
        .unwrap();
    message.target.as_mut().unwrap().recipients = vec!["noreply@example.invalid".into()];
    app.live_service = true;
    app.poll_background();
    assert!(
        app.decision_job
            .as_ref()
            .is_none_or(|(id, _, _)| *id != MessageId(5))
    );
}

#[test]
fn for_you_questions_and_about_me_are_owner_editable_and_invalidate_old_answers() {
    let mut app = job_app(DecisionProvider::Jev);
    let old = evaluate(&job(&app), &FakeDecision(answers(&job(&app), 0.1))).unwrap();
    assert!(
        job(&app).request()["state"]["owner"]["about"].is_null(),
        "blank About me sends nothing"
    );
    app.settings
        .set(
            "ai.about",
            "Toby. Work: Gema. Groups: family chat, graphql-orm maintainers.",
        )
        .unwrap();
    app.settings
        .set(
            "spam.for_you_prompt",
            "Would `owner` want to read or answer `communication` personally?",
        )
        .unwrap();
    app.settings.set("spam.for_you_no", "").unwrap();
    let request = job(&app).request();
    assert_eq!(
        request["state"]["owner"]["about"],
        "Toby. Work: Gema. Groups: family chat, graphql-orm maintainers."
    );
    let for_you = &request["questions"]["attention__for_you"];
    assert!(
        for_you["instructions"]
            .as_str()
            .unwrap()
            .ends_with("Would `owner` want to read or answer `communication` personally?")
    );
    // A blank description is left out; the other one stays.
    assert!(for_you["criteria"]["true"].is_string());
    assert!(for_you["criteria"].get("false").is_none());
    // Answers to the old wording no longer apply.
    assert!(!app.apply_classification(MessageId(5), old));
    // OpenAI predicates carry the same wording inline.
    app.settings.set("ai.provider", "openai").unwrap();
    let request = job(&app).request();
    let question = request["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["name"] == "attention__for_you")
        .unwrap();
    let text = question["instructions"].as_str().unwrap();
    assert!(
        text.contains("personally? Yes: ") && !text.contains("No: "),
        "{text}"
    );
    // Shared validation: questions stay nonempty and descriptions bounded.
    assert!(app.settings.set("spam.person_prompt", "  ").is_err());
    assert!(app.settings.set("ai.about", &"x".repeat(2001)).is_err());
    assert!(
        app.settings
            .set("spam.person_yes", &"x".repeat(2001))
            .is_err()
    );
}
