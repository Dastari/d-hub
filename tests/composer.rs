use d_hub::{
    app::{App, Session},
    composer::{Composer, Event, SendState},
    config::Settings,
    domain::{Focus, MessageId},
    fixtures,
    storage::DemoStore,
};

fn app() -> App {
    App::new(Session::default(), Settings::default()).unwrap()
}
fn write(app: &mut App, body: &str) {
    app.editor.insert(body);
    app.update_body();
}

#[test]
fn incoming_follows_only_an_inactive_empty_drawer() {
    let mut app = app();
    let original = app.session.composer.reply_target().unwrap().clone();
    app.arrive();
    assert_eq!(app.session.composer.reply_target(), Some(&original));
    assert_eq!(app.new_items, 1);
    app.focus(false);
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(101)
    );
    app.arrive();
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(102)
    );
    let ready = app.session.composer.reply_target().cloned();
    app.focus(true);
    assert_eq!(app.session.composer.reply_target().cloned(), ready);
}

#[test]
fn explicit_selection_changes_an_empty_target() {
    let mut app = app();
    app.select(MessageId(2));
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(2)
    );
}

#[test]
fn every_background_and_browsing_event_preserves_a_locked_target() {
    for attachment_only in [true, false] {
        let mut app = app();
        if attachment_only {
            app.attach();
        } else {
            write(&mut app, "Draft for Kai 👩🏽‍💻");
        }
        let draft = app.session.composer.draft.clone();
        app.select(MessageId(3));
        app.set_focus(Focus::Work);
        app.arrive();
        app.focus(false);
        app.arrive();
        app.focus(true);
        app.open_reader();
        assert_eq!(app.session.composer.draft, draft);
        assert_eq!(app.selected, Some(MessageId(3)));
        assert_eq!(app.reader.unwrap().id, MessageId(3));
    }
}

#[test]
fn deleting_all_content_does_not_release_the_lock() {
    let mut app = app();
    write(&mut app, "Keep this target");
    app.attach();
    let target = app.session.composer.reply_target().cloned();
    app.session.composer.apply(Event::Body(String::new()));
    app.session.composer.apply(Event::RemoveAttachment(0));
    app.select(MessageId(2));
    app.focus(false);
    app.arrive();
    assert!(app.session.composer.draft.is_some());
    assert_eq!(app.session.composer.reply_target().cloned(), target);
    assert!(app.session.composer.apply(Event::Submit).is_none());
}

#[test]
fn send_freezes_the_original_account_recipient_and_revision() {
    let mut app = app();
    write(&mut app, "hello");
    let original = app.session.composer.draft.clone().unwrap();
    app.select(MessageId(4));
    let operation = app.session.composer.apply(Event::Submit).unwrap();
    assert_eq!(operation.draft, original);
    assert_eq!(operation.draft.target.recipients, ["kai@example.invalid"]);
    assert_eq!(operation.draft.target.account, "demo-personal");
    app.session
        .composer
        .apply(Event::Body("changed during send".into()));
    app.session
        .composer
        .apply(Event::Attach(fixtures::attachment()));
    assert_eq!(app.session.composer.draft.as_ref(), Some(&original));
    assert!(app.session.composer.apply(Event::Submit).is_none());
    app.session
        .composer
        .apply(Event::Accepted(operation.operation + 1));
    assert!(matches!(
        app.session.composer.send,
        SendState::Submitting(_)
    ));
    app.session
        .composer
        .apply(Event::Accepted(operation.operation));
    assert!(app.session.composer.draft.is_none());
    assert_eq!(app.session.composer.sent, [operation]);
}

#[test]
fn success_stays_on_sent_context_until_focus_is_lost() {
    let mut app = app();
    write(&mut app, "hello");
    let operation = app.session.composer.apply(Event::Submit).unwrap();
    app.arrive();
    app.session
        .composer
        .apply(Event::Accepted(operation.operation));
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(5)
    );
    app.focus(false);
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(101)
    );
}

#[test]
fn failure_restores_editability_and_preserves_body_and_attachments() {
    let mut app = app();
    write(&mut app, "do not lose this");
    app.attach();
    let draft = app.session.composer.draft.clone();
    let operation = app.session.composer.apply(Event::Submit).unwrap();
    app.session
        .composer
        .apply(Event::Failed(operation.operation + 1, "stale error".into()));
    assert!(!app.session.composer.editable());
    app.session
        .composer
        .apply(Event::Failed(operation.operation, "offline".into()));
    assert!(app.session.composer.editable());
    assert_eq!(app.session.composer.draft, draft);
    app.select(MessageId(4));
    assert_eq!(app.session.composer.draft, draft);
}

#[test]
fn uncertain_and_interrupted_sends_are_never_blindly_retried() {
    let mut app = app();
    write(&mut app, "maybe accepted");
    let operation = app.session.composer.apply(Event::Submit).unwrap();
    let bytes = serde_json::to_vec(&app.session).unwrap();
    let mut reopened =
        App::new(serde_json::from_slice(&bytes).unwrap(), Settings::default()).unwrap();
    assert!(matches!(
        reopened.session.composer.send,
        SendState::Uncertain(_)
    ));
    assert!(reopened.session.composer.apply(Event::Submit).is_none());
    reopened.session.composer.apply(Event::Discard(None));
    assert!(reopened.session.composer.draft.is_some());
    reopened
        .session
        .composer
        .apply(Event::Accepted(operation.operation));
    assert_eq!(reopened.session.composer.sent.len(), 1);
    reopened
        .session
        .composer
        .apply(Event::Accepted(operation.operation));
    assert_eq!(reopened.session.composer.sent.len(), 1);
}

#[test]
fn save_switch_restores_each_conversations_own_draft() {
    let mut app = app();
    write(&mut app, "Kai's draft");
    app.attach();
    let kai = app.session.composer.draft.clone();
    app.select(MessageId(4));
    app.save_switch();
    write(&mut app, "Sasha's draft");
    let sasha = app.session.composer.draft.clone();
    app.select(MessageId(5));
    app.save_switch();
    assert_eq!(app.session.composer.draft, kai);
    app.select(MessageId(4));
    app.save_switch();
    assert_eq!(app.session.composer.draft, sasha);
}

#[test]
fn no_eligible_message_means_no_hidden_recipient() {
    let mut session = Session::default();
    for message in &mut session.messages {
        message.target = None;
    }
    let mut app = App::new(session, Settings::default()).unwrap();
    assert!(app.session.composer.reply_target().is_none());
    write(&mut app, "cannot send");
    app.attach();
    assert!(app.session.composer.draft.is_none());
    assert!(app.session.composer.apply(Event::Submit).is_none());
}

#[test]
fn focus_change_drops_a_hidden_empty_target_for_the_selected_visible_message() {
    let mut app = app();
    let hidden = app.session.composer.reply_target().unwrap().incoming;
    app.set_focus(Focus::Work);
    assert!(app.visible().iter().all(|m| m.id != hidden));
    // The empty reply follows the highlighted message, never one the focus hides.
    let selected = app.selected_message().and_then(|m| m.target.as_ref());
    assert!(selected.is_some());
    assert_eq!(app.session.composer.reply_target(), selected);
    app.focus(false);
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(4)
    );
}

#[test]
fn old_import_and_outgoing_messages_cannot_displace_latest_context() {
    let mut app = app();
    app.focus(false);
    let target = app.session.composer.reply_target().cloned();
    let mut message = fixtures::messages().remove(0);
    message.timestamp = 1;
    app.session.messages.push(message);
    let mut outgoing = fixtures::arrival(10);
    outgoing.incoming = false;
    app.session.messages.push(outgoing);
    app.follow_latest();
    assert_eq!(app.session.composer.reply_target().cloned(), target);
}

#[test]
fn explicit_discard_can_release_even_an_empty_locked_draft() {
    let mut app = app();
    app.attach();
    app.session.composer.apply(Event::RemoveAttachment(0));
    app.select(MessageId(4));
    app.discard();
    assert!(app.session.composer.draft.is_none());
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(4)
    );
}

#[test]
fn draft_persistence_and_single_writer_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let store = DemoStore::open(dir.path().to_owned()).unwrap();
    let mut app = app();
    write(&mut app, "Persistent 東京\nsecond line");
    app.attach();
    store.save(&app.session).unwrap();
    assert!(DemoStore::open(dir.path().to_owned()).is_err());
    let session = store.load::<Session>().unwrap().unwrap();
    let original = session.composer.clone();
    drop(store);
    let reopened = DemoStore::open(dir.path().to_owned()).unwrap();
    assert_eq!(
        reopened.load::<Session>().unwrap().unwrap().composer,
        original
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(dir.path().join("demo-state.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn malformed_state_is_rejected_without_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo-state.json");
    std::fs::write(&path, "{broken").unwrap();
    let store = DemoStore::open(dir.path().to_owned()).unwrap();
    assert!(store.load::<Session>().is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "{broken");
    assert!(
        App::new(
            Session {
                schema_version: 900,
                ..Session::default()
            },
            Settings::default()
        )
        .is_err()
    );
}

#[test]
fn target_tuple_keys_distinguish_accounts_and_delimiters() {
    let a = fixtures::messages().remove(0).target.unwrap();
    let mut b = a.clone();
    b.account = "another-account".into();
    assert_ne!(a.draft_key(), b.draft_key());
    let mut a = a;
    a.account = "a:b".into();
    a.conversation = "c".into();
    b.account = "a".into();
    b.conversation = "b:c".into();
    assert_ne!(a.draft_key(), b.draft_key());
}

#[test]
fn missing_source_does_not_retarget_a_draft() {
    let mut app = app();
    write(&mut app, "keep this");
    let draft = app.session.composer.draft.clone();
    app.session.messages.retain(|m| m.id != MessageId(5));
    app.focus(false);
    app.arrive();
    assert_eq!(app.session.composer.draft, draft);
    assert!(app.submit().is_none());
    assert_eq!(app.session.composer.draft, draft);
}

#[test]
fn focused_arrival_preserves_the_visible_feed_anchor() {
    let mut app = app();
    app.feed_offset = 1;
    let anchor = app.visible()[app.feed_offset].id;
    app.arrive();
    assert_eq!(app.visible()[app.feed_offset].id, anchor);
}

#[test]
fn following_a_conversation_with_saved_work_cannot_overwrite_it() {
    let mut app = app();
    app.settings.reply_first = false;
    write(&mut app, "saved Kai reply");
    let saved = app.session.composer.draft.clone().unwrap();
    app.select(MessageId(4));
    app.save_switch();
    app.focus(false);
    app.focus(true);
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(5)
    );
    write(&mut app, "replacement");
    app.attach();
    assert!(app.session.composer.draft.is_none());
    assert_eq!(
        app.session.saved.get(&saved.target.draft_key()),
        Some(&saved)
    );
    app.select(MessageId(5));
    assert_eq!(app.session.composer.draft.as_ref(), Some(&saved));
}

#[test]
fn edits_and_target_selection_interleave_without_redirecting() {
    let targets: Vec<_> = fixtures::messages().into_iter().map(|m| m.target).collect();
    for initial in &targets {
        for selected in &targets {
            let mut composer = Composer::default();
            composer.apply(Event::Select(initial.clone()));
            composer.apply(Event::Body("reply".into()));
            composer.apply(Event::Select(selected.clone()));
            composer.apply(Event::Body(String::new()));
            composer.apply(Event::Select(selected.clone()));
            assert_eq!(composer.reply_target(), initial.as_ref());
        }
    }
}

/// Applies the drawer's recorded actions to an owner, as the service does, and checks both
/// arrive at the same composer. Settings changes travel as Configure, as the IPC link sends them.
fn replay(client: &mut App, owner: &mut App, before: &Settings) {
    for action in std::mem::take(&mut client.actions) {
        d_hub::service::apply(owner, &action).unwrap();
    }
    if &client.settings != before {
        d_hub::service::apply(
            owner,
            &d_hub::service::Action::Configure {
                before: before.clone(),
                settings: client.settings.clone(),
            },
        )
        .unwrap();
    }
    assert_eq!(owner.session.composer, client.session.composer);
    assert_eq!(owner.selected, client.selected);
}

#[test]
fn explicit_filter_changes_release_an_emptied_draft_and_follow_what_is_shown() {
    let mut client = app();
    let mut owner = app();
    client.shared = true;
    write(&mut client, "x");
    client.editor = d_hub::editor::Editor::default();
    client.update_body();
    let before = client.settings.clone();
    replay(&mut client, &mut owner, &before);
    let emptied = client.session.composer.draft.clone();
    assert!(
        emptied.as_ref().is_some_and(|d| d.is_empty()),
        "emptying keeps the lock"
    );
    // Background arrivals never release it.
    client.arrive();
    replay(&mut client, &mut owner, &before);
    assert_eq!(client.session.composer.draft, emptied);
    // Dismissing everything (explicitly) releases it and leaves no hidden recipient.
    while let Some(id) = client.visible().first().map(|m| m.id) {
        client.selected = Some(id);
        owner.selected = Some(id);
        client.toggle_dismissed();
        replay(&mut client, &mut owner, &before);
    }
    assert!(client.session.composer.draft.is_none());
    assert!(client.session.composer.reply_target().is_none());
    // Showing dismissed replies to the selected visible message, not an old one.
    let before = client.settings.clone();
    client.toggle_show_dismissed();
    replay(&mut client, &mut owner, &before);
    let selected = client.selected_message().unwrap();
    assert_eq!(
        client.session.composer.reply_target(),
        selected.target.as_ref()
    );
    // A draft with content is never released or moved by filter changes.
    write(&mut client, "keep this");
    let draft = client.session.composer.draft.clone();
    let before = client.settings.clone();
    replay(&mut client, &mut owner, &before);
    client.toggle_show_dismissed();
    replay(&mut client, &mut owner, &before);
    assert_eq!(client.session.composer.draft, draft);
}
