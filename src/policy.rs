use crate::domain::{Focus, Message, Provider};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusProfile {
    pub id: String,
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub terms: Vec<String>,
    #[serde(default)]
    pub fixture_focus: Option<Focus>,
}

pub fn default_profiles() -> Vec<FocusProfile> {
    vec![
        FocusProfile { id: "work".into(), name: "Work".into(), prompt: "Does this communication concern my work, clients, projects or professional responsibilities?".into(), terms: vec![], fixture_focus: Some(Focus::Work) },
        FocusProfile { id: "personal".into(), name: "Home".into(), prompt: "Does this communication concern my personal life, home, family, friends or leisure?".into(), terms: vec![], fixture_focus: Some(Focus::Personal) },
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpamPolicy {
    pub enabled: bool,
    pub prompt: String,
    pub threshold: f64,
    pub local_terms: Vec<String>,
}

impl Default for SpamPolicy {
    fn default() -> Self {
        Self { enabled: true, prompt: "Is this unsolicited spam, a promotional email, marketing newsletter or sales offer rather than a personal or transactional communication? Do not exclude receipts, security alerts or messages from actual people just because they contain a link.".into(), threshold: 0.85, local_terms: vec![] }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionProvider {
    #[default]
    Jev,
    OpenAI,
}

impl DecisionProvider {
    pub fn value(self) -> &'static str {
        match self {
            Self::Jev => "jev",
            Self::OpenAI => "openai",
        }
    }
    pub fn default_model(self) -> &'static str {
        match self {
            Self::Jev => "jev-latest",
            Self::OpenAI => "gpt-6-luna",
        }
    }
    pub fn credential_ref(self) -> &'static str {
        match self {
            Self::Jev => "ai-jev",
            Self::OpenAI => "ai-openai",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AiSettings {
    pub enabled: bool,
    pub provider: DecisionProvider,
    pub model: String,
    pub threshold: f64,
    pub timeout_ms: u64,
    pub hold_pending: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: DecisionProvider::Jev,
            model: "jev-latest".into(),
            threshold: 0.6,
            timeout_ms: 5000,
            hold_pending: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allow_send: bool,
    #[serde(default = "default_poll_seconds")]
    pub poll_seconds: u64,
    pub id: String,
    pub provider: Provider,
    #[serde(default)]
    pub username: String,
    #[serde(default = "default_tenant")]
    pub tenant: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub allow_ai: bool,
    #[serde(default = "default_discord_mode")]
    pub discord_mode: String,
    #[serde(default)]
    pub notification_sender: String,
    #[serde(default = "discord_app")]
    pub notification_app: String,
}

fn default_poll_seconds() -> u64 {
    30
}
fn default_tenant() -> String {
    "organizations".into()
}
fn default_discord_mode() -> String {
    "personal".into()
}
fn discord_app() -> String {
    "Discord".into()
}

impl AccountConfig {
    pub fn new(id: &str, provider: Provider) -> Self {
        Self {
            id: id.into(),
            provider,
            enabled: false,
            allow_send: false,
            poll_seconds: default_poll_seconds(),
            username: String::new(),
            tenant: default_tenant(),
            client_id: String::new(),
            resources: vec![],
            allow_ai: false,
            discord_mode: default_discord_mode(),
            notification_sender: String::new(),
            notification_app: discord_app(),
        }
    }
    pub fn credential_ref(&self) -> String {
        format!("account-{}", self.id)
    }
    pub fn auth_description(&self) -> &'static str {
        match self.provider {
            Provider::ICloud => "iCloud app-specific password · IMAP/SMTP",
            Provider::Discord if self.discord_mode == "notifications" => {
                "Official Discord desktop notifications, filtered by sender; no Discord credentials, remote unread or in-app replies"
            }
            Provider::Discord if self.discord_mode == "personal" => {
                "Full personal Discord client required but unsupported by Discord's official API; QR/password/user-token login unavailable"
            }
            Provider::Discord => {
                "Discord bot token · authorised bot resources only; does not satisfy personal DM requirements"
            }
            Provider::Office365 | Provider::Teams => {
                "Microsoft delegated OAuth device login · tenant and public-client app registration required"
            }
        }
    }
    pub fn authentication_supported(&self) -> bool {
        self.provider != Provider::Discord || self.discord_mode == "bot"
    }
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 48
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

pub fn probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

pub fn validate(
    profiles: &[FocusProfile],
    accounts: &[AccountConfig],
    ai: &AiSettings,
    spam: &SpamPolicy,
) -> Result<()> {
    if profiles.len() > 32 || accounts.len() > 32 {
        bail!("at most 32 focus profiles and 32 accounts are supported");
    }
    let mut ids = std::collections::BTreeSet::new();
    for profile in profiles {
        if !identifier(&profile.id) || profile.id == "all" || !ids.insert(&profile.id) {
            bail!("profile IDs must be unique simple identifiers, excluding all");
        }
        if profile.name.trim().is_empty()
            || profile.name.len() > 80
            || profile.prompt.trim().is_empty()
            || profile.prompt.len() > 8192
        {
            bail!("profile names need 1–80 bytes and prompts 1–8192 bytes");
        }
        validate_terms(&profile.terms)?;
    }
    ids.clear();
    for account in accounts {
        if !(15..=3600).contains(&account.poll_seconds) {
            bail!("account poll_seconds must be 15-3600");
        }
        if !identifier(&account.id) || !ids.insert(&account.id) {
            bail!("account IDs must be unique simple identifiers");
        }
        if account.username.len() > 320
            || account.client_id.len() > 128
            || account.resources.len() > 100
            || account
                .resources
                .iter()
                .any(|r| r.is_empty() || r.len() > 256)
        {
            bail!("account configuration exceeds its bounds");
        }
        if !["notifications", "personal", "bot"].contains(&account.discord_mode.as_str()) {
            bail!("Discord mode must be notifications, personal or bot");
        }
        if account.notification_sender.len() > 320
            || account.notification_app.trim().is_empty()
            || account.notification_app.len() > 128
        {
            bail!("invalid Discord notification sender/application");
        }
        if account.tenant.is_empty()
            || account.tenant.len() > 255
            || !account
                .tenant
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.'))
        {
            bail!("invalid Microsoft tenant");
        }
    }
    if !probability(ai.threshold)
        || !probability(spam.threshold)
        || !(100..=30_000).contains(&ai.timeout_ms)
    {
        bail!("thresholds must be 0–1; AI timeout must be 100–30000 ms");
    }
    if ai.model.trim().is_empty()
        || ai.model.len() > 128
        || spam.prompt.trim().is_empty()
        || spam.prompt.len() > 8192
    {
        bail!("model and spam prompt must be nonempty and bounded");
    }
    validate_terms(&spam.local_terms)
}

fn validate_terms(terms: &[String]) -> Result<()> {
    if terms.len() > 100 || terms.iter().any(|t| t.trim().is_empty() || t.len() > 256) {
        bail!("terms must contain at most 100 nonempty entries of at most 256 bytes");
    }
    Ok(())
}

pub fn matches_terms(message: &Message, terms: &[String]) -> bool {
    let content = format!("{} {} {}", message.sender, message.subject, message.body).to_lowercase();
    terms
        .iter()
        .any(|term| content.contains(&term.to_lowercase()))
}

pub fn fingerprint(message: &Message, settings: &crate::config::Settings) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&(
        1,
        message.id,
        &message.sender,
        &message.subject,
        &message.body,
        &message.target,
        message.provider(),
        message.account(),
        settings.accounts.iter().find(|a| {
            Some(a.id.as_str()) == message.account() && Some(a.provider) == message.provider()
        }),
        &settings.profiles,
        &settings.ai,
        &settings.spam,
    ))
    .expect("classification input serializes");
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Classification {
    pub fingerprint: String,
    pub provider: DecisionProvider,
    pub profiles: BTreeMap<String, f64>,
    pub spam: f64,
}
