use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug)]
pub struct TextInput {
    pub text: String,
    cursor: usize,
}

#[derive(Clone, Debug)]
pub enum Edit {
    Insert(String),
    Left,
    Right,
    Home,
    End,
    Backspace,
    Delete,
    Clear,
}

impl TextInput {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.into(),
            cursor: text.len(),
        }
    }

    pub fn update(&mut self, edit: Edit) {
        match edit {
            Edit::Insert(text) => {
                let text: String = text
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                self.text.insert_str(self.cursor, &text);
                self.cursor += text.len();
            }
            Edit::Left => self.cursor = self.previous(),
            Edit::Right => self.cursor = self.next(),
            Edit::Home => self.cursor = 0,
            Edit::End => self.cursor = self.text.len(),
            Edit::Backspace => {
                let previous = self.previous();
                self.text.replace_range(previous..self.cursor, "");
                self.cursor = previous;
            }
            Edit::Delete => {
                self.text.replace_range(self.cursor..self.next(), "");
            }
            Edit::Clear => {
                self.text.clear();
                self.cursor = 0;
            }
        }
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    fn next(&self) -> usize {
        self.cursor
            + self.text[self.cursor..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(0)
    }

    // Horizontal scrolling is derived from the cursor, so rendering remains read-only.
    pub fn visible(&self, width: u16) -> (&str, u16) {
        let mut start = 0;
        while self.text[start..self.cursor].width() >= usize::from(width.max(1))
            && start < self.cursor
        {
            start += self.text[start..].chars().next().unwrap().len_utf8();
        }
        (
            &self.text[start..],
            self.text[start..self.cursor].width() as u16,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_unicode_at_character_boundaries_and_keeps_cursor_visible() {
        let mut input = TextInput::new("a界é");
        input.update(Edit::Left);
        input.update(Edit::Backspace);
        input.update(Edit::Insert("λ".into()));
        assert_eq!(input.text, "aλé");
        input.update(Edit::Delete);
        assert_eq!(input.text, "aλ");
        input.update(Edit::End);
        assert_eq!(input.visible(2), ("λ", 1));
        input.update(Edit::Home);
        input.update(Edit::Backspace);
        assert_eq!(input.text, "aλ");
    }
}
