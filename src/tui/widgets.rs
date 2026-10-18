//! Small rendering helpers shared by the drawer, settings and modals.
use super::*;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn padded(rect: Rect) -> Rect {
    rect.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 0,
    })
}

pub(super) fn text<'a>(value: impl Into<Text<'a>>, color: Color) -> Paragraph<'a> {
    Paragraph::new(value).style(Style::default().fg(color))
}

pub(super) fn overlay(
    frame: &mut Frame,
    area: Rect,
    p: Palette,
    title: &str,
    body: &str,
    scroll: u16,
) {
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

/// Word-wraps by display cells, breaking only overlong words at grapheme boundaries.
pub(super) fn wrapped(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for word in value.split_whitespace() {
        let word_width = word.width();
        if !line.is_empty() && used + 1 + word_width > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        if !line.is_empty() {
            line.push(' ');
            used += 1;
        }
        for grapheme in word.graphemes(true) {
            let cells = grapheme.width();
            if used + cells > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push_str(grapheme);
            used += cells;
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// First line of `value` within `budget` cells, with … when cut and ↵ when more lines follow.
pub(super) fn preview(value: &str, budget: usize) -> String {
    let safe = safe_text(value);
    if safe.is_empty() {
        return "Not set".into();
    }
    let first = safe.lines().next().unwrap_or_default();
    let mut result = String::new();
    let mut used = 0;
    for grapheme in first.graphemes(true) {
        if used + grapheme.width() > budget.saturating_sub(2) {
            result.push('…');
            return result;
        }
        used += grapheme.width();
        result.push_str(grapheme);
    }
    if safe.lines().count() > 1 {
        result.push_str(" ↵");
    }
    result
}
