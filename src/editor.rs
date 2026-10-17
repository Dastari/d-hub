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
        let column = UnicodeWidthStr::width(prefix.rsplit('\n').next().unwrap_or(""));
        (row, column)
    }

    fn vertical(&mut self, direction: isize) {
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
}
