use crate::domain::safe_text;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Default)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
    pub top: usize,
    pub left: usize,
    /// Columns the text last wrapped at; 0 moves Up/Down by hard lines.
    pub wrap: usize,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        let text = safe_text(text);
        Self {
            cursor: text.len(),
            text,
            ..Self::default()
        }
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(self.text.len(), |g| self.cursor + g.len())
    }

    pub fn insert(&mut self, text: &str) {
        let text = safe_text(text);
        self.text.insert_str(self.cursor, &text);
        let end = self.cursor + text.len();
        // Inserting a combining mark/ZWJ can merge clusters on either side.
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= end)
            .unwrap_or(self.text.len());
    }

    pub fn position(&self) -> (usize, usize) {
        let prefix = &self.text[..self.cursor];
        let row = prefix.bytes().filter(|b| *b == b'\n').count();
        let column = prefix.rsplit('\n').next().unwrap_or("").width();
        (row, column)
    }

    fn vertical(&mut self, direction: isize) {
        if self.wrap > 0 {
            let rows = soft_rows(&self.text, self.wrap);
            let row = row_of(&rows, self.cursor);
            let target = row.saturating_add_signed(direction).min(rows.len() - 1);
            let column = self.text[rows[row].0..self.cursor].width();
            let (start, end) = rows[target];
            // A row ending where the next one starts (after a wrapped space) keeps the cursor on
            // this row by stopping before its last grapheme.
            let end = if rows.get(target + 1).is_some_and(|next| next.0 == end) {
                self.text[start..end]
                    .grapheme_indices(true)
                    .next_back()
                    .map_or(start, |(i, _)| start + i)
            } else {
                end
            };
            let mut used = 0;
            self.cursor = end;
            for (i, grapheme) in self.text[start..end].grapheme_indices(true) {
                if used + grapheme.width() > column {
                    self.cursor = start + i;
                    break;
                }
                used += grapheme.width();
            }
            return;
        }
        let current_row = self.position().0;
        let lines: Vec<&str> = self.text.split('\n').collect();
        let target_row = current_row
            .saturating_add_signed(direction)
            .min(lines.len() - 1);
        let column = self.text[..self.cursor]
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .graphemes(true)
            .count();
        let offset: usize = lines
            .iter()
            .take(target_row)
            .map(|line| line.len() + 1)
            .sum();
        self.cursor = offset
            + lines[target_row]
                .graphemes(true)
                .take(column)
                .map(str::len)
                .sum::<usize>();
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        let before = self.text.clone();
        match key.code {
            KeyCode::Char(c)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.insert(&c.to_string())
            }
            KeyCode::Enter => self.insert("\n"),
            KeyCode::Tab => self.insert("    "),
            KeyCode::Backspace if self.cursor > 0 => {
                let start = self.previous();
                self.text.replace_range(start..self.cursor, "");
                self.cursor = start;
                self.snap_cursor();
            }
            KeyCode::Delete if self.cursor < self.text.len() => {
                let end = self.next();
                self.text.replace_range(self.cursor..end, "");
                self.snap_cursor();
            }
            KeyCode::Left => self.cursor = self.previous(),
            KeyCode::Right => self.cursor = self.next(),
            KeyCode::Up => self.vertical(-1),
            KeyCode::Down => self.vertical(1),
            KeyCode::Home => {
                self.cursor -= self.text[..self.cursor]
                    .rsplit('\n')
                    .next()
                    .unwrap_or("")
                    .len()
            }
            KeyCode::End => {
                self.cursor += self.text[self.cursor..]
                    .split('\n')
                    .next()
                    .unwrap_or("")
                    .len()
            }
            _ => {}
        }
        self.text != before
    }

    fn snap_cursor(&mut self) {
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= self.cursor)
            .unwrap_or(self.text.len());
    }

    pub fn viewport(&mut self, width: usize, height: usize) -> (u16, u16) {
        let (row, col) = self.position();
        self.top = self
            .top
            .min(row)
            .max(row.saturating_sub(height.saturating_sub(1)));
        self.left = self
            .left
            .min(col)
            .max(col.saturating_sub(width.saturating_sub(1)));
        ((row - self.top) as u16, (col - self.left) as u16)
    }

    /// Visual rows when the text wraps in a box `width` columns wide. One column stays free so
    /// the cursor never sits past the edge.
    pub fn wrapped_rows(&self, width: usize) -> Vec<(usize, usize)> {
        soft_rows(&self.text, width.saturating_sub(1).max(1))
    }

    /// Wraps the text for a `width` × `height` box, scrolling so the cursor shows. Returns the
    /// visible rows and the cursor's row and column within them.
    pub fn wrapped(&mut self, width: usize, height: usize) -> (Vec<&str>, (u16, u16)) {
        self.wrap = width.saturating_sub(1).max(1);
        self.left = 0;
        let rows = soft_rows(&self.text, self.wrap);
        let row = row_of(&rows, self.cursor);
        let column = self.text[rows[row].0..self.cursor].width();
        self.top = self
            .top
            .min(row)
            .max((row + 1).saturating_sub(height.max(1)))
            .min(rows.len() - 1);
        let visible = rows
            .iter()
            .skip(self.top)
            .take(height)
            .map(|(s, e)| &self.text[*s..*e])
            .collect();
        (visible, ((row - self.top) as u16, column as u16))
    }
}

/// The visual row holding byte `cursor`; at a wrap point the cursor starts the next row.
fn row_of(rows: &[(usize, usize)], cursor: usize) -> usize {
    rows.iter().rposition(|(s, _)| *s <= cursor).unwrap_or(0)
}

/// Byte ranges of each visual row when `text` wraps at `width` columns, breaking after spaces
/// where possible. A space that would overflow ends its row instead of starting the next one.
/// Hard newlines always start a row.
pub fn soft_rows(text: &str, width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        let (mut start, mut used, mut space) = (0, 0, None);
        for (i, grapheme) in line.grapheme_indices(true) {
            let w = grapheme.width();
            if used + w > width && grapheme == " " {
                rows.push((offset + start, offset + i + 1));
                (start, used, space) = (i + 1, 0, None);
                continue;
            }
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
