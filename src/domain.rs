use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MessageId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    ICloud,
    Office365,
    Teams,
    Discord,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::ICloud => "iCloud",
            Self::Office365 => "Office 365",
            Self::Teams => "Teams",
            Self::Discord => "Discord",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFilter {
    #[default]
    All,
    Discord,
    Teams,
    Email,
    ICloud,
    Office365,
}

impl SourceFilter {
    pub const ALL: [Self; 6] = [
        Self::All,
        Self::ICloud,
        Self::Discord,
        Self::Teams,
        Self::Email,
        Self::Office365,
    ];

    pub fn value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Discord => "discord",
            Self::Teams => "teams",
            Self::Email => "email",
            Self::ICloud => "icloud",
            Self::Office365 => "office365",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Discord => "Discord",
            Self::Teams => "Teams",
            Self::Email => "Email",
            Self::ICloud => "iCloud",
            Self::Office365 => "Office 365",
        }
    }

    pub fn cycle(self, backwards: bool) -> Self {
        let index = Self::ALL
            .iter()
            .position(|source| *source == self)
            .unwrap_or(0);
        let offset = if backwards { Self::ALL.len() - 1 } else { 1 };
        Self::ALL[(index + offset) % Self::ALL.len()]
    }

    pub fn matches(self, provider: Option<Provider>) -> bool {
        match self {
            Self::All => true,
            Self::Discord => provider == Some(Provider::Discord),
            Self::Teams => provider == Some(Provider::Teams),
            Self::Email => matches!(provider, Some(Provider::ICloud | Provider::Office365)),
            Self::ICloud => provider == Some(Provider::ICloud),
            Self::Office365 => provider == Some(Provider::Office365),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Focus {
    #[default]
    All,
    Work,
    Personal,
}

impl Focus {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Work => "work",
            Self::Personal => "personal",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Work,
            Self::Work => Self::Personal,
            Self::Personal => Self::All,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplyMode {
    Sender,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyTarget {
    pub provider: Provider,
    pub account: String,
    pub conversation: String,
    pub incoming: MessageId,
    pub recipients: Vec<String>,
    pub reply_mode: ReplyMode,
}

impl ReplyTarget {
    pub fn draft_key(&self) -> String {
        // Include provider, account, conversation and reply mode without delimiter ambiguity.
        serde_json::to_string(&(
            self.provider,
            &self.account,
            &self.conversation,
            &self.reply_mode,
        ))
        .expect("target tuple is serializable")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub timestamp: u64,
    pub time: String,
    pub sender: String,
    pub subject: String,
    pub body: String,
    pub focus: Focus,
    pub unread: bool,
    pub incoming: bool,
    pub target: Option<ReplyTarget>,
    #[serde(default)]
    pub remote: Option<crate::read_state::RemoteMessage>,
    #[serde(default)]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub account: Option<String>,
}

impl Message {
    pub fn provider(&self) -> Option<Provider> {
        self.provider
            .or_else(|| self.target.as_ref().map(|t| t.provider))
    }
    pub fn account(&self) -> Option<&str> {
        self.account
            .as_deref()
            .or_else(|| self.target.as_ref().map(|t| t.account.as_str()))
    }
    pub fn matches_source(&self, source: SourceFilter) -> bool {
        source.matches(self.provider())
    }

    pub fn matches(&self, focus: Focus) -> bool {
        focus == Focus::All || self.focus == focus
    }

    pub fn eligible(&self, focus: Focus) -> bool {
        self.incoming && self.target.is_some() && self.matches(focus)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub bytes: u64,
    pub mime: String,
}

/// Strip ANSI/OSC sequences and control characters before any terminal rendering.
pub fn safe_text(text: &str) -> String {
    strip_ansi_escapes::strip_str(text.replace("\r\n", "\n"))
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .replace('\t', "    ")
}
