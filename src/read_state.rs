use crate::domain::MessageId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteMessage {
    GraphChat {
        account: String,
        chat: String,
        message: String,
    },
    Imap {
        account: String,
        mailbox: String,
        uid_validity: u32,
        uid: u32,
    },
    Graph {
        account: String,
        message: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageState {
    pub seen: bool,
    pub dismissed: bool,
    pub revision: u64,
    pub classification: Option<crate::policy::Classification>,
    pub classification_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadChange {
    #[serde(default)]
    pub account_scope: Option<String>,
    pub message: MessageId,
    pub remote: RemoteMessage,
    pub unread: bool,
    pub revision: u64,
    pub error: Option<String>,
}

/// Implemented by the single provider owner; acknowledgements are revision checked.
pub trait ReadTransport {
    fn set_unread(&mut self, remote: &RemoteMessage, unread: bool) -> Result<(), String>;
}
