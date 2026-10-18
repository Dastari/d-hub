use crate::{
    app::{App, MockOutcome, Mode},
    composer::{Event as ComposerEvent, SendState},
    config::{Settings, Theme},
    domain::{Provider, SourceFilter, safe_text},
    editor::Editor,
    storage::DemoStore,
};
use anyhow::{Context, Result, bail};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, KeyboardEnhancementFlags, MouseButton, MouseEventKind,
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
    widgets::{Block, Clear, List, ListItem, Padding, Paragraph, Wrap},
};
use std::{
    io::{self, IsTerminal},
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const HELP: &str = "COMPOSE · reply-first is on by default\n  Up / Down      save reply and switch messages\n  Left / Right   switch source and restore its reply\n  Alt+arrows     move within reply text\n  i / r          enter insert mode\n  Enter          newline (never sends)\n  Ctrl+Enter     mock send; Alt+Enter / F5 fallback\n  Esc            leave insert mode, keep draft\n  Ctrl+A         attach harmless fixture\n\nBROWSE · navigation mode\n  j / k, arrows  select communication\n  gg / G         first / last message\n  Ctrl+d / u     move down / up a page\n  mouse wheel    scroll without selecting\n  Left / Right   cycle source filter (All, iCloud, Discord, Teams, Email, Office 365)\n  f              cycle focus profile\n  /              search feed\n  a / x          add / remove last fixture attachment\n  s              save draft and switch to selected\n  e / D          dismiss or restore / include dismissed\n  u              toggle unread (local until provider connected)\n  S              include spam matches\n  v              open official Discord app\n  d              discard draft with confirmation\n  o / Ctrl+o     full preview; Ctrl+o tries Ghostty\n  n              simulate an incoming message\n  F6             simulate drawer active / inactive\n  F7             next mock outcome: success / failure / uncertain\n  :              command palette (Tab cycles commands)\n  ,              shared settings\n  t              change palette\n  ?              this help\n  q / Ctrl+q     save and quit\n\nREADER\n  j / k, arrows, PageUp / Down  scroll\n  /                           search; Enter jumps next\n  q / Esc                     close reader only\n\nCOMMANDS\n  attach · remove · incoming · follow · save-switch\n  discard · send · fail · uncertain · reconcile · settings\n  focus all|work|personal · theme midnight|dusk\n  source all|discord|teams|email|icloud|office365\n  profile add|remove ID · profile prompt ID\n  account add icloud|office365|teams|discord ID\n  auth ACCOUNT|ai · spam prompt\n  motion · preview · help · quit\n\nDemo messages and sends are fictional. Notification capture is read-only.\nImages, live accounts, daemon IPC and the Omarchy plugin are later milestones.";
const COMMANDS: &[&str] = &[
    "attach",
    "remove",
    "incoming",
    "follow",
    "save-switch",
    "discard",
    "send",
    "fail",
    "uncertain",
    "reconcile",
    "settings",
    "focus all",
    "focus work",
    "focus personal",
    "source all",
    "source discord",
    "source teams",
    "source email",
    "source icloud",
    "source office365",
    "theme midnight",
    "theme dusk",
    "motion",
    "preview",
    "help",
    "quit",
];

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableFocusChange,
            EnableMouseCapture
        )?;
        // Unsupported terminals ignore this protocol; F5 remains an unambiguous send key.
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            PopKeyboardEnhancementFlags,
            DisableMouseCapture,
            DisableFocusChange,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Clone, Copy)]
struct Palette {
    bg: Color,
    surface: Color,
    fg: Color,
    muted: Color,
    accent: Color,
    second: Color,
    selected: Color,
    warning: Color,
}

fn palette(theme: Theme, elapsed: Duration, reduced: bool) -> Palette {
    let (base, alternate) = match theme {
        Theme::Midnight => ([145, 184, 255], [207, 163, 255]),
        Theme::Dusk => ([222, 174, 255], [255, 181, 164]),
    };
    let mix = if reduced {
        0.0
    } else {
        (elapsed.as_secs_f64() * std::f64::consts::TAU / 12.0).sin() * 0.5 + 0.5
    };
    let channel = |i| (f64::from(base[i]) * (1.0 - mix) + f64::from(alternate[i]) * mix) as u8;
    Palette {
        bg: if theme == Theme::Midnight {
            Color::Rgb(17, 19, 24)
        } else {
            Color::Rgb(27, 22, 33)
        },
        surface: if theme == Theme::Midnight {
            Color::Rgb(25, 29, 38)
        } else {
            Color::Rgb(39, 31, 45)
        },
        fg: Color::Rgb(230, 234, 242),
        muted: Color::Rgb(153, 164, 184),
        accent: Color::Rgb(channel(0), channel(1), channel(2)),
        second: Color::Rgb(191, 166, 232),
        selected: if theme == Theme::Midnight {
            Color::Rgb(35, 46, 64)
        } else {
            Color::Rgb(61, 42, 66)
        },
        warning: Color::Rgb(244, 199, 139),
    }
}

fn source_color(source: SourceFilter, fallback: Color) -> Color {
    match source {
        SourceFilter::Discord => Color::Rgb(88, 101, 242),
        SourceFilter::Teams => Color::Rgb(139, 131, 224),
        SourceFilter::ICloud => Color::Rgb(60, 163, 255),
        SourceFilter::Office365 => Color::Rgb(244, 123, 75),
        SourceFilter::Email => Color::Rgb(109, 202, 196),
        SourceFilter::All => fallback,
    }
}

fn provider_color(provider: Option<Provider>, fallback: Color) -> Color {
    source_color(
        match provider {
            Some(Provider::Discord) => SourceFilter::Discord,
            Some(Provider::Teams) => SourceFilter::Teams,
            Some(Provider::ICloud) => SourceFilter::ICloud,
            Some(Provider::Office365) => SourceFilter::Office365,
            None => SourceFilter::All,
        },
        fallback,
    )
}

fn source_selection(color: Color, bg: Color) -> Color {
    match (color, bg) {
        (Color::Rgb(r, g, b), Color::Rgb(br, bg, bb)) => Color::Rgb(
            ((r as u16 + br as u16 * 4) / 5) as u8,
            ((g as u16 + bg as u16 * 4) / 5) as u8,
            ((b as u16 + bb as u16 * 4) / 5) as u8,
        ),
        _ => bg,
    }
}

#[derive(Default)]
struct HitAreas {
    feed: Rect,
    composer: Rect,
    attachments: Vec<(Rect, usize)>,
}

fn padded(rect: Rect) -> Rect {
    rect.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 0,
    })
}

fn text<'a>(value: impl Into<Text<'a>>, color: Color) -> Paragraph<'a> {
    Paragraph::new(value).style(Style::default().fg(color))
}

fn overlay(frame: &mut Frame, area: Rect, p: Palette, title: &str, body: &str, scroll: u16) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .style(Style::default().bg(p.surface))
            .padding(Padding::new(2, 2, 1, 1)),
        area,
    );
    let inner = area.inner(ratatui::layout::Margin {
        horizontal: 2,
        vertical: 1,
    });
    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        text(safe_text(title), p.accent)
            .style(Style::default().fg(p.accent).add_modifier(Modifier::BOLD)),
        parts[0],
    );
    frame.render_widget(
        text(safe_text(body), p.fg)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        parts[1],
    );
    frame.render_widget(
        text("Esc close · arrows / PageUp / PageDown scroll", p.muted),
        parts[2],
    );
}

pub fn render(frame: &mut Frame, app: &mut App, elapsed: Duration) {
    draw(frame, app, elapsed);
}

fn draw(frame: &mut Frame, app: &mut App, elapsed: Duration) -> HitAreas {
    let p = palette(app.settings.theme, elapsed, app.settings.reduced_motion);
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(p.bg).fg(p.fg)),
        area,
    );
    let area = area.inner(ratatui::layout::Margin {
        horizontal: app.settings.padding_horizontal,
        vertical: app.settings.padding_vertical,
    });
    if area.width < 28 || area.height < 16 {
        frame.render_widget(
            text(
                "d-hub · offline demo\nResize or reduce padding: content needs 28 × 16.\nCtrl+q saves and quits.",
                p.fg,
            ),
            area,
        );
        return HitAreas::default();
    }
    let composer_height = (area.height / 3).clamp(6, 11);
    let preview_height = if area.height >= 28 { 5 } else { 3 };
    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(preview_height),
        Constraint::Length(composer_height),
        Constraint::Length(2),
    ])
    .split(area);
    let header = Line::from(vec![
        Span::styled(
            "d-hub  ",
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ‹ {} › ", app.settings.source.label()),
            Style::default()
                .fg(source_color(app.settings.source, p.accent))
                .bg(p.surface),
        ),
        Span::styled(
            format!(" · {}", safe_text(app.settings.profile_label())),
            Style::default().fg(p.muted),
        ),
        Span::styled(
            if app.live_feed {
                "  NOTIFICATIONS"
            } else if app.service_socket.is_some() {
                "  SERVICE DEMO"
            } else {
                "  OFFLINE DEMO"
            },
            Style::default().fg(p.muted),
        ),
    ]);
    frame.render_widget(Paragraph::new(header), parts[0]);

    let rows = (parts[1].height / 3).max(1) as usize;
    app.feed_offset = app.feed_offset.min(app.visible().len().saturating_sub(1));
    let visible = app.visible();
    let items: Vec<_> = visible
        .iter()
        .skip(app.feed_offset)
        .take(rows)
        .map(|message| {
            let selected = Some(message.id) == app.selected;
            let source = match (message.provider(), message.target.is_some()) {
                (Some(Provider::Discord), true) => "Discord bot (demo)",
                (Some(Provider::Discord), false) => "Discord notification",
                (provider, _) => provider.map_or("unsupported", |p| p.label()),
            };
            let color = provider_color(message.provider(), p.accent);
            let suffix = format!(
                "{}{}",
                if app.dismissed(message.id) {
                    " · dismissed"
                } else {
                    ""
                },
                if app.spam_match(message) {
                    " · filtered"
                } else {
                    ""
                }
            );
            let lines = vec![
                Line::from(vec![
                    Span::styled(
                        if message.unread { "● " } else { "○ " },
                        Style::default().fg(color),
                    ),
                    Span::styled(
                        safe_text(&message.sender),
                        Style::default().fg(p.fg).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {} · {}{}", source, safe_text(&message.time), suffix),
                        Style::default().fg(p.muted),
                    ),
                ]),
                Line::from(Span::styled(
                    format!("  {}", safe_text(&message.subject)),
                    Style::default().fg(if selected { color } else { p.muted }),
                )),
                Line::raw(""),
            ];
            ListItem::new(lines).style(Style::default().bg(if selected {
                source_selection(color, p.bg)
            } else {
                p.bg
            }))
        })
        .collect();
    if items.is_empty() {
        frame.render_widget(
            text("No messages match this source/focus.", p.muted),
            parts[1],
        );
    } else {
        frame.render_widget(List::new(items), parts[1]);
    }

    let preview = app
        .selected_message()
        .map(|m| safe_text(&m.body))
        .unwrap_or_else(|| "Select a communication to read its context.".into());
    frame.render_widget(
        text(preview, p.muted).wrap(Wrap { trim: false }),
        padded(parts[2]),
    );

    frame.render_widget(
        Block::default().style(Style::default().bg(p.surface)),
        parts[3],
    );
    let composer = padded(parts[3]);
    let target = app.session.composer.reply_target();
    let locked = app.session.composer.draft.is_some();
    let recipient = target
        .map(|t| {
            t.recipients
                .iter()
                .map(|s| safe_text(s))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| {
            if app.live_feed {
                "Read-only preview · v opens official Discord DM list".into()
            } else {
                "No eligible reply target".into()
            }
        });
    let mut state: String = match &app.session.composer.send {
        SendState::Submitting(_) => {
            if app.settings.reduced_motion {
                "… MOCK SEND ".into()
            } else {
                format!(
                    "{} MOCK SEND ",
                    SPINNER[(elapsed.as_millis() / 80) as usize % SPINNER.len()]
                )
            }
        }
        SendState::Uncertain(_) => "? UNCERTAIN · :reconcile ".into(),
        SendState::Failed(_) => "! FAILED · draft retained ".into(),
        SendState::Ready if locked => "◆ LOCKED ".into(),
        _ => "↳ Reply ".into(),
    };
    if composer.width < 40 {
        state = match &app.session.composer.send {
            SendState::Submitting(_) => "… ",
            SendState::Uncertain(_) => "? ",
            SendState::Failed(_) => "! ",
            _ if locked => "◆ ",
            _ => "↳ ",
        }
        .into();
    }
    let account = target.map_or("", |t| t.account.as_str());
    let attachment_count = app
        .session
        .composer
        .draft
        .as_ref()
        .map_or(0, |d| d.attachments.len());
    let account = format!(
        "{}{}",
        safe_text(account),
        if attachment_count > 0 {
            format!(" +{attachment_count}")
        } else {
            String::new()
        }
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                state,
                Style::default().fg(if locked { p.second } else { p.accent }),
            ),
            Span::styled(account, Style::default().fg(p.muted)),
        ])),
        Rect {
            height: 1,
            ..composer
        },
    );
    frame.render_widget(
        text(recipient, p.fg),
        Rect::new(composer.x, composer.y + 1, composer.width, 1),
    );
    let mut hit = HitAreas {
        feed: parts[1],
        composer: parts[3],
        ..HitAreas::default()
    };
    let mut edit_top = composer.y + 3;
    if let Some(draft) = &app.session.composer.draft
        && !draft.attachments.is_empty()
    {
        let mut x = composer.x;
        for (index, attachment) in draft.attachments.iter().enumerate() {
            let label = format!(
                " + {} · {} B × ",
                safe_text(&attachment.name),
                attachment.bytes
            );
            let width = unicode_width::UnicodeWidthStr::width(label.as_str())
                .min(composer.right().saturating_sub(x) as usize) as u16;
            if width == 0 {
                break;
            }
            let chip = Rect::new(x, composer.y + 2, width, 1);
            frame.render_widget(
                text(label, p.accent).style(Style::default().fg(p.accent).bg(p.selected)),
                chip,
            );
            hit.attachments.push((chip, index));
            x = x.saturating_add(width + 1);
        }
        edit_top += 1;
    }
    let edit_area = Rect::new(
        composer.x,
        edit_top,
        composer.width,
        composer.bottom().saturating_sub(edit_top + 1),
    );
    if edit_area.height > 0 {
        let (row, col) = app
            .editor
            .viewport(edit_area.width as usize, edit_area.height as usize);
        if app.editor.text.is_empty() {
            frame.render_widget(
                text(
                    if app.live_feed {
                        "History and replies are unavailable here."
                    } else {
                        "Write a reply…"
                    },
                    p.muted,
                ),
                edit_area,
            );
        } else {
            frame.render_widget(
                text(app.editor.text.clone(), p.fg)
                    .scroll((app.editor.top as u16, app.editor.left as u16)),
                edit_area,
            );
        }
        if app.mode == Mode::Insert && app.session.composer.editable() {
            frame.set_cursor_position((edit_area.x + col, edit_area.y + row));
        }
    }
    let mode = match app.mode {
        Mode::Insert => "INSERT",
        Mode::Navigate => "NAV",
        Mode::Command => "COMMAND",
        Mode::Search => "SEARCH",
        _ => "VIEW",
    };
    frame.render_widget(
        text(
            if app.live_feed {
                format!("{mode}  ·  v open Discord  ·  e dismiss  ·  ? help")
            } else if app.settings.reply_first {
                format!("{mode} · ↑↓ reply · ←→ source · Alt+arrows cursor · F5 mock send")
            } else {
                format!("{mode}  ·  F5 mock send  ·  Esc browse  ·  ? help")
            },
            p.muted,
        ),
        Rect::new(composer.x, composer.bottom() - 1, composer.width, 1),
    );
    let outcome = match app.next_outcome {
        MockOutcome::Success => "success",
        MockOutcome::Failure => "failure",
        MockOutcome::Uncertain => "uncertain",
    };
    frame.render_widget(
        text(
            format!(
                "{} · {} new · {}",
                if app.focused {
                    "active"
                } else {
                    "inactive (simulated/focus)"
                },
                app.new_items,
                if app.live_feed {
                    if app.bridge_ready {
                        "capture ready"
                    } else {
                        "capture unavailable / starting"
                    }
                } else {
                    outcome
                }
            ),
            p.muted,
        ),
        Rect {
            height: 1,
            ..parts[4]
        },
    );
    frame.render_widget(
        text(safe_text(&app.notice), p.warning),
        Rect::new(parts[4].x, parts[4].y + 1, parts[4].width, 1),
    );

    match app.mode {
        Mode::Help => overlay(
            frame,
            area,
            p,
            "Controls · all sends simulated",
            HELP,
            app.reader_scroll,
        ),
        Mode::Settings => {
            let rows = app.settings.entries();
            let body = rows
                .iter()
                .enumerate()
                .skip(app.setting_index.saturating_sub(1))
                .take(3)
                .map(|(i, setting)| {
                    format!(
                        "{} {}  {}\n   {}\n",
                        if i == app.setting_index { "›" } else { " " },
                        setting.name,
                        safe_text(&setting.value).lines().next().unwrap_or(""),
                        safe_text(&setting.help)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            overlay(
                frame,
                area,
                p,
                "Settings · Enter edit · arrows cycle · j/k select",
                &body,
                0,
            );
        }
        Mode::Discard => overlay(
            frame,
            area,
            p,
            "Discard this draft?",
            "Text and attachments will be removed.\n\ny confirms · n / Esc keeps the draft",
            0,
        ),
        Mode::EditSetting => {
            let title = format!(
                "{} · Ctrl+s save · Esc cancel",
                app.edit_key.as_deref().unwrap_or("Setting")
            );
            let cursor_row = app.command.position().0 as u16;
            let scroll = cursor_row.saturating_sub(area.height.saturating_sub(8));
            overlay(frame, area, p, &title, &app.command.text, scroll);
        }
        Mode::Secret => overlay(
            frame,
            area,
            p,
            "Credential · Enter store · Esc cancel",
            &"•".repeat(app.command.text.chars().count().min(80)),
            0,
        ),
        Mode::Command | Mode::Search => {
            let prefix = if app.mode == Mode::Command { ":" } else { "/" };
            let line = format!("{prefix}{}", app.command.text);
            let area = Rect::new(parts[4].x, parts[4].y + 1, parts[4].width, 1);
            frame.render_widget(Clear, area);
            frame.render_widget(
                text(line, p.accent).style(Style::default().fg(p.accent).bg(p.surface)),
                area,
            );
            if app.mode == Mode::Command {
                let available = COMMANDS
                    .iter()
                    .filter(|c| c.starts_with(&app.command.text))
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" · ");
                frame.render_widget(
                    text(available, p.muted),
                    Rect {
                        height: 1,
                        ..parts[4]
                    },
                );
            }
            let column =
                unicode_width::UnicodeWidthStr::width(&app.command.text[..app.command.cursor])
                    as u16
                    + 1;
            frame.set_cursor_position((area.x + column.min(area.width.saturating_sub(1)), area.y));
        }
        Mode::Reader => {
            if let Some(message) = &app.reader {
                let mut body = safe_text(&message.body);
                if !app.reader_query.is_empty() {
                    body = format!("Search: {}\n\n{body}", safe_text(&app.reader_query));
                }
                overlay(
                    frame,
                    area,
                    p,
                    &format!(
                        "{} · {}",
                        safe_text(&message.sender),
                        safe_text(&message.subject)
                    ),
                    &body,
                    app.reader_scroll,
                );
            }
        }
        _ => {}
    }
    hit
}

fn send(app: &mut App) -> Option<(Instant, u64, MockOutcome)> {
    if app.live_feed || app.live_setup {
        app.notice = "Live replies are not implemented · open the official client to reply.".into();
        return None;
    }
    app.notice = "No send started · write a reply, or resolve the pending outcome.".into();
    if let Some(operation) = app.submit() {
        if app.shared {
            app.notice = "Mock send queued with the service · nothing will be delivered.".into();
            return None;
        }
        app.notice = "Simulating send locally · no provider is connected.".into();
        Some((
            Instant::now() + Duration::from_millis(650),
            operation.operation,
            app.next_outcome,
        ))
    } else {
        None
    }
}

fn finish_send(app: &mut App, operation: u64, outcome: MockOutcome) {
    match outcome {
        MockOutcome::Success => {
            app.accepted(operation);
            app.notice = "MOCK SENT · stored locally, nothing was delivered.".into();
        }
        MockOutcome::Failure => {
            app.session.composer.apply(ComposerEvent::Failed(
                operation,
                "Simulated transport failure".into(),
            ));
            app.notice =
                "MOCK FAILED · draft and recipient retained. F7 changes next result.".into();
        }
        MockOutcome::Uncertain => {
            app.session
                .composer
                .apply(ComposerEvent::Uncertain(operation));
            app.notice =
                "MOCK UNCERTAIN · retry disabled. :reconcile simulates confirmed acceptance."
                    .into();
        }
    }
    app.sync_editor();
    if !app.focused {
        app.follow_latest();
    }
}

fn remove_last(app: &mut App) {
    if let Some(index) = app
        .session
        .composer
        .draft
        .as_ref()
        .and_then(|d| d.attachments.len().checked_sub(1))
    {
        app.remove_attachment(index);
    }
}

fn launch_reader(app: &mut App, config_path: Option<&Path>) {
    if app.live_feed {
        app.open_reader();
        return;
    }
    let Some(message) = app.selected_message() else {
        return;
    };
    let result = std::env::current_exe().and_then(|binary| {
        let mut command = Command::new("ghostty");
        command.arg("-e").arg(binary);
        if let Some(path) = config_path {
            let path = if path.is_absolute() {
                path.to_owned()
            } else {
                std::env::current_dir()?.join(path)
            };
            command.arg("--config").arg(path);
        }
        command.arg("preview");
        if let Some(socket) = &app.service_socket {
            command.arg("--socket").arg(socket);
        } else {
            command.arg("--demo");
        }
        command.arg(message.id.0.to_string()).spawn()
    });
    match result {
        Ok(mut child) => {
            if app.settings.mark_read_on_select
                && let Some(id) = app.selected
            {
                app.set_unread(id, false);
            }
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            app.notice = "Requested Ghostty reader · drawer draft preserved.".into();
        }
        Err(_) => {
            app.open_reader();
            app.notice = "Ghostty unavailable · opened in-process fixture reader.".into();
        }
    }
}

pub fn parse_provider(value: &str) -> Result<Provider> {
    match value {
        "icloud" => Ok(Provider::ICloud),
        "office365" => Ok(Provider::Office365),
        "teams" => Ok(Provider::Teams),
        "discord" => Ok(Provider::Discord),
        _ => bail!("provider must be icloud, office365, teams or discord"),
    }
}

fn edit_setting(app: &mut App, name: &str) {
    match app.settings.value(name) {
        Ok(value) => {
            app.command = Editor::new(&value);
            app.edit_key = Some(name.into());
            app.mode = Mode::EditSetting;
        }
        Err(e) => app.notice = e.to_string(),
    }
}

fn begin_auth(app: &mut App, id: &str) {
    if !app.live_setup {
        app.notice = "Account setup is explicit · use d-hub settings or d-hub account auth outside the offline demo.".into();
        return;
    }
    if app.auth_job.is_some() {
        app.notice = "Authentication already in progress.".into();
        return;
    }
    let reference = if id == "ai" {
        app.settings.ai.provider.credential_ref().to_owned()
    } else {
        let Some(account) = app.settings.accounts.iter().find(|a| a.id == id).cloned() else {
            app.notice = "Unknown account · add it through :account add PROVIDER ID.".into();
            return;
        };
        if !account.authentication_supported() {
            app.notice = account.auth_description().into();
            return;
        }
        if !crate::auth::SecretService::available() {
            app.notice =
                "Install libsecret (secret-tool) and unlock the desktop keyring before setup."
                    .into();
            return;
        }
        if matches!(account.provider, Provider::Office365 | Provider::Teams) {
            let (tx, rx) = std::sync::mpsc::sync_channel(8);
            app.auth_job = Some(rx);
            std::thread::spawn(move || {
                if let Err(e) = crate::auth::authenticate_graph(
                    &account,
                    &crate::auth::SecretService,
                    |notice| {
                        let _ = tx.send(notice);
                    },
                ) {
                    let _ = tx.send(format!("Sign-in failed: {e}"));
                }
            });
            app.notice = "Starting Microsoft device login…".into();
            return;
        }
        account.credential_ref()
    };
    if !crate::auth::SecretService::available() {
        app.notice =
            "Install libsecret (secret-tool) and unlock the desktop keyring before setup.".into();
        return;
    }
    app.secret_ref = Some(reference);
    app.command = Editor::default();
    app.mode = Mode::Secret;
}

fn open_discord(app: &mut App) {
    if app.selected_message().and_then(|m| m.provider()) != Some(Provider::Discord) {
        app.notice = "Select a Discord notification first.".into();
        return;
    }
    match Command::new("xdg-open")
        .arg("discord://-/channels/@me")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            app.notice = "Requested official Discord DM list · choose the conversation there for history and replies.".into();
        }
        Err(_) => {
            app.notice =
                "Could not open Discord · open its official client for history and replies.".into()
        }
    }
}

fn run_command(app: &mut App, command: &str) -> bool {
    app.mode = Mode::Navigate;
    let words: Vec<_> = command.split_whitespace().collect();
    match words.as_slice() {
        ["attach"] if !app.live_feed && !app.live_setup => app.attach(),
        ["remove"] => remove_last(app),
        ["incoming"] if !app.live_feed && !app.live_setup => app.arrive(),
        ["follow"] => {
            app.focus(false);
            app.focus(true);
            app.new_items = 0;
            app.sync_editor();
        }
        ["save-switch"] => app.save_switch(),
        ["discard"] => app.mode = Mode::Discard,
        ["fail"] => {
            app.next_outcome = MockOutcome::Failure;
            app.notice = "Next mock send will fail.".into();
        }
        ["uncertain"] => {
            app.next_outcome = MockOutcome::Uncertain;
            app.notice = "Next mock send will have uncertain acceptance.".into();
        }
        ["reconcile"] => {
            if let SendState::Uncertain(operation) = &app.session.composer.send {
                let id = operation.operation;
                if app.shared {
                    app.actions
                        .push(crate::service::Action::Reconcile { operation: id });
                }
                app.accepted(id);
                app.sync_editor();
                app.notice =
                    "MOCK RECONCILED · simulated acceptance confirmed without resending.".into();
            }
        }
        ["settings"] => app.mode = Mode::Settings,
        ["profile", "add", id] => match app.settings.add_profile(id) {
            Ok(()) => app.notice = format!("Profile {id} added · edit its prompt in settings."),
            Err(e) => app.notice = e.to_string(),
        },
        ["profile", "remove", id] => match app.settings.remove_profile(id) {
            Ok(()) => app.refresh_filters(),
            Err(e) => app.notice = e.to_string(),
        },
        ["profile", "prompt", id] => edit_setting(app, &format!("profiles.{id}.prompt")),
        ["spam", "prompt"] => edit_setting(app, "spam.prompt"),
        ["account", "add", provider, id] => {
            match parse_provider(provider).and_then(|p| app.settings.add_account(id, p)) {
                Ok(()) => {
                    app.notice = format!(
                        "Account {id} added · edit account fields in settings; :auth {id} starts setup."
                    )
                }
                Err(e) => app.notice = e.to_string(),
            }
        }
        ["auth", id] => begin_auth(app, id),
        ["dismiss"] => app.toggle_dismissed(),
        ["dismissed"] => app.toggle_show_dismissed(),
        ["focus", value] => app.set_profile(value),
        ["source", value] => match app.settings.set("source", value) {
            Ok(()) => app.set_source(app.settings.source),
            Err(e) => app.notice = e.to_string(),
        },
        ["theme", value] => {
            if let Err(e) = app.settings.set("theme", value) {
                app.notice = e.to_string();
            }
        }
        ["motion"] => app.settings.reduced_motion = !app.settings.reduced_motion,
        ["preview"] => app.open_reader(),
        ["help"] => {
            app.mode = Mode::Help;
            app.reader_scroll = 0;
        }
        ["quit"] => return true,
        _ => app.notice = "Unknown command · :help lists supported commands.".into(),
    }
    false
}

fn scroll(app: &mut App, down: bool, amount: u16) {
    let limit = if app.mode == Mode::Help {
        HELP.lines().count() * 4
    } else {
        app.reader.as_ref().map_or(0, |m| m.body.chars().count())
    };
    if down {
        app.reader_scroll = app
            .reader_scroll
            .saturating_add(amount)
            .min(limit.min(u16::MAX as usize) as u16);
    } else {
        app.reader_scroll = app.reader_scroll.saturating_sub(amount);
    }
}

fn key(
    app: &mut App,
    key: KeyEvent,
    rows: usize,
    pending: &mut Option<(Instant, u64, MockOutcome)>,
    g_prefix: &mut Option<Instant>,
    config_path: Option<&Path>,
) -> bool {
    if key.kind == KeyEventKind::Release {
        return false;
    }
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    if control && key.code == KeyCode::Char('q') {
        if app.mode == Mode::Secret {
            use zeroize::Zeroize;
            app.command.text.zeroize();
        }
        return true;
    }
    if key.code == KeyCode::F(6) {
        app.focus(!app.focused);
        app.notice =
            "Simulated focus transition (terminal focus reporting is also enabled).".into();
        return false;
    }
    if key.code == KeyCode::F(7) {
        app.next_outcome = match app.next_outcome {
            MockOutcome::Success => MockOutcome::Failure,
            MockOutcome::Failure => MockOutcome::Uncertain,
            MockOutcome::Uncertain => MockOutcome::Success,
        };
        return false;
    }
    if matches!(app.mode, Mode::Insert | Mode::Navigate) {
        if (key.code == KeyCode::Enter
            && key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT))
            || key.code == KeyCode::F(5)
        {
            if pending.is_none() {
                *pending = send(app);
            }
            return false;
        }
        if control && key.code == KeyCode::Char('o') {
            launch_reader(app, config_path);
            return false;
        }
        if control && key.code == KeyCode::Char('a') && !app.live_feed && !app.live_setup {
            app.attach();
            return false;
        }
    }
    match app.mode {
        Mode::Insert => {
            let alt = key.modifiers.contains(KeyModifiers::ALT);
            if app.settings.reply_first
                && !alt
                && !control
                && matches!(
                    key.code,
                    KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
                )
            {
                match key.code {
                    KeyCode::Up => app.move_reply(-1, rows),
                    KeyCode::Down => app.move_reply(1, rows),
                    KeyCode::Left => app.cycle_reply_source(true),
                    KeyCode::Right => app.cycle_reply_source(false),
                    _ => {}
                }
            } else if key.code == KeyCode::Esc {
                app.mode = Mode::Navigate;
            } else if app.session.composer.editable() && app.editor.key(key) {
                app.update_body();
            }
        }
        Mode::Command | Mode::Search => match key.code {
            KeyCode::Esc => {
                app.mode = if app.reader.is_some() && app.mode == Mode::Search {
                    Mode::Reader
                } else {
                    Mode::Navigate
                };
            }
            KeyCode::Enter => {
                let command = app.command.text.clone();
                if app.mode == Mode::Command {
                    if command == "send" {
                        app.mode = Mode::Navigate;
                        if pending.is_none() {
                            *pending = send(app);
                        }
                    } else if run_command(app, &command) {
                        return true;
                    }
                } else if app.reader.is_some() {
                    app.reader_query = command;
                    app.reader_scroll = app
                        .reader
                        .as_ref()
                        .and_then(|m| {
                            m.body.lines().position(|line| {
                                line.to_lowercase()
                                    .contains(&app.reader_query.to_lowercase())
                            })
                        })
                        .unwrap_or(0)
                        .min(u16::MAX as usize) as u16;
                    app.mode = Mode::Reader;
                } else {
                    app.query = command;
                    app.feed_offset = 0;
                    app.mode = Mode::Navigate;
                }
            }
            KeyCode::Tab if app.mode == Mode::Command => {
                let current = app.command.text.trim();
                let next = COMMANDS
                    .iter()
                    .position(|c| *c == current)
                    .map(|i| COMMANDS[(i + 1) % COMMANDS.len()])
                    .or_else(|| COMMANDS.iter().find(|c| c.starts_with(current)).copied())
                    .unwrap_or(COMMANDS[0]);
                app.command = Editor::new(next);
            }
            _ => {
                app.command.key(key);
            }
        },
        Mode::Settings => {
            let rows = app.settings.entries();
            app.setting_index = app.setting_index.min(rows.len() - 1);
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Navigate,
                KeyCode::Down | KeyCode::Char('j') => {
                    app.setting_index = (app.setting_index + 1) % rows.len()
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.setting_index = (app.setting_index + rows.len() - 1) % rows.len()
                }
                KeyCode::Enter => edit_setting(app, &rows[app.setting_index].name),
                KeyCode::Char(' ') | KeyCode::Right | KeyCode::Left => {
                    let row = &rows[app.setting_index];
                    if row.choices.is_empty() {
                        edit_setting(app, &row.name);
                    } else {
                        let index = row
                            .choices
                            .iter()
                            .position(|v| *v == row.value)
                            .unwrap_or(0);
                        let offset = if key.code == KeyCode::Left {
                            row.choices.len() - 1
                        } else {
                            1
                        };
                        let value = &row.choices[(index + offset) % row.choices.len()];
                        match app.settings.set(&row.name, value) {
                            Ok(()) => {
                                app.refresh_filters();
                                app.notice = format!("{} saved as {}", row.name, value);
                            }
                            Err(e) => app.notice = e.to_string(),
                        }
                    }
                }
                _ => {}
            }
        }
        Mode::EditSetting => {
            if key.code == KeyCode::Esc {
                app.command = Editor::default();
                app.mode = Mode::Settings;
            } else if control && key.code == KeyCode::Char('s') {
                if let Some(name) = app.edit_key.clone() {
                    match app.settings.set(&name, &app.command.text) {
                        Ok(()) => {
                            app.refresh_filters();
                            app.command = Editor::default();
                            app.mode = Mode::Settings;
                            app.notice = format!("{name} saved.");
                        }
                        Err(e) => app.notice = e.to_string(),
                    }
                }
            } else {
                app.command.key(key);
            }
        }
        Mode::Secret => {
            if key.code == KeyCode::Esc {
                use zeroize::Zeroize;
                app.command.text.zeroize();
                app.command = Editor::default();
                app.secret_ref = None;
                app.mode = Mode::Navigate;
            } else if key.code == KeyCode::Enter {
                let secret = zeroize::Zeroizing::new(std::mem::take(&mut app.command.text));
                app.command = Editor::default();
                app.mode = Mode::Navigate;
                if let Some(reference) = app.secret_ref.take() {
                    let (tx, rx) = std::sync::mpsc::sync_channel(2);
                    app.auth_job = Some(rx);
                    std::thread::spawn(move || {
                        use crate::auth::CredentialStore;
                        let notice = match crate::auth::SecretService.store(&reference, &secret) {
                            Ok(()) => "Credential stored in desktop keyring. Provider connection is a separate step.".into(),
                            Err(e) => format!("Credential storage failed: {e}"),
                        };
                        let _ = tx.send(notice);
                    });
                }
            } else {
                app.command.key(key);
            }
        }
        Mode::Discard => match key.code {
            KeyCode::Char('y') => app.discard(),
            KeyCode::Esc | KeyCode::Char('n') => app.mode = Mode::Navigate,
            _ => {}
        },
        Mode::Help | Mode::Reader => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.mode = Mode::Navigate;
                app.reader = None;
            }
            KeyCode::Char('/') if app.mode == Mode::Reader => {
                app.mode = Mode::Search;
                app.command = Editor::new(&app.reader_query);
            }
            KeyCode::Down | KeyCode::Char('j') => scroll(app, true, 1),
            KeyCode::Up | KeyCode::Char('k') => scroll(app, false, 1),
            KeyCode::PageDown => scroll(app, true, 10),
            KeyCode::PageUp => scroll(app, false, 10),
            KeyCode::Home | KeyCode::Char('g') => app.reader_scroll = 0,
            _ => {}
        },
        Mode::Navigate => match key.code {
            KeyCode::Left => app.set_source(app.settings.source.cycle(true)),
            KeyCode::Right => app.set_source(app.settings.source.cycle(false)),
            KeyCode::Char('q') => return true,
            KeyCode::Char('i' | 'r') if !app.live_feed && !app.live_setup => {
                if app.settings.reply_first
                    && let Some(id) = app.selected
                {
                    app.switch_reply(id);
                } else if app.session.composer.draft.is_none()
                    && let Some(id) = app.selected
                {
                    app.select(id);
                }
                app.mode = Mode::Insert;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.move_selection(1, rows);
                app.new_items = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1, rows),
            KeyCode::PageDown => app.move_selection(rows as isize, rows),
            KeyCode::PageUp => app.move_selection(-(rows as isize), rows),
            KeyCode::Char('d') if control => app.move_selection(rows as isize, rows),
            KeyCode::Char('u') if control => app.move_selection(-(rows as isize), rows),
            KeyCode::Home => {
                app.move_selection(isize::MIN, rows);
            }
            KeyCode::End | KeyCode::Char('G') => {
                app.move_selection(isize::MAX, rows);
            }
            KeyCode::Char('g') => {
                if g_prefix.is_some_and(|time| time.elapsed() < Duration::from_millis(500)) {
                    app.move_selection(isize::MIN, rows);
                    *g_prefix = None;
                } else {
                    *g_prefix = Some(Instant::now());
                    app.notice = "g… · press g for first message".into();
                }
            }
            KeyCode::Char('f') => {
                let profile = app.settings.cycle_focus();
                app.set_profile(&profile);
            }
            KeyCode::Char('e') => app.toggle_dismissed(),
            KeyCode::Char('D') => app.toggle_show_dismissed(),
            KeyCode::Char('S') => {
                app.settings.show_spam = !app.settings.show_spam;
                app.refresh_filters();
            }
            KeyCode::Char('u') => {
                if let Some(message) = app.selected_message() {
                    let (id, unread) = (message.id, !message.unread);
                    app.set_unread(id, unread);
                }
            }
            KeyCode::Char('v') => open_discord(app),
            KeyCode::Char('t') => app.settings.theme = app.settings.theme.next(),
            KeyCode::Char('n') if !app.live_feed && !app.live_setup => app.arrive(),
            KeyCode::Char('a') if !app.live_feed && !app.live_setup => app.attach(),
            KeyCode::Char('x') => remove_last(app),
            KeyCode::Char('s') => app.save_switch(),
            KeyCode::Char('d') => app.mode = Mode::Discard,
            KeyCode::Char('o') => app.open_reader(),
            KeyCode::Char(',') => app.mode = Mode::Settings,
            KeyCode::Char('?') => {
                app.mode = Mode::Help;
                app.reader_scroll = 0;
            }
            KeyCode::Char(':') => {
                app.mode = Mode::Command;
                app.command = Editor::default();
            }
            KeyCode::Char('/') => {
                app.mode = Mode::Search;
                app.command = Editor::new(&app.query);
            }
            _ => {}
        },
    }
    if key.code != KeyCode::Char('g') {
        *g_prefix = None;
    }
    false
}

pub fn run(
    app: App,
    store: Option<&DemoStore>,
    config_path: Option<&Path>,
    reader_only: bool,
) -> Result<()> {
    run_inner(app, store, config_path, reader_only, None)
}
pub fn run_connected(app: App, link: crate::ipc::Link, reader_only: bool) -> Result<()> {
    run_inner(app, None, None, reader_only, Some(link))
}
fn run_inner(
    mut app: App,
    store: Option<&DemoStore>,
    config_path: Option<&Path>,
    reader_only: bool,
    mut link: Option<crate::ipc::Link>,
) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("interactive terminal required; use demo --snapshot for a text preview");
    }
    let _guard = TerminalGuard::enter()?;
    let mut terminal = DefaultTerminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    // Restore terminal modes before the panic report as well as on ordinary errors.
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(
            io::stdout(),
            PopKeyboardEnhancementFlags,
            DisableMouseCapture,
            DisableFocusChange,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
        old_hook(info);
    }));
    let started = Instant::now();
    let mut pending = None;
    let mut g_prefix: Option<Instant> = None;
    let mut dirty = true;
    let mut exit_notice = None;
    let mut hits = HitAreas::default();
    let mut previous_session = serde_json::to_vec(&app.session)?;
    let mut previous_settings = app.settings.clone();
    if let Some(store) = store {
        store.save(&app.session)?;
    }
    loop {
        if let Some(link) = &mut link {
            dirty |= link.sync(&mut app)?;
        }
        dirty |= app.poll_background();
        if let Some((deadline, operation, outcome)) = pending
            && Instant::now() >= deadline
        {
            pending = None;
            finish_send(&mut app, operation, outcome);
            dirty = true;
        }
        if g_prefix.is_some_and(|time| time.elapsed() >= Duration::from_millis(500)) {
            g_prefix = None;
            app.notice = "Prefix expired · ? for controls".into();
            dirty = true;
        }
        if dirty || !app.settings.reduced_motion {
            terminal.draw(|frame| {
                hits = draw(frame, &mut app, started.elapsed());
            })?;
            dirty = false;
        }
        let mut quit = false;
        if event::poll(Duration::from_millis(80))? {
            dirty = true;
            match event::read()? {
                Event::Key(event) => {
                    let mode = app.mode;
                    if link.as_ref().is_some_and(|l| !l.connected) && !reader_only {
                        quit = (event.modifiers.contains(KeyModifiers::CONTROL)
                            && event.code == KeyCode::Char('q'))
                            || event.code == KeyCode::Char('q');
                    } else {
                        quit = key(
                            &mut app,
                            event,
                            (hits.feed.height / 3).max(1) as usize,
                            &mut pending,
                            &mut g_prefix,
                            config_path,
                        );
                    }
                    if reader_only && mode == Mode::Reader && app.mode == Mode::Navigate {
                        quit = true;
                    }
                }
                Event::Paste(value) => {
                    if link.as_ref().is_none_or(|l| l.connected)
                        && app.mode == Mode::Insert
                        && app.session.composer.editable()
                    {
                        app.editor.insert(&value);
                        app.update_body();
                    } else if matches!(app.mode, Mode::EditSetting | Mode::Secret) {
                        if app.command.text.len() + value.len() <= 16_384 {
                            app.command.insert(&value);
                        }
                    } else if matches!(app.mode, Mode::Command | Mode::Search) {
                        app.command.insert(&value.replace(['\n', '\r'], " "));
                    }
                }
                Event::FocusGained => app.focus(true),
                Event::FocusLost => app.focus(false),
                Event::Resize(_, _) => {}
                Event::Mouse(mouse) => {
                    let point = ratatui::layout::Position::new(mouse.column, mouse.row);
                    match mouse.kind {
                        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                            if matches!(app.mode, Mode::Reader | Mode::Help) =>
                        {
                            scroll(&mut app, mouse.kind == MouseEventKind::ScrollDown, 3)
                        }
                        MouseEventKind::ScrollDown
                            if hits.feed.contains(point)
                                && matches!(app.mode, Mode::Insert | Mode::Navigate) =>
                        {
                            app.feed_offset =
                                (app.feed_offset + 1).min(app.visible().len().saturating_sub(1))
                        }
                        MouseEventKind::ScrollUp
                            if hits.feed.contains(point)
                                && matches!(app.mode, Mode::Insert | Mode::Navigate) =>
                        {
                            app.feed_offset = app.feed_offset.saturating_sub(1)
                        }
                        MouseEventKind::Down(MouseButton::Left)
                            if matches!(app.mode, Mode::Insert | Mode::Navigate) =>
                        {
                            if let Some((_, index)) = hits
                                .attachments
                                .iter()
                                .find(|(area, _)| area.contains(point))
                            {
                                app.remove_attachment(*index);
                            } else if hits.composer.contains(point) {
                                app.mode = Mode::Insert;
                            } else if hits.feed.contains(point) {
                                let index =
                                    app.feed_offset + ((mouse.row - hits.feed.y) / 3) as usize;
                                let id = app.visible().get(index).map(|m| m.id);
                                if let Some(id) = id {
                                    if app.settings.reply_first {
                                        app.switch_reply(id);
                                    } else {
                                        app.select(id);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Some(store) = store {
            let bytes = serde_json::to_vec(&app.session)?;
            if bytes != previous_session {
                store.save(&app.session).context(
                    "could not persist demo; exiting with previous on-disk draft intact",
                )?;
                previous_session = bytes;
            }
        }
        if app.settings != previous_settings {
            if let Some(path) = config_path {
                app.settings.save(path)?;
            }
            previous_settings = app.settings.clone();
        }
        if quit {
            if let Some(link) = &mut link {
                link.flush(&mut app)?;
                if !link.connected {
                    exit_notice = Some(safe_text(&app.notice));
                }
            }
            break;
        }
    }
    drop(terminal);
    drop(_guard);
    if let Some(notice) = exit_notice {
        eprintln!("{notice}");
    }
    Ok(())
}

pub fn snapshot(app: &mut App, width: u16, height: u16) -> Result<String> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))?;
    terminal.draw(|frame| render(frame, app, Duration::ZERO))?;
    let buffer = terminal.backend().buffer();
    let mut output = String::new();
    for row in 0..height {
        let mut line = String::new();
        for col in 0..width {
            let cell = &buffer[(col, row)];
            if col > 0 && unicode_width::UnicodeWidthStr::width(buffer[(col - 1, row)].symbol()) > 1
            {
                continue;
            }
            line.push_str(cell.symbol());
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    Ok(output)
}

pub fn reader(id: crate::domain::MessageId, settings: Settings) -> Result<()> {
    let message = crate::fixtures::find(id).context("unknown demo message ID")?;
    let mut app = App::new(crate::app::Session::default(), settings)?;
    app.reader = Some(message);
    app.mode = Mode::Reader;
    run(app, None, None, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::Session, domain::SourceFilter};

    #[test]
    fn arrows_cycle_sources_in_navigation_but_move_the_insert_cursor() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.mode = Mode::Navigate;
        let mut pending = None;
        let mut prefix = None;
        for expected in SourceFilter::ALL
            .into_iter()
            .skip(1)
            .chain([SourceFilter::All])
        {
            key(
                &mut app,
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                4,
                &mut pending,
                &mut prefix,
                None,
            );
            assert_eq!(app.settings.source, expected);
        }
        key(
            &mut app,
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.settings.source, SourceFilter::Office365);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(
            app.session.composer.reply_target().unwrap().provider,
            crate::domain::Provider::Office365
        );
        app.settings.reply_first = false;
        app.editor = Editor::new("café");
        let cursor = app.editor.cursor;
        key(
            &mut app,
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.editor.cursor, cursor - 'é'.len_utf8());
        key(
            &mut app,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.editor.cursor, cursor);
        assert_eq!(app.settings.source, SourceFilter::Office365);
    }
    #[test]
    fn source_dots_and_selection_use_brand_colours_and_seen_markers() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.select(crate::domain::MessageId(2));
        let backend = ratatui::backend::TestBackend::new(100, 50);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, &mut app, Duration::ZERO);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let dots: Vec<_> = buffer
            .content
            .iter()
            .filter(|c| c.symbol() == "●" || c.symbol() == "○")
            .collect();
        assert_eq!(dots.len(), 5);
        assert_eq!(dots.iter().filter(|c| c.symbol() == "○").count(), 1);
        let discord = dots.iter().find(|c| c.symbol() == "○").unwrap();
        assert_eq!(discord.fg, Color::Rgb(88, 101, 242));
        assert_eq!(
            discord.bg,
            source_selection(
                discord.fg,
                palette(Theme::Midnight, Duration::ZERO, false).bg
            )
        );
        let mut colors = std::collections::HashSet::new();
        for dot in dots {
            colors.insert(format!("{:?}", dot.fg));
        }
        assert_eq!(colors.len(), 4);
    }

    #[test]
    fn prompt_editing_uses_shared_validation_and_secret_input_is_masked_and_cancellable() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.attach();
        let draft = app.session.composer.draft.clone();
        edit_setting(&mut app, "spam.prompt");
        app.command = Editor::new("Is this an unsolicited promotion?\nKeep family messages.");
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            4,
            &mut None,
            &mut None,
            None,
        );
        assert_eq!(app.mode, Mode::Settings);
        assert!(app.settings.spam.prompt.contains("Keep family"));
        assert_eq!(app.session.composer.draft, draft);
        app.mode = Mode::Secret;
        app.command = Editor::new("fictional-secret-marker");
        let frame = snapshot(&mut app, 100, 40).unwrap();
        assert!(!frame.contains("fictional-secret-marker"));
        assert!(frame.contains('•'));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            4,
            &mut None,
            &mut None,
            None,
        );
        assert!(app.command.text.is_empty());
        assert_eq!(app.session.composer.draft, draft);
    }
    #[test]
    fn reply_first_arrows_switch_drafts_and_alt_arrows_move_the_cursor() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.editor.insert("first 東京");
        app.update_body();
        let original = app.session.composer.draft.clone();
        let mut pending = None;
        let mut prefix = None;
        key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.selected, Some(crate::domain::MessageId(4)));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.editor.text, "j");
        key(
            &mut app,
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.session.composer.draft, original);
        let cursor = app.editor.cursor;
        key(
            &mut app,
            KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert!(app.editor.cursor < cursor);
        assert_eq!(app.settings.source, SourceFilter::All);
        key(
            &mut app,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.settings.source, SourceFilter::ICloud);
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.session.composer.draft, original);
    }
}
