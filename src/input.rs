use crate::{
    app::Message,
    query::{self, Focus},
    text_input::Edit,
};
use anyhow::{Result, bail};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Refresh,
    Quit,
    Query,
    LoadQuery,
    SaveQuery,
    OpenDetails,
    Back,
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
    pub query: char,
    pub load_query: char,
    pub save_query: char,
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
            query: '/',
            load_query: 'o',
            save_query: 's',
        }
    }
}

impl Keybindings {
    fn bindings(&self) -> [(char, Action); 9] {
        [
            (self.up, Action::Up),
            (self.down, Action::Down),
            (self.left, Action::Left),
            (self.right, Action::Right),
            (self.refresh, Action::Refresh),
            (self.quit, Action::Quit),
            (self.query, Action::Query),
            (self.load_query, Action::LoadQuery),
            (self.save_query, Action::SaveQuery),
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
            "{}/{} rows  {}/{} columns  Enter details  {} query  {} load  {} save  Tab focus  {} refresh  {} quit",
            self.down,
            self.up,
            self.left,
            self.right,
            self.query,
            self.load_query,
            self.save_query,
            self.refresh,
            self.quit
        )
    }
}

pub fn message(keys: &Keybindings, model: &query::QueryBar, key: KeyEvent) -> Option<Message> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
        return Some(Message::Action(Action::Quit));
    }
    if key.code == KeyCode::Enter && model.focus == Focus::Table && model.dialog.is_none() {
        return Some(Message::Action(Action::OpenDetails));
    }
    let query_message = match key.code {
        KeyCode::Tab => Some(query::Message::CycleFocus(false)),
        KeyCode::BackTab => Some(query::Message::CycleFocus(true)),
        KeyCode::Esc => Some(query::Message::Cancel),
        KeyCode::Enter => Some(query::Message::Activate),
        _ => None,
    };
    if let Some(message) = query_message {
        return Some(Message::Query(message));
    }
    if model.editing() {
        return edit(key).map(|edit| Message::Query(query::Message::Edit(edit)));
    }
    if model.dialog.is_some() {
        let delta = match keys.action(key) {
            Some(Action::Up) => -1,
            Some(Action::Down) => 1,
            _ if key.code == KeyCode::Up => -1,
            _ if key.code == KeyCode::Down => 1,
            _ => return None,
        };
        return Some(Message::Query(query::Message::MoveSelection(delta)));
    }
    if model.focus == Focus::Table {
        keys.action(key).map(Message::Action)
    } else {
        None
    }
}

pub fn details_message(keys: &Keybindings, key: KeyEvent) -> Option<Message> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let action = match key.code {
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Enter => Some(Action::OpenDetails),
        KeyCode::Up => Some(Action::Up),
        KeyCode::Down => Some(Action::Down),
        KeyCode::Left | KeyCode::BackTab => Some(Action::Left),
        KeyCode::Right | KeyCode::Tab => Some(Action::Right),
        _ => keys.action(key),
    }?;
    if action == Action::Quit {
        Some(Message::Action(action))
    } else {
        Some(Message::Details(crate::details::Message::Action(action)))
    }
}

pub fn mouse(
    model: &query::QueryBar,
    event: MouseEvent,
    area: ratatui::layout::Rect,
) -> Option<Message> {
    if model.dialog.is_some() || event.kind != MouseEventKind::Down(MouseButton::Left) {
        return None;
    }
    let [input, load, save] = crate::query_view::controls(area);
    let position = (event.column, event.row).into();
    if input.contains(position) {
        Some(Message::Query(query::Message::Focus(Focus::Input)))
    } else if load.contains(position) {
        Some(Message::Query(query::Message::OpenLoad))
    } else if save.contains(position) {
        Some(Message::Query(query::Message::OpenSave))
    } else {
        Some(Message::Query(query::Message::Focus(Focus::Table)))
    }
}

fn edit(key: KeyEvent) -> Option<Edit> {
    if key.code == KeyCode::Char('u') && key.modifiers == KeyModifiers::CONTROL {
        return Some(Edit::Clear);
    }
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    Some(match key.code {
        KeyCode::Char(c) => Edit::Insert(c.to_string()),
        KeyCode::Left => Edit::Left,
        KeyCode::Right => Edit::Right,
        KeyCode::Home => Edit::Home,
        KeyCode::End => Edit::End,
        KeyCode::Backspace => Edit::Backspace,
        KeyCode::Delete => Edit::Delete,
        _ => return None,
    })
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

    #[test]
    fn typing_in_query_does_not_trigger_navigation_or_quit() {
        let mut model = query::QueryBar::new("", Default::default());
        model.focus = Focus::Input;
        for c in ['h', 'j', 'k', 'l', 'q', 'r', '/'] {
            assert!(
                matches!(message(&Keybindings::default(), &model, KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)), Some(Message::Query(query::Message::Edit(Edit::Insert(text)))) if text == c.to_string())
            );
        }
        assert!(matches!(
            message(
                &Keybindings::default(),
                &model,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            Some(Message::Action(Action::Quit))
        ));
    }

    #[test]
    fn buttons_are_clickable_and_modal_input_captures_navigation_keys() {
        let mut model = query::QueryBar::new("", Default::default());
        let area = ratatui::layout::Rect::new(0, 1, 80, 3);
        let [_, load, save] = crate::query_view::controls(area);
        let click = |column| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 2,
            modifiers: KeyModifiers::NONE,
        };
        assert!(matches!(
            mouse(&model, click(load.x + 1), area),
            Some(Message::Query(query::Message::OpenLoad))
        ));
        assert!(matches!(
            mouse(&model, click(save.x + 1), area),
            Some(Message::Query(query::Message::OpenSave))
        ));
        query::update(&mut model, query::Message::OpenSave);
        assert!(matches!(
            message(
                &Keybindings::default(),
                &model,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            Some(Message::Query(query::Message::Edit(Edit::Insert(_))))
        ));
        query::update(&mut model, query::Message::Cancel);
        query::update(&mut model, query::Message::OpenLoad);
        assert!(matches!(
            message(
                &Keybindings::default(),
                &model,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            Some(Message::Query(query::Message::MoveSelection(1)))
        ));
    }
}
