//! Settings window navigation state. Window-local: never persisted or sent to the service.

/// Top-level settings groups; window-local navigation, never persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Accounts,
    General,
    Feed,
    Profiles,
    Ai,
    Spam,
}

impl SettingsSection {
    pub const ALL: [Self; 6] = [
        Self::Accounts,
        Self::General,
        Self::Feed,
        Self::Profiles,
        Self::Ai,
        Self::Spam,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Accounts => "Accounts",
            Self::General => "General",
            Self::Feed => "Feed & composer",
            Self::Profiles => "Focus profiles",
            Self::Ai => "AI decisions",
            Self::Spam => "Filtering",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SettingsPage {
    #[default]
    Home,
    Section(SettingsSection),
    Account(String),
    AddAccount,
    RemoveAccount(String),
    Profile(String),
}

impl SettingsPage {
    pub fn parent(&self) -> Option<Self> {
        match self {
            Self::Home => None,
            Self::Section(_) => Some(Self::Home),
            Self::Account(_) | Self::AddAccount => Some(Self::Section(SettingsSection::Accounts)),
            Self::RemoveAccount(id) => Some(Self::Account(id.clone())),
            Self::Profile(_) => Some(Self::Section(SettingsSection::Profiles)),
        }
    }
    pub fn section(&self) -> Option<SettingsSection> {
        match self {
            Self::Home => None,
            Self::Section(section) => Some(*section),
            Self::Account(_) | Self::AddAccount | Self::RemoveAccount(_) => {
                Some(SettingsSection::Accounts)
            }
            Self::Profile(_) => Some(SettingsSection::Profiles),
        }
    }
}

/// Unsaved add-account form; nothing is persisted until Create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub provider: crate::domain::Provider,
    pub id: String,
    pub name: String,
}

impl Default for NewAccount {
    fn default() -> Self {
        Self {
            provider: crate::domain::Provider::Discord,
            id: String::new(),
            name: String::new(),
        }
    }
}
