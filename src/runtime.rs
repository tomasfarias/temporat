use std::sync::Arc;

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
    let mut effect = app::update(&mut model, Message::Action(Action::Refresh));
    let mut client = None;
    // Dropping JoinSet cancels any pending connection/list request on quit or terminal error.
    let mut tasks = JoinSet::new();
    let mut request: Option<tokio::task::AbortHandle> = None;
    let mut detail_request: Option<tokio::task::AbortHandle> = None;
    let mut events = EventStream::new();
    while !model.quit {
        if let Some(effect) = effect.take() {
            match effect {
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
                            crate::temporal_details::load(
                                client.as_ref().expect("connected above"),
                                &request.execution,
                            )
                            .await
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
                        Completed::Saved(Message::Query(crate::query::Message::Saved {
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
                Event::Key(key) if model.details.is_some() => input::details_message(&config.keybindings, key),
                Event::Key(key) => input::message(&config.keybindings, &model.query, key),
                Event::Mouse(event) if model.details.is_none() => {
                    let size = terminal.size()?;
                    input::mouse(&model.query, event, view::areas(ratatui::layout::Rect::new(0, 0, size.width, size.height))[1])
                },
                Event::Paste(text) if model.details.is_none() && model.query.editing() => Some(Message::Query(crate::query::Message::Edit(crate::text_input::Edit::Insert(text)))),
                Event::Resize(width, height) => Some(Message::Resize { width, height }),
                _ => None,
            },
            result = tasks.join_next(), if !tasks.is_empty() => {
                let result = result.context("request task missing")?;
                if result.as_ref().is_err_and(|error| error.is_cancelled()) { continue; }
                match result.context("request task failed")? {
                    Completed::Page(connection, message) => { client = connection; Some(message) }
                    Completed::Saved(message) => Some(message),
                }
            }
        };
        if let Some(message) = message {
            effect = app::update(&mut model, message);
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
    Saved(Message),
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
    if result.is_err() {
        client = None;
    }
    (
        client,
        Message::Loaded {
            generation,
            result: result.map_err(|error| format!("{error:#}")),
        },
    )
}
