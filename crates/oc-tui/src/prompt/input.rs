//! Multi-line text buffer with cursor, selection and undo/redo used by the
//! prompt textarea. Character-indexed (Unicode scalar values), matching the
//! reference textarea's offset model where newlines count as one position.

const MAX_UNDO: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    chars: Vec<char>,
    cursor: usize,
    anchor: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct TextBuffer {
    chars: Vec<char>,
    cursor: usize,
    anchor: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_str(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        TextBuffer {
            cursor: chars.len(),
            ..TextBuffer {
                chars,
                ..Default::default()
            }
        }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn chars(&self) -> &[char] {
        &self.chars
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.chars.len());
        self.anchor = None;
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Selected char range (min, max) if any.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.map(|a| {
            let (lo, hi) = (a.min(self.cursor), a.max(self.cursor));
            (lo, hi)
        })
    }

    pub fn selection_text(&self) -> Option<String> {
        self.selection()
            .map(|(lo, hi)| self.chars[lo..hi].iter().collect())
    }

    pub fn has_selection(&self) -> bool {
        self.selection().is_some_and(|(lo, hi)| lo < hi)
    }

    pub fn set_text(&mut self, text: &str) {
        self.push_undo();
        self.chars = text.chars().collect();
        self.cursor = self.chars.len();
        self.anchor = None;
    }

    pub fn clear(&mut self) {
        if self.chars.is_empty() {
            return;
        }
        self.push_undo();
        self.chars.clear();
        self.cursor = 0;
        self.anchor = None;
    }

    pub fn insert(&mut self, c: char) {
        self.replace_range(self.cursor, self.cursor, &[c]);
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let chars: Vec<char> = text.chars().collect();
        self.replace_range(self.cursor, self.cursor, &chars);
    }

    pub fn backspace(&mut self) {
        let (lo, hi) = self.selected_or(self.cursor.saturating_sub(1), self.cursor);
        self.replace_range(lo, hi, &[]);
    }

    pub fn delete(&mut self) {
        let (lo, hi) = self.selected_or(self.cursor, (self.cursor + 1).min(self.chars.len()));
        self.replace_range(lo, hi, &[]);
    }

    pub fn delete_range(&mut self, start: usize, end: usize) {
        let start = start.min(self.chars.len());
        let end = end.min(self.chars.len());
        if start < end {
            self.replace_range(start, end, &[]);
        }
    }

    pub fn move_left(&mut self) {
        self.anchor = None;
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_right(&mut self) {
        self.anchor = None;
        if self.cursor < self.chars.len() {
            self.cursor += 1;
        }
    }

    pub fn select_left(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        self.anchor = Some(anchor);
    }

    pub fn select_right(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        if self.cursor < self.chars.len() {
            self.cursor += 1;
        }
        self.anchor = Some(anchor);
    }

    fn rowcol(&self, pos: usize) -> (usize, usize) {
        let mut row = 0;
        let mut col = 0;
        for &c in &self.chars[..pos] {
            if c == '\n' {
                row += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (row, col)
    }

    fn rowcol_to_pos(&self, row: usize, mut col: usize) -> usize {
        let mut current_row = 0usize;
        let mut pos = 0usize;
        for (idx, &c) in self.chars.iter().enumerate() {
            if current_row == row {
                if col == 0 {
                    return idx;
                }
                if c == '\n' {
                    return idx;
                }
                col -= 1;
                if col == 0 {
                    return idx + 1;
                }
            } else if c == '\n' {
                current_row += 1;
            }
            pos = idx + 1;
        }
        pos
    }

    pub fn move_up(&mut self) {
        self.anchor = None;
        let (row, col) = self.rowcol(self.cursor);
        if row == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = self.rowcol_to_pos(row - 1, col);
    }

    pub fn move_down(&mut self) {
        self.anchor = None;
        let (row, col) = self.rowcol(self.cursor);
        let line_count = self.chars.iter().filter(|&&c| c == '\n').count() + 1;
        if row + 1 >= line_count {
            self.cursor = self.chars.len();
            return;
        }
        self.cursor = self.rowcol_to_pos(row + 1, col);
    }

    pub fn select_up(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.move_up();
        self.anchor = Some(anchor);
    }

    pub fn select_down(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.move_down();
        self.anchor = Some(anchor);
    }

    /// Start of the current line (after a leading newline).
    pub fn line_start(&self) -> usize {
        self.chars[..self.cursor]
            .iter()
            .rposition(|&c| c == '\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    /// End of the current line (before the trailing newline).
    pub fn line_end(&self) -> usize {
        self.chars[self.cursor..]
            .iter()
            .position(|&c| c == '\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.chars.len())
    }

    pub fn buffer_home(&mut self) {
        self.anchor = None;
        self.cursor = 0;
    }

    pub fn buffer_end(&mut self) {
        self.anchor = None;
        self.cursor = self.chars.len();
    }

    pub fn line_home(&mut self) {
        self.anchor = None;
        self.cursor = self.line_start();
    }

    pub fn line_end_cursor(&mut self) {
        self.anchor = None;
        self.cursor = self.line_end();
    }

    pub fn select_buffer_home(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.cursor = 0;
        self.anchor = Some(anchor);
    }

    pub fn select_buffer_end(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.cursor = self.chars.len();
        self.anchor = Some(anchor);
    }

    pub fn select_line_home(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.cursor = self.line_start();
        self.anchor = Some(anchor);
    }

    pub fn select_line_end(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.cursor = self.line_end();
        self.anchor = Some(anchor);
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.chars.len();
    }

    /// Move to the previous word start.
    pub fn word_backward(&mut self) {
        self.anchor = None;
        let mut pos = self.cursor;
        while pos > 0 && self.chars[pos - 1].is_whitespace() {
            pos -= 1;
        }
        while pos > 0 && !self.chars[pos - 1].is_whitespace() {
            pos -= 1;
        }
        self.cursor = pos;
    }

    /// Move to the next word end.
    pub fn word_forward(&mut self) {
        self.anchor = None;
        let mut pos = self.cursor;
        while pos < self.chars.len() && self.chars[pos].is_whitespace() {
            pos += 1;
        }
        while pos < self.chars.len() && !self.chars[pos].is_whitespace() {
            pos += 1;
        }
        self.cursor = pos;
    }

    pub fn select_word_backward(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.word_backward();
        self.anchor = Some(anchor);
    }

    pub fn select_word_forward(&mut self) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.word_forward();
        self.anchor = Some(anchor);
    }

    pub fn delete_word_backward(&mut self) {
        let end = self.cursor;
        let mut pos = self.cursor;
        while pos > 0 && self.chars[pos - 1].is_whitespace() {
            pos -= 1;
        }
        while pos > 0 && !self.chars[pos - 1].is_whitespace() {
            pos -= 1;
        }
        let (lo, hi) = self.selected_or(pos, end);
        self.replace_range(lo, hi, &[]);
    }

    pub fn delete_word_forward(&mut self) {
        let start = self.cursor;
        let mut pos = self.cursor;
        while pos < self.chars.len() && self.chars[pos].is_whitespace() {
            pos += 1;
        }
        while pos < self.chars.len() && !self.chars[pos].is_whitespace() {
            pos += 1;
        }
        let (lo, hi) = self.selected_or(start, pos);
        self.replace_range(lo, hi, &[]);
    }

    pub fn delete_line(&mut self) {
        let (lo, hi) = self.selected_or(self.line_start(), self.line_end());
        let mut lo = lo;
        // Delete the trailing newline too so the line collapses.
        let hi2 = if hi < self.chars.len() && self.chars[hi] == '\n' {
            hi + 1
        } else {
            hi
        };
        if hi2 == self.chars.len() && lo > 0 && self.chars[lo - 1] == '\n' {
            lo -= 1;
        }
        self.replace_range(lo, hi2, &[]);
    }

    pub fn delete_to_line_end(&mut self) {
        let (lo, hi) = self.selected_or(self.cursor, self.line_end());
        self.replace_range(lo, hi, &[]);
    }

    pub fn delete_to_line_start(&mut self) {
        let (lo, hi) = self.selected_or(self.line_start(), self.cursor);
        self.replace_range(lo, hi, &[]);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(Snapshot {
                chars: self.chars.clone(),
                cursor: self.cursor,
                anchor: self.anchor,
            });
            self.chars = prev.chars;
            self.cursor = prev.cursor;
            self.anchor = prev.anchor;
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(Snapshot {
                chars: self.chars.clone(),
                cursor: self.cursor,
                anchor: self.anchor,
            });
            self.chars = next.chars;
            self.cursor = next.cursor;
            self.anchor = next.anchor;
        }
    }

    fn selected_or(&self, fallback_lo: usize, fallback_hi: usize) -> (usize, usize) {
        self.selection().unwrap_or((
            fallback_lo.min(self.chars.len()),
            fallback_hi.min(self.chars.len()),
        ))
    }

    fn push_undo(&mut self) {
        self.undo.push(Snapshot {
            chars: self.chars.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn replace_range(&mut self, start: usize, end: usize, replacement: &[char]) {
        let start = start.min(self.chars.len());
        let end = end.min(self.chars.len());
        if start > end {
            return;
        }
        if start == end && replacement.is_empty() {
            return;
        }
        self.push_undo();
        self.chars.splice(start..end, replacement.iter().copied());
        self.cursor = start + replacement.len();
        self.anchor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_text() {
        let mut b = TextBuffer::new();
        b.insert_str("hello");
        assert_eq!(b.text(), "hello");
        assert_eq!(b.cursor(), 5);
    }

    #[test]
    fn insert_at_cursor() {
        let mut b = TextBuffer::from_str("abcd");
        b.set_cursor(2);
        b.insert_str("XY");
        assert_eq!(b.text(), "abXYcd");
        assert_eq!(b.cursor(), 4);
    }

    #[test]
    fn backspace_and_delete() {
        let mut b = TextBuffer::from_str("abc");
        b.set_cursor(3);
        b.backspace();
        assert_eq!(b.text(), "ab");

        let mut b = TextBuffer::from_str("abc");
        b.set_cursor(0);
        b.delete();
        assert_eq!(b.text(), "bc");
    }

    #[test]
    fn selection_deletes() {
        let mut b = TextBuffer::from_str("abcdef");
        b.set_cursor(1);
        b.select_right();
        b.select_right();
        b.delete();
        assert_eq!(b.text(), "adef");
    }

    #[test]
    fn undo_redo() {
        let mut b = TextBuffer::new();
        b.insert_str("foo");
        b.insert_str("bar");
        assert_eq!(b.text(), "foobar");
        b.undo();
        assert_eq!(b.text(), "foo");
        b.undo();
        assert_eq!(b.text(), "");
        b.redo();
        assert_eq!(b.text(), "foo");
        b.redo();
        assert_eq!(b.text(), "foobar");
    }

    #[test]
    fn movement_across_lines() {
        let mut b = TextBuffer::from_str("ab\ncd\nef");
        b.set_cursor(5); // after 'd' on line 1 (col 2)
        b.move_up();
        assert_eq!(b.cursor(), 2); // col 2 on line 0 -> after 'b'
        b.move_down();
        assert_eq!(b.cursor(), 5); // back on line 1, col 2
        b.move_down();
        assert_eq!(b.cursor(), 8); // line 2 "ef" is only col 2, clamped to end
        b.move_up();
        assert_eq!(b.cursor(), 5);
    }

    #[test]
    fn line_boundaries() {
        let mut b = TextBuffer::from_str("ab\ncd\n");
        b.set_cursor(5);
        assert_eq!(b.line_start(), 3);
        assert_eq!(b.line_end(), 5);
        b.line_home();
        assert_eq!(b.cursor(), 3);
        b.line_end_cursor();
        assert_eq!(b.cursor(), 5);
        b.buffer_home();
        assert_eq!(b.cursor(), 0);
        b.buffer_end();
        assert_eq!(b.cursor(), 6);
    }

    #[test]
    fn word_movement() {
        let mut b = TextBuffer::from_str("foo bar baz");
        b.set_cursor(b.len());
        b.word_backward();
        assert_eq!(&b.text()[..b.cursor()], "foo bar ");
        b.word_backward();
        assert_eq!(&b.text()[..b.cursor()], "foo ");
        b.word_forward();
        assert_eq!(&b.text()[..b.cursor()], "foo bar");
        b.word_forward();
        assert_eq!(b.cursor(), b.len());
    }

    #[test]
    fn delete_words() {
        let mut b = TextBuffer::from_str("foo bar baz");
        b.set_cursor(b.len());
        b.delete_word_backward();
        assert_eq!(b.text(), "foo bar ");
        b.set_cursor(0);
        b.delete_word_forward();
        assert_eq!(b.text(), " bar ");
    }

    #[test]
    fn delete_to_line_edges() {
        let mut b = TextBuffer::from_str("ab\ncd");
        b.set_cursor(4);
        b.delete_to_line_start();
        assert_eq!(b.text(), "ab\nd");
        let mut b = TextBuffer::from_str("ab\ncd");
        b.set_cursor(3);
        b.delete_to_line_end();
        assert_eq!(b.text(), "ab\n");
    }

    #[test]
    fn delete_line_collapses() {
        let mut b = TextBuffer::from_str("a\nb\nc");
        b.set_cursor(2);
        b.delete_line();
        assert_eq!(b.text(), "a\nc");
    }

    #[test]
    fn select_all() {
        let mut b = TextBuffer::from_str("hello");
        b.select_all();
        assert_eq!(b.selection(), Some((0, 5)));
        b.delete();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn non_ascii_handling() {
        let mut b = TextBuffer::from_str("héllo");
        b.set_cursor(2);
        b.insert_str("y");
        assert_eq!(b.text(), "héyllo");
        assert_eq!(b.len(), 6);
    }

    #[test]
    fn clear_when_empty_does_not_undo() {
        let mut b = TextBuffer::new();
        b.clear();
        assert!(!b.can_undo());
    }
}
