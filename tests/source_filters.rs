use d_hub::{
    app::{App, Session},
    composer::Event,
    config::Settings,
    domain::{Focus, MessageId, SourceFilter},
    tui,
};

fn app() -> App {
    App::new(Session::default(), Settings::default()).unwrap()
}

#[test]
fn email_combines_mail_sources_and_intersects_with_focus() {
    let mut app = app();
    app.set_source(SourceFilter::Email);
    assert_eq!(
        app.visible().iter().map(|m| m.id).collect::<Vec<_>>(),
        [MessageId(5), MessageId(4), MessageId(1)]
    );
    app.set_focus(Focus::Work);
    assert_eq!(
        app.visible().iter().map(|m| m.id).collect::<Vec<_>>(),
        [MessageId(4)]
    );
    app.set_source(SourceFilter::Teams);
    assert_eq!(
        app.visible().iter().map(|m| m.id).collect::<Vec<_>>(),
        [MessageId(3)]
    );
    app.set_focus(Focus::Personal);
    assert!(app.visible().is_empty());
    assert!(app.session.composer.reply_target().is_none());
    assert!(
        tui::snapshot(&mut app, 80, 36)
            .unwrap()
            .contains("in other sources or focus profiles")
    );
}

#[test]
fn source_changes_and_arrivals_preserve_drafts_and_submitted_revisions() {
    let mut app = app();
    app.editor.insert("Still replying to Kai");
    app.update_body();
    app.attach();
    let draft = app.session.composer.draft.clone();
    let operation = app.submit().unwrap();
    for source in SourceFilter::ALL {
        app.set_source(source);
        app.focus(false);
        app.arrive();
        app.focus(true);
        assert_eq!(app.session.composer.draft, draft);
        assert_eq!(
            app.session.composer.reply_target().unwrap().incoming,
            MessageId(5)
        );
    }
    app.session
        .composer
        .apply(Event::Accepted(operation.operation));
    assert_eq!(app.session.composer.sent, [operation]);
}

#[test]
fn inactive_empty_composer_follows_only_the_selected_source_and_focus() {
    let mut app = app();
    app.set_source(SourceFilter::Discord);
    assert!(app.session.composer.reply_target().is_none());
    app.focus(false);
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(2)
    );
    app.arrive();
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(2)
    );
    app.set_source(SourceFilter::Teams);
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(101)
    );
    app.set_source(SourceFilter::Email);
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(5)
    );
    app.set_focus(Focus::Work);
    assert_eq!(
        app.session.composer.reply_target().unwrap().incoming,
        MessageId(4)
    );
}

#[test]
fn reopening_filtered_feed_keeps_hidden_draft_but_previews_a_visible_message() {
    let mut app = app();
    app.attach();
    let draft = app.session.composer.draft.clone();
    app.set_source(SourceFilter::Discord);
    let reopened = App::new(app.session, app.settings).unwrap();
    assert_eq!(reopened.settings.source, SourceFilter::Discord);
    assert_eq!(reopened.selected, Some(MessageId(2)));
    assert_eq!(reopened.session.composer.draft, draft);
}

#[test]
fn source_setting_is_backward_compatible_and_round_trips_each_option() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "schema_version = 1\nfocus = 'work'\n").unwrap();
    let mut settings = Settings::load(&path).unwrap();
    assert_eq!(settings.source, SourceFilter::All);
    for source in SourceFilter::ALL {
        let manual: Settings = toml::from_str(&format!("source = '{}'\n", source.value())).unwrap();
        assert_eq!(manual.source, source);
        settings.set("source", source.value()).unwrap();
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().source, source);
    }
    assert!(settings.set("source", "unknown-provider").is_err());
}
