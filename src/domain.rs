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
            Self::Discord => "Discord bot",
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
}

impl Message {
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
