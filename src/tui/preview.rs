//! The pane under the feed and the full reader. A chat burst reads as a short transcript with a
//! time per message; an email shows its sender and full local date above the body.
use super::*;
use crate::{domain::Message, when};
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

/// "Mira <mira@example.com>" when the reply address differs from the display name.
fn from(message: &Message) -> String {
    let sender = safe_text(&message.sender);
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
        let header = format!(
            "{} · {}",
            from(message),
            when::full(message.timestamp, &safe_text(&message.time))
        );
        lines.extend(
            wrapped(&header, width)
                .into_iter()
                .map(|l| Line::styled(l, muted)),
        );
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

pub(super) fn draw(frame: &mut Frame, area: Rect, p: Palette, app: &App) {
    let Some(message) = app.selected_message() else {
        frame.render_widget(
            text("Select a communication to read its context.", p.muted),
            area,
        );
        return;
    };
    let (width, height) = (area.width as usize, area.height as usize);
    let (lines, selected) = layout(app, message, width, p);
    if lines.len() <= height {
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }
    // Too long: keep the selected message in view, preferring the newest end of a chat, and
    // give "more" markers their own lines so they never cover it.
    let total = lines.len();
    let place = |rows: usize| {
        if message.is_mail() {
            0
        } else {
            (total - rows).min(selected.start)
        }
    };
    let mut rows = height.saturating_sub(1).max(1);
    let mut start = place(rows);
    if height >= 3 && start > 0 && start + rows < total {
        rows = height - 2;
        start = place(rows);
    }
    let more = Style::default().fg(p.muted).add_modifier(Modifier::ITALIC);
    let mut window = vec![];
    if start > 0 && height >= 2 {
        window.push(Line::styled("↑ earlier · o reads all", more));
    }
    window.extend(lines[start..start + rows].iter().cloned());
    if start + rows < total && window.len() < height {
        window.push(Line::styled("↓ more · o reads all", more));
    }
    frame.render_widget(Paragraph::new(window), area);
}

/// Title and body for the reader: the full email with its headers, or the chat burst as a
/// transcript with dates and times.
pub(super) fn reader(app: &App) -> Option<(String, String)> {
    let message = app.reader.as_ref()?;
    let title = format!(
        "{} · {}",
        safe_text(&message.sender),
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
            safe_text(&m.sender),
            safe_text(m.body.trim())
        ));
    }
    Some((title, body.trim_end().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::super::feed::tests::{app, message};
    use super::*;

    /// Renders only the preview pane.
    fn pane(app: &App, width: u16, height: u16) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        let p = palette(Theme::Midnight, Duration::ZERO, true);
        terminal.draw(|f| draw(f, f.area(), p, app)).unwrap();
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
        let frame = pane(&app, 50, 8);
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
    fn email_preview_and_reader_show_the_sender_address_and_full_local_date() {
        let mut app = inbox();
        app.select(MessageId(5));
        let date = when::full(1_792_292_580 + 300, "");
        let frame = pane(&app, 120, 6);
        assert!(
            frame.contains(&format!("Mira <mira@example.invalid> · {date}")),
            "{frame}"
        );
        assert!(frame.contains("Shall we bring a picnic?"));
        app.open_reader();
        let (title, body) = reader(&app).unwrap();
        assert_eq!(title, "Mira · Sunday");
        assert!(body.starts_with(&format!("From: Mira <mira@example.invalid>\nDate: {date}")));
    }

    #[test]
    fn long_stacks_keep_the_selected_message_visible_and_point_to_the_reader() {
        let mut app = inbox();
        app.select(MessageId(1));
        let frame = pane(&app, 30, 3);
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
