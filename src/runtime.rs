use std::sync::Arc;

use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use temporalio_client::Client;
use tokio::task::JoinSet;

use crate::{
    app::{self, Effect, Message, Model},
    config::Config,
    input::Action,
    temporal, view,
};

pub async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    config: Arc<Config>,
    offline: bool,
) -> Result<()> {
    let mut model = Model::new(&config, offline);
    app::update(&mut model, Message::Resize(terminal.size()?.height));
    let mut effect = app::update(&mut model, Message::Action(Action::Refresh));
    let mut client = None;
    // Dropping JoinSet cancels any pending connection/list request on quit or terminal error.
    let mut tasks = JoinSet::new();
    let mut events = EventStream::new();
    while !model.quit {
        if let Some(effect) = effect.take() {
            tasks.spawn(execute(config.clone(), client.take(), effect));
        }
        terminal.draw(|frame| view::view(frame, &model, &config))?;
        let message = tokio::select! {
            event = events.next() => match event.context("terminal event stream closed")?? {
                Event::Key(key) => config.keybindings.action(key).map(Message::Action),
                Event::Resize(_, height) => Some(Message::Resize(height)),
                _ => None,
            },
            result = tasks.join_next(), if !tasks.is_empty() => {
                let (connection, message) = result.context("request task missing")?.context("request task failed")?;
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

async fn execute(
    config: Arc<Config>,
    mut client: Option<Client>,
    effect: Effect,
) -> (Option<Client>, Message) {
    let result = async {
        if client.is_none() {
            client = Some(temporal::connect(&config.connection).await?);
        }
        match effect {
            Effect::LoadPage { token, page_size } => {
                temporal::list_page(
                    client.as_ref().expect("connected above"),
                    &config.table.columns,
                    page_size,
                    token,
                    config.initial_query(),
                )
                .await
            }
        }
    }
    .await;
    if result.is_err() {
        client = None;
    }
    (
        client,
        Message::Loaded(result.map_err(|error| format!("{error:#}"))),
    )
}
