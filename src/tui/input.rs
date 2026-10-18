//! Keyboard and mouse dispatch per mode; normal typing is never consumed as navigation.
use super::*;

pub(super) fn scroll(app: &mut App, down: bool, amount: u16) {
    let limit = if app.mode == Mode::Help {
        HELP.lines().count() * 4
    } else {
        preview::reader(app).map_or(0, |(_, body)| body.chars().count())
    };
    if down {
        app.reader_scroll = app
            .reader_scroll
            .saturating_add(amount)
            .min(limit.min(u16::MAX as usize) as u16);
    } else {
        app.reader_scroll = app.reader_scroll.saturating_sub(amount);
    }
}

pub(super) fn key(
    app: &mut App,
    key: KeyEvent,
    rows: usize,
    pending: &mut Option<(Instant, u64, MockOutcome)>,
    g_prefix: &mut Option<Instant>,
    config_path: Option<&Path>,
) -> bool {
    if key.kind == KeyEventKind::Release {
        return false;
    }
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    if app.mode == Mode::DiscordQr {
        if let Some(setup) = &mut app.discord_setup
            && discord_setup_key(setup, key)
        {
            app.discord_setup = None;
            app.mode = Mode::Settings;
            return control && key.code == KeyCode::Char('q');
        }
        return false;
    }
    if control && key.code == KeyCode::Char('q') {
        if app.mode == Mode::Secret {
            use zeroize::Zeroize;
            app.command.text.zeroize();
        }
        return true;
    }
    if key.code == KeyCode::F(6) {
        app.focus(!app.focused);
        app.notice =
            "Simulated focus transition (terminal focus reporting is also enabled).".into();
        return false;
    }
    if key.code == KeyCode::F(7) && !app.live_service {
        app.next_outcome = match app.next_outcome {
            MockOutcome::Success => MockOutcome::Failure,
            MockOutcome::Failure => MockOutcome::Uncertain,
            MockOutcome::Uncertain => MockOutcome::Success,
        };
        return false;
    }
    if matches!(app.mode, Mode::Insert | Mode::Navigate) {
        if (key.code == KeyCode::Enter
            && key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT))
            || key.code == KeyCode::F(5)
        {
            if pending.is_none() {
                *pending = send(app);
            }
            return false;
        }
        if control && key.code == KeyCode::Char('o') {
            launch_reader(app, config_path);
            return false;
        }
        // Dismiss without replying; works while typing because Ctrl never inserts text.
        if control && key.code == KeyCode::Char('e') {
            app.toggle_dismissed();
            return false;
        }
        // Delete dismisses and marks read: always while browsing, and while typing only before
        // any text exists (afterwards it edits the reply).
        if key.code == KeyCode::Delete
            && key.modifiers.is_empty()
            && (app.mode == Mode::Navigate
                || (app.editor.text.is_empty()
                    && app
                        .session
                        .composer
                        .draft
                        .as_ref()
                        .is_none_or(|d| d.is_empty())))
        {
            app.dismiss_read();
            return false;
        }
        // PageUp/PageDown page the preview pane; the composer has no pages of its own.
        if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) && key.modifiers.is_empty() {
            app.scroll_preview(if key.code == KeyCode::PageDown { 1 } else { -1 });
            return false;
        }
        if control
            && key.code == KeyCode::Char('a')
            && !app.live_feed
            && !app.live_setup
            && !app.live_service
        {
            app.attach();
            return false;
        }
    }
    match app.mode {
        Mode::Insert => {
            let alt = key.modifiers.contains(KeyModifiers::ALT);
            if app.settings.reply_first
                && !alt
                && !control
                && matches!(
                    key.code,
                    KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
                )
            {
                match key.code {
                    KeyCode::Up => app.move_reply(-1, rows),
                    KeyCode::Down => app.move_reply(1, rows),
                    KeyCode::Left => app.cycle_reply_source(true),
                    KeyCode::Right => app.cycle_reply_source(false),
                    _ => {}
                }
            } else if key.code == KeyCode::Esc {
                app.mode = Mode::Navigate;
            } else if app.session.composer.editable() && app.editor.key(key) {
                app.update_body();
            }
        }
        Mode::Command | Mode::Search => match key.code {
            KeyCode::Esc => {
                app.mode = if app.reader.is_some() && app.mode == Mode::Search {
                    Mode::Reader
                } else if app.mode == Mode::Command {
                    app.command_origin
                } else {
                    Mode::Navigate
                };
            }
            KeyCode::Enter => {
                let command = app.command.text.clone();
                if app.mode == Mode::Command {
                    if commands::input(&command).trim() == "send" {
                        app.mode = Mode::Navigate;
                        if pending.is_none() {
                            *pending = send(app);
                        }
                    } else if run_command(app, &command) {
                        return true;
                    }
                } else if app.reader.is_some() {
                    app.reader_query = command;
                    app.reader_scroll = preview::reader(app)
                        .and_then(|(_, body)| {
                            body.lines().position(|line| {
                                line.to_lowercase()
                                    .contains(&app.reader_query.to_lowercase())
                            })
                        })
                        .unwrap_or(0)
                        .min(u16::MAX as usize) as u16;
                    app.mode = Mode::Reader;
                } else {
                    app.query = command;
                    app.feed_offset = 0;
                    app.mode = Mode::Navigate;
                }
            }
            KeyCode::Tab if app.mode == Mode::Command => {
                commands::complete(app, key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::BackTab if app.mode == Mode::Command => {
                commands::complete(app, true);
            }
            _ => {
                app.command_completion = None;
                app.command.key(key);
            }
        },
        Mode::Settings => settings::key(app, key),
        Mode::EditSetting => {
            let single_line = app
                .edit_key
                .as_deref()
                .is_some_and(|name| !settings::multiline(name));
            if key.code == KeyCode::Esc {
                app.command = Editor::default();
                app.mode = Mode::Settings;
            } else if (control && key.code == KeyCode::Char('s'))
                || (single_line && key.code == KeyCode::Enter)
            {
                if let Some(name) = app.edit_key.clone() {
                    let value = app.command.text.clone();
                    match settings::commit_edit(app, &name, &value) {
                        Ok(()) => app.command = Editor::default(),
                        Err(e) => app.notice = e.to_string(),
                    }
                }
            } else {
                app.command.key(key);
            }
        }
        Mode::DiscordQr => {}
        Mode::Secret => {
            if key.code == KeyCode::Esc {
                use zeroize::Zeroize;
                app.command.text.zeroize();
                app.command = Editor::default();
                app.secret_ref = None;
                app.mode = secret_return(app);
            } else if key.code == KeyCode::Enter {
                let secret = zeroize::Zeroizing::new(std::mem::take(&mut app.command.text));
                app.command = Editor::default();
                app.mode = secret_return(app);
                if let Some(reference) = app.secret_ref.take() {
                    let (tx, rx) = std::sync::mpsc::sync_channel(2);
                    app.auth_job = Some(rx);
                    std::thread::spawn(move || {
                        use crate::auth::CredentialStore;
                        let notice = match crate::auth::SecretService.store(&reference, &secret) {
                            Ok(()) => "Credential stored in desktop keyring. Provider connection is a separate step.".into(),
                            Err(e) => format!("Credential storage failed: {e}"),
                        };
                        let _ = tx.send(notice);
                    });
                }
            } else {
                app.command.key(key);
            }
        }
        Mode::Discard => match key.code {
            KeyCode::Char('y') => app.discard(),
            KeyCode::Esc | KeyCode::Char('n') => app.mode = Mode::Navigate,
            _ => {}
        },
        Mode::Help | Mode::Reader => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.mode = Mode::Navigate;
                app.reader = None;
            }
            KeyCode::Char('/') if app.mode == Mode::Reader => {
                app.mode = Mode::Search;
                app.command = Editor::new(&app.reader_query);
            }
            KeyCode::Down | KeyCode::Char('j') => scroll(app, true, 1),
            KeyCode::Up | KeyCode::Char('k') => scroll(app, false, 1),
            KeyCode::PageDown => scroll(app, true, 10),
            KeyCode::PageUp => scroll(app, false, 10),
            KeyCode::Home | KeyCode::Char('g') => app.reader_scroll = 0,
            _ => {}
        },
        Mode::Navigate => match key.code {
            KeyCode::Left => app.set_source(app.settings.source.cycle(true)),
            KeyCode::Right => app.set_source(app.settings.source.cycle(false)),
            KeyCode::Char('q') => return true,
            KeyCode::Char('i' | 'r') if !app.live_feed && !app.live_setup => {
                if app.settings.reply_first
                    && let Some(id) = app.selected
                {
                    app.switch_reply(id);
                } else if app.session.composer.draft.is_none()
                    && let Some(id) = app.selected
                {
                    app.select(id);
                }
                app.mode = Mode::Insert;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.move_selection(1, rows);
                app.new_items = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1, rows),
            KeyCode::Char('d') if control => app.move_selection(rows as isize, rows),
            KeyCode::Char('u') if control => app.move_selection(-(rows as isize), rows),
            KeyCode::Home => {
                app.move_selection(isize::MIN, rows);
            }
            KeyCode::End | KeyCode::Char('G') => {
                app.move_selection(isize::MAX, rows);
            }
            KeyCode::Char('g') => {
                if g_prefix.is_some_and(|time| time.elapsed() < Duration::from_millis(500)) {
                    app.move_selection(isize::MIN, rows);
                    *g_prefix = None;
                } else {
                    *g_prefix = Some(Instant::now());
                    app.notice = "g… · press g for first message".into();
                }
            }
            KeyCode::Char('f') => {
                let profile = app.settings.cycle_focus();
                app.set_profile(&profile);
            }
            KeyCode::Char('e') => app.toggle_dismissed(),
            KeyCode::Char('D') => app.toggle_show_dismissed(),
            KeyCode::Char('S') => {
                app.settings.show_spam = !app.settings.show_spam;
                app.refresh_filters();
            }
            KeyCode::Char('u') => {
                if let Some(message) = app.selected_message() {
                    let (id, unread) = (message.id, !message.unread);
                    app.set_unread(id, unread);
                }
            }
            KeyCode::Char('v') => open_discord(app),
            KeyCode::Char('t') => app.settings.theme = app.settings.theme.next(),
            KeyCode::Char('n') if !app.live_feed && !app.live_setup && !app.live_service => {
                app.arrive()
            }
            KeyCode::Char('a') if !app.live_feed && !app.live_setup && !app.live_service => {
                app.attach()
            }
            KeyCode::Char('x') => remove_last(app),
            KeyCode::Char('s') => app.save_switch(),
            KeyCode::Char('d') => app.mode = Mode::Discard,
            KeyCode::Char('o') => app.open_reader(),
            KeyCode::Char(',') => {
                app.settings_entry = app.settings_page.clone();
                app.mode = Mode::Settings;
            }
            KeyCode::Char('?') => {
                app.mode = Mode::Help;
                app.reader_scroll = 0;
            }
            KeyCode::Char(':') => {
                commands::open(app);
            }
            KeyCode::Char('/') => {
                app.mode = Mode::Search;
                app.command = Editor::new(&app.query);
            }
            _ => {}
        },
    }
    if key.code != KeyCode::Char('g') {
        *g_prefix = None;
    }
    false
}

/// Wheel and click routing; scrolling never selects, and hover never retargets a reply.
pub(super) fn mouse(app: &mut App, mouse: MouseEvent, hits: &HitAreas) {
    let point = ratatui::layout::Position::new(mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if app.mode == Mode::Settings => {
            let last = settings::items(app).len().saturating_sub(1);
            app.setting_index = if mouse.kind == MouseEventKind::ScrollDown {
                (app.setting_index + 3).min(last)
            } else {
                app.setting_index.saturating_sub(3)
            };
        }
        MouseEventKind::Down(MouseButton::Left) if app.mode == Mode::Settings => {
            if let Some((_, page)) = hits
                .settings_nav
                .iter()
                .find(|(area, _)| area.contains(point))
            {
                settings::open(app, page.clone());
            } else if let Some((_, index)) =
                hits.settings.iter().find(|(area, _)| area.contains(point))
            {
                settings::click(app, *index);
            }
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
            if matches!(app.mode, Mode::Reader | Mode::Help) =>
        {
            scroll(app, mouse.kind == MouseEventKind::ScrollDown, 3)
        }
        MouseEventKind::ScrollDown
            if hits.feed.contains(point) && matches!(app.mode, Mode::Insert | Mode::Navigate) =>
        {
            app.feed_offset = (app.feed_offset + 1).min(app.visible().len().saturating_sub(1))
        }
        MouseEventKind::ScrollUp
            if hits.feed.contains(point) && matches!(app.mode, Mode::Insert | Mode::Navigate) =>
        {
            app.feed_offset = app.feed_offset.saturating_sub(1)
        }
        MouseEventKind::Down(MouseButton::Left)
            if matches!(app.mode, Mode::Insert | Mode::Navigate) =>
        {
            if let Some((_, index)) = hits
                .attachments
                .iter()
                .find(|(area, _)| area.contains(point))
            {
                app.remove_attachment(*index);
            } else if hits.composer.contains(point) {
                app.mode = Mode::Insert;
            } else if let Some((_, id)) = hits
                .feed_items
                .iter()
                .find(|(area, _)| area.contains(point))
            {
                let id = *id;
                {
                    if app.settings.reply_first {
                        app.switch_reply(id);
                    } else {
                        app.select(id);
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::Session, domain::SourceFilter};

    #[test]
    fn settings_page_keys_follow_visible_rows_and_keep_reply_context() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.editor.insert("retained draft");
        app.update_body();
        let before = serde_json::to_vec(&app.session).unwrap();
        app.mode = Mode::Settings;
        app.settings_page = SettingsPage::Section(SettingsSection::Feed);
        snapshot(&mut app, 40, 26).unwrap();
        let page = app.settings_page_size;
        let last = settings::items(&app).len() - 1;
        assert!(page < last, "page {page} must be shorter than the section");
        let mut pending = None;
        let mut prefix = None;
        for code in [
            KeyCode::PageDown,
            KeyCode::End,
            KeyCode::Home,
            KeyCode::PageDown,
            KeyCode::PageUp,
        ] {
            key(
                &mut app,
                KeyEvent::new(code, KeyModifiers::NONE),
                4,
                &mut pending,
                &mut prefix,
                None,
            );
            match code {
                KeyCode::PageDown => assert_eq!(app.setting_index, page),
                KeyCode::End => assert_eq!(app.setting_index, last),
                _ => assert_eq!(app.setting_index, 0),
            }
        }
        assert_eq!(serde_json::to_vec(&app.session).unwrap(), before);
    }

    #[test]
    fn arrows_cycle_sources_in_navigation_but_move_the_insert_cursor() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.mode = Mode::Navigate;
        let mut pending = None;
        let mut prefix = None;
        for expected in SourceFilter::ALL
            .into_iter()
            .skip(1)
            .chain([SourceFilter::All])
        {
            key(
                &mut app,
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                4,
                &mut pending,
                &mut prefix,
                None,
            );
            assert_eq!(app.settings.source, expected);
        }
        key(
            &mut app,
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.settings.source, SourceFilter::Office365);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(
            app.session.composer.reply_target().unwrap().provider,
            crate::domain::Provider::Office365
        );
        app.settings.reply_first = false;
        app.editor = Editor::new("café");
        let cursor = app.editor.cursor;
        key(
            &mut app,
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.editor.cursor, cursor - 'é'.len_utf8());
        key(
            &mut app,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.editor.cursor, cursor);
        assert_eq!(app.settings.source, SourceFilter::Office365);
    }
    #[test]
    fn prompt_editing_uses_shared_validation_and_secret_input_is_masked_and_cancellable() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.attach();
        let draft = app.session.composer.draft.clone();
        edit_setting(&mut app, "spam.prompt");
        app.command = Editor::new("Is this an unsolicited promotion?\nKeep family messages.");
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            4,
            &mut None,
            &mut None,
            None,
        );
        assert_eq!(app.mode, Mode::Settings);
        assert!(app.settings.spam.prompt.contains("Keep family"));
        assert_eq!(app.session.composer.draft, draft);
        app.mode = Mode::Secret;
        app.command = Editor::new("fictional-secret-marker");
        let frame = snapshot(&mut app, 100, 40).unwrap();
        assert!(!frame.contains("fictional-secret-marker"));
        assert!(frame.contains('•'));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            4,
            &mut None,
            &mut None,
            None,
        );
        assert!(app.command.text.is_empty());
        assert_eq!(app.session.composer.draft, draft);
    }
    #[test]
    fn reply_first_arrows_switch_drafts_and_alt_arrows_move_the_cursor() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.editor.insert("first 東京");
        app.update_body();
        let original = app.session.composer.draft.clone();
        let mut pending = None;
        let mut prefix = None;
        key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.selected, Some(crate::domain::MessageId(4)));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.editor.text, "j");
        key(
            &mut app,
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.session.composer.draft, original);
        let cursor = app.editor.cursor;
        key(
            &mut app,
            KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert!(app.editor.cursor < cursor);
        assert_eq!(app.settings.source, SourceFilter::All);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.settings.source, SourceFilter::ICloud);
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.session.composer.draft, original);
    }
}
