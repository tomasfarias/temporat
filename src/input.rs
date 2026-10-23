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
    Profiles,
    ToggleAutoRefresh,
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
    pub profiles: char,
    pub auto_refresh: char,
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
            profiles: 'n',
            auto_refresh: 'a',
        }
    }
}

impl Keybindings {
    fn bindings(&self) -> [(char, Action); 11] {
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
            (self.profiles, Action::Profiles),
            (self.auto_refresh, Action::ToggleAutoRefresh),
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
        KeyCode::Esc => Some(query::Message::Cancel),
        KeyCode::Enter => Some(query::Message::Activate),
        _ => None,
    };
    if let Some(message) = query_message {
        return Some(Message::Query(message));
    }
    if model.focus == Focus::Input
        && model.dialog.is_none()
        && key.modifiers == KeyModifiers::CONTROL
    {
        match key.code {
            KeyCode::Char('o') => return Some(Message::Query(query::Message::OpenLoad)),
            KeyCode::Char('s') => return Some(Message::Query(query::Message::OpenSave)),
            _ => {}
        }
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
        KeyCode::Left => Some(Action::Left),
        KeyCode::Right => Some(Action::Right),
        _ => keys.action(key),
    }?;
    if matches!(
        action,
        Action::Quit | Action::Profiles | Action::ToggleAutoRefresh
    ) {
        Some(Message::Action(action))
    } else {
        Some(Message::Details(crate::details::Message::Action(action)))
    }
}

pub fn error_message(keys: &Keybindings, key: KeyEvent) -> Option<Message> {
    use crate::error_popup::Message as Error;
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let message = match key.code {
        KeyCode::Enter | KeyCode::Esc => Error::Close,
        KeyCode::Up => Error::Scroll(-1),
        KeyCode::Down => Error::Scroll(1),
        KeyCode::PageUp => Error::Page(-1),
        KeyCode::PageDown => Error::Page(1),
        KeyCode::Home => Error::Scroll(isize::MIN),
        KeyCode::End => Error::Scroll(isize::MAX),
        _ => match keys.action(key) {
            Some(Action::Quit) => return Some(Message::Action(Action::Quit)),
            Some(Action::Up) => Error::Scroll(-1),
            Some(Action::Down) => Error::Scroll(1),
            _ => return None,
        },
    };
    Some(Message::Error(message))
}

pub fn mouse(
    model: &query::QueryBar,
    event: MouseEvent,
    area: ratatui::layout::Rect,
) -> Option<Message> {
    if model.dialog.is_some() || event.kind != MouseEventKind::Down(MouseButton::Left) {
        return None;
    }
    let [input, load, save, clear] = crate::query_view::controls(area);
    let position = (event.column, event.row).into();
    if input.contains(position) {
        Some(Message::Query(query::Message::Focus(Focus::Input)))
    } else if load.contains(position) {
        Some(Message::Query(query::Message::OpenLoad))
    } else if save.contains(position) {
        Some(Message::Query(query::Message::OpenSave))
    } else if clear.contains(position) {
        Some(Message::Query(query::Message::Clear))
    } else {
        Some(Message::Query(query::Message::Focus(Focus::Table)))
    }
}

pub(crate) fn edit(key: KeyEvent) -> Option<Edit> {
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
    fn auto_refresh_toggle_is_configurable_and_does_not_steal_query_input() {
        let keys = Keybindings {
            auto_refresh: 'A',
            ..Default::default()
        };
        keys.validate().unwrap();
        let key = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
        let mut query = query::QueryBar::new("", Default::default());
        assert!(matches!(
            message(&keys, &query, key),
            Some(Message::Action(Action::ToggleAutoRefresh))
        ));
        assert!(matches!(
            details_message(&keys, key),
            Some(Message::Action(Action::ToggleAutoRefresh))
        ));
        query.focus = Focus::Input;
        assert!(
            matches!(message(&keys, &query, key), Some(Message::Query(query::Message::Edit(Edit::Insert(text)))) if text == "A")
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
        let [_, load, save, _] = crate::query_view::controls(area);
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
