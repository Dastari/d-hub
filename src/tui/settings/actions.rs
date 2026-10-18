//! Page actions and the edit overlay: every change goes through the shared typed settings path.
use super::*;
use crate::app::NewAccount;
use crate::config::SettingRow;

pub(super) const PROVIDERS: [Provider; 4] = [
    Provider::Discord,
    Provider::ICloud,
    Provider::Office365,
    Provider::Teams,
];
pub(super) const NEW_ID: &str = "new_account.id";
pub(super) const NEW_NAME: &str = "new_account.name";
pub(super) const NEW_PROFILE: &str = "new_profile.id";

pub(in crate::tui) fn multiline(name: &str) -> bool {
    [".prompt", ".terms", "local_terms", ".resources"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        || matches!(name, "profiles" | "accounts")
}

pub(in crate::tui) fn edit_title(name: &str) -> String {
    let label = match name {
        NEW_ID => "New account ID".into(),
        NEW_NAME => "New account display name".into(),
        NEW_PROFILE => "New focus profile ID".into(),
        _ => format!("{} · {name}", field_label(name)),
    };
    if multiline(name) {
        format!("{label} · Ctrl+s save · Esc cancel")
    } else {
        format!("{label} · Enter save · Esc cancel")
    }
}

pub(super) fn begin_edit(app: &mut App, name: &str, value: &str) {
    app.command = Editor::new(value);
    app.edit_key = Some(name.into());
    app.mode = Mode::EditSetting;
}

/// Saves an edit from the overlay; pseudo-keys belong to page-local forms.
pub(in crate::tui) fn commit_edit(app: &mut App, name: &str, value: &str) -> Result<()> {
    match name {
        NEW_ID => {
            let id = value.trim();
            if !crate::policy::identifier(id) {
                bail!("account IDs use 1–48 letters, digits, - or _");
            }
            if account(app, id).is_some() {
                bail!("account {id} already exists; choose another ID");
            }
            app.new_account.id = id.into();
            app.notice = format!("Account ID {id} · this ID cannot change later.");
        }
        NEW_NAME => {
            if value.trim().len() > 80 || value.chars().any(char::is_control) {
                bail!("display names need at most 80 bytes on one line");
            }
            app.new_account.name = value.trim().into();
            app.notice.clear();
        }
        NEW_PROFILE => {
            let id = value.trim();
            app.settings.add_profile(id)?;
            open(app, SettingsPage::Profile(id.into()));
            app.notice = format!("Profile {id} added · edit its name and decision prompt.");
            return Ok(());
        }
        _ => {
            app.settings.set(name, value)?;
            app.refresh_filters();
            app.notice = format!("{} saved.", field_label(name));
        }
    }
    app.mode = Mode::Settings;
    Ok(())
}

pub(super) fn create_account(app: &mut App) {
    let NewAccount { provider, id, name } = app.new_account.clone();
    match app.settings.add_named_account(&id, provider, &name) {
        Ok(()) => {
            open_at(
                app,
                SettingsPage::Account(id.clone()),
                &Item::Action(Action::SignIn(id.clone())),
            );
            app.notice = format!(
                "Added {} · press Enter on Sign in.",
                account(app, &id).map_or(id.as_str(), |a| a.label())
            );
        }
        Err(e) => app.notice = e.to_string(),
    }
}

pub(super) fn set_enabled(app: &mut App, id: &str, enabled: bool) {
    match app
        .settings
        .set(&format!("accounts.{id}.enabled"), &enabled.to_string())
    {
        Ok(()) => {
            let missing_sign_in = account(app, id).is_some_and(|a| {
                a.provider == Provider::Discord
                    && a.discord_mode == "personal"
                    && a.discord_user_id.is_empty()
            });
            app.notice = if !enabled {
                format!("{id} disconnected · sign-in and settings kept.")
            } else if missing_sign_in {
                format!("{id} enabled · sign in to start receiving messages.")
            } else {
                format!("{id} connecting · Allow replies controls sending.")
            };
            if let Some(index) = items(app).iter().position(|i| {
                matches!(i, Item::Action(Action::Connect(a) | Action::Disconnect(a)) if a == id)
            }) {
                app.setting_index = index;
            }
        }
        Err(e) => app.notice = e.to_string(),
    }
}

pub(super) fn remove_account(app: &mut App, id: &str) {
    let busy = match &app.session.composer.send {
        SendState::Submitting(op) | SendState::Uncertain(op) => op.draft.target.account == id,
        _ => false,
    };
    if busy {
        app.notice =
            "A send from this account is pending or uncertain · resolve it before removing.".into();
        return;
    }
    if app.auth_job.is_some() || app.discord_setup.as_ref().is_some_and(|s| s.job.is_some()) {
        app.notice = "Finish or cancel the current sign-in before removing an account.".into();
        return;
    }
    let account = match app.settings.remove_account(id) {
        Ok(account) => account,
        Err(e) => {
            app.notice = e.to_string();
            return;
        }
    };
    app.refresh_filters();
    open(app, SettingsPage::Section(SettingsSection::Accounts));
    let label = account.label().to_owned();
    if !(app.live_setup || app.live_service) || !crate::auth::SecretService::available() {
        app.notice = format!("Removed {label}.");
        return;
    }
    app.notice = format!("Removed {label} · erasing stored sign-in…");
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    app.auth_job = Some(rx);
    std::thread::spawn(move || {
        use crate::auth::CredentialStore;
        let notice = match crate::auth::SecretService.remove(&account.credential_ref()) {
            Ok(()) => format!("Removed {label} · stored sign-in erased."),
            Err(_) => format!(
                "Removed {label} · sign-in not erased; unlock the keyring, then d-hub account forget {}",
                account.id
            ),
        };
        let _ = tx.send(notice);
    });
}

/// Runs the read-only connection check off the render thread; results land on the page.
fn begin_check(app: &mut App, id: &str) {
    if !app.live_setup && !app.live_service {
        app.notice =
            "Connection checks need a live profile · the offline demo never connects.".into();
        return;
    }
    if app.check_job.is_some() {
        app.notice = "A connection check is already running.".into();
        return;
    }
    let Some(account) = account(app, id).cloned() else {
        return;
    };
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    app.check_job = Some((account.id.clone(), rx));
    app.account_checks.remove(&account.id);
    app.notice = format!("Checking {}… nothing is synced or sent.", account.label());
    std::thread::spawn(move || {
        let _ = tx.send(crate::providers::check::run(&account));
    });
}

pub(super) fn run(app: &mut App, action: Action) {
    match action {
        Action::AddAccount => open(app, SettingsPage::AddAccount),
        Action::CreateAccount => create_account(app),
        Action::SignIn(id) => {
            begin_auth(app, &id);
            if matches!(app.mode, Mode::Secret | Mode::DiscordQr) {
                app.command_origin = Mode::Settings;
            }
        }
        Action::Test(id) => begin_check(app, &id),
        Action::Connect(id) => set_enabled(app, &id, true),
        Action::Disconnect(id) => set_enabled(app, &id, false),
        Action::AskRemove(id) => open(app, SettingsPage::RemoveAccount(id)),
        Action::ConfirmRemove(id) => remove_account(app, &id),
        Action::AddProfile => begin_edit(app, NEW_PROFILE, ""),
        Action::RemoveProfile(id) => match app.settings.remove_profile(&id) {
            Ok(()) => {
                app.refresh_filters();
                open(app, SettingsPage::Section(SettingsSection::Profiles));
                app.notice = format!("Profile {id} removed.");
            }
            Err(e) => app.notice = e.to_string(),
        },
        Action::AiKey => {
            begin_auth(app, "ai");
            if app.mode == Mode::Secret {
                app.command_origin = Mode::Settings;
            }
        }
        Action::Back => {
            back(app);
        }
    }
}

pub(super) fn cycle_provider(app: &mut App, reverse: bool) {
    let index = PROVIDERS
        .iter()
        .position(|p| *p == app.new_account.provider)
        .unwrap_or(0);
    let offset = if reverse { PROVIDERS.len() - 1 } else { 1 };
    let suggested = app.settings.suggest_account_id(app.new_account.provider);
    app.new_account.provider = PROVIDERS[(index + offset) % PROVIDERS.len()];
    // Only replace an ID the owner has not typed themselves.
    if app.new_account.id.is_empty() || app.new_account.id == suggested {
        app.new_account.id = app.settings.suggest_account_id(app.new_account.provider);
    }
}

pub(super) fn cycle_field(app: &mut App, name: &str, row: &SettingRow, reverse: bool) {
    if row.choices.is_empty() {
        begin_edit(app, name, &row.value);
        return;
    }
    let index = row
        .choices
        .iter()
        .position(|v| *v == row.value)
        .unwrap_or(0);
    let offset = if reverse { row.choices.len() - 1 } else { 1 };
    let value = &row.choices[(index + offset) % row.choices.len()];
    match app.settings.set(name, value) {
        Ok(()) => {
            app.refresh_filters();
            app.notice = format!("{} saved as {value}.", field_label(name));
        }
        Err(e) => app.notice = e.to_string(),
    }
}
