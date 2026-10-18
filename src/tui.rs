//! Ratatui frontend. The root holds shared imports and hit areas; each submodule owns one surface.
mod actions;
mod auth;
pub(crate) mod commands;
mod discord_qr;
mod drawer;
mod event_loop;
mod feed;
mod help;
mod input;
mod preview;
mod settings;
mod theme;
mod widgets;

pub use self::{
    actions::parse_provider,
    discord_qr::discord_setup,
    drawer::render,
    event_loop::{reader, run, run_connected, snapshot},
};
use self::{
    actions::{edit_setting, finish_send, launch_reader, open_discord, remove_last, send},
    auth::{begin_auth, secret_return},
    commands::run_command,
    discord_qr::{discord_setup_key, draw_discord_qr, new_discord_setup, poll_discord_setup},
    drawer::draw,
    event_loop::TerminalGuard,
    help::HELP,
    input::{key, mouse},
    theme::{Palette, SPINNER, palette, provider_color, source_color, source_selection},
    widgets::{overlay, padded, preview, text, wrapped},
};

use crate::{
    app::{App, MockOutcome, Mode, SettingsPage, SettingsSection},
    composer::{Event as ComposerEvent, SendState},
    config::{Settings, Theme},
    domain::{MessageId, Provider, SourceFilter, safe_text},
    editor::Editor,
    storage::DemoStore,
};
use anyhow::{Context, Result, bail};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Clear, Padding, Paragraph, Wrap},
};
use std::{
    io::{self, IsTerminal},
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

/// Screen regions from the last frame, used to route mouse input.
#[derive(Default)]
struct HitAreas {
    feed: Rect,
    composer: Rect,
    attachments: Vec<(Rect, usize)>,
    settings: Vec<(Rect, usize)>,
    settings_nav: Vec<(Rect, SettingsPage)>,
    /// Rendered rows of variable height, newest first.
    feed_items: Vec<(Rect, MessageId)>,
    /// Messages fully visible in the feed, used as the keyboard page size.
    feed_page: usize,
}
