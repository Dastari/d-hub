//! The pane under the feed and the full reader. A chat burst reads as a short transcript with a
//! time per message; an email previews its body alone (the feed row already names the sender,
//! subject and time) and the reader adds its sender and full local date.
use super::*;
use crate::{domain::Message, when};
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

/// "Mira <mira@example.com>" when the reply address differs from the display name.
fn from(message: &Message) -> String {
    let sender = crate::domain::display_name(&message.sender);
    let address = message
        .target
        .iter()
        .flat_map(|t| &t.recipients)
        .find(|r| r.contains('@'))
        .map(|r| safe_text(r));
    match address {
        Some(address) if !sender.is_empty() && address != sender => {
            format!("{sender} <{address}>")
        }
        Some(address) => address,
        None => sender,
    }
}

/// The selected chat burst oldest first, or the message alone (mail, or a standalone reader).
fn burst<'a>(app: &'a App, message: &'a Message) -> Vec<&'a Message> {
    let mut burst = app.burst(message.id);
    if !burst.iter().any(|m| m.id == message.id) {
        return vec![message];
    }
    burst.reverse();
    burst
}

/// Wraps each source line separately so a chat's own line breaks survive.
fn body_lines(body: &str, width: usize) -> Vec<String> {
    let body = safe_text(body);
    let lines: Vec<_> = body
        .trim()
        .lines()
        .flat_map(|line| wrapped(line, width))
        .collect();
    if lines.iter().all(|l| l.is_empty()) {
        vec!["(no text)".into()]
    } else {
        lines
    }
}

/// Preview lines and the line range of the selected message.
fn layout(
    app: &App,
    message: &Message,
    width: usize,
    p: Palette,
) -> (Vec<Line<'static>>, Range<usize>) {
    let muted = Style::default().fg(p.muted);
    let mut lines = vec![];
    if message.is_mail() {
        lines.extend(
            body_lines(&message.body, width)
                .into_iter()
                .map(|l| Line::styled(l, Style::default().fg(p.fg))),
        );
        let all = 0..lines.len();
        return (lines, all);
    }
    let burst = burst(app, message);
    let header: Vec<_> = when::day(burst[0].timestamp)
        .into_iter()
        .chain((burst.len() > 1).then(|| format!("{} messages", burst.len())))
        .collect();
    if !header.is_empty() {
        lines.push(Line::styled(header.join(" · "), muted));
    }
    let mut selected = 0..0;
    let mut day = when::day(burst[0].timestamp);
    for m in burst {
        let this_day = when::day(m.timestamp);
        if this_day.is_some() && this_day != day {
            lines.push(Line::styled(this_day.clone().unwrap_or_default(), muted));
            day = this_day;
        }
        let start = lines.len();
        let clock = when::clock(m.timestamp, &safe_text(&m.time));
        let indent = clock.width() + 2;
        let clock_style = if m.id == message.id {
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
        } else {
            muted
        };
        for (index, line) in body_lines(&m.body, width.saturating_sub(indent).max(1))
            .into_iter()
            .enumerate()
        {
            let lead = if index == 0 {
                Span::styled(format!("{clock}  "), clock_style)
            } else {
                Span::raw(" ".repeat(indent))
            };
            lines.push(Line::from(vec![
                lead,
                Span::styled(line, Style::default().fg(p.fg)),
            ]));
        }
        if m.id == message.id {
            selected = start..lines.len();
        }
    }
    (lines, selected)
}

/// Draws the preview under a dotted rule and returns its page size and the scroll offsets
/// (relative to the default view) that PageUp/PageDown may reach.
pub(super) fn draw(frame: &mut Frame, area: Rect, p: Palette, app: &App) -> (usize, isize, isize) {
    let muted = Style::default().fg(p.muted);
    frame.render_widget(
        Paragraph::new(Line::styled("·".repeat(area.width as usize), muted)),
        Rect { height: 1, ..area },
    );
    let area = padded(Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    });
    let Some(message) = app.selected_message() else {
        frame.render_widget(
            text("Select a communication to read its context.", p.muted),
            area,
        );
        return (1, 0, 0);
    };
    let (width, height) = (area.width as usize, area.height as usize);
    let (lines, selected) = layout(app, message, width, p);
    let total = lines.len();
    if total <= height || height == 0 {
        frame.render_widget(Paragraph::new(lines), area);
        return (height.max(1), 0, 0);
    }
    // Too long: "more" markers get their own lines so they never cover text. By default keep
    // the selected chat message in view (preferring the newest end); mail starts at the top.
    let rows_from = |start: usize| {
        let mut rows = height - usize::from(start > 0 && height >= 2);
        if start + rows < total && rows > 1 {
            rows -= 1;
        }
        rows
    };
    let last = total - (height - usize::from(height >= 2)).max(1);
    let default = if message.is_mail() {
        0
    } else {
        selected.start.min(last)
    };
    let start = (default as isize + app.preview_offset()).clamp(0, last as isize) as usize;
    let rows = rows_from(start);
    let more = muted.add_modifier(Modifier::ITALIC);
    let mut window = vec![];
    if start > 0 && height >= 2 {
        window.push(Line::styled("↑ PgUp earlier · o reads all", more));
    }
    window.extend(lines[start..(start + rows).min(total)].iter().cloned());
    if start + rows < total && window.len() < height {
        window.push(Line::styled("↓ PgDn more · o reads all", more));
    }
    frame.render_widget(Paragraph::new(window), area);
    let page = height.saturating_sub(2).max(1);
    (page, -(default as isize), last as isize - default as isize)
}

/// Title and body for the reader: the full email with its headers, or the chat burst as a
/// transcript with dates and times.
pub(super) fn reader(app: &App) -> Option<(String, String)> {
    let message = app.reader.as_ref()?;
    let title = format!(
        "{} · {}",
        crate::domain::display_name(&message.sender),
        safe_text(&message.subject)
    );
    if message.is_mail() {
        let body = format!(
            "From: {}\nDate: {}\n\n{}",
            from(message),
            when::full(message.timestamp, &safe_text(&message.time)),
            safe_text(&message.body)
        );
        return Some((title, body));
    }
    let mut body = String::new();
    let mut day = None;
    for m in burst(app, message) {
        let this_day = when::day(m.timestamp);
        if let Some(label) = this_day.as_ref().filter(|_| this_day != day) {
            body.push_str(&format!("── {label} ──\n\n"));
            day = this_day.clone();
        }
        body.push_str(&format!(
            "{} · {}\n{}\n\n",
            when::clock(m.timestamp, &safe_text(&m.time)),
            crate::domain::display_name(&m.sender),
            safe_text(m.body.trim())
        ));
    }
    Some((title, body.trim_end().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::super::feed::tests::{app, message};
    use super::*;

    /// Renders only the preview pane (its dotted rule included) and records its scroll bounds.
    fn pane(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        let p = palette(Theme::Midnight, Duration::ZERO, true);
        terminal
            .draw(|f| app.preview_bounds = draw(f, f.area(), p, app))
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

    fn find(app: &mut App, id: u64) -> &mut Message {
        let id = MessageId(id);
        app.session
            .messages
            .iter_mut()
            .find(|m| m.id == id)
            .unwrap()
    }

    /// Three Discord messages in a row, a Teams message, then an email; real timestamps.
    fn inbox() -> App {
        let base = 1_792_292_580;
        let mut messages = vec![
            message(1, Provider::Discord, "Son", "dm", "hey are you around"),
            message(
                2,
                Provider::Discord,
                "Son",
                "dm",
                "need a lift\nafter practice",
            ),
            message(3, Provider::Discord, "Son", "dm", "you there?"),
            message(4, Provider::Teams, "Jules", "chat", "standup moved"),
            message(
                5,
                Provider::ICloud,
                "Mira",
                "thread",
                "Shall we bring a picnic?",
            ),
        ];
        for m in &mut messages {
            m.timestamp = base + m.id.0 * 60;
        }
        messages[4].subject = "Sunday".into();
        messages[4].target.as_mut().unwrap().recipients = vec!["mira@example.invalid".into()];
        app(messages)
    }

    #[test]
    fn a_selected_chat_stack_previews_every_message_with_its_time() {
        let mut app = inbox();
        app.select(MessageId(3));
        let frame = pane(&mut app, 50, 8);
        let base = 1_792_292_580;
        assert!(frame.contains(&when::day(base + 60).unwrap()), "{frame}");
        assert!(frame.contains("3 messages"), "{frame}");
        for (id, body) in [
            (1, "hey are you around"),
            (2, "need a lift"),
            (3, "you there?"),
        ] {
            let line = frame.lines().find(|l| l.contains(body)).expect(body);
            assert!(line.contains(&when::clock(base + id * 60, "")), "{line}");
        }
        assert!(frame.contains("after practice"), "chat line breaks survive");
        assert!(
            !frame.contains("standup"),
            "other sources stay out of the stack"
        );
        // Oldest first, like a conversation.
        assert!(frame.find("hey are you").unwrap() < frame.find("you there").unwrap());
    }

    #[test]
    fn email_preview_shows_only_the_body_and_the_reader_adds_sender_and_date() {
        let mut app = inbox();
        find(&mut app, 5).sender = "Mira <mira@example.invalid>".into();
        app.select(MessageId(5));
        let date = when::full(1_792_292_580 + 300, "");
        let frame = pane(&mut app, 120, 6);
        let mut rows = frame.lines();
        assert!(
            rows.next().unwrap().trim_end().chars().all(|c| c == '·'),
            "{frame}"
        );
        assert_eq!(
            rows.next().unwrap().trim(),
            "Shall we bring a picnic?",
            "{frame}"
        );
        for repeated in ["mira@example.invalid", "Sunday", &date] {
            assert!(
                !frame.contains(repeated),
                "{repeated} repeats the feed row: {frame}"
            );
        }
        app.open_reader();
        let (title, body) = reader(&app).unwrap();
        assert_eq!(title, "Mira · Sunday");
        assert!(body.starts_with(&format!("From: Mira <mira@example.invalid>\nDate: {date}")));
    }

    #[test]
    fn page_keys_scroll_a_long_preview_and_a_new_selection_starts_at_the_top() {
        let mut app = inbox();
        let body: Vec<_> = (1..=30).map(|n| format!("paragraph {n}")).collect();
        find(&mut app, 5).body = body.join("\n");
        app.select(MessageId(5));
        let frame = pane(&mut app, 40, 7);
        assert!(
            frame.contains("paragraph 1\n") || frame.contains("paragraph 1 "),
            "{frame}"
        );
        assert!(frame.contains("PgDn more"), "{frame}");
        let page_down = KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE);
        key(&mut app, page_down, 4, &mut None, &mut None, None);
        let frame = pane(&mut app, 40, 7);
        assert!(!frame.contains("paragraph 1 "), "{frame}");
        assert!(frame.contains("PgUp earlier"), "{frame}");
        assert_eq!(
            app.selected,
            Some(MessageId(5)),
            "paging never moves the selection"
        );
        for _ in 0..20 {
            key(&mut app, page_down, 4, &mut None, &mut None, None);
        }
        let frame = pane(&mut app, 40, 7);
        assert!(frame.contains("paragraph 30"), "{frame}");
        assert!(!frame.contains("PgDn more"), "{frame}");
        let page_up = KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE);
        key(&mut app, page_up, 4, &mut None, &mut None, None);
        assert!(!pane(&mut app, 40, 7).contains("paragraph 30"));
        // Another message, then back: each selection opens at its default view.
        app.select(MessageId(4));
        app.select(MessageId(5));
        assert!(pane(&mut app, 40, 7).contains("paragraph 1 "));
    }

    #[test]
    fn delete_dismisses_and_marks_read_only_before_a_reply_is_started() {
        let mut app = inbox();
        app.settings.mark_read_on_select = false;
        app.switch_reply(MessageId(3));
        let delete = KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE);
        key(&mut app, delete, 4, &mut None, &mut None, None);
        for id in 1..=3 {
            assert!(app.dismissed(MessageId(id)));
            assert!(
                !app.session
                    .messages
                    .iter()
                    .any(|m| m.id == MessageId(id) && m.unread)
            );
        }
        assert_eq!(app.selected, Some(MessageId(4)));
        // Once typing starts Delete edits the reply and dismisses nothing.
        app.editor.insert("ok");
        app.update_body();
        app.editor
            .key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        key(&mut app, delete, 4, &mut None, &mut None, None);
        assert_eq!(app.editor.text, "k");
        assert!(!app.dismissed(MessageId(4)));
        assert!(
            app.session
                .messages
                .iter()
                .any(|m| m.id == MessageId(4) && m.unread)
        );
    }

    #[test]
    fn long_stacks_keep_the_selected_message_visible_and_point_to_the_reader() {
        let mut app = inbox();
        app.select(MessageId(1));
        let frame = pane(&mut app, 30, 3);
        assert!(frame.contains("hey are you around"), "{frame}");
        assert!(frame.contains("o reads all"), "{frame}");
        app.open_reader();
        let (_, body) = reader(&app).unwrap();
        for text in [
            "hey are you around",
            "need a lift\nafter practice",
            "you there?",
        ] {
            assert!(body.contains(text), "{body}");
        }
    }

    #[test]
    fn ctrl_e_dismisses_the_whole_stack_while_typing_and_continues_below() {
        let mut app = inbox();
        app.switch_reply(MessageId(3));
        assert_eq!(app.mode, Mode::Insert);
        let ctrl_e = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
        key(&mut app, ctrl_e, 4, &mut None, &mut None, None);
        for id in 1..=3 {
            assert!(app.dismissed(MessageId(id)));
        }
        assert!(app.editor.text.is_empty(), "Ctrl+E never types");
        // Newest first, the next message below the stack is Jules on Teams; the empty
        // composer follows that explicit move.
        assert_eq!(app.selected, Some(MessageId(4)));
        assert_eq!(
            app.session.composer.reply_target().map(|t| t.incoming),
            Some(MessageId(4))
        );
        assert!(app.notice.contains("3 messages from Son"), "{}", app.notice);

        // Mail is dismissed one message at a time.
        app.select(MessageId(5));
        app.toggle_dismissed();
        assert!(app.dismissed(MessageId(5)));
        assert!(!app.dismissed(MessageId(4)));

        // Shown again with D, the stack restores as one.
        app.toggle_show_dismissed();
        app.select(MessageId(2));
        app.toggle_dismissed();
        assert!((1..=3).all(|id| !app.dismissed(MessageId(id))));
    }

    #[test]
    fn dismissing_never_discards_or_redirects_a_reply_in_progress() {
        let mut app = inbox();
        app.switch_reply(MessageId(2));
        app.editor.insert("on my way");
        app.update_body();
        app.toggle_dismissed();
        assert!((1..=3).all(|id| !app.dismissed(MessageId(id))));
        assert!(app.notice.contains("Reply in progress"));
        let draft = app.session.composer.draft.as_ref().unwrap();
        assert_eq!(draft.target.incoming, MessageId(2));
        // Another message can still be dismissed; the locked draft keeps its recipient.
        app.mode = Mode::Navigate;
        app.select(MessageId(4));
        app.toggle_dismissed();
        assert!(app.dismissed(MessageId(4)));
        let draft = app.session.composer.draft.as_ref().unwrap();
        assert_eq!(draft.target.incoming, MessageId(2));
        assert_eq!(draft.body, "on my way");
    }

    #[test]
    fn viewing_a_stack_marks_all_of_it_read_and_nothing_else() {
        let mut app = inbox();
        app.select(MessageId(3));
        let unread = |app: &App, id| {
            app.session
                .messages
                .iter()
                .any(|m| m.id == MessageId(id) && m.unread)
        };
        assert!((1..=3).all(|id| !unread(&app, id)));
        assert!(unread(&app, 4) && unread(&app, 5));
        app.settings.mark_read_on_select = false;
        app.select(MessageId(4));
        assert!(unread(&app, 4), "the setting still turns marking off");
    }

    #[test]
    fn an_accepted_reply_dismisses_its_stack_but_not_newer_arrivals() {
        let mut app = inbox();
        // Replying to the middle message clears it and the older one only.
        app.switch_reply(MessageId(2));
        app.editor.insert("yes");
        app.update_body();
        let operation = app.submit().unwrap().operation;
        app.accepted(operation);
        assert!(app.dismissed(MessageId(1)) && app.dismissed(MessageId(2)));
        assert!(!app.dismissed(MessageId(3)));

        // A message from the same person arriving while the reply is in flight stays.
        app.switch_reply(MessageId(3));
        app.editor.insert("on my way");
        app.update_body();
        let operation = app.submit().unwrap().operation;
        let mut late = message(6, Provider::Discord, "Son", "dm", "actually wait");
        late.timestamp = 1_792_292_580 + 600;
        app.session.messages.push(late);
        app.accepted(operation);
        assert!(app.dismissed(MessageId(3)));
        assert!(!app.dismissed(MessageId(6)));
        assert!(
            app.session
                .messages
                .iter()
                .any(|m| m.id == MessageId(6) && m.unread)
        );
        assert!(
            !app.dismissed(MessageId(4)),
            "other conversations are untouched"
        );
    }
}
