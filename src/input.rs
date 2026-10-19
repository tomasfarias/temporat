use anyhow::{Result, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Refresh,
    Quit,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keybindings {
    pub up: char,
    pub down: char,
    pub left: char,
    pub right: char,
    pub refresh: char,
    pub quit: char,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            up: 'k',
            down: 'j',
            left: 'h',
            right: 'l',
            refresh: 'r',
            quit: 'q',
        }
    }
}

impl Keybindings {
    fn bindings(&self) -> [(char, Action); 6] {
        [
            (self.up, Action::Up),
            (self.down, Action::Down),
            (self.left, Action::Left),
            (self.right, Action::Right),
            (self.refresh, Action::Refresh),
            (self.quit, Action::Quit),
        ]
    }

    pub fn validate(&self) -> Result<()> {
        let bindings = self.bindings();
        for (index, (key, _)) in bindings.iter().enumerate() {
            if key.is_control() || key.is_whitespace() {
                bail!("keybindings must use visible characters");
            }
            if bindings[..index]
                .iter()
                .any(|(previous, _)| previous == key)
            {
                bail!("keybindings must use distinct characters (duplicate {key})");
            }
        }
        Ok(())
    }

    pub fn action(&self, key: KeyEvent) -> Option<Action> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            return Some(Action::Quit);
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return None;
        }
        self.bindings().into_iter().find_map(|(character, action)| {
            (key.code == KeyCode::Char(character)).then_some(action)
        })
    }

    pub fn help(&self) -> String {
        format!(
            "{}/{} rows  {}/{} columns  {} refresh  {} quit",
            self.down, self.up, self.left, self.right, self.refresh, self.quit
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_vim_and_custom_keys_without_stealing_modified_keys() {
        let mut keys = Keybindings::default();
        assert_eq!(
            keys.action(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            Some(Action::Down)
        );
        keys.down = 'J';
        assert_eq!(
            keys.action(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT)),
            Some(Action::Down)
        );
        assert_eq!(
            keys.action(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::ALT)),
            None
        );
        assert_eq!(
            keys.action(KeyEvent::new_with_kind(
                KeyCode::Char('J'),
                KeyModifiers::SHIFT,
                KeyEventKind::Release
            )),
            None
        );
        assert_eq!(
            keys.action(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn rejects_ambiguous_bindings() {
        let keys = Keybindings {
            down: 'k',
            ..Default::default()
        };
        assert!(keys.validate().is_err());
    }
}
