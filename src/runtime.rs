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
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste)?;
    let _input_mode = InputMode;
    let mut model = Model::new(&config, offline);
    app::update(&mut model, Message::Resize(terminal.size()?.height));
    let mut effect = app::update(&mut model, Message::Action(Action::Refresh));
    let mut client = None;
    // Dropping JoinSet cancels any pending connection/list request on quit or terminal error.
    let mut tasks = JoinSet::new();
    let mut request: Option<tokio::task::AbortHandle> = None;
    let mut events = EventStream::new();
    while !model.quit {
        if let Some(effect) = effect.take() {
            if let Some(request) = request.take() {
                request.abort();
            }
            request = Some(tasks.spawn(execute(config.clone(), client.clone(), effect)));
        }
        terminal.draw(|frame| view::view(frame, &model, &config))?;
        let message = tokio::select! {
            event = events.next() => match event.context("terminal event stream closed")?? {
                Event::Key(key) => input::message(&config.keybindings, model.query.focus, key),
                Event::Paste(text) if model.query.focus == crate::query::Focus::Input => Some(Message::Query(crate::query::Message::Edit(crate::text_input::Edit::Insert(text)))),
                Event::Resize(_, height) => Some(Message::Resize(height)),
                _ => None,
            },
            result = tasks.join_next(), if !tasks.is_empty() => {
                let result = result.context("request task missing")?;
                if result.as_ref().is_err_and(|error| error.is_cancelled()) { continue; }
                let (connection, message) = result.context("request task failed")?;
                client = connection;
                Some(message)
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
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    }
}

async fn execute(
    config: Arc<Config>,
    mut client: Option<Client>,
    effect: Effect,
) -> (Option<Client>, Message) {
    let Effect::LoadPage {
        token,
        page_size,
        query,
        generation,
    } = effect;
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
