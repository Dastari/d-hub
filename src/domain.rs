use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct MessageId(pub u64);

// JSON object keys become strings when buffered inside a tagged IPC response.
impl<'de> Deserialize<'de> for MessageId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct IdVisitor;
        impl serde::de::Visitor<'_> for IdVisitor {
            type Value = MessageId;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an unsigned message ID or decimal map key")
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(MessageId(value))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map(MessageId).map_err(E::custom)
            }
        }
        deserializer.deserialize_any(IdVisitor)
    }
}

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
        // Each incoming message owns its reply, including within the same conversation.
        serde_json::to_string(&(
            self.provider,
            &self.account,
            &self.conversation,
            self.incoming,
            &self.reply_mode,
        ))
        .expect("target tuple is serializable")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub account_scope: Option<String>,
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
    pub fn is_mail(&self) -> bool {
        matches!(
            self.provider(),
            Some(Provider::ICloud | Provider::Office365)
        )
    }
    /// Adjacent messages with equal keys stack under one feed heading. Mail groups by sender;
    /// chats also by conversation, so a group DM never merges with a 1:1.
    pub fn stack_key(&self) -> (Option<Provider>, Option<&str>, Option<&str>, &str) {
        let conversation = (!self.is_mail())
            .then(|| self.target.as_ref().map(|t| t.conversation.as_str()))
            .flatten();
        (
            self.provider(),
            self.account(),
            conversation,
            self.sender.as_str(),
        )
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

/// Strip ANSI/OSC sequences, control characters and invisible formatting characters before any
/// terminal rendering.
pub fn safe_text(text: &str) -> String {
    strip_ansi_escapes::strip_str(text.replace("\r\n", "\n"))
        .chars()
        .map(|c| match c {
            '\u{2028}' | '\u{2029}' => '\n',
            c => c,
        })
        .filter(|c| (!c.is_control() || *c == '\n' || *c == '\t') && !invisible(*c))
        .collect::<String>()
        .replace('\t', "    ")
}

/// Formatting characters that draw nothing but that terminals and the layout's width tables can
/// disagree about (soft hyphens, zero-width spaces, fillers, bidi overrides). Mail preheaders
/// are full of them; one disagreement shifts the rest of the row and leaves stale cells behind.
/// The emoji joiner and emoji/text presentation selectors stay.
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{17B4}'
            | '\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'
            | '\u{200C}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0D}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// A line drawn only of table borders or rules ("────", "|", "-----"), as older HTML renders
/// and plain-text mail produce; previews skip it.
pub fn rule_line(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty()
        && line.chars().all(|c| {
            matches!(
                c,
                '\u{2500}'..='\u{259F}' | '-' | '=' | '_' | '|' | '+' | '*' | '~' | ' '
            )
        })
}

/// The name part of "Name <address>" (or the address alone), for headings and notices.
pub fn display_name(sender: &str) -> String {
    let sender = safe_text(sender);
    let trimmed = sender.trim();
    let Some((name, address)) = trimmed
        .strip_suffix('>')
        .and_then(|rest| rest.rsplit_once('<'))
    else {
        return trimmed.to_owned();
    };
    let name = name.trim().trim_matches('"').trim();
    if name.is_empty() {
        address.trim().to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_formatting_never_reaches_the_terminal_but_emoji_sequences_survive() {
        let preheader = "Sale\u{00AD}\u{034F}\u{200C}\u{200B} now\u{202E}evil\u{FEFF}\u{2028}next";
        assert_eq!(safe_text(preheader), "Sale nowevil\nnext");
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} \u{2764}\u{FE0F}";
        assert_eq!(safe_text(family), family);
        assert_eq!(safe_text("\u{1b}[31mred\u{7}"), "red");
    }

    #[test]
    fn senders_are_shown_by_name_with_the_address_as_fallback() {
        assert_eq!(display_name("Apple <noreply@email.apple.com>"), "Apple");
        assert_eq!(
            display_name("\"Hi-Fi, JB\" <deals@example.invalid>"),
            "Hi-Fi, JB"
        );
        assert_eq!(
            display_name("<only@example.invalid>"),
            "only@example.invalid"
        );
        assert_eq!(display_name("AWildWarrior"), "AWildWarrior");
        assert_eq!(display_name("a <b> c"), "a <b> c");
    }
}
