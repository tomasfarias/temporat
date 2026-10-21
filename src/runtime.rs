use std::{collections::VecDeque, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use temporalio_client::Client;
use tokio::task::JoinSet;

use crate::{
    app::{self, Effect, Message, Model},
    config::Config,
    input::{self, Action},
    temporal, view,
};

pub async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    config: Arc<Config>,
    offline: bool,
) -> Result<()> {
    crossterm::execute!(
        std::io::stdout(),
        crossterm::event::EnableBracketedPaste,
        crossterm::event::EnableMouseCapture
    )?;
    let _input_mode = InputMode;
    let mut model = Model::new(&config, offline);
    let size = terminal.size()?;
    app::update(
        &mut model,
        Message::Resize {
            width: size.width,
            height: size.height,
        },
    );
    let mut effects: VecDeque<_> = app::update(&mut model, Message::Action(Action::Refresh))
        .into_iter()
        .collect();
    let mut client = None;
    // Dropping JoinSet cancels any pending connection/list request on quit or terminal error.
    let mut tasks = JoinSet::new();
    let mut request: Option<tokio::task::AbortHandle> = None;
    let mut detail_request: Option<tokio::task::AbortHandle> = None;
    let mut events = EventStream::new();
    while !model.quit {
        while let Some(effect) = effects.pop_front() {
            match effect {
                Effect::Batch(batch) => {
                    for effect in batch.into_iter().rev() {
                        effects.push_front(effect);
                    }
                }
                Effect::Preview(request) => {
                    if let Some(task) = detail_request.take() {
                        task.abort();
                    }
                    detail_request = Some(tasks.spawn(async move {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        Completed::Local(Message::PreviewReady {
                            generation: request.generation,
                        })
                    }));
                }
                Effect::Details(request) => {
                    if let Some(task) = detail_request.take() {
                        task.abort();
                    }
                    let config = config.clone();
                    let mut client = client.clone();
                    detail_request = Some(tasks.spawn(async move {
                        let result = async {
                            if client.is_none() {
                                client = Some(temporal::connect(&config.connection).await?);
                            }
                            let client = client.as_ref().expect("connected above");
                            match request.kind {
                                crate::details::Kind::Overview => {
                                    crate::temporal_details::load(client, &request.execution)
                                        .await
                                        .map(crate::details::Content::Overview)
                                }
                                crate::details::Kind::InputResult => {
                                    crate::temporal_details::input_result(
                                        client,
                                        &request.execution,
                                    )
                                    .await
                                        .map(crate::details::Content::InputResult)
                                }
                                crate::details::Kind::History { token } => {
                                    let (events, next_page_token) = crate::temporal_details::history_page(client, &request.execution, token, temporalio_common::protos::temporal::api::enums::v1::HistoryEventFilterType::AllEvent).await?;
                                    Ok(crate::details::Content::History(crate::history::Page { events: events.into_iter().map(crate::history::Event::from_proto).collect(), next_page_token }))
                                }
                            }
                        }
                        .await;
                        Completed::Page(
                            client,
                            Message::Details(crate::details::Message::Loaded {
                                generation: request.generation,
                                result: result.map_err(|error: anyhow::Error| format!("{error:#}")),
                            }),
                        )
                    }));
                }
                Effect::CancelDetails => {
                    if let Some(task) = detail_request.take() {
                        task.abort();
                    }
                }
                Effect::LoadPage {
                    token,
                    page_size,
                    query,
                    generation,
                } => {
                    if let Some(request) = request.take() {
                        request.abort();
                    }
                    let config = config.clone();
                    let client = client.clone();
                    request = Some(tasks.spawn(async move {
                        let (client, message) =
                            load(config, client, token, page_size, query, generation).await;
                        Completed::Page(client, message)
                    }));
                }
                Effect::SaveQuery { name, query } => {
                    let path = config.path.clone();
                    tasks.spawn_blocking(move || {
                        let result = crate::saved_queries::save(&path, &name, &query)
                            .map_err(|error| format!("{error:#}"));
                        Completed::Local(Message::Query(crate::query::Message::Saved {
                            name,
                            result,
                        }))
                    });
                }
            }
        }
        terminal.draw(|frame| view::view(frame, &model, &config))?;
        let message = tokio::select! {
            event = events.next() => match event.context("terminal event stream closed")?? {
                Event::Key(key) if model.error.is_some() => input::error_message(&config.keybindings, key),
                Event::Key(key) if model.details_focused => input::details_message(&config.keybindings, key),
                Event::Key(key) => input::message(&config.keybindings, &model.query, key),
                Event::Mouse(event) if model.error.is_some() => match event.kind {
                    crossterm::event::MouseEventKind::ScrollUp => Some(Message::Error(crate::error_popup::Message::Scroll(-3))),
                    crossterm::event::MouseEventKind::ScrollDown => Some(Message::Error(crate::error_popup::Message::Scroll(3))),
                    _ => None,
                },
                Event::Mouse(event) if model.query.dialog.is_none() => {
                    let size = terminal.size()?;
                    let (table, details) = crate::layout::panes(ratatui::layout::Rect::new(0, 0, size.width, size.height));
                    if details.is_some_and(|area| area.contains((event.column, event.row).into())) {
                        (event.kind == crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left))
                            .then_some(Message::Action(Action::OpenDetails))
                    } else if details.is_some() || !model.details_focused {
                        input::mouse(&model.query, event, view::areas(table)[1])
                    } else { None }
                },
                Event::Paste(text) if model.error.is_none() && !model.details_focused && model.query.editing() => Some(Message::Query(crate::query::Message::Edit(crate::text_input::Edit::Insert(text)))),
                Event::Resize(width, height) => Some(Message::Resize { width, height }),
                _ => None,
            },
            result = tasks.join_next(), if !tasks.is_empty() => {
                let result = result.context("request task missing")?;
                if result.as_ref().is_err_and(|error| error.is_cancelled()) { continue; }
                match result.context("request task failed")? {
                    Completed::Page(connection, message) => { if connection.is_some() { client = connection; } Some(message) }
                    Completed::Local(message) => Some(message),
                }
            }
        };
        if let Some(message) = message {
            effects.extend(app::update(&mut model, message));
            if model.details.is_none()
                && let Some(task) = detail_request.take()
            {
                task.abort();
            }
        }
    }
    Ok(())
}

struct InputMode;

impl Drop for InputMode {
    fn drop(&mut self) {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableBracketedPaste,
            crossterm::event::DisableMouseCapture
        );
    }
}

enum Completed {
    Page(Option<Client>, Message),
    Local(Message),
}

async fn load(
    config: Arc<Config>,
    mut client: Option<Client>,
    token: Vec<u8>,
    page_size: u16,
    query: String,
    generation: u64,
) -> (Option<Client>, Message) {
    let result = async {
        if client.is_none() {
            client = Some(temporal::connect(&config.connection).await?);
        }
        temporal::list_page(
            client.as_ref().expect("connected above"),
            &config.table.columns,
            page_size,
            token,
            &query,
        )
        .await
    }
    .await;
    (
        client,
        Message::Loaded {
            generation,
            result: result.map_err(|error| format!("{error:#}")),
        },
    )
}
