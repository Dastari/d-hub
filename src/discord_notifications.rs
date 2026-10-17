//! A notification-only bridge from the official Discord client. No user tokens.
use crate::{
    domain::{Focus, Message, MessageId, Provider, safe_text},
    policy::AccountConfig,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::mpsc::{Receiver, sync_channel},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub event_id: String,
    pub app: String,
    pub summary: String,
    pub body: String,
}

pub enum Event {
    Ready,
    Notification(Notification),
    Error(String),
}

pub fn matches(account: &AccountConfig, event: &Notification) -> bool {
    account.provider == Provider::Discord
        && account.discord_mode == "notifications"
        && !account.notification_sender.trim().is_empty()
        && event.app.eq_ignore_ascii_case(&account.notification_app)
        && safe_text(&event.summary)
            .trim()
            .eq_ignore_ascii_case(account.notification_sender.trim())
}

pub fn into_message(
    account: &AccountConfig,
    event: &Notification,
    id: MessageId,
    timestamp: u64,
) -> Option<Message> {
    if !matches(account, event) {
        return None;
    }
    Some(Message {
        id,
        timestamp,
        time: format!(
            "{:02}:{:02} UTC",
            timestamp / 3600 % 24,
            timestamp / 60 % 60
        ),
        sender: safe_text(&event.summary).chars().take(320).collect(),
        subject: "Discord personal notification · open official client to reply".into(),
        body: safe_text(&event.body).chars().take(12_000).collect(),
        focus: Focus::Personal,
        unread: true,
        incoming: true,
        target: None,
        remote: None,
        provider: Some(Provider::Discord),
        account: Some(account.id.clone()),
    })
}

pub fn start(accounts: Vec<AccountConfig>) -> Result<Receiver<Event>> {
    let accounts: Vec<_> = accounts
        .into_iter()
        .filter(|a| a.provider == Provider::Discord && a.discord_mode == "notifications")
        .collect();
    if accounts.is_empty()
        || accounts
            .iter()
            .any(|a| a.notification_sender.trim().is_empty())
    {
        bail!(
            "configure a Discord notification account and its exact notification_sender before starting capture"
        );
    }
    let (tx, rx) = sync_channel(128);
    std::thread::spawn(move || {
        let result = monitor(&accounts, |event| tx.send(event).is_ok());
        if let Err(error) = result {
            let _ = tx.send(Event::Error(format!("Discord notification capture unavailable: {error:#}. No desktop/bus policy was changed.")));
        }
    });
    Ok(rx)
}

fn monitor(accounts: &[AccountConfig], mut emit: impl FnMut(Event) -> bool) -> Result<()> {
    let connection = zbus::blocking::Connection::session()
        .context("could not connect to the desktop session bus")?;
    let bus_id: String = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetId",
            &(),
        )?
        .body()
        .deserialize()?;
    let mut apps = std::collections::BTreeSet::new();
    for account in accounts {
        apps.insert(account.notification_app.clone());
        apps.insert(account.notification_app.to_lowercase());
    }
    let mut rules = Vec::new();
    for app in apps {
        rules.push(
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::MethodCall)
                .interface("org.freedesktop.Notifications")?
                .member("Notify")?
                .add_arg(app)?
                .build()
                .to_string(),
        );
    }
    let mut messages = zbus::blocking::MessageIterator::from(&connection);
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Monitoring"),
            "BecomeMonitor",
            &(rules, 0_u32),
        )
        .context("session bus refused notification monitoring")?;
    if !emit(Event::Ready) {
        return Ok(());
    }
    for message in &mut messages {
        let message = message.context("desktop notification stream disconnected")?;
        let header = message.header();
        if message.message_type() != zbus::message::Type::MethodCall
            || header
                .interface()
                .is_none_or(|i| i.as_str() != "org.freedesktop.Notifications")
            || header.member().is_none_or(|m| m.as_str() != "Notify")
        {
            continue;
        }
        type Notify = (
            String,
            u32,
            String,
            String,
            String,
            Vec<String>,
            HashMap<String, zbus::zvariant::OwnedValue>,
            i32,
        );
        let Ok((app, _, _, summary, body, _, _, _)) = message.body().deserialize::<Notify>() else {
            continue;
        };
        let event = Notification {
            event_id: format!(
                "{}:{}:{}",
                bus_id,
                header.sender().map(|s| s.as_str()).unwrap_or("unknown"),
                message.primary_header().serial_num()
            ),
            app,
            summary,
            body,
        };
        if accounts.iter().any(|a| matches(a, &event)) && !emit(Event::Notification(event)) {
            break;
        }
    }
    Ok(())
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
