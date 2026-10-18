//! Explicit credential entry; secrets never leave the masked editor or keyring.
use super::*;

pub(super) fn begin_auth(app: &mut App, id: &str) {
    if !app.live_setup && !app.live_service {
        app.notice = "Account setup is explicit · use d-hub settings or d-hub account auth outside the offline demo.".into();
        return;
    }
    if app.auth_job.is_some() || app.discord_setup.as_ref().is_some_and(|s| s.job.is_some()) {
        app.notice = "Authentication already in progress.".into();
        return;
    }
    let reference = if id == "ai" {
        app.settings.ai.provider.credential_ref().to_owned()
    } else {
        let Some(account) = app.settings.accounts.iter().find(|a| a.id == id).cloned() else {
            app.notice = "Unknown account · add it through :account add PROVIDER ID.".into();
            return;
        };
        if account.provider == Provider::Discord && account.discord_mode == "personal" {
            if app.live_service && !app.service_supports_discord_qr {
                app.notice =
                    "Restart the older daemon: d-hub service stop, then reopen d-hub for QR setup."
                        .into();
                return;
            }
            app.discord_setup = Some(new_discord_setup(&account));
            app.mode = Mode::DiscordQr;
            return;
        }
        if !account.authentication_supported() {
            app.notice = account.auth_description().into();
            return;
        }
        if !crate::auth::SecretService::available() {
            app.notice =
                "Install libsecret (secret-tool) and unlock the desktop keyring before setup."
                    .into();
            return;
        }
        if matches!(account.provider, Provider::Office365 | Provider::Teams) {
            let (tx, rx) = std::sync::mpsc::sync_channel(8);
            app.auth_job = Some(rx);
            std::thread::spawn(move || {
                if let Err(e) = crate::auth::authenticate_graph(
                    &account,
                    &crate::auth::SecretService,
                    |notice| {
                        let _ = tx.send(notice);
                    },
                ) {
                    let _ = tx.send(format!("Sign-in failed: {e}"));
                }
            });
            app.notice = "Starting Microsoft device login…".into();
            return;
        }
        account.credential_ref()
    };
    if !crate::auth::SecretService::available() {
        app.notice =
            "Install libsecret (secret-tool) and unlock the desktop keyring before setup.".into();
        return;
    }
    app.secret_ref = Some(reference);
    app.command = Editor::default();
    app.mode = Mode::Secret;
}

/// Credential entry returns to settings when it was started there.
pub(super) fn secret_return(app: &App) -> Mode {
    if app.command_origin == Mode::Settings {
        Mode::Settings
    } else {
        Mode::Navigate
    }
}
