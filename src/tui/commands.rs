//! Command entry and completion never interpret composer text or execute a paste.
use super::*;

pub(crate) struct Completion {
    candidates: Vec<String>,
    index: usize,
}

pub(super) fn input(value: &str) -> &str {
    value
        .trim_start()
        .strip_prefix(':')
        .unwrap_or(value.trim_start())
        .trim_start()
}

pub(super) fn open(app: &mut App) {
    app.command_origin = app.mode;
    app.mode = Mode::Command;
    app.command = Editor::default();
    app.command_completion = None;
}

fn candidates(app: &App) -> Vec<String> {
    let mut result: Vec<String> = [
        "account add discord ",
        "account add icloud ",
        "account add office365 ",
        "account add teams ",
        "auth ai",
        "profile add ",
        "spam prompt",
        "connections",
        "settings",
        "update",
        "attach",
        "remove",
        "incoming",
        "follow",
        "save-switch",
        "discard",
        "send",
        "fail",
        "uncertain",
        "reconcile",
        "resolve accepted",
        "resolve not-sent",
        "dismiss",
        "dismissed",
        "focus all",
        "source all",
        "source discord",
        "source teams",
        "source email",
        "source icloud",
        "source office365",
        "theme midnight",
        "theme dusk",
        "motion",
        "preview",
        "help",
        "quit",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for account in &app.settings.accounts {
        result.push(format!("auth {}", account.id));
        result.push(format!("account enable {}", account.id));
        result.push(format!("account disable {}", account.id));
    }
    for profile in &app.settings.profiles {
        for action in ["focus", "profile prompt", "profile remove"] {
            result.push(format!("{action} {}", profile.id));
        }
    }
    result
}

pub(super) fn complete(app: &mut App, reverse: bool) {
    // Completing in the middle must not erase the rest of an account ID or command.
    if app.command.cursor != app.command.text.len() {
        return;
    }
    if let Some(completion) = &mut app.command_completion {
        let len = completion.candidates.len();
        completion.index = (completion.index + if reverse { len - 1 } else { 1 }) % len;
        app.command = Editor::new(&completion.candidates[completion.index]);
        return;
    }
    let prefix = input(&app.command.text);
    let candidates: Vec<_> = candidates(app)
        .into_iter()
        .filter(|c| c.starts_with(prefix))
        .collect();
    if candidates.is_empty() {
        return; // Never replace an unknown command or user-entered ID with an unrelated command.
    }
    let index = if reverse { candidates.len() - 1 } else { 0 };
    app.command = Editor::new(&candidates[index]);
    app.command_completion = Some(Completion { candidates, index });
}

pub(super) fn paste(app: &mut App, value: &str) {
    match app.mode {
        Mode::Insert if app.session.composer.editable() => {
            app.editor.insert(value);
            app.update_body();
        }
        Mode::Settings | Mode::Navigate if value.trim_start().starts_with(':') => {
            open(app);
            paste(app, value);
        }
        Mode::Command | Mode::Search => {
            if app.command.text.len() + value.len() <= 16_384 {
                let value = if app.mode == Mode::Command && app.command.text.is_empty() {
                    input(value)
                } else {
                    value
                };
                app.command.insert(&value.replace(['\n', '\r'], " "));
                app.command_completion = None;
            }
        }
        Mode::EditSetting | Mode::Secret if app.command.text.len() + value.len() <= 16_384 => {
            app.command.insert(value);
        }
        _ => {}
    }
}

fn hint(value: &str) -> &'static str {
    if input(value).starts_with("account") {
        "account add PROVIDER ID · account enable ID / disable ID · auth ID signs in"
    } else if input(value).starts_with("auth") {
        "auth ACCOUNT · use an existing account ID; auth ai configures the decision provider"
    } else if input(value).starts_with("profile") {
        "profile add ID · profile prompt ID · profile remove ID"
    } else {
        "Tab / Shift+Tab complete · Enter run · Esc cancel"
    }
}

pub(super) fn draw(frame: &mut Frame, area: Rect, p: Palette, app: &mut App) {
    let height = area.height.min(9);
    let panel = Rect::new(area.x, area.bottom() - height, area.width, height);
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Block::default().style(Style::default().bg(p.selected)),
        panel,
    );
    let inner = panel.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    frame.render_widget(
        text("Command · Enter run · Esc cancel", p.accent),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let line = Rect::new(inner.x + 1, inner.y + 1, inner.width.saturating_sub(1), 1);
    frame.render_widget(text(":", p.accent), Rect::new(inner.x, line.y, 1, 1));
    let (_, col) = app.command.viewport(line.width as usize, 1);
    frame.render_widget(
        text(app.command.text.clone(), p.fg)
            .scroll((0, app.command.left.min(u16::MAX as usize) as u16)),
        line,
    );
    frame.set_cursor_position((line.x + col, line.y));
    let suggestions = candidates(app)
        .into_iter()
        .filter(|c| c.starts_with(input(&app.command.text)))
        .map(|c| {
            if c.ends_with(' ') {
                format!("{c}<ID>")
            } else {
                c
            }
        })
        .take(4)
        .collect::<Vec<_>>()
        .join(" · ");
    frame.render_widget(
        text(suggestions, p.second).wrap(Wrap { trim: false }),
        Rect::new(inner.x, inner.y + 3, inner.width, 1),
    );
    frame.render_widget(
        text(hint(&app.command.text), p.muted).wrap(Wrap { trim: false }),
        Rect::new(
            inner.x,
            inner.y + 4,
            inner.width,
            inner.height.saturating_sub(4),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Session;

    fn press(app: &mut App, code: KeyCode) {
        key(
            app,
            KeyEvent::new(code, KeyModifiers::NONE),
            4,
            &mut None,
            &mut None,
            None,
        );
    }

    #[test]
    fn completion_covers_providers_and_live_account_and_profile_ids_without_erasing_input() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.settings
            .add_account("personal", Provider::Discord)
            .unwrap();
        app.settings.add_profile("family").unwrap();
        app.mode = Mode::Settings;
        open(&mut app);
        app.command = Editor::new("account add");
        for provider in ["discord", "icloud", "office365", "teams", "discord"] {
            press(&mut app, KeyCode::Tab);
            assert_eq!(app.command.text, format!("account add {provider} "));
        }
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.command.text, "account add teams ");
        for (prefix, expected) in [
            ("auth p", "auth personal"),
            ("profile prompt f", "profile prompt family"),
            ("account add discord my-id", "account add discord my-id"),
            ("unknown", "unknown"),
        ] {
            app.command_completion = None;
            app.command = Editor::new(prefix);
            press(&mut app, KeyCode::Tab);
            assert_eq!(app.command.text, expected);
        }
    }

    #[test]
    fn pasted_commands_wait_for_enter_preserve_drafts_and_return_to_settings() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.editor.insert("Retained reply 東京");
        app.update_body();
        let before = serde_json::to_vec(&app.session).unwrap();
        app.mode = Mode::Settings;
        paste(&mut app, ":account add discord personal\r\n");
        assert_eq!(app.mode, Mode::Command);
        assert!(app.settings.accounts.is_empty());
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Settings);
        assert_eq!(app.settings.accounts[0].id, "personal");
        assert!(!app.settings.accounts[0].enabled);
        assert!(
            app.settings.entries()[app.setting_index]
                .name
                .starts_with("accounts.personal.")
        );
        assert!(
            snapshot(&mut app, 38, 50)
                .unwrap()
                .contains("Added personal")
        );
        assert_eq!(before, serde_json::to_vec(&app.session).unwrap());
        let settings = app.settings.clone();
        for command in [
            ":account add discord personal",
            ":account add invalid nope",
            ":account add discord",
        ] {
            paste(&mut app, command);
            press(&mut app, KeyCode::Enter);
            assert_eq!(app.settings, settings);
        }
        assert!(app.notice.contains("Usage:"));
        paste(&mut app, ":account add icloud cancelled");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Settings);
        assert_eq!(app.settings, settings);
        assert_eq!(before, serde_json::to_vec(&app.session).unwrap());
    }

    #[test]
    fn colon_paste_in_insert_remains_literal_reply_text() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        paste(&mut app, ":account add discord personal");
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.editor.text, ":account add discord personal");
        assert!(app.settings.accounts.is_empty());
    }

    #[test]
    fn narrow_command_viewport_tracks_cursor_and_leaves_settings_visible() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.mode = Mode::Settings;
        open(&mut app);
        app.command = Editor::new("account add discord long-personal-account");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(28, 40)).unwrap();
        terminal
            .draw(|f| render(f, &mut app, Duration::ZERO))
            .unwrap();
        let end = app.command.left;
        assert!(end > 0);
        assert!(snapshot(&mut app, 28, 40).unwrap().contains("Settings"));
        press(&mut app, KeyCode::Home);
        terminal
            .draw(|f| render(f, &mut app, Duration::ZERO))
            .unwrap();
        assert_eq!(app.command.left, 0);
        let buffer = terminal.backend().buffer();
        let rendered: String = buffer.content.iter().map(|c| c.symbol()).collect();
        assert!(rendered.contains(":account add discord"));
    }
}
