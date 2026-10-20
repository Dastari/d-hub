use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use d_hub::{
    app::{App, Mode, Session},
    config::Settings,
    domain::{MessageId, safe_text},
    editor::Editor,
    tui,
};
use unicode_segmentation::UnicodeSegmentation;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn ordinary_navigation_characters_stay_text_in_the_editor() {
    let mut editor = Editor::default();
    for c in "jkg?/:hello".chars() {
        editor.key(key(KeyCode::Char(c)));
    }
    editor.key(key(KeyCode::Enter));
    editor.insert("next line");
    assert_eq!(editor.text, "jkg?/:hello\nnext line");
}

#[test]
fn editing_moves_and_deletes_whole_graphemes() {
    let clusters = ["e\u{301}", "東京", "👩🏽‍💻", "🇦🇺"];
    for text in clusters {
        let mut editor = Editor::new(text);
        let last = text.graphemes(true).next_back().unwrap();
        editor.key(key(KeyCode::Left));
        assert_eq!(editor.cursor, text.len() - last.len());
        editor.key(key(KeyCode::Delete));
        assert_eq!(editor.text, &text[..text.len() - last.len()]);
        let mut editor = Editor::new(text);
        editor.key(key(KeyCode::Backspace));
        assert_eq!(editor.text, &text[..text.len() - last.len()]);
    }
}

#[test]
fn combining_insert_and_newline_deletion_keep_cursor_on_a_boundary() {
    let mut editor = Editor::new("e");
    editor.insert("\u{301}");
    editor.key(key(KeyCode::Left));
    assert_eq!(editor.cursor, 0);
    editor.key(key(KeyCode::Right));
    assert_eq!(editor.cursor, "e\u{301}".len());
    let mut editor = Editor::new("e\n\u{301}");
    editor.cursor = 2;
    editor.key(key(KeyCode::Backspace));
    assert_eq!(editor.text, "e\u{301}");
    assert_eq!(editor.cursor, editor.text.len());
    editor.key(key(KeyCode::Backspace));
    assert_eq!(editor.text, "");
}

#[test]
fn multiline_movement_and_viewport_use_display_widths() {
    let mut editor = Editor::new("東京a\nhello");
    editor.key(key(KeyCode::Up));
    assert_eq!(editor.position(), (0, 5));
    editor.key(key(KeyCode::Home));
    assert_eq!(editor.position(), (0, 0));
    editor.key(key(KeyCode::Right));
    assert_eq!(editor.position(), (0, 2));
    let (row, col) = editor.viewport(2, 1);
    assert_eq!(row, 0);
    assert!(col < 2);
}

#[test]
fn long_replies_wrap_at_words_and_alt_arrows_move_between_wrapped_rows() {
    let mut editor = Editor::new("one two three four five");
    // Ten columns wrap at nine, leaving room for the cursor at a row's end.
    let (rows, cursor) = editor.wrapped(10, 5);
    assert_eq!(rows, ["one two ", "three ", "four five"]);
    assert_eq!(cursor, (2, 9));
    editor.key(key(KeyCode::Up));
    assert_eq!(&editor.text[editor.cursor..], " four five");
    editor.key(key(KeyCode::Up));
    assert_eq!(&editor.text[editor.cursor..], "wo three four five");
    let (_, cursor) = editor.wrapped(10, 1);
    assert_eq!(cursor, (0, 5), "scrolled to the cursor's row");
    assert_eq!(editor.top, 0);
    editor.key(key(KeyCode::Down));
    editor.key(key(KeyCode::Down));
    let (rows, cursor) = editor.wrapped(10, 1);
    assert_eq!((rows, cursor), (vec!["four five"], (0, 5)));
    let mut word = Editor::new("abcdefghijkl");
    assert_eq!(word.wrapped(5, 5).0, ["abcd", "efgh", "ijkl"]);
}

#[test]
fn terminal_sequences_are_stripped_before_rendering_or_insertion() {
    let untrusted = "ok\x1b[2J\x1b]52;c;Y2xpcGJvYXJk\x07\x00\x08\x1b[31mred\x1b[0m\r\nnext";
    assert_eq!(safe_text(untrusted), "okred\nnext");
    let mut editor = Editor::default();
    editor.insert(untrusted);
    assert_eq!(editor.text, "okred\nnext");
}

#[test]
fn resize_handles_wide_text_attachments_and_long_targets() {
    let mut app = App::new(Session::default(), Settings::default()).unwrap();
    app.editor
        .insert("東京 👩🏽‍💻 e\u{301} long reply\nsecond line");
    app.update_body();
    app.attach();
    for width in [1, 20, 28, 32, 52, 80, 120] {
        for height in [1, 10, 16, 24, 40] {
            let frame = tui::snapshot(&mut app, width, height).unwrap();
            assert_eq!(frame.lines().count(), height as usize);
        }
    }
    assert_eq!(
        app.session.composer.draft.as_ref().unwrap().body,
        "東京 👩🏽‍💻 e\u{301} long reply\nsecond line"
    );
}

#[test]
fn preview_and_theme_changes_leave_draft_revision_untouched() {
    let mut app = App::new(Session::default(), Settings::default()).unwrap();
    app.editor.insert("keep me");
    app.update_body();
    let draft = app.session.composer.draft.clone();
    app.select(MessageId(4));
    app.open_reader();
    app.settings.theme = app.settings.theme.next();
    app.reader_scroll = 12;
    let frame = tui::snapshot(&mut app, 70, 30).unwrap();
    assert!(frame.contains("Sasha"));
    assert_eq!(app.mode, Mode::Reader);
    assert_eq!(app.session.composer.draft, draft);
}

#[test]
fn narrow_drawer_keeps_recipient_and_account_visible() {
    let mut app = App::new(Session::default(), Settings::default()).unwrap();
    app.attach();
    let frame = tui::snapshot(&mut app, 28, 20).unwrap();
    assert!(frame.contains("kai@example.invalid"));
    assert!(frame.contains("demo-personal"));
}

#[test]
fn outer_padding_defaults_to_zero_and_can_be_changed_without_touching_a_draft() {
    let mut app = App::new(Session::default(), Settings::default()).unwrap();
    app.editor.insert("Keep my recipient and reply.");
    app.update_body();
    let draft = app.session.composer.draft.clone();
    assert!(
        tui::snapshot(&mut app, 66, 36)
            .unwrap()
            .starts_with("d-hub")
    );
    app.settings.set("padding_horizontal", "3").unwrap();
    app.settings.set("padding_vertical", "2").unwrap();
    let frame = tui::snapshot(&mut app, 66, 36).unwrap();
    let lines: Vec<_> = frame.lines().collect();
    assert!(lines[..2].iter().all(|s| s.is_empty()));
    assert!(lines[2].starts_with("   d-hub"));
    assert!(lines[34..].iter().all(|s| s.is_empty()));
    assert!(
        lines
            .iter()
            .filter(|s| !s.is_empty())
            .all(|s| s.starts_with("   "))
    );
    assert_eq!(app.session.composer.draft, draft);
    // Padding can consume the available space: show the fallback without losing the draft.
    app.settings.padding_horizontal = 16;
    app.settings.padding_vertical = 16;
    for (width, height) in [(1, 1), (28, 16), (80, 40)] {
        tui::snapshot(&mut app, width, height).unwrap();
    }
    assert_eq!(app.session.composer.draft, draft);
}

#[test]
fn settings_keep_the_last_padding_control_visible_in_a_small_window() {
    let mut app = App::new(Session::default(), Settings::default()).unwrap();
    app.mode = Mode::Settings;
    app.settings_page = d_hub::app::SettingsPage::Section(d_hub::app::SettingsSection::General);
    app.setting_index = 4; // padding_vertical, the last General field
    let frame = tui::snapshot(&mut app, 28, 20).unwrap();
    assert!(frame.contains("Vertical padding"));
}
