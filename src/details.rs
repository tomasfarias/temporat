use crate::{history, input::Action, workflow::Execution};

pub struct Snapshot {
    pub overview: String,
}

pub enum Content {
    Overview(Snapshot),
    InputResult(String),
    History(history::Page),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Overview,
    InputResult,
    History { token: Vec<u8> },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    InputResult,
    History,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub generation: u64,
    pub execution: Execution,
    pub kind: Kind,
}

pub struct Model {
    pub request: Request,
    pub snapshot: Option<Snapshot>,
    pub input_result: Option<String>,
    pub tab: Tab,
    pub history: Option<history::Page>,
    pub event_selected: usize,
    pub event_offset: usize,
    pub event_open: bool,
    pub status: String,
    pub scroll: usize,
    pub width: u16,
    pub visible_rows: usize,
    loading: bool,
}

pub enum Message {
    Action(Action),
    Resize {
        width: u16,
        height: u16,
    },
    Loaded {
        generation: u64,
        result: Result<Content, String>,
    },
}

pub enum Command {
    Load(Request),
    Close,
}

impl Model {
    pub fn new(request: Request, width: u16, height: u16) -> Self {
        Self {
            request,
            snapshot: None,
            input_result: None,
            tab: Tab::Overview,
            history: None,
            event_selected: 0,
            event_offset: 0,
            event_open: false,
            status: "Loading execution…".into(),
            scroll: 0,
            width: width.saturating_sub(2).max(1),
            visible_rows: usize::from(height.saturating_sub(5)).max(1),
            loading: true,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let text = match self.tab {
            Tab::Overview => self
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.overview.as_str()),
            Tab::InputResult => self.input_result.as_deref(),
            Tab::History if self.event_open => self
                .history
                .as_ref()
                .and_then(|page| page.events.get(self.event_selected))
                .map(|event| event.details.as_str()),
            Tab::History => None,
        };
        text.unwrap_or("")
            .lines()
            .flat_map(|line| {
                textwrap::wrap(line, usize::from(self.width))
                    .into_iter()
                    .map(|line| line.into_owned())
            })
            .collect()
    }

    fn load_tab(&mut self, refresh: bool) -> Option<Command> {
        if self.loading {
            return None;
        }
        let (kind, missing) = match self.tab {
            Tab::Overview => (Kind::Overview, self.snapshot.is_none()),
            Tab::InputResult => (Kind::InputResult, self.input_result.is_none()),
            Tab::History => (Kind::History { token: Vec::new() }, self.history.is_none()),
        };
        if !refresh && !missing {
            return None;
        }
        self.loading = true;
        self.status = "Loading execution…".into();
        self.request.kind = kind;
        Some(Command::Load(self.request.clone()))
    }

    fn prefetch_history(&mut self) -> Option<Command> {
        if self.loading || self.tab != Tab::History || self.event_open {
            return None;
        }
        let page = self.history.as_ref()?;
        let desired = (self.event_offset + self.visible_rows)
            .max(self.event_selected + 1 + (self.visible_rows / 2).max(1));
        if page.next_page_token.is_empty() || page.events.len() >= desired {
            return None;
        }
        self.request.kind = Kind::History {
            token: page.next_page_token.clone(),
        };
        self.loading = true;
        self.status = "Loading history…".into();
        Some(Command::Load(self.request.clone()))
    }
}

pub fn update(model: &mut Model, message: Message) -> Option<Command> {
    let old_visible_rows = model.visible_rows;
    let prefetch = matches!(
        &message,
        Message::Action(Action::Down | Action::Back)
            | Message::Loaded { .. }
            | Message::Resize { .. }
    );
    match message {
        Message::Action(Action::Back) if model.event_open => {
            model.event_open = false;
            model.scroll = 0;
        }
        Message::Action(Action::Back) => return Some(Command::Close),
        Message::Action(Action::Refresh) => return model.load_tab(true),
        Message::Action(action @ (Action::Left | Action::Right)) => {
            let tabs = [Tab::Overview, Tab::InputResult, Tab::History];
            model.tab = tabs
                [(model.tab as usize + if action == Action::Right { 1 } else { 2 }) % tabs.len()];
            model.event_open = false;
            model.scroll = 0;
            return model.load_tab(false).or_else(|| model.prefetch_history());
        }
        Message::Action(Action::OpenDetails) if model.tab == Tab::History => {
            model.event_open = model
                .history
                .as_ref()
                .is_some_and(|page| !page.events.is_empty());
            model.scroll = 0;
        }
        Message::Action(Action::Up) if model.tab == Tab::History && !model.event_open => {
            model.event_selected = model.event_selected.saturating_sub(1);
        }
        Message::Action(Action::Down) if model.tab == Tab::History && !model.event_open => {
            model.event_selected = (model.event_selected + 1).min(
                model
                    .history
                    .as_ref()
                    .map(|page| page.events.len())
                    .unwrap_or(0)
                    .saturating_sub(1),
            );
        }
        Message::Action(Action::Up) => model.scroll = model.scroll.saturating_sub(1),
        Message::Action(Action::Down) => model.scroll += 1,
        Message::Resize { width, height } => {
            model.width = width.saturating_sub(2).max(1);
            model.visible_rows = usize::from(height.saturating_sub(5)).max(1);
        }
        Message::Loaded { generation, result } if generation == model.request.generation => {
            model.loading = false;
            match result {
                Ok(content) => {
                    match content {
                        Content::Overview(snapshot) => model.snapshot = Some(snapshot),
                        Content::InputResult(text) => model.input_result = Some(text),
                        Content::History(page) => {
                            if let Kind::History { token } = &model.request.kind {
                                if !token.is_empty()
                                    && page.events.is_empty()
                                    && &page.next_page_token == token
                                {
                                    model.status =
                                        "Error: history pagination made no progress".into();
                                    return None;
                                }
                                if token.is_empty() {
                                    model.history = Some(page);
                                    model.event_selected = 0;
                                    model.event_offset = 0;
                                    model.event_open = false;
                                } else if let Some(history) = &mut model.history {
                                    history.events.extend(page.events);
                                    history.next_page_token = page.next_page_token;
                                }
                            }
                        }
                    }
                    model.status = "Execution details".into();
                    if let Some(command) = model.load_tab(false) {
                        return Some(command);
                    }
                }
                Err(error) => {
                    model.status = format!("Error: {error}");
                    return None;
                }
            }
        }
        Message::Loaded { .. } => return None,
        _ => {}
    }
    model.scroll = model
        .scroll
        .min(model.lines().len().saturating_sub(model.visible_rows));
    model.event_offset = model.event_offset.min(model.event_selected);
    if model.event_selected >= model.event_offset + model.visible_rows {
        model.event_offset = model.event_selected + 1 - model.visible_rows;
    }
    if prefetch && model.visible_rows >= old_visible_rows {
        return model.prefetch_history();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn model() -> Model {
        Model::new(
            Request {
                generation: 1,
                execution: Execution {
                    workflow_id: "id".into(),
                    run_id: "run".into(),
                },
                kind: Kind::Overview,
            },
            20,
            8,
        )
    }

    #[test]
    fn ignores_stale_responses_and_keeps_details_on_refresh_error() {
        let mut model = model();
        update(
            &mut model,
            Message::Loaded {
                generation: 0,
                result: Ok(Content::Overview(Snapshot {
                    overview: "stale".into(),
                })),
            },
        );
        assert!(model.snapshot.is_none());
        update(
            &mut model,
            Message::Loaded {
                generation: 1,
                result: Ok(Content::Overview(Snapshot {
                    overview: "current".into(),
                })),
            },
        );
        assert!(matches!(
            update(&mut model, Message::Action(Action::Refresh)),
            Some(Command::Load(_))
        ));
        update(
            &mut model,
            Message::Loaded {
                generation: 1,
                result: Err("unavailable".into()),
            },
        );
        assert_eq!(model.snapshot.unwrap().overview, "current");
        assert!(model.status.contains("unavailable"));
    }

    #[test]
    fn loads_input_tab_after_pending_overview_and_caches_it() {
        let mut model = model();
        assert!(update(&mut model, Message::Action(Action::Right)).is_none());
        let command = update(
            &mut model,
            Message::Loaded {
                generation: 1,
                result: Ok(Content::Overview(Snapshot {
                    overview: "Status: Running".into(),
                })),
            },
        );
        assert!(matches!(
            command,
            Some(Command::Load(Request {
                kind: Kind::InputResult,
                ..
            }))
        ));
        update(
            &mut model,
            Message::Loaded {
                generation: 1,
                result: Ok(Content::InputResult("Input: 42".into())),
            },
        );
        assert_eq!(model.lines(), ["Input: 42"]);
        update(&mut model, Message::Action(Action::Left));
        assert!(update(&mut model, Message::Action(Action::Right)).is_none());
    }

    fn page(ids: &[i64], token: &[u8]) -> Message {
        Message::Loaded {
            generation: 1,
            result: Ok(Content::History(history::Page {
                events: ids
                    .iter()
                    .map(|id| history::Event {
                        id: *id,
                        time: "time".into(),
                        kind: "Test".into(),
                        details: format!("Event {id}"),
                    })
                    .collect(),
                next_page_token: token.to_vec(),
            })),
        }
    }

    #[test]
    fn fills_history_view_and_appends_without_moving_selection() {
        let mut model = model();
        model.tab = Tab::History;
        model.request.kind = Kind::History { token: vec![] };
        let command = update(&mut model, page(&[1], &[42]));
        assert!(
            matches!(command, Some(Command::Load(Request { kind: Kind::History { token }, .. })) if token == [42])
        );
        assert!(update(&mut model, page(&[2, 3, 4], &[43])).is_none());
        assert_eq!(model.event_selected, 0);
        update(&mut model, Message::Action(Action::Down));
        update(&mut model, Message::Action(Action::OpenDetails));
        assert!(model.event_open);
        assert_eq!(model.lines(), ["Event 2"]);
        assert!(update(&mut model, Message::Action(Action::Back)).is_none());
        assert!(!model.event_open);
        assert_eq!(model.event_selected, 1);
        assert!(matches!(
            update(&mut model, Message::Action(Action::Back)),
            Some(Command::Close)
        ));
    }

    #[test]
    fn history_failure_retains_cached_events_and_can_retry() {
        let mut model = model();
        model.tab = Tab::History;
        model.request.kind = Kind::History { token: vec![] };
        update(&mut model, page(&[1], &[42]));
        update(
            &mut model,
            Message::Loaded {
                generation: 1,
                result: Err("unavailable".into()),
            },
        );
        assert_eq!(model.history.as_ref().unwrap().events.len(), 1);
        let command = update(&mut model, Message::Action(Action::Down));
        assert!(
            matches!(command, Some(Command::Load(Request { kind: Kind::History { token }, .. })) if token == [42])
        );
        assert!(update(&mut model, page(&[], &[42])).is_none());
        assert!(model.status.contains("no progress"));
    }
}
