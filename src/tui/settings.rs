//! Settings occupy the window; row heights follow wrapped content, not a fixed item count.
use super::*;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

fn wrapped(value: &str, width: usize) -> Vec<String> {
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

fn section(name: &str) -> String {
    if name == "profiles" || name.starts_with("profiles.") {
        "Focus profiles".into()
    } else if name.starts_with("ai.") {
        "AI decisions".into()
    } else if name.starts_with("spam.") {
        "Spam filtering".into()
    } else if let Some(rest) = name.strip_prefix("accounts.") {
        format!("Account · {}", rest.split('.').next().unwrap_or_default())
    } else if name == "accounts" {
        "Accounts".into()
    } else {
        "General".into()
    }
}

fn preview(value: &str, budget: usize) -> String {
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

pub(super) fn draw(frame: &mut Frame, area: Rect, p: Palette, app: &mut App) -> HitAreas {
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(Style::default().bg(p.surface)), area);
    let inner = area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    let rows = app.settings.entries();
    app.setting_index = app.setting_index.min(rows.len().saturating_sub(1));
    let first_setup = app.settings.accounts.is_empty() && (app.live_service || app.live_setup);
    let title = if first_setup {
        "Set up accounts"
    } else {
        "Settings"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}/{}", app.setting_index + 1, rows.len()),
                Style::default().fg(p.second),
            ),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let mut top = inner.y + 2;
    if first_setup {
        let instructions = if inner.height >= 24 {
            vec![
                ("Add an account with : commands", p.muted),
                (":account add discord personal", p.second),
                ("Then :auth personal for QR sign-in.", p.muted),
                ("iCloud, Office365 and Teams are also available.", p.muted),
            ]
        } else {
            vec![(": commands to add accounts", p.muted)]
        };
        for (value, color) in instructions {
            let lines: Vec<_> = wrapped(value, inner.width as usize)
                .into_iter()
                .map(|line| Line::styled(line, Style::default().fg(color)))
                .collect();
            let height = lines.len() as u16;
            frame.render_widget(
                Paragraph::new(lines),
                Rect::new(inner.x, top, inner.width, height),
            );
            top += height;
        }
        top += 1;
    }
    let footer: Vec<Line<'static>> = if inner.width >= 90 {
        vec![Line::styled(
            "↑↓ / j k select · ←→ change · Enter edit · PgUp/PgDn page · Home/End · : commands · Esc close",
            Style::default().fg(p.muted),
        )]
    } else {
        [
            "↑↓ select · Enter edit",
            "←→ change · : commands",
            "PgUp/PgDn · Esc close",
        ]
        .into_iter()
        .map(|line| Line::styled(line, Style::default().fg(p.muted)))
        .collect()
    };
    let footer_height = footer.len() as u16;
    let notice: Vec<_> = wrapped(&safe_text(&app.notice), inner.width as usize)
        .into_iter()
        .take(2)
        .map(|line| Line::styled(line, Style::default().fg(p.second)))
        .collect();
    let notice_height = notice.len() as u16;
    let bottom = inner
        .bottom()
        .saturating_sub(footer_height + notice_height + 2);
    let viewport = Rect::new(
        inner.x,
        top.min(bottom),
        inner.width,
        bottom.saturating_sub(top),
    );
    frame.render_widget(
        Paragraph::new(notice),
        Rect::new(inner.x, bottom + 1, inner.width, notice_height),
    );
    frame.render_widget(
        Paragraph::new(footer),
        Rect::new(
            inner.x,
            inner.bottom().saturating_sub(footer_height),
            inner.width,
            footer_height,
        ),
    );
    if viewport.height == 0 {
        return HitAreas::default();
    }

    let width = viewport.width as usize;
    let wide = width >= 78;
    let name_width = if wide { 34 } else { width.saturating_sub(2) };
    let value_width = if wide {
        width - name_width - 4
    } else {
        width.saturating_sub(4)
    };
    let mut lines = Vec::new();
    let mut ranges = Vec::new();
    let mut previous_section = String::new();
    for (index, row) in rows.iter().enumerate() {
        let heading = section(&row.name);
        if heading != previous_section {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            for line in wrapped(&heading, width) {
                lines.push(Line::styled(
                    line,
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                ));
            }
            previous_section = heading;
        }
        let start = lines.len();
        let names = wrapped(&row.name, name_width);
        let values = wrapped(&preview(&row.value, value_width * 2), value_width);
        let selected = index == app.setting_index;
        let name_style = Style::default().fg(p.fg).add_modifier(if selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });
        if wide {
            for line in 0..names.len().max(values.len()) {
                let name = names.get(line).map_or("", String::as_str);
                lines.push(Line::from(vec![
                    Span::styled(
                        if line == 0 && selected { "› " } else { "  " },
                        Style::default().fg(p.accent),
                    ),
                    Span::styled(name.to_owned(), name_style),
                    Span::raw(" ".repeat(name_width.saturating_sub(name.width()) + 2)),
                    Span::styled(
                        values.get(line).cloned().unwrap_or_default(),
                        Style::default().fg(p.second),
                    ),
                ]));
            }
        } else {
            for (line, name) in names.into_iter().enumerate() {
                lines.push(Line::from(vec![
                    Span::styled(
                        if line == 0 && selected { "› " } else { "  " },
                        Style::default().fg(p.accent),
                    ),
                    Span::styled(name, name_style),
                ]));
            }
            for value in values {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(value, Style::default().fg(p.second)),
                ]));
            }
        }
        for note in wrapped(&safe_text(&row.help), width.saturating_sub(4)) {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(note, Style::default().fg(p.muted)),
            ]));
        }
        if selected {
            for line in &mut lines[start..] {
                line.style = line.style.bg(p.selected);
            }
        }
        ranges.push(start..lines.len());
        lines.push(Line::default());
    }
    let height = viewport.height as usize;
    let selected = &ranges[app.setting_index];
    app.settings_scroll = app.settings_scroll.min(lines.len().saturating_sub(height));
    if selected.start < app.settings_scroll || selected.len() > height {
        app.settings_scroll = selected.start;
    } else if selected.end > app.settings_scroll + height {
        app.settings_scroll = selected.end - height;
    }
    // Backfill near the end so a tall window never leaves most rows blank.
    app.settings_scroll = app.settings_scroll.min(lines.len().saturating_sub(height));
    let mut hit = HitAreas::default();
    for (index, range) in ranges.iter().enumerate() {
        let start = range.start.max(app.settings_scroll);
        let end = range.end.min(app.settings_scroll + height);
        if end > start {
            hit.settings.push((
                Rect::new(
                    viewport.x,
                    viewport.y + (start - app.settings_scroll) as u16,
                    viewport.width,
                    (end - start) as u16,
                ),
                index,
            ));
        }
    }
    app.settings_page_size = hit.settings.len().saturating_sub(1).max(1);
    if let Some((area, _)) = hit
        .settings
        .iter()
        .find(|(_, index)| *index == app.setting_index)
    {
        frame.render_widget(
            Block::default().style(Style::default().bg(p.selected)),
            *area,
        );
    }
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(app.settings_scroll)
                .take(height)
                .collect::<Vec<_>>(),
        ),
        viewport,
    );
    hit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Session;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn settings_use_tall_windows_and_keep_the_selected_field_visible_when_resized() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.mode = Mode::Settings;
        app.editor
            .insert("Draft stays with its original recipient 東京");
        app.update_body();
        let before = serde_json::to_vec(&app.session).unwrap();
        let small = super::super::snapshot(&mut app, 40, 20).unwrap();
        let large = super::super::snapshot(&mut app, 40, 65).unwrap();
        assert!(large.contains("padding_vertical"));
        assert!(!small.contains("padding_vertical"));
        assert!(app.settings_page_size > 8);
        for index in [0, 10, 22, app.settings.entries().len() - 1] {
            app.setting_index = index;
            for (width, height) in [(28, 20), (40, 65), (120, 45), (28, 16)] {
                let output = super::super::snapshot(&mut app, width, height).unwrap();
                assert!(
                    output.contains('›'),
                    "selected field hidden at {width}x{height}"
                );
                assert_eq!(output.lines().count(), height as usize);
            }
        }
        assert_eq!(serde_json::to_vec(&app.session).unwrap(), before);
    }

    #[test]
    fn settings_sections_labels_values_and_notes_have_distinct_styles_in_both_themes() {
        for theme in [Theme::Midnight, Theme::Dusk] {
            let mut app = App::new(
                Session::default(),
                Settings {
                    theme,
                    reduced_motion: true,
                    ..Settings::default()
                },
            )
            .unwrap();
            app.mode = Mode::Settings;
            let mut terminal = Terminal::new(TestBackend::new(110, 42)).unwrap();
            terminal
                .draw(|frame| {
                    draw(
                        frame,
                        frame.area(),
                        palette(theme, Duration::ZERO, true),
                        &mut app,
                    );
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let color_at = |needle: &str| {
                for y in 0..42 {
                    let line: String = (0..110).map(|x| buffer[(x, y)].symbol()).collect();
                    if let Some(column) = line.find(needle) {
                        return buffer[(column as u16, y)].fg;
                    }
                }
                panic!("missing {needle}");
            };
            let colors = [
                color_at("General"),
                color_at("auto_update"),
                color_at("true"),
                color_at("Install stable"),
            ];
            for y in 0..42 {
                let line: String = (0..110).map(|x| buffer[(x, y)].symbol()).collect();
                if line.contains("auto_update") {
                    assert_eq!(
                        buffer[(108, y)].bg,
                        palette(theme, Duration::ZERO, true).selected
                    );
                }
            }
            for i in 0..colors.len() {
                for j in i + 1..colors.len() {
                    assert_ne!(colors[i], colors[j]);
                }
            }
            assert!(
                buffer
                    .content
                    .iter()
                    .any(|cell| cell.bg == palette(theme, Duration::ZERO, true).selected)
            );
        }
    }

    #[test]
    fn wrapping_preserves_unicode_clusters_and_limits_long_value_previews() {
        let value = "東京 👩🏽‍💻 e\u{301} a_very_long_unbroken_setting_name";
        for width in [4, 9, 24, 80] {
            let lines = wrapped(value, width);
            assert!(lines.iter().all(|line| line.width() <= width));
            assert_eq!(
                lines.join(" ").split_whitespace().collect::<String>(),
                value.split_whitespace().collect::<String>()
            );
        }
        let summary = preview(&"東京👩🏽‍💻".repeat(1000), 24);
        assert!(summary.width() <= 24 && summary.ends_with('…'));
        assert_eq!(preview("", 24), "Not set");
        assert_eq!(preview("hello\nworld", 24), "hello ↵");
    }
}
