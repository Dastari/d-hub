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
        "spam person",
        "spam for-you",
        "about",
        "accounts",
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
        result.push(format!("account rename {} ", account.id));
        result.push(format!("account remove {}", account.id));
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
        "account add [PROVIDER [ID]] · rename ID NAME · remove ID · enable/disable ID · auth ID signs in"
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

pub(super) fn run_command(app: &mut App, command: &str) -> bool {
    app.mode = app.command_origin;
    let words: Vec<_> = commands::input(command).split_whitespace().collect();
    match words.as_slice() {
        ["update"] if app.shared && app.live_service => {
            app.update_requested = true;
            app.notice = "Checking production releases…".into();
        }
        ["update"] => {
            app.notice = "Production updates require the live service; demo stays offline.".into()
        }
        ["attach"] if !app.live_feed && !app.live_setup && !app.live_service => app.attach(),
        ["remove"] => remove_last(app),
        ["incoming"] if !app.live_feed && !app.live_setup && !app.live_service => app.arrive(),
        ["follow"] => {
            app.focus(false);
            app.focus(true);
            app.new_items = 0;
            app.sync_editor();
        }
        ["save-switch"] => app.save_switch(),
        ["discard"] => app.mode = Mode::Discard,
        ["fail"] if !app.live_service => {
            app.next_outcome = MockOutcome::Failure;
            app.notice = "Next mock send will fail.".into();
        }
        ["uncertain"] if !app.live_service => {
            app.next_outcome = MockOutcome::Uncertain;
            app.notice = "Next mock send will have uncertain acceptance.".into();
        }
        ["reconcile"] if !app.live_service => {
            if let SendState::Uncertain(operation) = &app.session.composer.send {
                let id = operation.operation;
                if app.shared {
                    app.actions
                        .push(crate::service::Action::Reconcile { operation: id });
                }
                app.accepted(id);
                app.sync_editor();
                app.notice =
                    "MOCK RECONCILED · simulated acceptance confirmed without resending.".into();
            }
        }
        ["resolve", outcome] if app.live_service && ["accepted", "not-sent"].contains(outcome) => {
            if let SendState::Uncertain(op) = &app.session.composer.send {
                let action = crate::service::Action::Resolve {
                    expected: app.session.composer.clone(),
                    operation: op.operation,
                    accepted: *outcome == "accepted",
                };
                app.actions.push(action.clone());
                let shared = app.shared;
                app.shared = false;
                let _ = crate::service::apply(app, &action);
                app.shared = shared;
                app.notice="Owner-reported outcome recorded; verify in the original client before using resolve.".into();
            }
        }
        ["connections"] => {
            app.notice = app
                .settings.accounts
                .iter()
                .map(|account| {
                    let status = if !account.enabled { "disabled · :account enable ID starts sync" } else { app.provider_status.get(&account.id).map(String::as_str).unwrap_or("waiting for provider") };
                    format!("{}: {status}", account.id)
                })
                .collect::<Vec<_>>()
                .join(" · ")
        }
        ["settings"] => {
            app.settings_entry = app.settings_page.clone();
            app.mode = Mode::Settings;
        }
        ["profile", "add", id] => match app.settings.add_profile(id) {
            Ok(()) => app.notice = format!("Profile {id} added · edit its prompt in settings."),
            Err(e) => app.notice = e.to_string(),
        },
        ["profile", "remove", id] => match app.settings.remove_profile(id) {
            Ok(()) => app.refresh_filters(),
            Err(e) => app.notice = e.to_string(),
        },
        ["profile", "prompt", id] => edit_setting(app, &format!("profiles.{id}.prompt")),
        ["spam", "prompt"] => edit_setting(app, "spam.prompt"),
        ["spam", "person"] => edit_setting(app, "spam.person_prompt"),
        ["spam", "for-you"] => edit_setting(app, "spam.for_you_prompt"),
        ["about"] => edit_setting(app, "ai.about"),
        ["accounts"] => settings::open(app, SettingsPage::Section(SettingsSection::Accounts)),
        ["account", "add"] => settings::open(app, SettingsPage::AddAccount),
        ["account", "add", provider, rest @ ..] if rest.len() <= 1 => {
            match parse_provider(provider).and_then(|p| {
                let id = rest
                    .first()
                    .map_or_else(|| app.settings.suggest_account_id(p), |id| (*id).to_owned());
                app.settings.add_account(&id, p).map(|()| id)
            }) {
                Ok(id) => {
                    settings::open_at(
                        app,
                        SettingsPage::Account(id.clone()),
                        &settings::Item::Action(settings::Action::SignIn(id.clone())),
                    );
                    app.notice = format!("Added {id} · :auth {id} or Enter on Sign in.")
                }
                Err(e) => app.notice = e.to_string(),
            }
        }
        ["account", "rename", id, name @ ..] if !name.is_empty() => {
            match app.settings.set(&format!("accounts.{id}.name"), &name.join(" ")) {
                Ok(()) => app.notice = format!("Account {id} renamed to {}.", name.join(" ")),
                Err(e) => app.notice = e.to_string(),
            }
        }
        ["account", "remove", id] => {
            if app.settings.accounts.iter().any(|a| a.id == *id) {
                settings::open(app, SettingsPage::RemoveAccount((*id).into()));
            } else {
                app.notice = format!("unknown account {id}");
            }
        }
        ["account", action @ ("enable" | "disable" | "connect" | "disconnect"), id] => {
            let enable = matches!(*action, "enable" | "connect");
            match app.settings.set(&format!("accounts.{id}.enabled"), if enable { "true" } else { "false" }) {
                Ok(()) => app.notice = format!("Account {id} {} · :connections shows sync status; allow_send controls replies.", if enable { "enabled" } else { "disabled" }),
                Err(e) => app.notice = e.to_string(),
            }
        }
        ["account", ..] => app.notice = "Usage: :account add [PROVIDER [ID]] · rename ID NAME · remove ID · enable ID / disable ID".into(),
        ["auth", id] => begin_auth(app, id),
        ["dismiss"] => app.toggle_dismissed(),
        ["dismissed"] => app.toggle_show_dismissed(),
        ["focus", value] => app.set_profile(value),
        ["source", value] => match app.settings.set("source", value) {
            Ok(()) => app.set_source(app.settings.source),
            Err(e) => app.notice = e.to_string(),
        },
        ["theme", value] => {
            if let Err(e) = app.settings.set("theme", value) {
                app.notice = e.to_string();
            }
        }
        ["motion"] => app.settings.reduced_motion = !app.settings.reduced_motion,
        ["preview"] => app.open_reader(),
        ["help"] => {
            app.mode = Mode::Help;
            app.reader_scroll = 0;
        }
        ["quit"] => return true,
        _ => app.notice = "Unknown command · :help lists supported commands.".into(),
    }
    false
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
        assert_eq!(
            app.settings_page,
            crate::app::SettingsPage::Account("personal".into())
        );
        assert_eq!(
            super::super::settings::items(&app)[app.setting_index],
            super::super::settings::Item::Action(super::super::settings::Action::SignIn(
                "personal".into()
            ))
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
            ":account add discord too many-ids",
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

    #[test]
    fn production_update_command_is_explicit_and_keeps_demo_offline_and_drafts_locked() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.editor
            .insert("draft remains locked during update check");
        app.update_body();
        let before = app.session.clone();
        run_command(&mut app, "update");
        assert!(!app.update_requested);
        app.shared = true;
        app.live_service = true;
        run_command(&mut app, "update");
        assert!(app.update_requested);
        assert_eq!(
            serde_json::to_vec(&before).unwrap(),
            serde_json::to_vec(&app.session).unwrap()
        );
        assert!(app.actions.is_empty());
    }
}
