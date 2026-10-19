//! Connection health and cached-message statistics for account rows and pages.
use super::*;
use crate::policy::AccountConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    Off,
    Pending,
    Ok,
    Problem,
}

pub(crate) fn health(app: &App, account: &AccountConfig) -> (Health, String) {
    if !account.enabled {
        return (Health::Off, "Disconnected".into());
    }
    let Some(status) = app.provider_status.get(&account.id) else {
        return if app.live_service {
            (Health::Pending, "Waiting for the service".into())
        } else {
            (
                Health::Pending,
                "Enabled · connects in the live service".into(),
            )
        };
    };
    let lower = status.to_lowercase();
    let state = if lower == "disabled" {
        Health::Off
    } else if lower.contains("disconnect") || lower.contains("fail") || lower.contains("error") {
        Health::Problem
    } else if lower.starts_with("connected")
        || lower.contains(" connected")
        || lower.starts_with("reply accepted")
    {
        Health::Ok
    } else if lower.contains("connecting")
        || lower.contains("waiting")
        || lower.contains("catching up")
    {
        Health::Pending
    } else {
        Health::Problem
    };
    (state, safe_text(status))
}

pub(crate) fn health_style(state: Health, p: Palette) -> (&'static str, Color) {
    match state {
        Health::Ok => ("●", p.success),
        Health::Pending => ("◌", p.accent),
        Health::Off => ("○", p.muted),
        Health::Problem => ("!", p.warning),
    }
}

pub(super) struct Stats {
    pub(super) total: usize,
    pub(super) unread: usize,
    pub(super) latest: Option<String>,
}

pub(super) fn stats(app: &App, id: &str) -> Stats {
    let messages: Vec<_> = app
        .session
        .messages
        .iter()
        .filter(|m| m.account() == Some(id))
        .collect();
    Stats {
        total: messages.len(),
        unread: messages.iter().filter(|m| m.unread && m.incoming).count(),
        latest: messages
            .iter()
            .max_by_key(|m| (m.timestamp, m.id))
            .map(|m| crate::when::short(m.timestamp, &safe_text(&m.time))),
    }
}

pub(super) fn signed_in(app: &App, account: &AccountConfig) -> bool {
    if account.provider == Provider::Discord && account.discord_mode == "personal" {
        !account.discord_user_id.is_empty()
    } else {
        account.enabled && health(app, account).0 == Health::Ok
    }
}

pub(super) fn short_status(state: Health, status: &str) -> String {
    match state {
        Health::Ok => "connected".into(),
        Health::Off => "disconnected".into(),
        Health::Pending => "connecting".into(),
        Health::Problem => status.split(" · ").next().unwrap_or(status).into(),
    }
}
