//! Readable labels, summaries and per-page context for settings rows.
use super::*;
use crate::config::SettingRow;
use crate::policy::AccountConfig;
use std::collections::BTreeMap;

pub(super) fn field_label(name: &str) -> String {
    let last = name.rsplit('.').next().unwrap_or(name);
    match (name, last) {
        ("auto_update", _) => "Automatic updates",
        ("theme", _) => "Theme",
        ("reduced_motion", _) => "Reduced motion",
        ("padding_horizontal", _) => "Horizontal padding",
        ("padding_vertical", _) => "Vertical padding",
        ("reply_first", _) => "Reply-first arrows",
        ("insert_on_focus", _) => "Type on focus",
        ("focus", _) => "Focus profile",
        ("source", _) => "Source filter",
        ("mark_read_on_select", _) => "Mark read on select",
        ("dismiss_after_reply", _) => "Dismiss after reply",
        ("show_dismissed", _) => "Show dismissed",
        ("show_spam", _) => "Show filtered messages",
        ("ai.enabled", _) => "AI decisions",
        ("ai.provider", _) => "Provider",
        ("ai.model", _) => "Model",
        ("ai.threshold", _) => "Match threshold",
        ("ai.timeout_ms", _) => "Timeout (ms)",
        ("ai.hold_pending", _) => "Hold while pending",
        ("spam.enabled", _) => "Filtering",
        ("spam.no_reply", _) => "Hide no-reply and automated",
        ("spam.for_you", _) => "Only what is for you (AI)",
        ("spam.for_you_threshold", _) => "For-you threshold",
        ("spam.prompt", _) => "Promotion question",
        ("spam.threshold", _) => "Promotion threshold",
        ("spam.local_terms", _) => "Local exclusion terms",
        (_, "name") => {
            if name.starts_with("accounts.") {
                "Display name"
            } else {
                "Name"
            }
        }
        (_, "prompt") => "Decision prompt",
        (_, "terms") => "Local include terms",
        (_, "allow_send") => "Allow replies",
        (_, "allow_ai") => "Allow AI decisions",
        (_, "enabled") => "Connected",
        (_, "poll_seconds") => "Sync interval (s)",
        (_, "username") => "Email / username",
        (_, "tenant") => "Microsoft tenant",
        (_, "client_id") => "App (client) ID",
        (_, "resources") => "Chats / resources",
        (_, "discord_mode") => "Discord mode",
        (_, "discord_user_id") => "Pinned user ID",
        (_, "notification_sender") => "Notification sender",
        (_, "notification_app") => "Notification app",
        _ => last,
    }
    .into()
}

pub(super) fn provider_kind(provider: Provider) -> &'static str {
    match provider {
        Provider::Discord => "Discord · personal DMs",
        Provider::ICloud => "iCloud · mail",
        Provider::Office365 => "Office 365 · mail",
        Provider::Teams => "Teams · chats",
    }
}

pub(super) fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

pub(super) fn section_summary(app: &App, section: SettingsSection) -> String {
    let s = &app.settings;
    match section {
        SettingsSection::Accounts => {
            let connected = s
                .accounts
                .iter()
                .filter(|a| health(app, a).0 == Health::Ok)
                .count();
            match s.accounts.len() {
                0 => "None yet · add one".into(),
                n => format!("{n} · {connected} connected"),
            }
        }
        SettingsSection::General => {
            format!("{} · updates {}", s.theme.label(), on_off(s.auto_update))
        }
        SettingsSection::Feed => format!(
            "reply-first {} · source {} · focus {}",
            on_off(s.reply_first),
            s.source.label(),
            s.profile_label()
        ),
        SettingsSection::Profiles => s
            .profiles
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        SettingsSection::Ai => {
            if s.ai.enabled {
                format!("{} · {}", s.ai.provider.value(), s.ai.model)
            } else {
                "off".into()
            }
        }
        SettingsSection::Spam if !s.spam.enabled => "off".into(),
        SettingsSection::Spam => [
            s.spam.no_reply.then_some("no-reply"),
            Some("promotions"),
            (s.spam.for_you && s.ai.enabled).then_some("not for you"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", "),
    }
}

pub(super) fn section_note(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::Accounts => "Sign in, connect, rename or remove each account",
        SettingsSection::General => "Updates, theme, motion and padding",
        SettingsSection::Feed => "Reply-first navigation, filters and read behaviour",
        SettingsSection::Profiles => "Work, Home and custom focus prompts",
        SettingsSection::Ai => "Optional external decisions; per-account opt-in",
        SettingsSection::Spam => "What is hidden: no-reply, automated, promotional, not for you",
    }
}

pub(super) fn page_title(app: &App, page: &SettingsPage) -> String {
    match page {
        SettingsPage::Home => "Settings".into(),
        SettingsPage::Section(s) => s.label().into(),
        SettingsPage::Account(id) => account(app, id).map_or(id.clone(), |a| a.label().into()),
        SettingsPage::AddAccount => "Add account".into(),
        SettingsPage::RemoveAccount(_) => "Remove".into(),
        SettingsPage::Profile(id) => app
            .settings
            .profiles
            .iter()
            .find(|p| &p.id == id)
            .map_or(id.clone(), |p| p.name.clone()),
    }
}

pub(super) fn breadcrumb(app: &App) -> Vec<String> {
    let mut pages = vec![app.settings_page.clone()];
    while let Some(parent) = pages.last().and_then(SettingsPage::parent) {
        pages.push(parent);
    }
    pages
        .iter()
        .rev()
        .map(|page| safe_text(&page_title(app, page)))
        .collect()
}

/// Rendered content of a row: marker, label, value and explanatory note.
pub(super) struct Row {
    pub(super) marker: (String, Color),
    pub(super) label: String,
    pub(super) label_color: Color,
    pub(super) value: String,
    pub(super) value_color: Color,
    pub(super) note: String,
}

pub(super) fn describe(
    app: &App,
    item: &Item,
    entries: &BTreeMap<String, SettingRow>,
    p: Palette,
) -> Row {
    let plain = |label: String, value: String, note: String| Row {
        marker: (String::new(), p.muted),
        label,
        label_color: p.fg,
        value,
        value_color: p.second,
        note,
    };
    let action = |label: &str, note: String, color: Color| Row {
        marker: (String::new(), color),
        label: label.into(),
        label_color: color,
        value: String::new(),
        value_color: p.second,
        note,
    };
    match item {
        Item::Field(name) => {
            let row = entries.get(name);
            // An account's resources are mail folders, Teams chats or Discord channels.
            let provider = name
                .strip_prefix("accounts.")
                .and_then(|rest| rest.strip_suffix(".resources"))
                .and_then(|id| account(app, id))
                .map(|a| a.provider);
            let (label, default) = match provider {
                Some(Provider::ICloud) => ("Folders".into(), "INBOX"),
                Some(Provider::Office365) => ("Folders".into(), "inbox"),
                Some(Provider::Teams) => ("Chats".into(), "Not set"),
                _ => (field_label(name), "Not set"),
            };
            plain(
                label,
                row.map_or_else(String::new, |r| {
                    if r.value.trim().is_empty() {
                        default.into()
                    } else {
                        r.value.clone()
                    }
                }),
                row.map_or_else(String::new, |r| r.help.clone()),
            )
        }
        Item::Open(SettingsPage::Section(section)) => plain(
            section.label().into(),
            format!("{} ›", section_summary(app, *section)),
            section_note(*section).into(),
        ),
        Item::Open(SettingsPage::Account(id)) => {
            let Some(a) = account(app, id) else {
                return plain(id.clone(), String::new(), String::new());
            };
            let (state, status) = health(app, a);
            let (dot, color) = health_style(state, p);
            let unread = stats(app, id).unread;
            let mut value = format!("{} · {}", a.provider.label(), short_status(state, &status));
            if unread > 0 {
                value.push_str(&format!(" · {unread} unread"));
            }
            Row {
                marker: (dot.into(), color),
                label: a.label().into(),
                label_color: p.fg,
                value: format!("{value} ›"),
                value_color: p.second,
                note: if a.name.is_empty() {
                    String::new()
                } else {
                    format!("ID {}", a.id)
                },
            }
        }
        Item::Open(SettingsPage::Profile(id)) => {
            let profile = app.settings.profiles.iter().find(|p| &p.id == id);
            plain(
                profile.map_or(id.clone(), |p| p.name.clone()),
                "›".into(),
                profile.map_or_else(String::new, |p| p.prompt.clone()),
            )
        }
        Item::Open(page) => plain(page_title(app, page), "›".into(), String::new()),
        Item::Action(Action::AddAccount) => action(
            "+ Add account",
            "Discord, iCloud, Office 365 or Teams · add several of the same kind".into(),
            p.accent,
        ),
        Item::Action(Action::CreateAccount) => action(
            "Create account",
            "Saves it disconnected; sign in next, then connect".into(),
            p.accent,
        ),
        Item::Action(Action::SignIn(id)) => {
            let a = account(app, id);
            let again = a.is_some_and(|a| signed_in(app, a));
            action(
                if again { "Sign in again" } else { "Sign in" },
                a.map_or_else(String::new, |a| a.auth_description().into()),
                p.accent,
            )
        }
        Item::Action(Action::Test(id)) => action(
            if app.check_job.as_ref().is_some_and(|(job, _)| job == id) {
                "Testing connection…"
            } else {
                "Test connection"
            },
            match account(app, id).map(|a| a.provider) {
                Some(Provider::Teams) => "Read-only; lists your recent chats and their IDs",
                _ => "Read-only; lists your folders and checks the synced ones",
            }
            .into(),
            p.accent,
        ),
        Item::Action(Action::Connect(_)) => action(
            "Connect",
            "Start syncing with the stored sign-in".into(),
            p.accent,
        ),
        Item::Action(Action::Disconnect(_)) => action(
            "Disconnect",
            "Stop syncing; keep sign-in and settings".into(),
            p.accent,
        ),
        Item::Action(Action::AskRemove(_)) => {
            action("Remove account…", "Asks for confirmation".into(), p.warning)
        }
        Item::Action(Action::ConfirmRemove(id)) => action(
            "Remove account",
            format!(
                "Removes {} from d-hub",
                account(app, id).map_or(id.as_str(), |a| a.label())
            ),
            p.warning,
        ),
        Item::Action(Action::Back) => action("Cancel", "Keep this account".into(), p.accent),
        Item::Action(Action::AddProfile) => action(
            "+ Add profile",
            "Create a focus profile with its own decision prompt".into(),
            p.accent,
        ),
        Item::Action(Action::RemoveProfile(_)) => action(
            "Remove profile",
            "Removes this focus profile immediately".into(),
            p.warning,
        ),
        Item::Action(Action::AiKey) => action(
            "Store API key",
            "Saves the selected provider's key in the desktop keyring".into(),
            p.accent,
        ),
        Item::New(NewField::Provider) => plain(
            "Provider".into(),
            format!("‹ {} ›", provider_kind(app.new_account.provider)),
            AccountConfig::new("new", app.new_account.provider)
                .auth_description()
                .into(),
        ),
        Item::New(NewField::Id) => plain(
            "Account ID".into(),
            app.new_account.id.clone(),
            "Permanent key for CLI commands, sign-in and drafts".into(),
        ),
        Item::New(NewField::Name) => plain(
            "Display name".into(),
            app.new_account.name.clone(),
            "Optional label in the feed and composer, e.g. Home or Work; rename any time".into(),
        ),
    }
}

const NO_ACCOUNTS: &str =
    "No accounts yet. Choose + Add account to connect Discord, iCloud, Office 365 or Teams.";
const REMOVAL_EFFECTS: &str = "Stops syncing, deletes this account's settings and erases its \
    stored sign-in from the keyring. Messages already in the feed stay until dismissed; nothing \
    is deleted from the provider.";

/// Non-selectable context above the rows.
/// The latest connection check: one line per step, with discovered folders or chats.
fn check_lines(app: &App, id: &str, p: Palette) -> Vec<(String, Color)> {
    const SHOWN: usize = 12;
    let Some(steps) = app.account_checks.get(id) else {
        return vec![];
    };
    let mut lines = vec![("Last connection check".to_owned(), p.second)];
    for step in steps {
        let (mark, color) = if step.ok {
            ("✓", p.success)
        } else {
            ("✗", p.warning)
        };
        lines.push((format!("{mark} {} · {}", step.name, step.detail), color));
        for item in step.items.iter().take(SHOWN) {
            lines.push((format!("    {item}"), p.muted));
        }
        if step.items.len() > SHOWN {
            lines.push((
                format!(
                    "    … {} more · d-hub account test {id} lists all",
                    step.items.len() - SHOWN
                ),
                p.muted,
            ));
        }
    }
    lines
}

pub(super) fn header(app: &App, p: Palette) -> Vec<(String, Color)> {
    match &app.settings_page {
        SettingsPage::Section(SettingsSection::Accounts) if app.settings.accounts.is_empty() => {
            vec![
                (NO_ACCOUNTS.into(), p.muted),
                ("Or type :account add discord".into(), p.second),
            ]
        }
        SettingsPage::Section(SettingsSection::Accounts) => vec![(
            "Each account signs in and syncs independently.".into(),
            p.muted,
        )],
        SettingsPage::Account(id) => {
            let Some(a) = account(app, id) else {
                return vec![];
            };
            let (state, status) = health(app, a);
            let (dot, color) = health_style(state, p);
            let stats = stats(app, id);
            let mut identity = format!("{} · ID {}", provider_kind(a.provider), a.id);
            if a.provider == Provider::Discord && a.discord_mode == "personal" {
                identity.push_str(&if a.discord_user_id.is_empty() {
                    " · not signed in".to_owned()
                } else if a.username.is_empty() {
                    " · signed in".to_owned()
                } else {
                    format!(" · signed in as {}", a.username)
                });
            } else if !a.username.is_empty() {
                identity.push_str(&format!(" · {}", a.username));
            }
            let mut activity = format!("{} cached · {} unread", stats.total, stats.unread);
            if let Some(latest) = stats.latest {
                activity.push_str(&format!(" · latest {latest}"));
            }
            activity.push_str(&format!(
                " · replies {}",
                if a.allow_send { "allowed" } else { "off" }
            ));
            let mut lines = vec![
                (format!("{dot} {status}"), color),
                (safe_text(&identity), p.muted),
                (activity, p.muted),
            ];
            lines.extend(check_lines(app, id, p));
            lines
        }
        SettingsPage::AddAccount => vec![(
            "Nothing connects until you sign in and choose Connect.".into(),
            p.muted,
        )],
        SettingsPage::RemoveAccount(id) => {
            let label = account(app, id).map_or(id.as_str(), |a| a.label());
            vec![
                (format!("Remove {}?", safe_text(label)), p.warning),
                (REMOVAL_EFFECTS.into(), p.muted),
            ]
        }
        _ => vec![],
    }
}
