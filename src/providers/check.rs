//! Read-only connection checks for account setup. Nothing is synced, marked read or sent.
//! A Microsoft check refreshes the access token, which may rotate the stored refresh token
//! exactly as sync would.
use super::*;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    /// Discovered names (folders, chats) worth showing in full.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
}

impl Step {
    pub fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok: true,
            detail: crate::domain::safe_text(&detail.into()),
            items: vec![],
        }
    }
    pub fn failed(name: impl Into<String>, error: impl std::fmt::Display) -> Self {
        Self {
            name: name.into(),
            ok: false,
            detail: crate::domain::safe_text(&format!("{error:#}")),
            items: vec![],
        }
    }
    pub fn with_items(mut self, items: Vec<String>) -> Self {
        self.items = items
            .iter()
            .map(|item| crate::domain::safe_text(item))
            .collect();
        self
    }
}

/// Checks sign-in and the resources the account would sync.
pub fn run(account: &AccountConfig) -> Vec<Step> {
    match account.provider {
        Provider::ICloud => match SecretService.load(&account.credential_ref()) {
            Ok(password) => icloud::check(account, &password),
            Err(_) => vec![Step::failed(
                "Credential",
                "No app-specific password stored, or the keyring is locked; use Sign in",
            )],
        },
        Provider::Office365 | Provider::Teams => match graph::Graph::new() {
            Ok(mut graph) => graph.probe(account),
            Err(e) => vec![Step::failed("Setup", e)],
        },
        Provider::Discord => vec![Step::failed(
            "Check",
            "Not available for Discord; Connect and watch the account status instead",
        )],
    }
}
