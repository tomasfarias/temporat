use crate::{input::Action, workflow::Execution};

pub struct Snapshot {
    pub overview: String,
}

pub enum Content {
    Overview(Snapshot),
    InputResult(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Overview,
    InputResult,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    InputResult,
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
        };
        if !refresh && !missing {
            return None;
        }
        self.loading = true;
        self.status = "Loading execution…".into();
        self.request.kind = kind;
        Some(Command::Load(self.request.clone()))
    }
}

pub fn update(model: &mut Model, message: Message) -> Option<Command> {
    match message {
        Message::Action(Action::Back) => return Some(Command::Close),
        Message::Action(Action::Refresh) => return model.load_tab(true),
        Message::Action(Action::Left | Action::Right) => {
            model.tab = if model.tab == Tab::Overview {
                Tab::InputResult
            } else {
                Tab::Overview
            };
            model.scroll = 0;
            return model.load_tab(false);
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
                    }
                    model.status = "Execution details".into();
                    if let Some(command) = model.load_tab(false) {
                        return Some(command);
                    }
                }
                Err(error) => model.status = format!("Error: {error}"),
            }
        }
        _ => {}
    }
    model.scroll = model
        .scroll
        .min(model.lines().len().saturating_sub(model.visible_rows));
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
}
