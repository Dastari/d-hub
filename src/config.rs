use crate::{domain::Focus, storage::write_private};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub schema_version: u32,
    pub focus: Focus,
    pub theme: Theme,
    pub reduced_motion: bool,
    pub insert_on_focus: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            focus: Focus::All,
            theme: Theme::Midnight,
            reduced_motion: false,
            insert_on_focus: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Key {
    Focus,
    Theme,
    ReducedMotion,
    InsertOnFocus,
}

pub struct Setting {
    pub key: Key,
    pub name: &'static str,
    pub help: &'static str,
    pub choices: &'static [&'static str],
}

pub const REGISTRY: &[Setting] = &[
    Setting {
        key: Key::Focus,
        name: "focus",
        help: "Filter the shared feed",
        choices: &["all", "work", "personal"],
    },
    Setting {
        key: Key::Theme,
        name: "theme",
        help: "Shared palette for every source",
        choices: &["midnight", "dusk"],
    },
    Setting {
        key: Key::ReducedMotion,
        name: "reduced_motion",
        help: "Use static colours and activity indicators",
        choices: &["false", "true"],
    },
    Setting {
        key: Key::InsertOnFocus,
        name: "insert_on_focus",
        help: "Enter insert mode when an empty drawer gains focus",
        choices: &["true", "false"],
    },
];

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let value: Self = match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).context("invalid configuration")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error.into()),
        };
        if value.schema_version != 1 {
            bail!("unsupported configuration schema {}", value.schema_version);
        }
        Ok(value)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_private(path, toml::to_string_pretty(self)?.as_bytes())
    }

    pub fn get(&self, key: Key) -> String {
        match key {
            Key::Focus => self.focus.label().into(),
            Key::Theme => self.theme.label().into(),
            Key::ReducedMotion => self.reduced_motion.to_string(),
            Key::InsertOnFocus => self.insert_on_focus.to_string(),
        }
    }

    pub fn set(&mut self, name: &str, value: &str) -> Result<()> {
        let setting = setting(name)?;
        if !setting.choices.contains(&value) {
            bail!("{name} must be one of {}", setting.choices.join(", "));
        }
        match setting.key {
            Key::Focus => {
                self.focus = match value {
                    "work" => Focus::Work,
                    "personal" => Focus::Personal,
                    _ => Focus::All,
                }
            }
            Key::Theme => {
                self.theme = if value == "dusk" {
                    Theme::Dusk
                } else {
                    Theme::Midnight
                }
            }
            Key::ReducedMotion => self.reduced_motion = value == "true",
            Key::InsertOnFocus => self.insert_on_focus = value == "true",
        }
        Ok(())
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
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(
            std::env::var_os("HOME")
                .context("HOME or XDG_CONFIG_HOME is required; use --config")?,
        )
        .join(".config"),
    };
    Ok(directory.join("d-hub/config.toml"))
}
