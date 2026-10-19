use crate::{
    config::{Column, Config},
    input::Action,
    query::{self, QueryBar},
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
    pub query: QueryBar,
    generation: u64,
    offline: bool,
    loading: Option<LoadKind>,
    next_page_token: Vec<u8>,
    page_size: Option<u16>,
    desired_rows: usize,
}

pub enum Message {
    Action(Action),
    Resize(u16),
    Loaded {
        generation: u64,
        result: Result<Page, String>,
    },
    Query(query::Message),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    LoadPage {
        token: Vec<u8>,
        page_size: u16,
        query: String,
        generation: u64,
    },
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
            query: QueryBar::new(config.initial_query()),
            generation: 0,
            offline: true,
            loading: None,
            next_page_token: Vec::new(),
            page_size: config.table.page_size,
            desired_rows: 1,
        }
    }
}

pub fn update(model: &mut Model, message: Message) -> Option<Effect> {
    let previous_visible_rows = model.visible_rows;
    let scrolling_down = matches!(&message, Message::Action(Action::Down));
    let resizing = matches!(&message, Message::Resize(_));
    let received_page = matches!(&message, Message::Loaded { .. });
    match message {
        Message::Action(Action::Quit) => {
            model.quit = true;
            return None;
        }
        Message::Action(Action::Up) => model.selected = model.selected.saturating_sub(1),
        Message::Action(Action::Down) => {
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
        Message::Action(Action::Query) => {
            query::update(&mut model.query, query::Message::Focus(query::Focus::Input));
        }
        Message::Query(message) => {
            if query::update(&mut model.query, message).is_some() {
                // A new query invalidates both cached rows and in-flight page responses.
                model.generation += 1;
                model.loading = None;
                model.rows.clear();
                model.next_page_token.clear();
                model.selected = 0;
                model.row_offset = 0;
                model.desired_rows = model.visible_rows;
                return begin_load(model, LoadKind::Replace);
            }
        }
        Message::Resize(height) => {
            model.visible_rows = usize::from(height.saturating_sub(8)).max(1)
        }
        Message::Loaded { generation, result } => {
            if generation != model.generation {
                return None;
            }
            let kind = model.loading.take()?;
            match result {
                Ok(page) => {
                    if matches!(kind, LoadKind::Append)
                        && page.rows.is_empty()
                        && !page.next_page_token.is_empty()
                        && page.next_page_token == model.next_page_token
                    {
                        model.status = "Error: workflow pagination made no progress".into();
                        return None;
                    }
                    match kind {
                        LoadKind::Replace => {
                            model.rows = page.rows;
                            model.selected = 0;
                            model.row_offset = 0;
                            model.desired_rows = model.visible_rows;
                        }
                        LoadKind::Append => {
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
                Err(error) => {
                    model.status = format!("Error: {error}");
                    return None;
                }
            }
        }
    }
    model.row_offset = model.row_offset.min(model.selected);
    if model.selected >= model.row_offset + model.visible_rows {
        model.row_offset = model.selected + 1 - model.visible_rows;
    }
    if scrolling_down {
        // Keep half a screen ahead of the cursor, fetching before the last loaded row.
        model.desired_rows = (model.row_offset + model.visible_rows)
            .max(model.selected + 1 + (model.visible_rows / 2).max(1));
    } else if resizing {
        model.desired_rows = model.row_offset + model.visible_rows;
    }
    if (scrolling_down || (resizing && model.visible_rows > previous_visible_rows) || received_page)
        && model.rows.len() < model.desired_rows
        && !model.next_page_token.is_empty()
    {
        return begin_load(model, LoadKind::Append);
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
    Some(Effect::LoadPage {
        token,
        query: model.query.applied.clone(),
        generation: model.generation,
        page_size: model
            .page_size
            .unwrap_or(model.visible_rows.clamp(1, 1000) as u16),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(token: &[u8], page_size: u16) -> Option<Effect> {
        Some(Effect::LoadPage {
            token: token.to_vec(),
            page_size,
            query: String::new(),
            generation: 0,
        })
    }

    fn resize(rows: u16) -> Message {
        Message::Resize(rows + 8)
    }

    #[test]
    fn navigation_clamps_and_scrolls_to_keep_selection_visible() {
        let mut model = Model::preview(&Config::default());
        update(&mut model, resize(3));
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
        Message::Loaded {
            generation: 0,
            result: Ok(Page {
                rows: names.iter().map(|name| vec![(*name).into()]).collect(),
                next_page_token: token.to_vec(),
            }),
        }
    }

    #[test]
    fn fills_one_screen_then_prefetches_before_reaching_the_bottom() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, resize(6));
        assert_eq!(
            update(&mut model, Message::Action(Action::Refresh)),
            load(&[], 6)
        );
        assert_eq!(update(&mut model, Message::Action(Action::Refresh)), None);
        assert_eq!(
            update(&mut model, page(&["a", "b", "c", "d", "e", "f"], &[42])),
            None
        );
        for _ in 0..2 {
            assert_eq!(update(&mut model, Message::Action(Action::Down)), None);
        }
        assert_eq!(
            update(&mut model, Message::Action(Action::Down)),
            load(&[42], 6)
        );
        assert_eq!(model.selected, 3);
        assert_eq!(update(&mut model, Message::Action(Action::Down)), None);
        update(&mut model, page(&["g", "h", "i", "j", "k", "l"], &[]));
        assert_eq!(model.rows.len(), 12);
        assert_eq!(model.selected, 4);
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
        update(
            &mut model,
            Message::Loaded {
                generation: 0,
                result: Err("unavailable".into()),
            },
        );
        assert_eq!(model.rows, [vec!["first"]]);
        assert!(model.status.contains("unavailable"));
        assert_eq!(
            update(&mut model, Message::Action(Action::Down)),
            load(&[42], 1)
        );
        update(&mut model, Message::Action(Action::Quit));
        assert!(model.quit);
    }

    #[test]
    fn short_and_empty_pages_continue_loading_until_the_screen_is_filled() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, resize(3));
        update(&mut model, Message::Action(Action::Refresh));
        assert_eq!(update(&mut model, page(&["first"], &[1])), load(&[1], 3));
        assert_eq!(update(&mut model, page(&[], &[2])), load(&[2], 3));
        assert_eq!(update(&mut model, page(&["second", "third"], &[3])), None);
        assert_eq!(model.rows.len(), 3);
        assert_eq!(model.selected, 0);
    }

    #[test]
    fn resize_grows_from_cached_rows_then_loads_more_without_resetting_selection() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, resize(2));
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, page(&["a", "b", "c", "d"], &[1]));
        update(&mut model, Message::Action(Action::Down));
        update(&mut model, Message::Action(Action::Right));
        assert_eq!(update(&mut model, resize(4)), None);
        assert_eq!(update(&mut model, resize(6)), load(&[1], 6));
        assert_eq!(update(&mut model, resize(8)), None);
        assert_eq!(update(&mut model, page(&["e", "f"], &[2])), load(&[2], 8));
        assert_eq!(update(&mut model, page(&["g", "h"], &[3])), None);
        assert_eq!((model.selected, model.selected_column), (1, 1));
        assert_eq!(update(&mut model, resize(2)), None);
        assert_eq!(model.rows.len(), 8);
        assert_eq!(update(&mut model, resize(8)), None);
        assert_eq!(model.rows.len(), 8);
    }

    #[test]
    fn shrinking_during_a_request_caches_its_results_without_loading_another_page() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, resize(4));
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, page(&["a", "b"], &[1]));
        update(&mut model, resize(1));
        assert_eq!(update(&mut model, page(&["c"], &[2])), None);
        assert_eq!(model.rows.len(), 3);
        assert_eq!(model.visible_rows, 1);
    }

    #[test]
    fn fixed_page_size_is_honored_while_filling_the_viewport() {
        let mut config = Config::default();
        config.table.page_size = Some(2);
        let mut model = Model::new(&config, false);
        update(&mut model, resize(5));
        assert_eq!(
            update(&mut model, Message::Action(Action::Refresh)),
            load(&[], 2)
        );
        assert_eq!(update(&mut model, page(&["a", "b"], &[1])), load(&[1], 2));
        assert_eq!(update(&mut model, page(&["c"], &[])), None);
        assert_eq!(update(&mut model, Message::Resize(100)), None);
    }

    #[test]
    fn repeated_empty_page_stops_instead_of_spinning() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, page(&[], &[1]));
        assert_eq!(update(&mut model, page(&[], &[1])), None);
        assert!(model.status.contains("no progress"));
    }

    #[test]
    fn applying_a_query_invalidates_old_pages_and_keeps_its_filter_on_pagination() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, Message::Action(Action::Query));
        update(
            &mut model,
            Message::Query(query::Message::Edit(crate::text_input::Edit::Insert(
                "ExecutionStatus = 'Failed'".into(),
            ))),
        );
        let effect = update(&mut model, Message::Query(query::Message::Activate));
        assert_eq!(
            effect,
            Some(Effect::LoadPage {
                token: vec![],
                page_size: 1,
                query: "ExecutionStatus = 'Failed'".into(),
                generation: 1
            })
        );
        assert_eq!(update(&mut model, page(&["stale"], &[9])), None);
        assert!(model.rows.is_empty());
        update(
            &mut model,
            Message::Loaded {
                generation: 1,
                result: Ok(Page {
                    rows: vec![vec!["new".into()]],
                    next_page_token: vec![3],
                }),
            },
        );
        assert_eq!(
            update(&mut model, Message::Action(Action::Down)),
            Some(Effect::LoadPage {
                token: vec![3],
                page_size: 1,
                query: "ExecutionStatus = 'Failed'".into(),
                generation: 1
            })
        );
    }
}
