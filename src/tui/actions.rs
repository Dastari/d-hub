//! Drawer actions: sends, attachments, reader windows and external hand-offs.
use super::*;

pub(super) fn send(app: &mut App) -> Option<(Instant, u64, MockOutcome)> {
    if (app.live_feed || app.live_setup) && !app.live_service {
        app.notice = "Live replies are not implemented · open the official client to reply.".into();
        return None;
    }
    if app.live_service {
        let Some(draft) = app.session.composer.draft.as_ref() else {
            app.notice = "Write a reply first.".into();
            return None;
        };
        if !app.settings.accounts.iter().any(|a| {
            a.id == draft.target.account
                && a.provider == draft.target.provider
                && a.enabled
                && a.allow_send
        }) {
            app.notice="Sending disabled · enable accounts.ID.allow_send in settings after checking the displayed recipient.".into();
            return None;
        }
    }
    app.notice = "No send started · write a reply, or resolve the pending outcome.".into();
    if let Some(operation) = app.submit() {
        if app.shared {
            app.notice = if app.live_service {
                "Reply queued with the live service."
            } else {
                "Mock send queued with the service · nothing will be delivered."
            }
            .into();
            return None;
        }
        app.notice = "Simulating send locally · no provider is connected.".into();
        Some((
            Instant::now() + Duration::from_millis(650),
            operation.operation,
            app.next_outcome,
        ))
    } else {
        None
    }
}

pub(super) fn finish_send(app: &mut App, operation: u64, outcome: MockOutcome) {
    match outcome {
        MockOutcome::Success => {
            app.accepted(operation);
            app.notice = "MOCK SENT · stored locally, nothing was delivered.".into();
        }
        MockOutcome::Failure => {
            app.session.composer.apply(ComposerEvent::Failed(
                operation,
                "Simulated transport failure".into(),
            ));
            app.notice =
                "MOCK FAILED · draft and recipient retained. F7 changes next result.".into();
        }
        MockOutcome::Uncertain => {
            app.session
                .composer
                .apply(ComposerEvent::Uncertain(operation));
            app.notice =
                "MOCK UNCERTAIN · retry disabled. :reconcile simulates confirmed acceptance."
                    .into();
        }
    }
    app.sync_editor();
    if !app.focused {
        app.follow_latest();
    }
}

pub(super) fn remove_last(app: &mut App) {
    if let Some(index) = app
        .session
        .composer
        .draft
        .as_ref()
        .and_then(|d| d.attachments.len().checked_sub(1))
    {
        app.remove_attachment(index);
    }
}

pub(super) fn launch_reader(app: &mut App, config_path: Option<&Path>) {
    if app.live_feed {
        app.open_reader();
        return;
    }
    let Some(message) = app.selected_message() else {
        return;
    };
    let result = std::env::current_exe().and_then(|binary| {
        let mut command = Command::new("ghostty");
        command.arg("-e").arg(binary);
        if let Some(path) = config_path {
            let path = if path.is_absolute() {
                path.to_owned()
            } else {
                std::env::current_dir()?.join(path)
            };
            command.arg("--config").arg(path);
        }
        command.arg("preview");
        if let Some(socket) = &app.service_socket {
            command.arg("--socket").arg(socket);
        } else {
            command.arg("--demo");
        }
        command.arg(message.id.0.to_string()).spawn()
    });
    match result {
        Ok(mut child) => {
            if let Some(id) = app.selected {
                app.mark_seen(id);
            }
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            app.notice = "Requested Ghostty reader · drawer draft preserved.".into();
        }
        Err(_) => {
            app.open_reader();
            app.notice = "Ghostty unavailable · opened in-process fixture reader.".into();
        }
    }
}

pub fn parse_provider(value: &str) -> Result<Provider> {
    match value {
        "icloud" => Ok(Provider::ICloud),
        "office365" => Ok(Provider::Office365),
        "teams" => Ok(Provider::Teams),
        "discord" => Ok(Provider::Discord),
        _ => bail!("provider must be icloud, office365, teams or discord"),
    }
}

pub(super) fn edit_setting(app: &mut App, name: &str) {
    settings::edit(app, name);
}

pub(super) fn open_discord(app: &mut App) {
    if app.selected_message().and_then(|m| m.provider()) != Some(Provider::Discord) {
        app.notice = "Select a Discord notification first.".into();
        return;
    }
    match Command::new("xdg-open")
        .arg("discord://-/channels/@me")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            app.notice = "Requested official Discord DM list · choose the conversation there for history and replies.".into();
        }
        Err(_) => {
            app.notice =
                "Could not open Discord · open its official client for history and replies.".into()
        }
    }
}

/// Opens a preview link; the notice names the real host, whatever the link text said.
pub(super) fn open_link(app: &mut App, url: &str) {
    app.notice = match links::open(url) {
        Ok(()) => format!("Opening {}", safe_text(&links::short(url))),
        Err(error) => format!("Could not open link · {error}"),
    };
}
