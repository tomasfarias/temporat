use crate::{
    config::{Column, Config},
    input::Action,
    workflow::Page,
};

// Elm: only update changes the model; view reads it; the runtime executes effects.
pub struct Model {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
    pub selected: usize,
    pub row_offset: usize,
    pub selected_column: usize,
    pub visible_rows: usize,
    pub status: String,
    pub quit: bool,
    offline: bool,
    loading: Option<LoadKind>,
    next_page_token: Vec<u8>,
}

pub enum Message {
    Action(Action),
    Resize(u16),
    Loaded(Result<Page, String>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    LoadPage { token: Vec<u8> },
}

#[derive(Clone, Copy)]
enum LoadKind {
    Replace,
    Append,
}

impl Model {
    pub fn new(config: &Config, offline: bool) -> Self {
        let mut model = Self::preview(config);
        model.offline = offline;
        if !offline {
            model.rows.clear();
            model.status = "Ready".into();
        }
        model
    }

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
            selected_column: 0,
            visible_rows: 1,
            status: "Offline keybinding preview".into(),
            quit: false,
            offline: true,
            loading: None,
            next_page_token: Vec::new(),
        }
    }
}

pub fn update(model: &mut Model, message: Message) -> Option<Effect> {
    match message {
        Message::Action(Action::Quit) => model.quit = true,
        Message::Action(Action::Up) => model.selected = model.selected.saturating_sub(1),
        Message::Action(Action::Down) => {
            if model.selected + 1 >= model.rows.len() && !model.next_page_token.is_empty() {
                return begin_load(model, LoadKind::Append);
            }
            model.selected = (model.selected + 1).min(model.rows.len().saturating_sub(1))
        }
        Message::Action(Action::Left) => {
            model.selected_column = model.selected_column.saturating_sub(1)
        }
        Message::Action(Action::Right) => {
            model.selected_column =
                (model.selected_column + 1).min(model.columns.len().saturating_sub(1))
        }
        Message::Action(Action::Refresh) => return begin_load(model, LoadKind::Replace),
        Message::Resize(height) => {
            model.visible_rows = usize::from(height.saturating_sub(5)).max(1)
        }
        Message::Loaded(result) => {
            let kind = model.loading.take()?;
            match result {
                Ok(page) => {
                    match kind {
                        LoadKind::Replace => {
                            model.rows = page.rows;
                            model.selected = 0;
                            model.row_offset = 0;
                        }
                        LoadKind::Append => {
                            // Only advance to the new page if the user is still at the bottom.
                            if !page.rows.is_empty() && model.selected + 1 >= model.rows.len() {
                                model.selected = model.rows.len();
                            }
                            model.rows.extend(page.rows);
                        }
                    }
                    model.next_page_token = page.next_page_token;
                    let more = if model.next_page_token.is_empty() {
                        ""
                    } else {
                        " · more rows available"
                    };
                    model.status = format!("{} workflows{more}", model.rows.len());
                }
                Err(error) => model.status = format!("Error: {error}"),
            }
        }
    }
    model.row_offset = model.row_offset.min(model.selected);
    if model.selected >= model.row_offset + model.visible_rows {
        model.row_offset = model.selected + 1 - model.visible_rows;
    }
    None
}

fn begin_load(model: &mut Model, kind: LoadKind) -> Option<Effect> {
    // One request at a time makes responses ordered and repeated refresh keys harmless.
    if model.offline || model.loading.is_some() {
        return None;
    }
    let token = match kind {
        LoadKind::Replace => Vec::new(),
        LoadKind::Append => model.next_page_token.clone(),
    };
    model.loading = Some(kind);
    model.status = "Loading workflows…".into();
    Some(Effect::LoadPage { token })
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
        assert_eq!(model.selected_column, model.columns.len() - 1);
    }

    #[test]
    fn empty_table_and_tiny_terminal_are_navigable() {
        let mut model = Model::preview(&Config::default());
        model.rows.clear();
        update(&mut model, Message::Resize(0));
        update(&mut model, Message::Action(Action::Down));
        assert_eq!(model.selected, 0);
        assert_eq!(update(&mut model, Message::Action(Action::Refresh)), None);
    }

    fn page(names: &[&str], token: &[u8]) -> Message {
        Message::Loaded(Ok(Page {
            rows: names.iter().map(|name| vec![(*name).into()]).collect(),
            next_page_token: token.to_vec(),
        }))
    }

    #[test]
    fn paginates_at_bottom_and_refresh_replaces_results() {
        let mut model = Model::new(&Config::default(), false);
        assert_eq!(
            update(&mut model, Message::Action(Action::Refresh)),
            Some(Effect::LoadPage { token: vec![] })
        );
        assert_eq!(update(&mut model, Message::Action(Action::Refresh)), None);
        update(&mut model, page(&["first"], &[42]));
        assert_eq!(
            update(&mut model, Message::Action(Action::Down)),
            Some(Effect::LoadPage { token: vec![42] })
        );
        update(&mut model, page(&["second"], &[]));
        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.selected, 1);
        assert_eq!(update(&mut model, Message::Action(Action::Down)), None);
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, page(&[], &[]));
        assert!(model.rows.is_empty());
        assert_eq!((model.selected, model.row_offset), (0, 0));
    }

    #[test]
    fn failure_preserves_rows_and_page_token_for_retry() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, page(&["first"], &[42]));
        update(&mut model, Message::Action(Action::Down));
        update(&mut model, Message::Loaded(Err("unavailable".into())));
        assert_eq!(model.rows, [vec!["first"]]);
        assert!(model.status.contains("unavailable"));
        assert_eq!(
            update(&mut model, Message::Action(Action::Down)),
            Some(Effect::LoadPage { token: vec![42] })
        );
        update(&mut model, Message::Action(Action::Quit));
        assert!(model.quit);
    }

    #[test]
    fn empty_intermediate_page_keeps_pagination_available() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, page(&[], &[1]));
        assert_eq!(
            update(&mut model, Message::Action(Action::Down)),
            Some(Effect::LoadPage { token: vec![1] })
        );
    }
}
