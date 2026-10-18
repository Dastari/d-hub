//! Settings layout: breadcrumb, optional page list, wrapped rows, notice and footer.
//! Values and credentials are typed into their own row; there is no separate edit screen.
use super::*;
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

/// Left-hand page list for wide windows; also clickable.
pub(super) fn draw_nav(frame: &mut Frame, area: Rect, p: Palette, app: &App, hit: &mut HitAreas) {
    let current = app.settings_page.section();
    let mut y = area.y;
    let mut entry =
        |label: String, page: SettingsPage, active: bool, color: Color, indent: bool| {
            if y >= area.bottom() {
                return;
            }
            let rect = Rect::new(area.x, y, area.width, 1);
            let mut style = Style::default().fg(color);
            if active {
                style = style.bg(p.selected).add_modifier(Modifier::BOLD);
            }
            let prefix = if indent { "   " } else { " " };
            let budget = area.width.saturating_sub(prefix.len() as u16 + 1) as usize;
            frame.render_widget(
                Paragraph::new(Line::styled(
                    format!(
                        "{prefix}{}",
                        preview(&label, budget + 2).trim_end_matches(" ↵")
                    ),
                    style,
                ))
                .style(if active {
                    Style::default().bg(p.selected)
                } else {
                    Style::default()
                }),
                rect,
            );
            hit.settings_nav.push((rect, page));
            y += 1;
        };
    entry(
        "Overview".into(),
        SettingsPage::Home,
        app.settings_page == SettingsPage::Home,
        p.fg,
        false,
    );
    for section in SettingsSection::ALL {
        entry(
            section.label().into(),
            SettingsPage::Section(section),
            current == Some(section) && app.settings_page == SettingsPage::Section(section),
            if current == Some(section) {
                p.accent
            } else {
                p.fg
            },
            false,
        );
        if section == SettingsSection::Accounts && current == Some(section) {
            for a in &app.settings.accounts {
                let page = SettingsPage::Account(a.id.clone());
                let active = match &app.settings_page {
                    SettingsPage::Account(id) | SettingsPage::RemoveAccount(id) => *id == a.id,
                    _ => false,
                };
                let (dot, _) = health_style(health(app, a).0, p);
                entry(format!("{dot} {}", a.label()), page, active, p.muted, true);
            }
        }
        if section == SettingsSection::Profiles && current == Some(section) {
            for profile in &app.settings.profiles {
                let page = SettingsPage::Profile(profile.id.clone());
                let active = app.settings_page == page;
                entry(profile.name.clone(), page, active, p.muted, true);
            }
        }
    }
}

pub(in crate::tui) fn draw(frame: &mut Frame, area: Rect, p: Palette, app: &mut App) -> HitAreas {
    normalize(app);
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(Style::default().bg(p.surface)), area);
    let inner = area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    let rows = items(app);
    let first_setup = app.settings.accounts.is_empty() && (app.live_service || app.live_setup);
    let mut hit = HitAreas::default();

    // Breadcrumb title, trimmed from the left so the current page stays visible.
    let mut crumbs = breadcrumb(app);
    if first_setup && app.settings_page == SettingsPage::Section(SettingsSection::Accounts) {
        crumbs = vec!["Set up accounts".into()];
    }
    let counter = format!(
        "  {}/{}",
        (app.setting_index + 1).min(rows.len()),
        rows.len()
    );
    let fits = |crumbs: &[String]| {
        crumbs.join(" › ").width() + counter.width() + 2 <= inner.width as usize
    };
    if !fits(&crumbs) {
        let current = crumbs.pop().unwrap_or_default();
        crumbs = vec!["…".into(), current];
        if !fits(&crumbs) {
            crumbs.remove(0);
        }
    }
    let mut title = Vec::new();
    let last = crumbs.len() - 1;
    for (index, crumb) in crumbs.into_iter().enumerate() {
        if index > 0 {
            title.push(Span::styled(" › ", Style::default().fg(p.muted)));
        }
        title.push(if index == last {
            Span::styled(
                crumb,
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(crumb, Style::default().fg(p.muted))
        });
    }
    title.push(Span::styled(counter, Style::default().fg(p.second)));
    frame.render_widget(
        Paragraph::new(Line::from(title)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let editing = inline(app);
    // While a row is editing, its own hint replaces the navigation keys.
    let footer: Vec<Line<'static>> = if editing.is_some() {
        vec![]
    } else if inner.width >= 100 {
        vec![Line::styled(
            "↑↓ select · Enter open/edit · ←→ change · Esc back · Tab section · PgUp/PgDn · : commands · q close",
            Style::default().fg(p.muted),
        )]
    } else {
        [
            "↑↓ select · Enter open/edit",
            "←→ change · Esc back · q close",
            "Tab section · PgUp/PgDn · :",
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

    let body_top = inner.y + 2;
    let mut content = Rect::new(
        inner.x,
        body_top,
        inner.width,
        bottom.saturating_sub(body_top),
    );
    if inner.width >= 96 && content.height > 0 {
        let nav = Rect::new(content.x, content.y, 24, content.height);
        draw_nav(frame, nav, p, app, &mut hit);
        content.x += 27;
        content.width -= 27;
    }

    let mut top = content.y;
    for (value, color) in header(app, p) {
        for line in wrapped(&value, content.width as usize) {
            if top >= content.bottom() {
                break;
            }
            frame.render_widget(
                Paragraph::new(Line::styled(line, Style::default().fg(color))),
                Rect::new(content.x, top, content.width, 1),
            );
            top += 1;
        }
    }
    if top > content.y {
        top += 1;
    }
    let viewport = Rect::new(
        content.x,
        top.min(content.bottom()),
        content.width,
        content.bottom().saturating_sub(top),
    );
    if viewport.height == 0 || rows.is_empty() {
        return hit;
    }

    let entries: BTreeMap<_, _> = app
        .settings
        .entries()
        .into_iter()
        .map(|r| (r.name.clone(), r))
        .collect();
    let width = viewport.width as usize;
    let wide = width >= 56;
    let name_width = if wide {
        (width / 3).clamp(18, 30)
    } else {
        width.saturating_sub(4)
    };
    let value_width = if wide {
        width - name_width - 6
    } else {
        width.saturating_sub(4)
    };
    let mut lines = Vec::new();
    let mut ranges = Vec::new();
    // Where the in-place editor goes: first line, column, width and height within `lines`.
    let mut slot: Option<(usize, u16, u16, u16)> = None;
    for (index, item) in rows.iter().enumerate() {
        let row = describe(app, item, &entries, p);
        let selected = index == app.setting_index;
        let editor = editing.filter(|_| selected);
        let start = lines.len();
        let names = wrapped(&safe_text(&row.label), name_width);
        let values = if row.value.is_empty() || editor.is_some() {
            vec![]
        } else {
            wrapped(&preview(&row.value, value_width * 2), value_width)
        };
        let name_style = Style::default()
            .fg(row.label_color)
            .add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        let pointer = |line: usize| {
            Span::styled(
                if line == 0 && selected { "› " } else { "  " },
                Style::default().fg(p.accent),
            )
        };
        let marker = |line: usize| {
            if line == 0 && !row.marker.0.is_empty() {
                Span::styled(
                    format!("{} ", row.marker.0),
                    Style::default().fg(row.marker.1),
                )
            } else if !row.marker.0.is_empty() {
                Span::raw("  ")
            } else {
                Span::raw("")
            }
        };
        if wide {
            for line in 0..names.len().max(values.len()) {
                let name = names.get(line).map_or("", String::as_str);
                let pad = name_width
                    .saturating_sub(name.width() + if row.marker.0.is_empty() { 0 } else { 2 });
                lines.push(Line::from(vec![
                    pointer(line),
                    marker(line),
                    Span::styled(name.to_owned(), name_style),
                    Span::raw(" ".repeat(pad + 2)),
                    Span::styled(
                        values.get(line).cloned().unwrap_or_default(),
                        Style::default().fg(row.value_color),
                    ),
                ]));
            }
        } else {
            for (line, name) in names.into_iter().enumerate() {
                lines.push(Line::from(vec![
                    pointer(line),
                    marker(line),
                    Span::styled(name, name_style),
                ]));
            }
            for value in values {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(value, Style::default().fg(row.value_color)),
                ]));
            }
        }
        if let Some(kind) = editor {
            let tall = kind == (Inline::Text { multiline: true });
            if wide && !tall {
                // Single values replace the value column on the label's first line.
                slot = Some((start, name_width as u16 + 4, value_width as u16, 1));
            } else {
                let height = if tall {
                    (soft_rows(&app.command.text, width.saturating_sub(4)).len() + 1)
                        .clamp(3, (viewport.height as usize).saturating_sub(4).max(3))
                } else {
                    1
                };
                slot = Some((
                    lines.len(),
                    4,
                    width.saturating_sub(4).max(1) as u16,
                    height as u16,
                ));
                lines.extend((0..height).map(|_| Line::default()));
            }
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(actions::edit_hint(kind), Style::default().fg(p.second)),
            ]));
        }
        let mut note = safe_text(&row.note);
        if selected && let Item::Field(name) = item {
            note = if note.is_empty() {
                format!("config key {name}")
            } else {
                format!("{note} · config key {name}")
            };
        }
        if !note.is_empty() {
            let mut notes = wrapped(&note, width.saturating_sub(4));
            // Unselected rows keep two note lines; the selected row shows everything.
            if !selected && notes.len() > 2 {
                notes.truncate(2);
                notes[1] = preview(&notes[1], width.saturating_sub(4));
                if !notes[1].ends_with('…') {
                    notes[1].push('…');
                }
            }
            for line in notes {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(line, Style::default().fg(p.muted)),
                ]));
            }
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
    if let Some((line, _, _, rows)) = slot {
        let end = line + rows as usize + 1;
        if end > app.settings_scroll + height {
            app.settings_scroll = end.saturating_sub(height);
        }
        app.settings_scroll = app.settings_scroll.min(line);
    }
    // Backfill near the end so a tall window never leaves most rows blank.
    app.settings_scroll = app.settings_scroll.min(lines.len().saturating_sub(height));
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
    if let (Some(kind), Some((line, x, w, rows))) = (editing, slot) {
        let y = viewport.y + line.saturating_sub(app.settings_scroll) as u16;
        let field = Rect::new(viewport.x + x, y, w, rows).intersection(viewport);
        draw_editor(frame, field, p, app, kind);
    }
    hit
}

/// The editor inside its row: an input-coloured box with the terminal cursor.
fn draw_editor(frame: &mut Frame, field: Rect, p: Palette, app: &mut App, kind: Inline) {
    if field.is_empty() {
        return;
    }
    let style = Style::default().fg(p.fg).bg(p.surface);
    let (content, scroll, cursor) = if kind == (Inline::Text { multiline: true }) {
        // Prompts wrap at words like the rest of the page; the cursor follows the wrapped rows.
        let text = &app.command.text;
        let rows = soft_rows(text, field.width as usize);
        let cursor = app.command.cursor;
        let row = rows.iter().rposition(|(s, _)| *s <= cursor).unwrap_or(0);
        let column = text[rows[row].0..cursor].width();
        let height = field.height as usize;
        app.command.top = app
            .command
            .top
            .min(row)
            .max((row + 1).saturating_sub(height));
        let content = rows
            .iter()
            .map(|(s, e)| &text[*s..*e])
            .collect::<Vec<_>>()
            .join("\n");
        let visible = (row - app.command.top) as u16;
        (
            content,
            (app.command.top as u16, 0),
            (visible, column as u16),
        )
    } else if kind == Inline::Secret {
        // Only the length is shown; the cursor column counts mask characters.
        let column = app.command.text[..app.command.cursor].chars().count();
        let left = column.saturating_sub(field.width as usize - 1);
        let mask = "•".repeat(app.command.text.chars().count());
        (mask, (0, left as u16), (0, (column - left) as u16))
    } else {
        let cursor = app
            .command
            .viewport(field.width as usize, field.height as usize);
        (
            app.command.text.clone(),
            (app.command.top as u16, app.command.left as u16),
            cursor,
        )
    };
    frame.render_widget(Paragraph::new(content).style(style).scroll(scroll), field);
    frame.set_cursor_position((field.x + cursor.1, field.y + cursor.0));
}

/// Byte ranges of each visual row when `text` wraps at `width` columns, breaking after spaces
/// where possible. Hard newlines always start a row.
pub(super) fn soft_rows(text: &str, width: usize) -> Vec<(usize, usize)> {
    use unicode_segmentation::UnicodeSegmentation;
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        let (mut start, mut used, mut space) = (0, 0, None);
        for (i, grapheme) in line.grapheme_indices(true) {
            let w = grapheme.width();
            if used + w > width && i > start {
                let cut = space.filter(|s| *s > start).unwrap_or(i);
                rows.push((offset + start, offset + cut));
                used = line[cut..i].width();
                start = cut;
                space = None;
            }
            used += w;
            if grapheme == " " {
                space = Some(i + 1);
            }
        }
        rows.push((offset + start, offset + line.len()));
        offset += line.len() + 1;
    }
    rows
}
