//! The main drawer: header, chronological feed, preview, composer and overlays.
use super::*;

pub fn render(frame: &mut Frame, app: &mut App, elapsed: Duration) {
    draw(frame, app, elapsed);
}

pub(super) fn draw(frame: &mut Frame, app: &mut App, elapsed: Duration) -> HitAreas {
    let p = palette(app.settings.theme, elapsed, app.settings.reduced_motion);
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(p.bg).fg(p.fg)),
        area,
    );
    if app.mode == Mode::DiscordQr {
        if let Some(setup) = &app.discord_setup {
            draw_discord_qr(frame, area, p, setup);
        }
        return HitAreas::default();
    }
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
    if app.mode == Mode::Settings
        || (app.mode == Mode::Command && app.command_origin == Mode::Settings)
    {
        let hit = settings::draw(frame, area, p, app);
        if app.mode == Mode::Command {
            commands::draw(frame, area, p, app);
        }
        return hit;
    }
    let composer_height = (area.height / 3).clamp(6, 11);
    // Tall enough for a short chat burst or an email's sender, date and opening lines.
    let preview_height = if area.height >= 28 {
        (area.height / 5).clamp(5, 9)
    } else {
        3
    };
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
            if app.live_service {
                "  LIVE"
            } else if app.live_feed {
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

    let mut hit = HitAreas {
        feed: parts[1],
        composer: parts[3],
        ..HitAreas::default()
    };
    feed::draw(frame, parts[1], p, app, &mut hit);

    preview::draw(frame, padded(parts[2]), p, app);

    frame.render_widget(
        Block::default().style(Style::default().bg(p.surface)),
        parts[3],
    );
    let composer = padded(parts[3]);
    let target = app.session.composer.reply_target();
    let locked = app.session.composer.draft.is_some();
    let recipient = target.map(|t| reply_context(app, t)).unwrap_or_else(|| {
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
        SendState::Uncertain(_) => {
            if app.live_service {
                "? UNCERTAIN · check original client; retry locked ".into()
            } else {
                "? UNCERTAIN · :reconcile ".into()
            }
        }
        SendState::Failed(_) => "! FAILED · draft retained ".into(),
        SendState::Ready if locked => "◆ LOCKED ".into(),
        _ => "↳ Reply ".into(),
    };
    if app.live_service {
        state = state.replace("MOCK SEND", "SENDING");
    }
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
    let account = target.map_or_else(String::new, |t| {
        let account = app
            .settings
            .accounts
            .iter()
            .find(|a| a.id == t.account && a.provider == t.provider)
            .map_or(t.account.as_str(), |a| a.label());
        format!("{} · {account}", t.provider.label())
    });
    let attachment_count = app
        .session
        .composer
        .draft
        .as_ref()
        .map_or(0, |d| d.attachments.len());
    let account = format!(
        "{}{}",
        safe_text(&account),
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
                format!("{mode}  ·  v open Discord  ·  ^E dismiss  ·  ? help")
            } else if app.live_service {
                format!("{mode} · F5 SEND · ^E dismiss · ↑↓ reply · ←→ source · ? help")
            } else if app.settings.reply_first {
                format!("{mode} · F5 mock send · ^E dismiss · ↑↓ reply · ←→ source")
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
                } else if app.live_service {
                    app.provider_status
                        .values()
                        .next()
                        .map(String::as_str)
                        .unwrap_or("Waiting for enabled accounts")
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
            if app.live_service {
                "Controls · LIVE sends"
            } else {
                "Controls · all sends simulated"
            },
            HELP,
            app.reader_scroll,
        ),
        Mode::Discard => overlay(
            frame,
            area,
            p,
            "Discard this draft?",
            "Text and attachments will be removed.\n\ny confirms · n / Esc keeps the draft",
            0,
        ),
        Mode::EditSetting => {
            let title = settings::edit_title(app.edit_key.as_deref().unwrap_or("Setting"));
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
        Mode::Command => commands::draw(frame, area, p, app),
        Mode::Search => {
            let line = format!("/{}", app.command.text);
            let area = Rect::new(parts[4].x, parts[4].y + 1, parts[4].width, 1);
            frame.render_widget(Clear, area);
            frame.render_widget(
                text(line, p.accent).style(Style::default().fg(p.accent).bg(p.surface)),
                area,
            );
            let column =
                unicode_width::UnicodeWidthStr::width(&app.command.text[..app.command.cursor])
                    as u16
                    + 1;
            frame.set_cursor_position((area.x + column.min(area.width.saturating_sub(1)), area.y));
        }
        Mode::Reader => {
            if let Some((title, mut body)) = preview::reader(app) {
                if !app.reader_query.is_empty() {
                    body = format!("Search: {}\n\n{body}", safe_text(&app.reader_query));
                }
                overlay(frame, area, p, &title, &body, app.reader_scroll);
            }
        }
        _ => {}
    }
    hit
}

/// Who a reply goes to, in human terms: the person being answered, plus mail addresses.
/// Chat recipients are opaque provider IDs (e.g. Discord user snowflakes) and are not shown.
fn reply_context(app: &App, target: &crate::domain::ReplyTarget) -> String {
    let sender = app
        .session
        .messages
        .iter()
        .find(|m| m.id == target.incoming)
        .map(|m| safe_text(&m.sender))
        .filter(|s| !s.trim().is_empty());
    let addresses: Vec<_> = target
        .recipients
        .iter()
        .filter(|r| r.contains('@'))
        .map(|r| safe_text(r))
        .collect();
    let others = target.recipients.len().saturating_sub(1);
    match (sender, addresses.is_empty()) {
        (Some(name), false) if !addresses.contains(&name) => {
            format!("{name} <{}>", addresses.join(", "))
        }
        (_, false) => addresses.join(", "),
        (Some(name), true) if others > 0 => format!("{name} + {others} more"),
        (Some(name), true) => name,
        (None, true) => format!("{} conversation", target.provider.label()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Session;

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
    fn live_surface_labels_real_sends_and_blocks_disabled_accounts_and_simulation() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.live_service = true;
        app.shared = true;
        app.editor.insert("Fictional live reply");
        app.update_body();
        app.actions.clear();
        let mut pending = None;
        let mut prefix = None;
        key(
            &mut app,
            KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert!(app.actions.is_empty());
        assert!(app.notice.contains("Sending disabled"));
        let outcome = app.next_outcome;
        key(
            &mut app,
            KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert_eq!(app.next_outcome, outcome);
        let frame = snapshot(&mut app, 100, 40).unwrap();
        assert!(frame.contains("LIVE"));
        assert!(frame.contains("F5 SEND"));
        assert!(!frame.contains("SERVICE DEMO"));
        let account = app
            .session
            .composer
            .draft
            .as_ref()
            .unwrap()
            .target
            .account
            .clone();
        let mut account = crate::policy::AccountConfig::new(&account, Provider::ICloud);
        account.enabled = true;
        account.allow_send = true;
        app.settings.accounts.push(account);
        key(
            &mut app,
            KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
            4,
            &mut pending,
            &mut prefix,
            None,
        );
        assert!(pending.is_none());
        assert!(matches!(
            app.session.composer.send,
            SendState::Submitting(_)
        ));
        assert!(app.notice.contains("live service"));
    }

    #[test]
    fn composer_names_the_person_instead_of_provider_recipient_ids() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        let mut target = app
            .session
            .composer
            .reply_target()
            .cloned()
            .expect("target");
        let incoming = app
            .session
            .messages
            .iter_mut()
            .find(|m| m.id == target.incoming)
            .unwrap();
        incoming.sender = "AWildWarrior".into();
        target.provider = Provider::Discord;
        target.recipients = vec!["331305346087780362".into()];
        assert_eq!(reply_context(&app, &target), "AWildWarrior");
        target.recipients.push("100000000000000009".into());
        assert_eq!(reply_context(&app, &target), "AWildWarrior + 1 more");
        target.provider = Provider::ICloud;
        target.recipients = vec!["warrior@example.invalid".into()];
        assert_eq!(
            reply_context(&app, &target),
            "AWildWarrior <warrior@example.invalid>"
        );
        let frame = snapshot(&mut app, 40, 40).unwrap();
        assert!(!frame.contains("331305346087780362"));
    }
}
