use std::collections::VecDeque;

use crate::{
    config::{Column, Config},
    details, error_popup,
    input::Action,
    query::{self, QueryBar},
    workflow::{Page, WorkflowRow},
};

// Elm: only update changes the model; view reads it; the runtime executes effects.
pub struct Model {
    pub columns: Vec<Column>,
    pub rows: Vec<WorkflowRow>,
    pub selected: usize,
    pub row_offset: usize,
    pub selected_column: usize,
    pub visible_rows: usize,
    pub status: String,
    pub quit: bool,
    pub query: QueryBar,
    pub details: Option<details::Model>,
    pub details_focused: bool,
    pub error: Option<error_popup::Popup>,
    detail_generation: u64,
    preview_pending: bool,
    overview_cache: VecDeque<(crate::workflow::Execution, details::Snapshot)>,
    width: u16,
    height: u16,
    generation: u64,
    pending_query: Option<String>,
    offline: bool,
    loading: Option<LoadKind>,
    next_page_token: Vec<u8>,
    page_size: Option<u16>,
    desired_rows: usize,
}

pub enum Message {
    Action(Action),
    Resize {
        width: u16,
        height: u16,
    },
    Details(details::Message),
    PreviewReady {
        generation: u64,
    },
    Error(error_popup::Message),
    Loaded {
        generation: u64,
        result: Result<Page, String>,
    },
    Query(query::Message),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    Batch(Vec<Effect>),
    Preview(details::Request),
    Details(details::Request),
    CancelDetails,
    SaveQuery {
        name: String,
        query: String,
    },
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
                .map(|index| WorkflowRow {
                    cells: config
                        .table
                        .columns
                        .iter()
                        .map(|_| format!("Preview {index}"))
                        .collect(),
                    ..Default::default()
                })
                .collect(),
            selected: 0,
            row_offset: 0,
            selected_column: 0,
            visible_rows: 1,
            status: "Offline keybinding preview".into(),
            quit: false,
            query: QueryBar::new(config.initial_query(), config.queries.clone()),
            details: None,
            details_focused: false,
            error: None,
            detail_generation: 0,
            preview_pending: false,
            overview_cache: VecDeque::new(),
            width: 80,
            height: 24,
            generation: 0,
            pending_query: None,
            offline: true,
            loading: None,
            next_page_token: Vec::new(),
            page_size: config.table.page_size,
            desired_rows: 1,
        }
    }

    pub fn split(&self) -> bool {
        crate::layout::panes(self.area()).1.is_some()
    }

    fn area(&self) -> ratatui::layout::Rect {
        ratatui::layout::Rect::new(0, 0, self.width, self.height)
    }

    fn detail_area(&self) -> ratatui::layout::Rect {
        crate::layout::panes(self.area()).1.unwrap_or(self.area())
    }

    fn cache_overview(
        &mut self,
        execution: crate::workflow::Execution,
        snapshot: details::Snapshot,
    ) {
        self.overview_cache
            .retain(|(cached, _)| cached != &execution);
        self.overview_cache.push_front((execution, snapshot));
        self.overview_cache.truncate(32);
    }

    fn open_details(
        &mut self,
        execution: crate::workflow::Execution,
        overview: String,
    ) -> details::Request {
        self.detail_generation += 1;
        let request = details::Request {
            generation: self.detail_generation,
            execution,
            kind: details::Kind::Overview,
        };
        let area = self.detail_area();
        let mut detail = details::Model::new(request.clone(), area.width, area.height);
        detail.snapshot = self
            .overview_cache
            .iter()
            .find(|(cached, _)| cached == &request.execution)
            .map(|(_, snapshot)| snapshot.clone())
            .or_else(|| (!overview.is_empty()).then_some(details::Snapshot { overview }));
        if detail.snapshot.is_some() {
            detail.status = "Cached metadata · refreshing…".into();
        }
        self.details = Some(detail);
        request
    }
}

pub fn update(model: &mut Model, message: Message) -> Option<Effect> {
    let effect = update_inner(model, message);
    combine(effect, sync_preview(model))
}

fn combine(first: Option<Effect>, second: Option<Effect>) -> Option<Effect> {
    match (first, second) {
        (Some(first), Some(second)) => Some(Effect::Batch(vec![first, second])),
        (first, second) => first.or(second),
    }
}

fn sync_preview(model: &mut Model) -> Option<Effect> {
    if model.quit {
        return None;
    }
    if !model.split() {
        if !model.details_focused && model.details.take().is_some() {
            model.preview_pending = false;
            return Some(Effect::CancelDetails);
        }
        return None;
    }
    let row = model.rows.get(model.selected);
    let execution = row.and_then(|row| row.execution.clone());
    if model
        .details
        .as_ref()
        .map(|detail| &detail.request.execution)
        == execution.as_ref()
    {
        return None;
    }
    let Some(execution) = execution else {
        model.details = None;
        model.details_focused = false;
        model.preview_pending = false;
        return Some(Effect::CancelDetails);
    };
    let overview = row.unwrap().overview.clone();
    let request = model.open_details(execution, overview);
    model.preview_pending = true;
    Some(Effect::Preview(request))
}

fn update_inner(model: &mut Model, message: Message) -> Option<Effect> {
    let mut detail_effect = None;
    let previous_visible_rows = model.visible_rows;
    let scrolling_down = matches!(&message, Message::Action(Action::Down));
    let resizing = matches!(&message, Message::Resize { .. });
    let received_page = matches!(&message, Message::Loaded { .. });
    match message {
        Message::PreviewReady { generation } => {
            if model.preview_pending
                && let Some(detail) = &model.details
                && detail.request.generation == generation
            {
                model.preview_pending = false;
                return Some(Effect::Details(detail.request.clone()));
            }
            return None;
        }
        Message::Error(error_popup::Message::Close) => {
            model.error = None;
            return None;
        }
        Message::Error(message) => {
            if let Some(error) = &mut model.error {
                error.scroll(
                    message,
                    ratatui::layout::Rect::new(0, 0, model.width, model.height),
                );
            }
            return None;
        }
        Message::Action(Action::Quit) => {
            model.quit = true;
            return None;
        }
        Message::Action(Action::OpenDetails) => {
            if let Some(detail) = &model.details {
                model.details_focused = true;
                if model.preview_pending {
                    model.preview_pending = false;
                    return Some(Effect::Details(detail.request.clone()));
                }
                return None;
            }
            if let Some(execution) = model
                .rows
                .get(model.selected)
                .and_then(|row| row.execution.clone())
            {
                model.details_focused = true;
                let overview = model.rows[model.selected].overview.clone();
                let request = model.open_details(execution, overview);
                return Some(Effect::Details(request));
            }
        }
        Message::Action(Action::Back) => {}
        Message::Details(message) => {
            if let details::Message::Loaded {
                generation,
                result: Ok(details::Content::Overview(snapshot)),
            } = &message
                && let Some(detail) = &model.details
                && *generation == detail.request.generation
            {
                model.cache_overview(detail.request.execution.clone(), snapshot.clone());
            }
            if let Some(details) = &mut model.details {
                match details::update(details, message) {
                    Some(details::Command::Load(request)) => return Some(Effect::Details(request)),
                    Some(details::Command::Close) => {
                        model.details_focused = false;
                        if model.split() {
                            return None;
                        }
                        model.details = None;
                        if model.rows.len() < model.desired_rows
                            && !model.next_page_token.is_empty()
                            && let Some(effect) = begin_load(model, LoadKind::Append)
                        {
                            return Some(effect);
                        }
                        return Some(Effect::CancelDetails);
                    }
                    None => {}
                }
            }
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
            if matches!(
                &message,
                query::Message::Focus(_)
                    | query::Message::OpenLoad
                    | query::Message::OpenSave
                    | query::Message::Clear
            ) {
                model.details_focused = false;
            }
            let previous_query = model.query.applied.clone();
            match query::update(&mut model.query, message) {
                Some(query::Command::Save { name, query }) => {
                    return Some(Effect::SaveQuery { name, query });
                }
                Some(query::Command::Apply) => {
                    // Invalidate in-flight responses, but keep the current result set until
                    // the replacement succeeds. Its pagination token belongs to applied.
                    model.generation += 1;
                    model.loading = None;
                    if !model.offline {
                        model.pending_query =
                            Some(std::mem::replace(&mut model.query.applied, previous_query));
                    }
                    return begin_load(model, LoadKind::Replace);
                }
                None => {}
            }
        }
        Message::Action(Action::LoadQuery) => {
            query::update(&mut model.query, query::Message::OpenLoad);
        }
        Message::Action(Action::SaveQuery) => {
            query::update(&mut model.query, query::Message::OpenSave);
        }
        Message::Resize { width, height } => {
            model.width = width;
            model.height = height;
            model.visible_rows = usize::from(height.saturating_sub(8)).max(1);
            let detail_area = model.detail_area();
            if let Some(error) = &mut model.error {
                error.scroll(
                    error_popup::Message::Scroll(0),
                    ratatui::layout::Rect::new(0, 0, width, height),
                );
            }
            if let Some(details) = &mut model.details
                && let Some(details::Command::Load(request)) = details::update(
                    details,
                    details::Message::Resize {
                        width: detail_area.width,
                        height: detail_area.height,
                    },
                )
            {
                detail_effect = Some(Effect::Details(request));
            }
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
                        model.error = Some(error_popup::Popup::new(model.status.clone()));
                        return None;
                    }
                    match kind {
                        LoadKind::Replace => {
                            if let Some(query) = model.pending_query.take() {
                                model.query.applied = query;
                            }
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
                    let query_failed = model.pending_query.take().is_some();
                    if query_failed && model.query.dialog.is_none() {
                        model.query.focus = query::Focus::Input;
                    }
                    model.status = format!("Error: {error}");
                    let context = if query_failed {
                        "Could not apply query. Previous workflows are still shown.\nClose this message to edit the query."
                    } else {
                        "Could not load workflows. Cached results are still available."
                    };
                    model.error = Some(error_popup::Popup::new(format!("{context}\n\n{error}")));
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
        && (model.split() || !model.details_focused)
        && model.error.is_none()
        && model.rows.len() < model.desired_rows
        && !model.next_page_token.is_empty()
    {
        return combine(begin_load(model, LoadKind::Append), detail_effect);
    }
    detail_effect
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
    let query = match kind {
        LoadKind::Replace => model.pending_query.as_ref().unwrap_or(&model.query.applied),
        LoadKind::Append => &model.query.applied,
    }
    .clone();
    model.loading = Some(kind);
    model.status = "Loading workflows…".into();
    Some(Effect::LoadPage {
        token,
        query,
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
        Message::Resize {
            width: 80,
            height: rows + 8,
        }
    }

    #[test]
    fn details_use_hidden_identity_and_return_to_cached_table_selection() {
        let mut model = Model::new(&Config::default(), false);
        let execution = crate::workflow::Execution {
            workflow_id: "order".into(),
            run_id: "old-run".into(),
        };
        model.rows = vec![
            WorkflowRow::default(),
            WorkflowRow {
                execution: Some(execution.clone()),
                cells: vec!["Running".into()],
                ..Default::default()
            },
        ];
        model.selected = 1;
        model.row_offset = 1;
        model.selected_column = 2;
        assert_eq!(
            update(&mut model, Message::Action(Action::OpenDetails)),
            Some(Effect::Details(details::Request {
                generation: 1,
                execution: execution.clone(),
                kind: details::Kind::Overview
            }))
        );
        assert_eq!(
            update(
                &mut model,
                Message::Details(details::Message::Action(Action::Back))
            ),
            Some(Effect::CancelDetails)
        );
        assert_eq!(
            (model.selected, model.row_offset, model.selected_column),
            (1, 1, 2)
        );
        assert_eq!(model.rows.len(), 2);
        update(&mut model, Message::Action(Action::OpenDetails));
        update(
            &mut model,
            Message::Details(details::Message::Loaded {
                generation: 1,
                result: Err("old request".into()),
            }),
        );
        assert!(
            !model
                .details
                .as_ref()
                .unwrap()
                .status
                .contains("old request")
        );
    }

    #[test]
    fn resizing_details_executes_history_prefetch_and_refills_table_on_return() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, resize(1));
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, page(&["row"], &[9]));
        let request = details::Request {
            generation: 1,
            execution: crate::workflow::Execution {
                workflow_id: "id".into(),
                run_id: "run".into(),
            },
            kind: details::Kind::History { token: vec![] },
        };
        let mut detail = details::Model::new(request, 80, 6);
        detail.tab = details::Tab::History;
        details::update(
            &mut detail,
            details::Message::Loaded {
                generation: 1,
                result: Ok(details::Content::History(crate::history::Page {
                    events: (1..=4)
                        .map(|id| crate::history::Event {
                            id,
                            time: "".into(),
                            kind: "Test".into(),
                            details: "".into(),
                        })
                        .collect(),
                    next_page_token: vec![42],
                })),
            },
        );
        model.details = Some(detail);
        model.details_focused = true;
        assert!(
            matches!(update(&mut model, Message::Resize { width: 80, height: 20 }), Some(Effect::Details(details::Request { kind: details::Kind::History { token }, .. })) if token == [42])
        );
        assert!(
            matches!(update(&mut model, Message::Details(details::Message::Action(Action::Back))), Some(Effect::LoadPage { token, .. }) if token == [9])
        );
        assert_eq!(model.rows.len(), 1);
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
        update(
            &mut model,
            Message::Resize {
                width: 0,
                height: 0,
            },
        );
        update(&mut model, Message::Action(Action::Down));
        assert_eq!(model.selected, 0);
        assert_eq!(update(&mut model, Message::Action(Action::Refresh)), None);
    }

    fn page(names: &[&str], token: &[u8]) -> Message {
        Message::Loaded {
            generation: 0,
            result: Ok(Page {
                rows: names
                    .iter()
                    .map(|name| WorkflowRow {
                        cells: vec![(*name).into()],
                        ..Default::default()
                    })
                    .collect(),
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
        assert_eq!(model.rows[0].cells, ["first"]);
        assert!(model.status.contains("unavailable"));
        update(&mut model, Message::Error(error_popup::Message::Close));
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
        assert_eq!(
            update(
                &mut model,
                Message::Resize {
                    width: 80,
                    height: 100
                }
            ),
            None
        );
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
                    rows: vec![WorkflowRow {
                        cells: vec!["new".into()],
                        ..Default::default()
                    }],
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

    #[test]
    fn failed_query_retains_cached_rows_selection_and_original_pagination_filter() {
        let mut config = Config::default();
        config
            .queries
            .insert("Running".into(), "ExecutionStatus = 'Running'".into());
        config.table.default_query = Some("Running".into());
        let mut model = Model::new(&config, false);
        update(&mut model, resize(2));
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, page(&["a", "b", "c"], &[42]));
        update(&mut model, Message::Action(Action::Down));
        update(&mut model, Message::Action(Action::Down));
        update(&mut model, Message::Action(Action::Right));
        let selection = (model.selected, model.row_offset, model.selected_column);
        update(&mut model, Message::Action(Action::Query));
        update(
            &mut model,
            Message::Query(query::Message::Edit(crate::text_input::Edit::Clear)),
        );
        update(
            &mut model,
            Message::Query(query::Message::Edit(crate::text_input::Edit::Insert(
                "invalid query".into(),
            ))),
        );
        assert!(
            matches!(update(&mut model, Message::Query(query::Message::Activate)), Some(Effect::LoadPage { query, token, generation: 1, .. }) if query == "invalid query" && token.is_empty())
        );
        assert_eq!(model.rows.len(), 3);
        // An old prefetch must not replace or append to the preserved rows.
        update(&mut model, page(&["stale"], &[9]));
        update(
            &mut model,
            Message::Loaded {
                generation: 1,
                result: Err("invalid syntax".into()),
            },
        );
        assert_eq!(model.rows.len(), 3);
        assert_eq!(
            (model.selected, model.row_offset, model.selected_column),
            selection
        );
        assert_eq!(model.query.applied, "ExecutionStatus = 'Running'");
        assert_eq!(model.query.input.text, "invalid query");
        assert_eq!(model.query.focus, query::Focus::Input);
        assert!(
            model
                .error
                .as_ref()
                .unwrap()
                .text
                .contains("invalid syntax")
        );
        update(&mut model, Message::Error(error_popup::Message::Close));
        update(&mut model, Message::Query(query::Message::Cancel));
        assert!(
            matches!(update(&mut model, Message::Action(Action::Down)), Some(Effect::LoadPage { query, token, .. }) if query == "ExecutionStatus = 'Running'" && token == [42])
        );
    }

    #[test]
    fn successful_query_replaces_old_results_even_when_empty() {
        let mut model = Model::new(&Config::default(), false);
        update(&mut model, Message::Action(Action::Refresh));
        update(&mut model, page(&["old"], &[42]));
        update(&mut model, Message::Action(Action::Query));
        update(
            &mut model,
            Message::Query(query::Message::Edit(crate::text_input::Edit::Insert(
                "WorkflowId = 'missing'".into(),
            ))),
        );
        update(&mut model, Message::Query(query::Message::Activate));
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.query.applied, "");
        update(
            &mut model,
            Message::Loaded {
                generation: 1,
                result: Ok(Page {
                    rows: vec![],
                    next_page_token: vec![],
                }),
            },
        );
        assert!(model.rows.is_empty());
        assert!(model.next_page_token.is_empty());
        assert_eq!(model.query.applied, "WorkflowId = 'missing'");
    }

    fn preview_rows() -> Vec<WorkflowRow> {
        ["first-run", "second-run"]
            .into_iter()
            .map(|run| WorkflowRow {
                execution: Some(crate::workflow::Execution {
                    workflow_id: "same-workflow".into(),
                    run_id: run.into(),
                }),
                cells: vec![run.into()],
                overview: format!("Run ID: {run}\nStatus: Running"),
            })
            .collect()
    }

    #[test]
    fn automatic_preview_debounces_exact_runs_and_ignores_stale_results() {
        let mut model = Model::new(&Config::default(), false);
        model.rows = preview_rows();
        assert!(matches!(
            update(
                &mut model,
                Message::Resize {
                    width: 160,
                    height: 24
                }
            ),
            Some(Effect::Preview(details::Request { generation: 1, .. }))
        ));
        assert!(!model.details_focused);
        assert!(model.details.as_ref().unwrap().text().contains("first-run"));
        assert!(matches!(
            update(&mut model, Message::PreviewReady { generation: 1 }),
            Some(Effect::Details(_))
        ));
        update(
            &mut model,
            Message::Details(details::Message::Loaded {
                generation: 1,
                result: Ok(details::Content::Overview(details::Snapshot {
                    overview: "cached first run".into(),
                })),
            }),
        );
        assert!(matches!(
            update(&mut model, Message::Action(Action::Down)),
            Some(Effect::Preview(details::Request { generation: 2, .. }))
        ));
        assert_eq!(
            model.details.as_ref().unwrap().request.execution.run_id,
            "second-run"
        );
        assert!(update(&mut model, Message::PreviewReady { generation: 1 }).is_none());
        update(
            &mut model,
            Message::Details(details::Message::Loaded {
                generation: 1,
                result: Err("stale failure".into()),
            }),
        );
        assert!(!model.details.as_ref().unwrap().status.contains("stale"));
        // Enter starts the selected preview immediately, without waiting for the timer.
        assert!(matches!(
            update(&mut model, Message::Action(Action::OpenDetails)),
            Some(Effect::Details(details::Request { generation: 2, .. }))
        ));
        assert!(model.details_focused);
        assert!(update(&mut model, Message::PreviewReady { generation: 2 }).is_none());
        update(
            &mut model,
            Message::Details(details::Message::Action(Action::Back)),
        );
        assert!(!model.details_focused);
        assert!(model.details.is_some());
        update(&mut model, Message::Action(Action::Up));
        assert_eq!(model.details.as_ref().unwrap().text(), "cached first run");
        update(
            &mut model,
            Message::Details(details::Message::Loaded {
                generation: 2,
                result: Ok(details::Content::Overview(details::Snapshot {
                    overview: "late second run".into(),
                })),
            }),
        );
        assert_eq!(model.details.as_ref().unwrap().text(), "cached first run");
    }

    #[test]
    fn table_prefetch_and_preview_are_scheduled_together() {
        let mut model = Model::new(&Config::default(), false);
        update(
            &mut model,
            Message::Resize {
                width: 160,
                height: 10,
            },
        );
        update(&mut model, Message::Action(Action::Refresh));
        update(
            &mut model,
            Message::Loaded {
                generation: 0,
                result: Ok(Page {
                    rows: preview_rows(),
                    next_page_token: vec![42],
                }),
            },
        );
        let Some(Effect::Batch(effects)) = update(&mut model, Message::Action(Action::Down)) else {
            panic!("expected table and preview effects")
        };
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadPage { token, .. } if token == &[42]))
        );
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Preview(details::Request { generation: 2, .. })
        )));
    }

    #[test]
    fn resizing_switches_between_preview_and_fullscreen_without_losing_focus_or_selection() {
        let mut model = Model::new(&Config::default(), false);
        model.rows = preview_rows();
        model.selected = 1;
        update(
            &mut model,
            Message::Resize {
                width: 160,
                height: 24,
            },
        );
        assert_eq!(
            update(
                &mut model,
                Message::Resize {
                    width: 80,
                    height: 24
                }
            ),
            Some(Effect::CancelDetails)
        );
        assert!(model.details.is_none());
        assert!(update(&mut model, Message::PreviewReady { generation: 1 }).is_none());
        update(&mut model, Message::Action(Action::OpenDetails));
        assert!(model.details_focused);
        let generation = model.details.as_ref().unwrap().request.generation;
        update(
            &mut model,
            Message::Resize {
                width: 160,
                height: 24,
            },
        );
        assert!(model.details_focused);
        assert_eq!(
            model.details.as_ref().unwrap().request.generation,
            generation
        );
        assert!(model.details.as_ref().unwrap().width < 80);
        update(
            &mut model,
            Message::Details(details::Message::Action(Action::Back)),
        );
        assert!(!model.details_focused);
        assert_eq!(model.selected, 1);
        assert!(model.details.is_some());
        update(
            &mut model,
            Message::Resize {
                width: 80,
                height: 24,
            },
        );
        assert!(model.details.is_none());
    }
}
