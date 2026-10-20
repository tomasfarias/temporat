use crate::{input::Action, workflow::Execution};

pub struct Snapshot {
    pub overview: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub generation: u64,
    pub execution: Execution,
}

pub struct Model {
    pub request: Request,
    pub snapshot: Option<Snapshot>,
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
        result: Result<Snapshot, String>,
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
            status: "Loading execution…".into(),
            scroll: 0,
            width: width.saturating_sub(2).max(1),
            visible_rows: usize::from(height.saturating_sub(5)).max(1),
            loading: true,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.overview.as_str())
            .unwrap_or("")
            .lines()
            .flat_map(|line| {
                textwrap::wrap(line, usize::from(self.width))
                    .into_iter()
                    .map(|line| line.into_owned())
            })
            .collect()
    }
}

pub fn update(model: &mut Model, message: Message) -> Option<Command> {
    match message {
        Message::Action(Action::Back) => return Some(Command::Close),
        Message::Action(Action::Refresh) if !model.loading => {
            model.loading = true;
            model.status = "Loading execution…".into();
            return Some(Command::Load(model.request.clone()));
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
                Ok(snapshot) => {
                    model.snapshot = Some(snapshot);
                    model.status = "Execution details".into();
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
                result: Ok(Snapshot {
                    overview: "stale".into(),
                }),
            },
        );
        assert!(model.snapshot.is_none());
        update(
            &mut model,
            Message::Loaded {
                generation: 1,
                result: Ok(Snapshot {
                    overview: "current".into(),
                }),
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
}
