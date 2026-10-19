use crate::{
    config::{Column, Config},
    input::Action,
};

// Elm: only update changes the model; view reads it; the runtime executes effects.
pub struct Model {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
    pub selected: usize,
    pub row_offset: usize,
    pub column_offset: usize,
    pub visible_rows: usize,
    pub status: String,
    pub quit: bool,
}

pub enum Message {
    Action(Action),
    Resize(u16),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    Refresh,
}

impl Model {
    pub fn preview(config: &Config) -> Self {
        Self {
            columns: config.table.columns.clone(),
            rows: (1..=25)
                .map(|index| {
                    config
                        .table
                        .columns
                        .iter()
                        .map(|_| format!("Preview {index}"))
                        .collect()
                })
                .collect(),
            selected: 0,
            row_offset: 0,
            column_offset: 0,
            visible_rows: 1,
            status: "Offline keybinding preview".into(),
            quit: false,
        }
    }
}

pub fn update(model: &mut Model, message: Message) -> Option<Effect> {
    match message {
        Message::Action(Action::Quit) => model.quit = true,
        Message::Action(Action::Up) => model.selected = model.selected.saturating_sub(1),
        Message::Action(Action::Down) => {
            model.selected = (model.selected + 1).min(model.rows.len().saturating_sub(1))
        }
        Message::Action(Action::Left) => {
            model.column_offset = model.column_offset.saturating_sub(1)
        }
        Message::Action(Action::Right) => {
            model.column_offset =
                (model.column_offset + 1).min(model.columns.len().saturating_sub(1))
        }
        Message::Action(Action::Refresh) => return Some(Effect::Refresh),
        Message::Resize(height) => {
            model.visible_rows = usize::from(height.saturating_sub(5)).max(1)
        }
    }
    model.row_offset = model.row_offset.min(model.selected);
    if model.selected >= model.row_offset + model.visible_rows {
        model.row_offset = model.selected + 1 - model.visible_rows;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_clamps_and_scrolls_to_keep_selection_visible() {
        let mut model = Model::preview(&Config::default());
        update(&mut model, Message::Resize(8));
        for _ in 0..50 {
            update(&mut model, Message::Action(Action::Down));
        }
        assert_eq!((model.selected, model.row_offset), (24, 22));
        for _ in 0..50 {
            update(&mut model, Message::Action(Action::Up));
        }
        assert_eq!((model.selected, model.row_offset), (0, 0));
        for _ in 0..50 {
            update(&mut model, Message::Action(Action::Right));
        }
        assert_eq!(model.column_offset, model.columns.len() - 1);
    }

    #[test]
    fn empty_table_and_tiny_terminal_are_navigable() {
        let mut model = Model::preview(&Config::default());
        model.rows.clear();
        update(&mut model, Message::Resize(0));
        update(&mut model, Message::Action(Action::Down));
        assert_eq!(model.selected, 0);
        assert_eq!(
            update(&mut model, Message::Action(Action::Refresh)),
            Some(Effect::Refresh)
        );
    }
}
