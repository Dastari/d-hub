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
        || settings::inline(app).is_some()
        || (app.mode == Mode::Command && app.command_origin == Mode::Settings)
    {
        let hit = settings::draw(frame, area, p, app);
        if app.mode == Mode::Command {
            commands::draw(frame, area, p, app);
        }
        return hit;
    }
    // The reply box starts at one line and grows with the draft up to six, under a status line
    // and the recipient, with any attachment chips and the key hints.
    let attachments = app
        .session
        .composer
        .draft
        .as_ref()
        .is_some_and(|d| !d.attachments.is_empty());
    let draft_lines = app.editor.text.split('\n').count().clamp(1, 6) as u16;
    let composer_height = 3 + u16::from(attachments) + draft_lines;
    // A dotted rule, then room for a short chat burst or an email's opening lines.
    let preview_height = 1 + if area.height >= 28 {
        (area.height / 5).clamp(5, 9)
    } else {
        3
    };
    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(preview_height),
        Constraint::Length(composer_height),
        Constraint::Length(1),
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
            match app.filtered_count() {
                0 => String::new(),
                n if app.settings.show_spam => format!(" · {n} filtered shown"),
                n => format!(" · {n} hidden"),
            },
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

    app.preview_bounds = preview::draw(frame, parts[2], p, app, &mut hit);

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
        SendState::Ready
            if locked
                && app
                    .session
                    .composer
                    .draft
                    .as_ref()
                    .is_some_and(|d| d.is_empty()) =>
        {
            "◆ LOCKED · empty, Del releases ".into()
        }
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
    let mut edit_top = composer.y + 2;
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
    // Most useful first; hints that do not fit are dropped from the middle, keeping "? help".
    let mut hints: Vec<&str> = if app.live_feed {
        vec!["v open Discord", "Del/^E dismiss", "? help"]
    } else {
        let typing = app.mode == Mode::Insert;
        let mut hints = vec![match (typing, app.live_service) {
            (true, true) => "⏎ SEND",
            (true, false) => "⏎ mock send",
            (false, true) => "F5 SEND",
            (false, false) => "F5 mock send",
        }];
        if typing {
            hints.push(if app.modified_keys {
                "⇧⏎ newline"
            } else {
                "^J newline"
            });
        }
        hints.extend(["Del dismiss", "↑↓ reply", "PgUp/Dn preview", "? help"]);
        hints
    };
    let footer = |hints: &[&str]| format!("{mode} · {}", hints.join(" · "));
    while hints.len() > 2
        && unicode_width::UnicodeWidthStr::width(footer(&hints).as_str()) > composer.width as usize
    {
        hints.remove(hints.len() - 2);
    }
    frame.render_widget(
        text(footer(&hints), p.muted),
        Rect::new(composer.x, composer.bottom() - 1, composer.width, 1),
    );
    // One bottom line: a fresh notice, otherwise compact status icons.
    let bottom = if app.notice.is_empty() {
        status_bar(app, parts[4].width as usize, p)
    } else {
        // `preview` keeps it to one line with an ellipsis when cut.
        Line::styled(
            preview(&app.notice, parts[4].width as usize + 2),
            Style::default().fg(p.warning),
        )
    };
    frame.render_widget(Paragraph::new(bottom), parts[4]);

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
            let area = parts[4];
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

/// Connection dots per enabled account (named when they fit), arrivals since the drawer was
/// last read, and the demo's next mock outcome.
fn status_bar(app: &App, width: usize, p: Palette) -> Line<'static> {
    use super::settings::status::{health, health_style};
    let mut spans = vec![];
    if app.new_items > 0 {
        spans.push(Span::styled(
            format!("↓{} new  ", app.new_items),
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        ));
    }
    if app.live_feed {
        let (dot, color, label) = if app.bridge_ready {
            ("●", p.success, "capture")
        } else {
            ("!", p.warning, "capture unavailable")
        };
        spans.push(Span::styled(format!("{dot} "), Style::default().fg(color)));
        spans.push(Span::styled(label, Style::default().fg(p.muted)));
        return Line::from(spans);
    }
    if !app.live_service {
        let outcome = match app.next_outcome {
            MockOutcome::Success => "success",
            MockOutcome::Failure => "failure",
            MockOutcome::Uncertain => "uncertain",
        };
        spans.push(Span::styled(
            format!("demo · next send {outcome} (F7)"),
            Style::default().fg(p.muted),
        ));
        return Line::from(spans);
    }
    let accounts: Vec<_> = app
        .settings
        .accounts
        .iter()
        .filter(|a| a.enabled)
        .map(|a| {
            let (dot, color) = health_style(health(app, a).0, p);
            (dot, color, crate::domain::display_name(a.label()))
        })
        .collect();
    if accounts.is_empty() {
        spans.push(Span::styled(
            "○ no accounts connected · , settings",
            Style::default().fg(p.muted),
        ));
        return Line::from(spans);
    }
    let used: usize = spans.iter().map(|s| s.width()).sum();
    let named: usize = accounts
        .iter()
        .map(|(_, _, name)| 3 + unicode_width::UnicodeWidthStr::width(name.as_str()))
        .sum();
    for (dot, color, name) in accounts {
        spans.push(Span::styled(dot, Style::default().fg(color)));
        spans.push(if used + named <= width {
            Span::styled(format!(" {name}  "), Style::default().fg(p.muted))
        } else {
            Span::raw(" ")
        });
    }
    Line::from(spans)
}

/// Who a reply goes to, in human terms: the person being answered, plus mail addresses.
/// Chat recipients are opaque provider IDs (e.g. Discord user snowflakes) and are not shown.
fn reply_context(app: &App, target: &crate::domain::ReplyTarget) -> String {
    let sender = app
        .session
        .messages
        .iter()
        .find(|m| m.id == target.incoming)
        .map(|m| crate::domain::display_name(&m.sender))
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
        assert!(frame.contains("⏎ SEND"));
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

    #[test]
    fn the_reply_box_starts_at_one_line_and_grows_to_six() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        let rows = |app: &mut App| {
            let frame = snapshot(app, 100, 40).unwrap();
            let lines: Vec<_> = frame.lines().map(str::to_owned).collect();
            let footer = lines.iter().position(|l| l.contains("mock send")).unwrap();
            let typed = lines
                .iter()
                .position(|l| l.trim_start().starts_with("line ") || l.contains("Write a reply"));
            (footer, typed.unwrap(), frame)
        };
        let (footer, first, frame) = rows(&mut app);
        assert_eq!(footer - first, 1, "{frame}");
        app.mode = Mode::Insert;
        for n in 1..=3 {
            if n > 1 {
                app.editor.insert("\n");
            }
            app.editor.insert(&format!("line {n}"));
        }
        app.update_body();
        let (footer, first, frame) = rows(&mut app);
        assert_eq!(footer - first, 3, "{frame}");
        for n in 4..=9 {
            app.editor.insert(&format!("\nline {n}"));
        }
        app.update_body();
        let (footer, first, frame) = rows(&mut app);
        assert_eq!(footer - first, 6, "capped at six lines: {frame}");
        assert!(
            frame.contains("line 9") && !frame.contains("line 3"),
            "{frame}"
        );
    }

    #[test]
    fn the_feed_and_composer_name_mail_senders_without_repeating_their_address() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        let target = app
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
        incoming.sender = "Apple <hello@example.invalid>".into();
        assert_eq!(
            reply_context(&app, &{
                let mut t = target.clone();
                t.recipients = vec!["hello@example.invalid".into()];
                t
            }),
            "Apple <hello@example.invalid>"
        );
        let frame = snapshot(&mut app, 100, 40).unwrap();
        assert!(frame.lines().any(|l| l.starts_with("Apple  ")), "{frame}");
        assert!(
            !frame.contains("Apple <hello@example.invalid> <"),
            "{frame}"
        );
    }

    #[test]
    fn one_bottom_line_shows_a_fresh_notice_then_compact_account_status() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.live_service = true;
        for (id, provider, status) in [
            ("icloud", Provider::ICloud, "Connected · 3 new"),
            ("work", Provider::Office365, "Connecting…"),
            ("chat", Provider::Discord, "Sync failed: offline"),
        ] {
            let mut account = crate::policy::AccountConfig::new(id, provider);
            account.enabled = true;
            app.settings.accounts.push(account);
            app.provider_status.insert(id.into(), status.into());
        }
        app.notice = "Reply ready".into();
        let last = |app: &mut App, width| {
            snapshot(app, width, 40)
                .unwrap()
                .lines()
                .last()
                .unwrap()
                .to_owned()
        };
        assert_eq!(last(&mut app, 100), "Reply ready");
        // The notice clears itself after a few seconds; status icons take the line.
        assert!(!app.expire_notice());
        app.notice_since = Some((
            app.notice.clone(),
            std::time::Instant::now() - Duration::from_secs(9),
        ));
        assert!(app.expire_notice());
        assert!(app.notice.is_empty());
        app.new_items = 2;
        let wide = last(&mut app, 100);
        assert!(
            wide.starts_with("↓2 new  ● icloud  ◌ work  ! chat"),
            "{wide}"
        );
        assert!(!wide.contains("active"), "{wide}");
        // Too narrow for names: dots only, still in their colours.
        let narrow = last(&mut app, 30);
        assert_eq!(narrow.trim_end(), "↓2 new  ● ◌ !", "{narrow}");
        let frame = snapshot(&mut app, 100, 40).unwrap();
        assert!(!frame.contains("inactive"), "{frame}");
    }

    #[test]
    fn the_preview_rule_takes_the_source_colour_and_hides_without_a_selection() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        let p = palette(Theme::Midnight, Duration::ZERO, false);
        let rule_row = |app: &mut App| {
            let backend = ratatui::backend::TestBackend::new(60, 40);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            terminal
                .draw(|frame| {
                    draw(frame, app, Duration::ZERO);
                })
                .unwrap();
            let buffer = terminal.backend().buffer().clone();
            (0..40u16)
                .find(|&y| (0..60u16).all(|x| buffer[(x, y)].symbol() == "·"))
                .map(|y| (buffer[(0, y)].fg, buffer[(59, y)].fg))
        };
        let message = app.selected_message().cloned().unwrap();
        let source = provider_color(message.provider(), p.accent);
        let (left, right) = rule_row(&mut app).expect("rule under the feed");
        assert_ne!(left, right, "the rule fades across the width");
        assert_ne!(left, source);
        assert_ne!(left, p.muted);
        app.selected = None;
        assert!(rule_row(&mut app).is_none(), "no selection, no rule");
    }
}
