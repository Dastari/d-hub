//! Settings are pages: an overview, global sections and one page per account or profile.
//! This module owns page items and navigation; submodules own actions, labels, status and layout.
//! Page state is window-local and never persisted.
mod actions;
mod labels;
mod render;
mod status;
#[cfg(test)]
mod tests;

use self::{
    actions::{NEW_ID, NEW_NAME, begin_edit, cycle_field, cycle_provider, run},
    labels::{breadcrumb, describe, field_label, header},
    status::{Health, health, health_style, short_status, signed_in, stats},
};
pub(super) use self::{
    actions::{commit_edit, multiline},
    render::draw,
};
use super::*;
use crate::app::{NewAccount, SettingsPage, SettingsSection};
use crate::policy::AccountConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    AddAccount,
    CreateAccount,
    SignIn(String),
    Test(String),
    Connect(String),
    Disconnect(String),
    AskRemove(String),
    ConfirmRemove(String),
    AddProfile,
    RemoveProfile(String),
    AiKey,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewField {
    Provider,
    Id,
    Name,
}

/// One selectable row on a settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    /// A key in the shared typed registry, edited through the same path as the CLI.
    Field(String),
    Open(SettingsPage),
    Action(Action),
    New(NewField),
}
/// A value or credential being typed into the selected row; the page stays visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Inline {
    Text { multiline: bool },
    Secret,
}

pub(in crate::tui) fn inline(app: &App) -> Option<Inline> {
    match app.mode {
        Mode::EditSetting => Some(Inline::Text {
            multiline: app.edit_key.as_deref().is_some_and(multiline),
        }),
        Mode::Secret if app.command_origin == Mode::Settings => Some(Inline::Secret),
        _ => None,
    }
}

/// Opens the page holding a setting with that row editing in place (e.g. `:spam prompt`).
pub(in crate::tui) fn edit(app: &mut App, name: &str) {
    let value = match app.settings.value(name) {
        Ok(value) => value,
        Err(e) => {
            app.notice = e.to_string();
            return;
        }
    };
    let item = Item::Field(name.into());
    let pages: Vec<_> = SettingsSection::ALL
        .into_iter()
        .map(SettingsPage::Section)
        .chain(
            app.settings
                .profiles
                .iter()
                .map(|p| SettingsPage::Profile(p.id.clone())),
        )
        .chain(
            app.settings
                .accounts
                .iter()
                .map(|a| SettingsPage::Account(a.id.clone())),
        )
        .collect();
    let current = app.settings_page.clone();
    let mut found = None;
    for page in pages {
        app.settings_page = page.clone();
        if items(app).contains(&item) {
            found = Some(page);
            break;
        }
    }
    app.settings_page = current;
    match found {
        Some(page) => {
            open_at(app, page, &item);
            begin_edit(app, name, &value);
        }
        None => app.notice = format!("{name} has no settings row · use d-hub config set {name}"),
    }
}

pub(super) fn account<'a>(app: &'a App, id: &str) -> Option<&'a AccountConfig> {
    app.settings.accounts.iter().find(|a| a.id == id)
}

pub(super) fn account_fields(account: &AccountConfig) -> Vec<&'static str> {
    let mut fields = vec!["allow_send"];
    fields.extend(match account.provider {
        Provider::ICloud => &["username", "resources", "poll_seconds"][..],
        Provider::Office365 => &[
            "username",
            "tenant",
            "client_id",
            "resources",
            "poll_seconds",
        ],
        Provider::Teams => &[
            "username",
            "tenant",
            "client_id",
            "resources",
            "poll_seconds",
        ],
        Provider::Discord => match account.discord_mode.as_str() {
            "notifications" => &["discord_mode", "notification_sender", "notification_app"][..],
            "bot" => &["discord_mode", "resources"],
            _ => &["discord_mode", "discord_user_id"],
        },
    });
    fields.push("allow_ai");
    fields
}

/// Selectable rows for the current page.
pub(crate) fn items(app: &App) -> Vec<Item> {
    let field = |name: &str| Item::Field(name.into());
    match &app.settings_page {
        SettingsPage::Home => SettingsSection::ALL
            .into_iter()
            .map(|s| Item::Open(SettingsPage::Section(s)))
            .collect(),
        SettingsPage::Section(SettingsSection::Accounts) => app
            .settings
            .accounts
            .iter()
            .map(|a| Item::Open(SettingsPage::Account(a.id.clone())))
            .chain([Item::Action(Action::AddAccount)])
            .collect(),
        SettingsPage::Section(SettingsSection::General) => [
            "auto_update",
            "theme",
            "reduced_motion",
            "padding_horizontal",
            "padding_vertical",
        ]
        .map(field)
        .into(),
        SettingsPage::Section(SettingsSection::Feed) => [
            "reply_first",
            "insert_on_focus",
            "focus",
            "source",
            "mark_read_on_select",
            "dismiss_after_reply",
            "show_dismissed",
            "show_spam",
        ]
        .map(field)
        .into(),
        SettingsPage::Section(SettingsSection::Profiles) => app
            .settings
            .profiles
            .iter()
            .map(|p| Item::Open(SettingsPage::Profile(p.id.clone())))
            .chain([Item::Action(Action::AddProfile)])
            .collect(),
        SettingsPage::Section(SettingsSection::Ai) => [
            "ai.enabled",
            "ai.provider",
            "ai.model",
            "ai.threshold",
            "ai.timeout_ms",
            "ai.about",
            "ai.hold_pending",
        ]
        .map(field)
        .into_iter()
        .chain([Item::Action(Action::AiKey)])
        .collect(),
        SettingsPage::Section(SettingsSection::Spam) => [
            "spam.enabled",
            "spam.no_reply",
            "spam.for_you",
            "spam.for_you_threshold",
            "spam.person_prompt",
            "spam.person_yes",
            "spam.person_no",
            "spam.for_you_prompt",
            "spam.for_you_yes",
            "spam.for_you_no",
            "spam.prompt",
            "spam.threshold",
            "spam.local_terms",
        ]
        .map(field)
        .into(),
        SettingsPage::Profile(id) => ["name", "prompt", "terms"]
            .map(|f| field(&format!("profiles.{id}.{f}")))
            .into_iter()
            .chain([Item::Action(Action::RemoveProfile(id.clone()))])
            .collect(),
        SettingsPage::Account(id) => {
            let Some(a) = account(app, id) else {
                return vec![];
            };
            let mut rows = vec![
                field(&format!("accounts.{id}.name")),
                Item::Action(Action::SignIn(id.clone())),
            ];
            if a.provider != Provider::Discord {
                rows.push(Item::Action(Action::Test(id.clone())));
            }
            rows.extend([Item::Action(if a.enabled {
                Action::Disconnect(id.clone())
            } else {
                Action::Connect(id.clone())
            })]);
            rows.extend(
                account_fields(a)
                    .into_iter()
                    .map(|f| field(&format!("accounts.{id}.{f}"))),
            );
            rows.push(Item::Action(Action::AskRemove(id.clone())));
            rows
        }
        SettingsPage::AddAccount => vec![
            Item::New(NewField::Provider),
            Item::New(NewField::Id),
            Item::New(NewField::Name),
            Item::Action(Action::CreateAccount),
        ],
        SettingsPage::RemoveAccount(id) => vec![
            Item::Action(Action::ConfirmRemove(id.clone())),
            Item::Action(Action::Back),
        ],
    }
}

/// Fall back to the containing section when an account/profile disappears (CLI edit, removal).
pub(super) fn normalize(app: &mut App) {
    let valid = match &app.settings_page {
        SettingsPage::Account(id) | SettingsPage::RemoveAccount(id) => account(app, id).is_some(),
        SettingsPage::Profile(id) => app.settings.profiles.iter().any(|p| &p.id == id),
        _ => true,
    };
    if !valid {
        app.settings_page = match app.settings_page {
            SettingsPage::Profile(_) => SettingsPage::Section(SettingsSection::Profiles),
            _ => SettingsPage::Section(SettingsSection::Accounts),
        };
        app.setting_index = 0;
    }
    let len = items(app).len();
    app.setting_index = app.setting_index.min(len.saturating_sub(1));
}

pub(crate) fn open(app: &mut App, page: SettingsPage) {
    if page == SettingsPage::AddAccount {
        let provider = app.new_account.provider;
        app.new_account = NewAccount {
            provider,
            id: app.settings.suggest_account_id(provider),
            name: String::new(),
        };
    }
    // Arriving from the drawer (a command, startup) makes this the page Esc closes from.
    if !matches!(
        app.mode,
        Mode::Settings | Mode::EditSetting | Mode::Secret | Mode::DiscordQr
    ) {
        app.settings_entry = page.clone();
    }
    app.settings_page = page;
    app.setting_index = 0;
    app.settings_scroll = 0;
    app.mode = Mode::Settings;
}

/// Open a page with a particular row selected (e.g. Sign in after adding an account).
pub(crate) fn open_at(app: &mut App, page: SettingsPage, item: &Item) {
    open(app, page);
    if let Some(index) = items(app).iter().position(|i| i == item) {
        app.setting_index = index;
    }
}

/// Returns false at the index, where Esc closes settings.
pub(super) fn back(app: &mut App) -> bool {
    let Some(parent) = app.settings_page.parent() else {
        return false;
    };
    let from = std::mem::replace(&mut app.settings_page, parent);
    app.settings_scroll = 0;
    let rows = items(app);
    let target = match &from {
        SettingsPage::RemoveAccount(id) => Item::Action(Action::AskRemove(id.clone())),
        SettingsPage::AddAccount => Item::Action(Action::AddAccount),
        page => Item::Open(page.clone()),
    };
    app.setting_index = rows.iter().position(|i| *i == target).unwrap_or(0);
    true
}

pub(super) fn cycle_section(app: &mut App, reverse: bool) {
    let all = SettingsSection::ALL;
    let next = match app.settings_page.section() {
        None if reverse => all[all.len() - 1],
        None => all[0],
        Some(current) => {
            let index = all.iter().position(|s| *s == current).unwrap_or(0);
            all[(index + if reverse { all.len() - 1 } else { 1 }) % all.len()]
        }
    };
    open(app, SettingsPage::Section(next));
}

/// Enter: open pages, run actions, edit text and advance choices.
pub(super) fn activate(app: &mut App, reverse: bool, edit_text: bool) {
    let rows = items(app);
    let Some(item) = rows.get(app.setting_index).cloned() else {
        return;
    };
    match item {
        Item::Open(page) => open(app, page),
        Item::Action(action) => run(app, action),
        Item::Field(name) => {
            let entries = app.settings.entries();
            if let Some(row) = entries.iter().find(|r| r.name == name) {
                if row.choices.is_empty() && !edit_text {
                    return;
                }
                cycle_field(app, &name, row, reverse);
            }
        }
        Item::New(NewField::Provider) => cycle_provider(app, reverse),
        Item::New(NewField::Id) if edit_text => {
            let value = app.new_account.id.clone();
            begin_edit(app, NEW_ID, &value);
        }
        Item::New(NewField::Name) if edit_text => {
            let value = app.new_account.name.clone();
            begin_edit(app, NEW_NAME, &value);
        }
        Item::New(_) => {}
    }
}

pub(super) fn key(app: &mut App, key: KeyEvent) {
    normalize(app);
    let len = items(app).len().max(1);
    match key.code {
        KeyCode::Char(':') => commands::open(app),
        KeyCode::Char('q') => app.mode = Mode::Navigate,
        KeyCode::Esc if app.settings_page == app.settings_entry => app.mode = Mode::Navigate,
        KeyCode::Esc | KeyCode::Backspace => {
            if !back(app) && key.code == KeyCode::Esc {
                app.mode = Mode::Navigate;
            }
        }
        KeyCode::Tab => cycle_section(app, false),
        KeyCode::BackTab => cycle_section(app, true),
        KeyCode::Down | KeyCode::Char('j') => app.setting_index = (app.setting_index + 1) % len,
        KeyCode::Up | KeyCode::Char('k') => app.setting_index = (app.setting_index + len - 1) % len,
        KeyCode::PageDown => {
            app.setting_index = (app.setting_index + app.settings_page_size).min(len - 1)
        }
        KeyCode::PageUp => {
            app.setting_index = app.setting_index.saturating_sub(app.settings_page_size)
        }
        KeyCode::Home => app.setting_index = 0,
        KeyCode::End => app.setting_index = len - 1,
        KeyCode::Enter | KeyCode::Char(' ') => activate(app, false, true),
        // Arrows only change choices or follow links; they never run account actions.
        KeyCode::Right | KeyCode::Char('l') => match items(app).get(app.setting_index) {
            Some(Item::Open(_)) | Some(Item::Field(_)) | Some(Item::New(NewField::Provider)) => {
                activate(app, false, false)
            }
            _ => {}
        },
        KeyCode::Left | KeyCode::Char('h') => match items(app).get(app.setting_index) {
            Some(Item::Field(_)) | Some(Item::New(NewField::Provider)) => {
                activate(app, true, false)
            }
            _ => {
                back(app);
            }
        },
        _ => {}
    }
}

pub(super) fn click(app: &mut App, index: usize) {
    let already = index == app.setting_index;
    app.setting_index = index;
    // A second click follows a link; actions and values still need the keyboard.
    if already && matches!(items(app).get(index), Some(Item::Open(_))) {
        activate(app, false, false);
    }
}
