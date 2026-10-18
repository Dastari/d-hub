use crate::{
    domain::SourceFilter,
    policy::{self, AccountConfig, AiSettings, FocusProfile, SpamPolicy},
    storage::write_private,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    Midnight,
    Dusk,
}
impl Theme {
    pub fn next(self) -> Self {
        match self {
            Self::Midnight => Self::Dusk,
            Self::Dusk => Self::Midnight,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "midnight",
            Self::Dusk => "dusk",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub schema_version: u32,
    pub focus: String,
    pub source: SourceFilter,
    pub theme: Theme,
    pub reduced_motion: bool,
    pub insert_on_focus: bool,
    pub reply_first: bool,
    pub padding_horizontal: u16,
    pub padding_vertical: u16,
    pub show_dismissed: bool,
    pub show_spam: bool,
    pub mark_read_on_select: bool,
    pub dismiss_after_reply: bool,
    pub profiles: Vec<FocusProfile>,
    pub accounts: Vec<AccountConfig>,
    pub ai: AiSettings,
    pub spam: SpamPolicy,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            focus: "all".into(),
            source: SourceFilter::All,
            theme: Theme::Midnight,
            reduced_motion: false,
            insert_on_focus: true,
            reply_first: true,
            padding_horizontal: 0,
            padding_vertical: 0,
            show_dismissed: false,
            show_spam: false,
            mark_read_on_select: true,
            dismiss_after_reply: true,
            profiles: policy::default_profiles(),
            accounts: vec![],
            ai: AiSettings::default(),
            spam: SpamPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Key {
    Focus,
    Theme,
    ReducedMotion,
    InsertOnFocus,
    PaddingHorizontal,
    PaddingVertical,
    Source,
    ShowDismissed,
    ShowSpam,
    MarkReadOnSelect,
    DismissAfterReply,
    ReplyFirst,
}
pub struct Setting {
    pub key: Key,
    pub name: &'static str,
    pub help: &'static str,
    pub choices: &'static [&'static str],
}
const PADDING_CHOICES: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
];
pub const REGISTRY: &[Setting] = &[
    Setting {
        key: Key::ReplyFirst,
        name: "reply_first",
        help: "Keep typing in composer; arrows switch messages/sources, Alt+arrows edit text",
        choices: &["true", "false"],
    },
    Setting {
        key: Key::Focus,
        name: "focus",
        help: "Focus profile (independent of source)",
        choices: &["all", "work", "personal"],
    },
    Setting {
        key: Key::Theme,
        name: "theme",
        help: "Interface palette",
        choices: &["midnight", "dusk"],
    },
    Setting {
        key: Key::ReducedMotion,
        name: "reduced_motion",
        help: "Static colours and activity indicators",
        choices: &["false", "true"],
    },
    Setting {
        key: Key::InsertOnFocus,
        name: "insert_on_focus",
        help: "Enter insert mode when an empty drawer gains focus",
        choices: &["true", "false"],
    },
    Setting {
        key: Key::PaddingHorizontal,
        name: "padding_horizontal",
        help: "Outer left/right padding in terminal cells",
        choices: PADDING_CHOICES,
    },
    Setting {
        key: Key::PaddingVertical,
        name: "padding_vertical",
        help: "Outer top/bottom padding in terminal rows",
        choices: PADDING_CHOICES,
    },
    Setting {
        key: Key::Source,
        name: "source",
        help: "Source filter within the chronological feed",
        choices: &["all", "icloud", "discord", "teams", "email", "office365"],
    },
    Setting {
        key: Key::ShowDismissed,
        name: "show_dismissed",
        help: "Include dismissed messages (D)",
        choices: &["false", "true"],
    },
    Setting {
        key: Key::ShowSpam,
        name: "show_spam",
        help: "Include spam/promotion matches (S)",
        choices: &["false", "true"],
    },
    Setting {
        key: Key::MarkReadOnSelect,
        name: "mark_read_on_select",
        help: "Mark messages seen on explicit selection/open",
        choices: &["true", "false"],
    },
    Setting {
        key: Key::DismissAfterReply,
        name: "dismiss_after_reply",
        help: "Dismiss the answered item after accepted send",
        choices: &["true", "false"],
    },
];

pub struct SettingRow {
    pub name: String,
    pub value: String,
    pub help: String,
    pub choices: Vec<String>,
}
impl SettingRow {
    fn text(name: impl Into<String>, value: impl Into<String>, help: &str) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            help: help.into(),
            choices: vec![],
        }
    }
    fn choice(
        name: impl Into<String>,
        value: impl Into<String>,
        help: &str,
        choices: &[&str],
    ) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            help: help.into(),
            choices: choices.iter().map(|s| (*s).into()).collect(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let value: Self = match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).context("invalid configuration")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        value.validate()?;
        Ok(value)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        write_private(path, toml::to_string_pretty(self)?.as_bytes())
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("unsupported configuration schema {}", self.schema_version);
        }
        if self.focus != "all" && !self.profiles.iter().any(|p| p.id == self.focus) {
            bail!("unknown focus profile {}", self.focus);
        }
        for s in REGISTRY.iter().filter(|s| !matches!(s.key, Key::Focus)) {
            if !s.choices.contains(&self.get(s.key).as_str()) {
                bail!("{} must be one of {}", s.name, s.choices.join(", "));
            }
        }
        policy::validate(&self.profiles, &self.accounts, &self.ai, &self.spam)
    }
    pub fn get(&self, key: Key) -> String {
        match key {
            Key::ReplyFirst => self.reply_first.to_string(),
            Key::Focus => self.focus.clone(),
            Key::Source => self.source.value().into(),
            Key::Theme => self.theme.label().into(),
            Key::ReducedMotion => self.reduced_motion.to_string(),
            Key::InsertOnFocus => self.insert_on_focus.to_string(),
            Key::PaddingHorizontal => self.padding_horizontal.to_string(),
            Key::PaddingVertical => self.padding_vertical.to_string(),
            Key::ShowDismissed => self.show_dismissed.to_string(),
            Key::ShowSpam => self.show_spam.to_string(),
            Key::MarkReadOnSelect => self.mark_read_on_select.to_string(),
            Key::DismissAfterReply => self.dismiss_after_reply.to_string(),
        }
    }
    pub fn profile_label(&self) -> &str {
        if self.focus == "all" {
            "All"
        } else {
            self.profiles
                .iter()
                .find(|p| p.id == self.focus)
                .map_or("All", |p| p.name.as_str())
        }
    }
    pub fn cycle_focus(&self) -> String {
        let ids: Vec<_> = std::iter::once("all")
            .chain(self.profiles.iter().map(|p| p.id.as_str()))
            .collect();
        ids[(ids.iter().position(|s| *s == self.focus).unwrap_or(0) + 1) % ids.len()].into()
    }
    pub fn entries(&self) -> Vec<SettingRow> {
        let mut rows: Vec<_> = REGISTRY
            .iter()
            .map(|s| SettingRow::choice(s.name, self.get(s.key), s.help, s.choices))
            .collect();
        rows.iter_mut()
            .find(|r| r.name == "focus")
            .expect("focus registry row")
            .choices = std::iter::once("all".into())
            .chain(self.profiles.iter().map(|p| p.id.clone()))
            .collect();
        rows.push(SettingRow::text(
            "profiles",
            serde_json::to_string_pretty(&self.profiles).expect("profiles serialize"),
            "JSON profile list; :profile add ID creates one",
        ));
        for p in &self.profiles {
            rows.push(SettingRow::text(
                format!("profiles.{}.name", p.id),
                &p.name,
                "Profile display name",
            ));
            rows.push(SettingRow::text(
                format!("profiles.{}.prompt", p.id),
                &p.prompt,
                "Decision prompt for this profile",
            ));
            rows.push(SettingRow::text(format!("profiles.{}.terms", p.id), p.terms.join("\n"), "Optional local include terms, one per line; empty uses fixture tags or shows unclassified"));
        }
        rows.extend([
            SettingRow::choice(
                "ai.enabled",
                self.ai.enabled.to_string(),
                "External decisions need explicit per-account permission too",
                &["false", "true"],
            ),
            SettingRow::choice(
                "ai.provider",
                self.ai.provider.value(),
                "Jev or OpenAI Decisions",
                &["jev", "openai"],
            ),
            SettingRow::text("ai.model", &self.ai.model, "Provider model ID"),
            SettingRow::text(
                "ai.threshold",
                self.ai.threshold.to_string(),
                "Profile match probability threshold (0–1)",
            ),
            SettingRow::text(
                "ai.timeout_ms",
                self.ai.timeout_ms.to_string(),
                "Timeout in milliseconds (100–30000)",
            ),
            SettingRow::choice(
                "ai.hold_pending",
                self.ai.hold_pending.to_string(),
                "Hold eligible messages while a decision is pending; failures remain recoverable",
                &["false", "true"],
            ),
            SettingRow::choice(
                "spam.enabled",
                self.spam.enabled.to_string(),
                "Apply global spam policy before profile filtering",
                &["true", "false"],
            ),
            SettingRow::text(
                "spam.prompt",
                &self.spam.prompt,
                "Independent spam/promotional-content decision prompt",
            ),
            SettingRow::text(
                "spam.threshold",
                self.spam.threshold.to_string(),
                "Spam probability threshold (0–1)",
            ),
            SettingRow::text(
                "spam.local_terms",
                self.spam.local_terms.join("\n"),
                "Optional local exclusion terms, one per line; no AI needed",
            ),
            SettingRow::text(
                "accounts",
                serde_json::to_string_pretty(&self.accounts).expect("accounts serialize"),
                "JSON account list; :account add PROVIDER ID creates one; secrets never go here",
            ),
        ]);
        for a in &self.accounts {
            for (field, value, help) in [
                (
                    "enabled",
                    a.enabled,
                    "Connect this account in the live service",
                ),
                (
                    "allow_send",
                    a.allow_send,
                    "Permit explicit replies from this account",
                ),
            ] {
                rows.push(SettingRow::choice(
                    format!("accounts.{}.{field}", a.id),
                    value.to_string(),
                    help,
                    &["false", "true"],
                ));
            }
            rows.push(SettingRow::text(
                format!("accounts.{}.poll_seconds", a.id),
                a.poll_seconds.to_string(),
                "Sync interval, 15-3600 seconds",
            ));
            if a.provider == crate::domain::Provider::Discord {
                rows.push(SettingRow::choice(
                    format!("accounts.{}.discord_mode", a.id),
                    &a.discord_mode,
                    "Notification-only personal DM bridge; full personal client unsupported",
                    &["notifications", "personal", "bot"],
                ));
                rows.push(SettingRow::text(format!("accounts.{}.notification_sender", a.id), &a.notification_sender, "Exact notification title/name to include (e.g. your son's Discord display name)"));
                rows.push(SettingRow::text(
                    format!("accounts.{}.notification_app", a.id),
                    &a.notification_app,
                    "Desktop notification app name (default Discord)",
                ));
            }
            for (field, value, help) in [
                (
                    "username",
                    a.username.clone(),
                    "Full iCloud email address or signed-in Microsoft email/UPN; pins account identity",
                ),
                (
                    "tenant",
                    a.tenant.clone(),
                    "Microsoft tenant ID/domain or organizations/common/consumers",
                ),
                (
                    "client_id",
                    a.client_id.clone(),
                    "Microsoft public-client application ID (no client secret)",
                ),
                (
                    "resources",
                    a.resources.join("\n"),
                    "Authorised channel/chat resources, one per line",
                ),
            ] {
                rows.push(SettingRow::text(
                    format!("accounts.{}.{field}", a.id),
                    value,
                    help,
                ));
            }
            rows.push(SettingRow::choice(
                format!("accounts.{}.allow_ai", a.id),
                a.allow_ai.to_string(),
                "Allow bounded text from this account to the selected AI provider",
                &["false", "true"],
            ));
        }
        rows
    }
    pub fn value(&self, name: &str) -> Result<String> {
        self.entries()
            .into_iter()
            .find(|s| s.name == name)
            .map(|s| s.value)
            .with_context(|| format!("unknown setting {name}"))
    }
    pub fn set(&mut self, name: &str, value: &str) -> Result<()> {
        let mut next = self.clone();
        next.set_inner(name, value)?;
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn set_inner(&mut self, name: &str, value: &str) -> Result<()> {
        if let Some(s) = REGISTRY.iter().find(|s| s.name == name) {
            if !matches!(s.key, Key::Focus) && !s.choices.contains(&value) {
                bail!("{name} must be one of {}", s.choices.join(", "));
            }
            match s.key {
                Key::ReplyFirst => self.reply_first = value.parse()?,
                Key::Focus => {
                    self.focus = if value == "home" && !self.profiles.iter().any(|p| p.id == "home")
                    {
                        "personal".into()
                    } else {
                        value.into()
                    }
                }
                Key::Theme => {
                    self.theme = if value == "dusk" {
                        Theme::Dusk
                    } else {
                        Theme::Midnight
                    }
                }
                Key::Source => {
                    self.source = SourceFilter::ALL
                        .into_iter()
                        .find(|s| s.value() == value)
                        .context("invalid source")?
                }
                Key::ReducedMotion => self.reduced_motion = value.parse()?,
                Key::InsertOnFocus => self.insert_on_focus = value.parse()?,
                Key::PaddingHorizontal => self.padding_horizontal = value.parse()?,
                Key::PaddingVertical => self.padding_vertical = value.parse()?,
                Key::ShowDismissed => self.show_dismissed = value.parse()?,
                Key::ShowSpam => self.show_spam = value.parse()?,
                Key::MarkReadOnSelect => self.mark_read_on_select = value.parse()?,
                Key::DismissAfterReply => self.dismiss_after_reply = value.parse()?,
            }
            return Ok(());
        }
        let terms = || {
            value
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        match name {
            "profiles" => {
                self.profiles =
                    serde_json::from_str(value).context("profiles must be a JSON list")?
            }
            "accounts" => {
                self.accounts =
                    serde_json::from_str(value).context("accounts must be a JSON list")?
            }
            "ai.enabled" => self.ai.enabled = value.parse()?,
            "ai.provider" => {
                self.ai.provider = match value {
                    "jev" => policy::DecisionProvider::Jev,
                    "openai" => policy::DecisionProvider::OpenAI,
                    _ => bail!("AI provider must be jev or openai"),
                };
                self.ai.model = self.ai.provider.default_model().into();
            }
            "ai.model" => self.ai.model = value.into(),
            "ai.threshold" => self.ai.threshold = value.parse()?,
            "ai.timeout_ms" => self.ai.timeout_ms = value.parse()?,
            "ai.hold_pending" => self.ai.hold_pending = value.parse()?,
            "spam.enabled" => self.spam.enabled = value.parse()?,
            "spam.prompt" => self.spam.prompt = value.into(),
            "spam.threshold" => self.spam.threshold = value.parse()?,
            "spam.local_terms" => self.spam.local_terms = terms(),
            _ => {
                let parts: Vec<_> = name.split('.').collect();
                match parts.as_slice() {
                    ["profiles", id, field] => {
                        let p = self
                            .profiles
                            .iter_mut()
                            .find(|p| p.id == *id)
                            .context("unknown profile")?;
                        match *field {
                            "name" => p.name = value.into(),
                            "prompt" => p.prompt = value.into(),
                            "terms" => p.terms = terms(),
                            _ => bail!("unknown profile field"),
                        }
                    }
                    ["accounts", id, field] => {
                        let a = self
                            .accounts
                            .iter_mut()
                            .find(|a| a.id == *id)
                            .context("unknown account")?;
                        match *field {
                            "enabled" => a.enabled = value.parse()?,
                            "allow_send" => a.allow_send = value.parse()?,
                            "poll_seconds" => a.poll_seconds = value.parse()?,
                            "username" => a.username = value.into(),
                            "tenant" => a.tenant = value.into(),
                            "client_id" => a.client_id = value.into(),
                            "resources" => a.resources = terms(),
                            "allow_ai" => a.allow_ai = value.parse()?,
                            "discord_mode" => a.discord_mode = value.into(),
                            "notification_sender" => a.notification_sender = value.into(),
                            "notification_app" => a.notification_app = value.into(),
                            _ => bail!(
                                "unknown account field; secrets belong in the credential store"
                            ),
                        }
                    }
                    _ => bail!("unknown setting {name}"),
                }
            }
        }
        Ok(())
    }
    pub fn add_profile(&mut self, id: &str) -> Result<()> {
        let mut profiles = self.profiles.clone();
        profiles.push(FocusProfile {
            id: id.into(),
            name: id.into(),
            prompt: format!("Does this communication belong to my {id} focus?"),
            terms: vec![],
            fixture_focus: None,
        });
        self.set("profiles", &serde_json::to_string(&profiles)?)
    }
    pub fn remove_profile(&mut self, id: &str) -> Result<()> {
        if !self.profiles.iter().any(|p| p.id == id) {
            bail!("unknown profile {id}");
        }
        let mut next = self.clone();
        next.profiles.retain(|p| p.id != id);
        if next.focus == id {
            next.focus = "all".into();
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn add_account(&mut self, id: &str, provider: crate::domain::Provider) -> Result<()> {
        let mut accounts = self.accounts.clone();
        accounts.push(AccountConfig::new(id, provider));
        self.set("accounts", &serde_json::to_string(&accounts)?)
    }
}

pub fn setting(name: &str) -> Result<&'static Setting> {
    REGISTRY
        .iter()
        .find(|s| s.name == name)
        .with_context(|| format!("unknown setting {name}"))
}
pub fn default_path() -> Result<PathBuf> {
    let directory = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(
            std::env::var_os("HOME")
                .context("HOME or XDG_CONFIG_HOME is required; use --config")?,
        )
        .join(".config"),
    };
    Ok(directory.join("d-hub/config.toml"))
}
