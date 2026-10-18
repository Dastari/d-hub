use d_hub::{
    app::{App, Mode, Session},
    composer::{Event, SendState},
    config::Settings,
    domain::{MessageId, SourceFilter},
};
fn app() -> App {
    App::new(Session::default(), Settings::default()).unwrap()
}
fn write(app: &mut App, text: &str) {
    app.editor.insert(text);
    app.update_body();
}

#[test]
fn message_navigation_saves_each_draft_and_keeps_input_ready() {
    let mut app = app();
    write(&mut app, "Reply for Kai 東京");
    app.attach();
    let first = app.session.composer.draft.clone();
    app.move_reply(1, 4);
    assert_eq!(app.mode, Mode::Insert);
    assert_eq!(app.selected, Some(MessageId(4)));
    assert!(app.editor.text.is_empty());
    write(&mut app, "Different reply for Sasha");
    let second = app.session.composer.draft.clone();
    app.move_reply(-1, 4);
    assert_eq!(app.session.composer.draft, first);
    app.move_reply(1, 4);
    assert_eq!(app.session.composer.draft, second);
    let mut reopened = App::new(app.session, app.settings).unwrap();
    reopened.switch_reply(MessageId(5));
    assert_eq!(reopened.session.composer.draft, first);
}

#[test]
fn separate_incoming_messages_in_one_conversation_keep_separate_replies() {
    let mut app = app();
    let mut next = app
        .session
        .messages
        .iter()
        .find(|m| m.id == MessageId(5))
        .unwrap()
        .clone();
    next.id = MessageId(6);
    next.timestamp += 1;
    next.target.as_mut().unwrap().incoming = next.id;
    app.session.messages.push(next);
    write(&mut app, "Answer to the earlier question");
    let first = app.session.composer.draft.clone();
    app.switch_reply(MessageId(6));
    write(&mut app, "Answer to the later question");
    let second = app.session.composer.draft.clone();
    assert_ne!(
        first.as_ref().unwrap().target.draft_key(),
        second.as_ref().unwrap().target.draft_key()
    );
    app.switch_reply(MessageId(5));
    assert_eq!(app.session.composer.draft, first);
    app.switch_reply(MessageId(6));
    assert_eq!(app.session.composer.draft, second);
}

#[test]
fn source_navigation_parks_and_restores_replies_without_hidden_recipients() {
    let mut app = app();
    write(&mut app, "Home reply");
    let draft = app.session.composer.draft.clone();
    app.cycle_reply_source(false);
    assert_eq!(app.settings.source, SourceFilter::ICloud);
    assert_eq!(app.session.composer.draft, draft);
    app.cycle_reply_source(false);
    assert_eq!(app.settings.source, SourceFilter::Discord);
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(2)
    );
    write(&mut app, "Discord reply");
    app.cycle_reply_source(true);
    assert_eq!(app.session.composer.draft, draft);
    app.settings.set("focus", "personal").unwrap();
    app.cycle_reply_source(false);
    assert!(app.visible().is_empty());
    assert!(app.session.composer.reply_target().is_none());
    assert!(app.editor.text.is_empty());
    app.cycle_reply_source(true);
    assert_eq!(app.session.composer.draft, draft);
}

#[test]
fn pending_send_blocks_switching_and_background_events_cannot_redirect() {
    let mut app = app();
    write(&mut app, "Frozen reply");
    let op = app.submit().unwrap();
    let draft = app.session.composer.draft.clone();
    let source = app.settings.source;
    app.move_reply(1, 4);
    app.cycle_reply_source(false);
    assert_eq!(app.settings.source, source);
    assert_eq!(app.session.composer.draft, draft);
    assert_eq!(app.selected, Some(MessageId(5)));
    app.session.composer.apply(Event::Uncertain(op.operation));
    app.switch_reply(MessageId(4));
    assert!(matches!(app.session.composer.send, SendState::Uncertain(_)));
    assert_eq!(app.session.composer.draft, draft);
    app.arrive();
    app.set_source(SourceFilter::Teams);
    app.focus(false);
    assert_eq!(app.session.composer.draft, draft);
}

#[test]
fn legacy_conversation_draft_keys_migrate_without_losing_content() {
    let mut app = app();
    write(&mut app, "Legacy saved reply");
    let draft = app.session.composer.draft.clone().unwrap();
    app.switch_reply(MessageId(4));
    app.session.saved.clear();
    app.session
        .saved
        .insert("legacy-conversation-key".into(), draft.clone());
    let mut reopened = App::new(app.session, app.settings).unwrap();
    assert_eq!(reopened.session.saved[&draft.target.draft_key()], draft);
    reopened.switch_reply(MessageId(5));
    assert_eq!(reopened.session.composer.draft, Some(draft));
}
