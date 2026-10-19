//! Provider boundary: credentials/network live only in daemon-owned workers.
pub mod check;
pub mod discord;
pub mod graph;
pub mod icloud;
pub mod worker;
use crate::{
    auth::{CredentialStore, SecretService},
    composer::Submission,
    domain::{Focus, Message, MessageId, Provider, ReplyMode, ReplyTarget, safe_text},
    policy::AccountConfig,
    read_state::RemoteMessage,
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const BODY_LIMIT: usize = 16_000;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub account_scope: String,
    pub remote: RemoteMessage,
    pub sender: String,
    pub recipients: Vec<String>,
    pub subject: String,
    pub body: String,
    pub timestamp: u64,
    pub unread: Option<bool>,
    pub message_id: String,
    pub references: String,
    pub deleted: bool,
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub signals: crate::domain::Signals,
}
impl Item {
    pub fn message(&self, id: MessageId, account: &AccountConfig) -> Message {
        let target = (!self.deleted && !self.recipients.is_empty()).then(|| ReplyTarget {
            provider: account.provider,
            account: account.id.clone(),
            conversation: match &self.remote {
                RemoteMessage::GraphChat { chat, .. } => chat.clone(),
                RemoteMessage::DiscordDm { channel, .. } => channel.clone(),
                _ => serde_json::to_string(&self.remote).expect("remote serializes"),
            },
            incoming: id,
            recipients: self.recipients.clone(),
            reply_mode: ReplyMode::Sender,
        });
        Message {
            account_scope: Some(self.account_scope.clone()),
            id,
            timestamp: self.timestamp,
            time: chrono::DateTime::from_timestamp(self.timestamp as i64, 0)
                .map(|t| {
                    t.with_timezone(&chrono::Local)
                        .format("%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_default(),
            sender: safe_text(&self.sender),
            subject: safe_text(&self.subject),
            body: text(&self.body),
            focus: Focus::All,
            unread: self.unread.unwrap_or(true),
            incoming: true,
            target,
            remote: if matches!(
                self.remote,
                RemoteMessage::GraphChat { .. } | RemoteMessage::DiscordDm { .. }
            ) {
                None
            } else {
                Some(self.remote.clone())
            },
            provider: Some(account.provider),
            account: Some(account.id.clone()),
            signals: self.signals.clone(),
        }
    }
}
pub fn text(value: &str) -> String {
    let clean = safe_text(value);
    if clean.len() <= BODY_LIMIT {
        return clean;
    }
    let mut end = BODY_LIMIT;
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    let kept = &clean[..end];
    // Rendered HTML lists its links at the end; keep those the retained text still uses.
    let mut references = String::new();
    for line in clean[end..].lines() {
        let Some((number, _)) = line.strip_prefix('[').and_then(|r| r.split_once("]: ")) else {
            continue;
        };
        if !number.is_empty()
            && number.bytes().all(|b| b.is_ascii_digit())
            && kept.contains(&format!("][{number}]"))
            && references.len() + line.len() < BODY_LIMIT / 2
        {
            references.push('\n');
            references.push_str(line);
        }
    }
    format!(
        "{kept}\n[Preview truncated; open the original client for complete content]\n{references}"
    )
}
pub fn html(value: &str) -> String {
    // No resource fetching: links and images are rendered as text. Raw mode reads table cells
    // in order without borders, since most mail uses tables for layout, not data.
    text(
        &html2text::config::plain()
            .raw_mode(true)
            .string_from_read(value.as_bytes(), 100)
            .unwrap_or_else(|_| "[Unable to render HTML]".into()),
    )
}
pub fn timestamp(value: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|v| v.timestamp().max(0) as u64)
        .unwrap_or(0)
}
pub fn scope(account: &AccountConfig) -> String {
    use sha2::{Digest, Sha256};
    let identity = (
        account.provider,
        &account.username,
        &account.tenant,
        &account.client_id,
        &account.resources,
    );
    let bytes = if account.provider == Provider::Discord {
        serde_json::to_vec(&(identity, &account.discord_mode, &account.discord_user_id))
    } else {
        // Preserve the existing mail/Teams key: changing it would reimport their inboxes.
        serde_json::to_vec(&identity)
    }
    .expect("account serializes");
    // Never reuse cursors when account identity/resources change.
    format!("{}:{:x}", account.id, Sha256::digest(bytes))
}
#[derive(Debug, Clone)]
pub struct Batch {
    pub items: Vec<Item>,
    pub cursor: Value,
    pub status: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendResult {
    Accepted,
    Failed(String),
    Uncertain,
}
pub trait Transport: Send {
    fn sync(&mut self, account: &AccountConfig, cursor: Value) -> Result<Batch>;
    fn unread(
        &mut self,
        account: &AccountConfig,
        remote: &RemoteMessage,
        unread: bool,
    ) -> Result<()>;
    fn send(&mut self, account: &AccountConfig, item: &Item, submission: &Submission)
    -> SendResult;
}
pub struct Network {
    pub graph: graph::Graph,
    pub discord: discord::Discord,
    pub icloud: icloud::Connection,
}
impl Network {
    pub fn new() -> Result<Self> {
        Ok(Self {
            graph: graph::Graph::new()?,
            discord: discord::Discord::new()?,
            icloud: icloud::Connection::default(),
        })
    }
}
impl Transport for Network {
    fn sync(&mut self, account: &AccountConfig, cursor: Value) -> Result<Batch> {
        match account.provider {
            Provider::ICloud => {
                let password = SecretService.load(&account.credential_ref())?;
                self.icloud.with(account, &password, |session| {
                    icloud::sync_session(account, session, cursor.clone())
                })
            }
            Provider::Office365 | Provider::Teams => self.graph.sync(account, cursor),
            Provider::Discord => self.discord.sync(account, cursor),
        }
    }
    fn unread(
        &mut self,
        account: &AccountConfig,
        remote: &RemoteMessage,
        unread: bool,
    ) -> Result<()> {
        match account.provider {
            Provider::ICloud => {
                let password = SecretService.load(&account.credential_ref())?;
                self.icloud.with(account, &password, |session| {
                    icloud::unread(session, account, remote, unread)
                })
            }
            Provider::Office365 => self.graph.unread(account, remote, unread),
            _ => bail!("Remote read state is unsupported for this provider"),
        }
    }
    fn send(
        &mut self,
        account: &AccountConfig,
        item: &Item,
        submission: &Submission,
    ) -> SendResult {
        if item.deleted
            || item
                .message(submission.draft.target.incoming, account)
                .target
                .as_ref()
                != Some(&submission.draft.target)
        {
            return SendResult::Failed(
                "Original reply identity changed or was removed; draft retained".into(),
            );
        }
        if item.account_scope != scope(account) {
            return SendResult::Failed("Account identity changed; original draft retained".into());
        }
        if !account.enabled || !account.allow_send {
            return SendResult::Failed("Sending disabled for this account".into());
        }
        if !submission.draft.attachments.is_empty() {
            return SendResult::Failed(
                "Live attachment sending is not available yet; remove attachments".into(),
            );
        }
        match account.provider {
            Provider::ICloud => match SecretService.load(&account.credential_ref()) {
                Ok(secret) => icloud::send(&mut self.icloud, account, &secret, item, submission),
                Err(_) => SendResult::Failed("Credential missing or keyring locked".into()),
            },
            Provider::Office365 | Provider::Teams => self.graph.send(account, item, submission),
            Provider::Discord => self.discord.send(account, item, submission),
        }
    }
}

#[cfg(test)]
mod html_tests {
    #[test]
    fn truncated_html_keeps_the_links_its_text_still_uses() {
        let filler = "word ".repeat(super::BODY_LIMIT / 5);
        let body = format!(
            "[early][1] {filler} [late][2]\n\n[1]: https://a.example/1\n[2]: https://a.example/2"
        );
        let text = super::text(&body);
        assert!(text.contains("[1]: https://a.example/1"), "kept link");
        assert!(!text.contains("[2]: "), "link only cut text used");
        assert!(text.contains("[Preview truncated"));
    }

    #[test]
    fn layout_tables_render_as_text_without_borders() {
        let rendered = super::html(
            "<table><tr><td>Your Sunday afternoon trip</td><td>$12.00</td></tr>\
             <tr><td colspan=2>Thanks for riding</td></tr></table>",
        );
        for text in ["Your Sunday afternoon trip", "$12.00", "Thanks for riding"] {
            assert!(rendered.contains(text), "{rendered}");
        }
        assert!(
            !rendered
                .chars()
                .any(|c| ('\u{2500}'..='\u{257F}').contains(&c)),
            "{rendered}"
        );
    }
}
