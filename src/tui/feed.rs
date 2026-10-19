//! Feed layout. Consecutive messages with one `Message::stack_key` share a heading while they
//! stay adjacent in the current view; each shows its local time and a short body preview.
//! Rows vary in height, so the hit area of every rendered message is recorded for the mouse.
use super::*;
use crate::domain::Message;
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

/// Preview lines per message; mail spends the first on its subject.
const PREVIEW_LINES: usize = 3;
/// Continuation lines align after "  ● ".
const INDENT: usize = 4;

/// Provider plus account name; captured notifications stay visibly read-only.
pub(super) fn source_label(app: &App, message: &Message) -> String {
    let configured = app.settings.accounts.iter().find(|a| {
        Some(a.id.as_str()) == message.account() && Some(a.provider) == message.provider()
    });
    let base = match (message.provider(), message.target.is_some()) {
        (Some(Provider::Discord), false) => "Discord notification",
        (Some(Provider::Discord), true) if configured.is_none() && !app.live_service => {
            "Discord bot (demo)"
        }
        (provider, _) => provider.map_or("unsupported", |p| p.label()),
    };
    // Name the account so several of one provider stay distinguishable, unless the name only
    // repeats the provider ("iCloud · iCloud").
    match configured {
        Some(account) if !account.label().trim().eq_ignore_ascii_case(base) => {
            format!("{base} · {}", safe_text(account.label()))
        }
        _ => base.to_owned(),
    }
}

/// Body text on one line, without quoted reply history; links read as their text or host.
fn body_text(message: &Message) -> String {
    super::links::plain(&safe_text(&message.body))
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.starts_with('>'))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Wraps with a narrower first line (it carries the time) and at most `max` lines.
fn clamp(value: &str, first: usize, rest: usize, max: usize) -> Vec<String> {
    if value.is_empty() || max == 0 {
        return vec![];
    }
    let head = wrapped(value, first).swap_remove(0);
    let remainder = value[head.len()..].trim_start();
    let mut lines = vec![head];
    if !remainder.is_empty() {
        lines.extend(wrapped(remainder, rest));
    }
    if lines.len() > max {
        lines.truncate(max);
        let width = if max == 1 { first } else { rest };
        let last = &mut lines[max - 1];
        *last = preview(last, width);
        if !last.ends_with('…') {
            last.push('…');
        }
    }
    lines
}

struct Laid {
    lines: Vec<Line<'static>>,
    /// Line range of each message, including its heading when it opens a group.
    ranges: Vec<(MessageId, Range<usize>)>,
}

fn layout(app: &App, messages: &[&Message], width: usize, p: Palette, limit: usize) -> Laid {
    let mut laid = Laid {
        lines: vec![],
        ranges: vec![],
    };
    let mut previous = None;
    for message in messages {
        if laid.lines.len() >= limit {
            break;
        }
        let start = laid.lines.len();
        let color = provider_color(message.provider(), p.accent);
        let key = message.stack_key();
        if previous != Some(key) {
            if previous.is_some() {
                laid.lines.push(Line::default());
            }
            let sender = preview(&crate::domain::display_name(&message.sender), width);
            let source = format!("  {}", source_label(app, message));
            laid.lines.push(Line::from(vec![
                Span::styled(
                    sender,
                    Style::default().fg(p.fg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(source, Style::default().fg(p.muted)),
            ]));
        }
        previous = Some(key);
        let body_start = laid.lines.len();

        let mut meta = crate::when::short(message.timestamp, &safe_text(&message.time));
        if app.dismissed(message.id) {
            meta.push_str(" · dismissed");
        }
        if let Some(reason) = app.filter_rule(message) {
            meta.push_str(&format!(" · {}", reason.label()));
        }
        let rest = width.saturating_sub(INDENT).max(1);
        let mut first = rest.saturating_sub(meta.width() + 2);
        // Too narrow to share a line: the time gets its own.
        let meta_alone = first < 12;
        if meta_alone {
            first = rest;
        }
        let selected = Some(message.id) == app.selected;
        let text_color = if selected || message.unread {
            p.fg
        } else {
            p.muted
        };
        let mut rows: Vec<(String, Style)> = vec![];
        let body = body_text(message);
        if message.is_mail() {
            let subject = safe_text(&message.subject);
            for line in clamp(&subject, first, rest, 1) {
                rows.push((
                    line,
                    Style::default().fg(text_color).add_modifier(Modifier::BOLD),
                ));
            }
            let budget = PREVIEW_LINES - rows.len();
            for line in clamp(&body, rest, rest, budget) {
                rows.push((line, Style::default().fg(p.muted)));
            }
        } else {
            for line in clamp(&body, first, rest, PREVIEW_LINES) {
                rows.push((line, Style::default().fg(text_color)));
            }
        }
        if rows.is_empty() {
            rows.push((
                "(no text)".into(),
                Style::default().fg(p.muted).add_modifier(Modifier::ITALIC),
            ));
        }
        let dot = Span::styled(
            if message.unread { "  ● " } else { "  ○ " },
            Style::default().fg(color),
        );
        let meta_span = Span::styled(meta.clone(), Style::default().fg(p.muted));
        if meta_alone {
            laid.lines
                .push(Line::from(vec![dot.clone(), meta_span.clone()]));
        }
        for (index, (line, style)) in rows.into_iter().enumerate() {
            let lead = if index == 0 && !meta_alone {
                dot.clone()
            } else {
                Span::raw(" ".repeat(INDENT))
            };
            let mut spans = vec![lead];
            if index == 0 && !meta_alone {
                let gap = rest.saturating_sub(line.width() + meta.width()).max(2);
                spans.extend([
                    Span::styled(line, style),
                    Span::raw(" ".repeat(gap)),
                    meta_span.clone(),
                ]);
            } else {
                spans.push(Span::styled(line, style));
            }
            laid.lines.push(Line::from(spans));
        }
        if selected {
            let bg = source_selection(color, p.bg);
            for line in &mut laid.lines[body_start..] {
                line.style = line.style.bg(bg);
            }
        }
        laid.ranges.push((message.id, start..laid.lines.len()));
    }
    laid
}

/// Draws the feed from `app.feed_offset`, revealing the selection only when it changed, so
/// wheel scrolling is never pulled back. Records message hit areas and the visible count.
pub(super) fn draw(frame: &mut Frame, area: Rect, p: Palette, app: &mut App, hit: &mut HitAreas) {
    let visible = app.visible();
    if visible.is_empty() {
        let reasons = app.empty_reasons();
        let mut lines = vec![Line::styled(
            if reasons.is_empty() {
                "Nothing here yet."
            } else {
                "Nothing to show:"
            },
            Style::default().fg(p.muted),
        )];
        if reasons.is_empty() {
            lines.push(Line::styled(
                if app.live_service {
                    "New messages appear as connected accounts sync."
                } else {
                    "No messages match this source/focus."
                },
                Style::default().fg(p.muted),
            ));
        }
        lines.extend(
            reasons
                .into_iter()
                .map(|r| Line::styled(format!("· {r}"), Style::default().fg(p.fg))),
        );
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        return;
    }
    let width = area.width as usize;
    let height = area.height as usize;
    let mut offset = app.feed_offset.min(visible.len() - 1);
    let selected = visible.iter().position(|m| Some(m.id) == app.selected);
    if app.feed_revealed != app.selected
        && let Some(index) = selected
    {
        offset = offset.min(index);
        while offset < index {
            let laid = layout(app, &visible[offset..=index], width, p, usize::MAX);
            if laid.lines.len() <= height {
                break;
            }
            offset += 1;
        }
    }
    let laid = layout(app, &visible[offset..], width, p, height);
    let mut fully_visible = 0;
    for (id, range) in &laid.ranges {
        let end = range.end.min(height);
        if end > range.start {
            hit.feed_items.push((
                Rect::new(
                    area.x,
                    area.y + range.start as u16,
                    area.width,
                    (end - range.start) as u16,
                ),
                *id,
            ));
        }
        if range.end <= height {
            fully_visible += 1;
        }
    }
    hit.feed_page = fully_visible.max(1);
    if let Some((rect, _)) = hit
        .feed_items
        .iter()
        .find(|(_, id)| Some(*id) == app.selected)
    {
        let color = provider_color(
            visible
                .iter()
                .find(|m| Some(m.id) == app.selected)
                .and_then(|m| m.provider()),
            p.accent,
        );
        // Highlight below the heading only; the heading belongs to the whole group.
        let heading = laid
            .ranges
            .iter()
            .find(|(id, _)| Some(*id) == app.selected)
            .map_or(0, |(_, range)| {
                laid.lines[range.clone()]
                    .iter()
                    .take_while(|line| line.style.bg.is_none())
                    .count() as u16
            });
        let body = Rect {
            y: rect.y + heading.min(rect.height),
            height: rect.height.saturating_sub(heading),
            ..*rect
        };
        frame.render_widget(
            Block::default().style(Style::default().bg(source_selection(color, p.bg))),
            body,
        );
    }
    let selected_id = app.selected;
    frame.render_widget(Paragraph::new(laid.lines), area);
    app.feed_offset = offset;
    app.feed_revealed = selected_id;
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{
        app::Session,
        domain::{ReplyMode, ReplyTarget},
    };

    pub(in crate::tui) fn message(
        id: u64,
        provider: Provider,
        sender: &str,
        conversation: &str,
        body: &str,
    ) -> Message {
        let mut message = crate::fixtures::messages().remove(0);
        message.id = MessageId(id);
        message.timestamp = id;
        message.time = format!("10:{id:02}");
        message.provider = Some(provider);
        message.account = Some("home".into());
        message.sender = sender.into();
        message.subject = "Direct message".into();
        message.body = body.into();
        message.unread = true;
        message.target = Some(ReplyTarget {
            provider,
            account: "home".into(),
            conversation: conversation.into(),
            incoming: MessageId(id),
            recipients: vec!["331305346087780362".into()],
            reply_mode: ReplyMode::Sender,
        });
        message
    }

    pub(in crate::tui) fn app(messages: Vec<Message>) -> App {
        let mut app = App::new(
            Session {
                messages,
                ..Session::default()
            },
            Settings::default(),
        )
        .unwrap();
        app.mode = Mode::Navigate;
        app
    }

    /// Renders only the feed, so composer and preview-pane text cannot satisfy assertions.
    fn feed(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        let p = palette(Theme::Midnight, Duration::ZERO, true);
        terminal
            .draw(|f| draw(f, f.area(), p, app, &mut HitAreas::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn headings(frame: &str, sender: &str) -> usize {
        let heading = format!("{sender}  ");
        frame
            .lines()
            .filter(|line| line.starts_with(&heading))
            .count()
    }

    #[test]
    fn adjacent_messages_share_a_heading_only_while_adjacent_in_the_current_view() {
        let mut app = app(vec![
            message(1, Provider::Discord, "Son", "dm-1", "first"),
            message(2, Provider::Discord, "Son", "dm-1", "second"),
            message(3, Provider::Teams, "Jules", "chat-9", "in between"),
            message(4, Provider::Discord, "Son", "dm-1", "fourth"),
            message(5, Provider::Discord, "Son", "group-2", "group chat"),
        ]);
        let all = feed(&mut app, 60, 50);
        // Newest first: group(5) · Son(4) · Jules(3) · Son(2,1).
        assert_eq!(headings(&all, "Son"), 3, "{all}");
        assert_eq!(headings(&all, "Jules"), 1);
        assert!(
            !all.contains("Direct message"),
            "chat subjects are replaced by previews"
        );
        for body in ["first", "second", "in between", "fourth", "group chat"] {
            assert!(all.contains(body), "missing preview {body}");
        }
        app.set_source(SourceFilter::Discord);
        let discord = feed(&mut app, 60, 50);
        // Teams no longer separates them; the group DM is still its own conversation.
        assert_eq!(headings(&discord, "Son"), 2, "{discord}");
    }

    #[test]
    fn previews_are_bounded_and_mail_keeps_its_subject() {
        let long = "word ".repeat(200);
        let mut mail = message(
            2,
            Provider::ICloud,
            "Mira",
            "thread",
            &format!("> quoted\n{long}"),
        );
        mail.subject = "Sunday by the water".into();
        let mut app = app(vec![
            message(1, Provider::Discord, "Son", "dm", &long),
            mail,
        ]);
        let frame = feed(&mut app, 50, 50);
        assert!(frame.contains("Sunday by the water"));
        assert!(!frame.contains("quoted"));
        let lines: Vec<_> = frame.lines().filter(|l| l.contains("word")).collect();
        assert_eq!(
            lines.len(),
            2 + PREVIEW_LINES,
            "mail 2 body lines + chat 3:\n{frame}"
        );
        assert_eq!(
            lines.iter().filter(|l| l.trim_end().ends_with('…')).count(),
            2
        );
        assert!(frame.lines().all(|l| l.width() <= 50));
    }

    #[test]
    fn clicks_map_to_variable_height_messages_and_selection_is_revealed_once() {
        let messages: Vec<_> = (1..=12)
            .map(|id| {
                message(
                    id,
                    if id % 2 == 0 {
                        Provider::Discord
                    } else {
                        Provider::Teams
                    },
                    &format!("Person{id}"),
                    &format!("c{id}"),
                    &"long preview text ".repeat(id as usize),
                )
            })
            .collect();
        let mut app = app(messages);
        let oldest = MessageId(1);
        app.select(oldest);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 30)).unwrap();
        let mut hits = HitAreas::default();
        terminal
            .draw(|f| hits = super::super::draw(f, &mut app, Duration::ZERO))
            .unwrap();
        assert!(
            hits.feed_items.iter().any(|(_, id)| *id == oldest),
            "selection revealed"
        );
        assert!(app.feed_offset > 0);
        // Wheel scrolling away is not undone by the next frame.
        app.feed_offset = 0;
        terminal
            .draw(|f| hits = super::super::draw(f, &mut app, Duration::ZERO))
            .unwrap();
        assert_eq!(app.feed_offset, 0);
        // Clicking any line of a taller row selects that message.
        let (rect, id) = hits
            .feed_items
            .iter()
            .find(|(r, _)| r.height > 2)
            .copied()
            .unwrap();
        super::super::mouse(
            &mut app,
            crossterm::event::MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x + 3,
                row: rect.bottom() - 1,
                modifiers: KeyModifiers::NONE,
            },
            &hits,
        );
        assert_eq!(app.selected, Some(id));
    }

    #[test]
    fn live_discord_never_falls_back_to_the_demo_label() {
        let mut app = app(vec![message(1, Provider::Discord, "Son", "dm", "hi")]);
        app.live_service = true;
        let frame = feed(&mut app, 60, 30);
        assert!(!frame.contains("demo"));
        assert!(frame.contains("Son  Discord"));
    }

    #[test]
    fn account_names_that_repeat_the_provider_are_not_shown_twice() {
        let mut app = app(vec![message(1, Provider::ICloud, "Apple", "t", "hi")]);
        let mut account = crate::policy::AccountConfig::new("home", Provider::ICloud);
        account.name = "iCloud".into();
        app.settings.accounts.push(account);
        let frame = feed(&mut app, 60, 30);
        assert!(frame.contains("Apple  iCloud"), "{frame}");
        assert!(!frame.contains("iCloud · iCloud"), "{frame}");
        app.settings.accounts[0].name = "Personal".into();
        assert!(feed(&mut app, 60, 30).contains("Apple  iCloud · Personal"));
    }

    #[test]
    fn an_empty_feed_says_what_is_hidden_and_how_to_show_it() {
        let mut app = app(vec![
            message(1, Provider::ICloud, "Mira", "a", "hi"),
            message(2, Provider::ICloud, "Shop", "b", "sale"),
            message(3, Provider::Discord, "Son", "c", "hey"),
            message(4, Provider::ICloud, "Kai", "d", "dinner"),
        ]);
        app.live_service = true;
        // Live messages are untagged; the last keeps a demo tag outside the Work focus.
        for message in &mut app.session.messages {
            message.focus = crate::domain::Focus::All;
        }
        app.session.messages[3].focus = crate::domain::Focus::Personal;
        app.set_profile("work");
        app.session.messages[1].target.as_mut().unwrap().recipients =
            vec!["noreply@example.invalid".into()];
        app.session
            .message_state
            .entry(MessageId(1))
            .or_default()
            .dismissed = true;
        app.set_source(crate::domain::SourceFilter::ICloud);
        app.query = "invoice".into();
        let frame = feed(&mut app, 60, 12);
        for reason in [
            "Nothing to show:",
            "1 dismissed · D shows them",
            "1 filtered · S shows them",
            "searching \"invoice\"",
            "1 outside Work focus · f changes focus",
            "1 in other sources · ←→ changes source",
        ] {
            assert!(frame.contains(reason), "{reason}: {frame}");
        }
        let mut empty = super::tests::app(vec![]);
        empty.live_service = true;
        let frame = feed(&mut empty, 60, 6);
        assert!(frame.contains("Nothing here yet."), "{frame}");
        assert!(frame.contains("as connected accounts sync"), "{frame}");
    }
}
