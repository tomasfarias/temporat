use std::{collections::VecDeque, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use temporalio_client::Client;
use tokio::task::JoinSet;

use crate::{
    app::{Effect, Message},
    config::{Config, Connection},
    input::{self, Action},
    profiles::{self, Workspace},
    temporal,
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
    let mut workspace = Workspace::new(&config, offline);
    let size = terminal.size()?;
    profiles::update(
        &mut workspace,
        &config,
        Message::Resize {
            width: size.width,
            height: size.height,
        },
    );
    let mut effects: VecDeque<_> =
        profiles::update(&mut workspace, &config, Message::Action(Action::Refresh))
            .into_iter()
            .collect();
    let mut client = None;
    // Dropping JoinSet cancels any pending connection/list request on quit or terminal error.
    let mut tasks = JoinSet::new();
    let mut request: Option<tokio::task::AbortHandle> = None;
    let mut detail_request: Option<tokio::task::AbortHandle> = None;
    let mut switch_request: Option<tokio::task::AbortHandle> = None;
    let mut events = EventStream::new();
    let mut refresh_timer = crate::refresh::timer(config.table.refresh_interval_secs);
    let mut countdown_timer = crate::refresh::timer(1);
    let mut animation_timer = tokio::time::interval(Duration::from_millis(100));
    animation_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    while !workspace.active.quit {
        effects.extend(profiles::update(
            &mut workspace,
            &config,
            Message::RefreshCountdown(refresh_timer.remaining_secs()),
        ));
        while let Some(effect) = effects.pop_front() {
            let epoch = workspace.epoch;
            match effect {
                Effect::CancelPage => {
                    if let Some(task) = request.take() {
                        task.abort();
                    }
                }
                Effect::PrepareProfile {
                    id,
                    name,
                    query,
                    page_size,
                } => {
                    if let Some(task) = switch_request.take() {
                        task.abort();
                    }
                    let connection = config.profiles[&name].clone();
                    let columns = config.table.columns.clone();
                    switch_request = Some(tasks.spawn(async move {
                        let result = async {
                            if offline {
                                return Ok((
                                    None,
                                    crate::workflow::Page {
                                        rows: Vec::new(),
                                        next_page_token: Vec::new(),
                                    },
                                ));
                            }
                            let client = temporal::connect(&connection).await?;
                            let page = temporal::list_page(
                                &client,
                                &columns,
                                page_size,
                                Vec::new(),
                                &query,
                            )
                            .await?;
                            Ok((Some(client), page))
                        }
                        .await
                        .map_err(|error: anyhow::Error| format!("{error:#}"));
                        Completed::Prepared { id, result }
                    }));
                }
                Effect::CancelSwitch => {
                    if let Some(task) = switch_request.take() {
                        task.abort();
                    }
                }
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
                        Completed::Local(
                            epoch,
                            Message::PreviewReady {
                                generation: request.generation,
                            },
                        )
                    }));
                }
                Effect::Details(request) => {
                    if let Some(task) = detail_request.take() {
                        task.abort();
                    }
                    let connection = config.profiles[&workspace.active.profile].clone();
                    let mut client = client.clone();
                    detail_request = Some(tasks.spawn(async move {
                        let result = async {
                            if client.is_none() {
                                client = Some(temporal::connect(&connection).await?);
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
                            epoch,
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
                    let connection = config.profiles[&workspace.active.profile].clone();
                    let client = client.clone();
                    request = Some(tasks.spawn(async move {
                        let (client, message) = load(
                            config, connection, client, token, page_size, query, generation,
                        )
                        .await;
                        Completed::Page(epoch, client, message)
                    }));
                }
                Effect::SaveQuery { name, query } => {
                    let path = config.path.clone();
                    let profile = workspace.active.profile.clone();
                    tasks.spawn_blocking(move || {
                        let result = crate::saved_queries::save(&path, &name, &query)
                            .map_err(|error| format!("{error:#}"));
                        Completed::Global(Message::Profiles(profiles::Message::Saved {
                            profile,
                            name,
                            result,
                        }))
                    });
                }
            }
        }
        terminal.draw(|frame| profiles::view(frame, &workspace, &config))?;
        let model = &workspace.active;
        let mut prepared_client = None;
        let message = tokio::select! {
            _ = animation_timer.tick(), if model.refreshing() => Some(Message::AnimateRefresh),
            _ = countdown_timer.tick(), if model.auto_refresh && !offline => None,
            _ = refresh_timer.tick(), if model.auto_refresh && !offline => Some(Message::AutoRefresh),
            event = events.next() => match event.context("terminal event stream closed")?? {
                Event::Key(key) if model.error.is_some() => input::error_message(&config.keybindings, key),
                Event::Key(key) if workspace.picker.is_some() => profiles::input(key),
                Event::Key(key) if model.details_focused => input::details_message(&config.keybindings, key),
                Event::Key(key) => input::message(&config.keybindings, &model.query, key),
                Event::Mouse(event) if model.error.is_some() => match event.kind {
                    crossterm::event::MouseEventKind::ScrollUp => Some(Message::Error(crate::error_popup::Message::Scroll(-3))),
                    crossterm::event::MouseEventKind::ScrollDown => Some(Message::Error(crate::error_popup::Message::Scroll(3))),
                    _ => None,
                },
                Event::Mouse(event) if workspace.picker.is_none() && model.query.dialog.is_none() => {
                    let size = terminal.size()?;
                    let (table, details) = crate::layout::panes(ratatui::layout::Rect::new(0, 0, size.width, size.height));
                    if details.is_some_and(|area| area.contains((event.column, event.row).into())) {
                        (event.kind == crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left))
                            .then_some(Message::Action(Action::OpenDetails))
                    } else if table.contains((event.column, event.row).into()) && (details.is_some() || !model.details_focused) {
                        input::mouse(&model.query, event, crate::layout::table(table)[1])
                    } else { None }
                },
                Event::Paste(text) if model.error.is_none() && workspace.picker.is_some() => Some(Message::Profiles(profiles::Message::Edit(crate::text_input::Edit::Insert(text)))),
                Event::Paste(text) if model.error.is_none() && !model.details_focused && model.query.editing() => Some(Message::Query(crate::query::Message::Edit(crate::text_input::Edit::Insert(text)))),
                Event::Resize(width, height) => Some(Message::Resize { width, height }),
                _ => None,
            },
            result = tasks.join_next(), if !tasks.is_empty() => {
                let result = result.context("request task missing")?;
                if result.as_ref().is_err_and(|error| error.is_cancelled()) { continue; }
                let completed = result.context("request task failed")?;
                // Guard before accepting either a response or its client. Model generations
                // can coincide across profiles, and abort can race with task completion.
                if !completed.is_current(&workspace) { continue; }
                match completed {
                    Completed::Page(_, connection, message) => { if connection.is_some() { client = connection; } Some(message) }
                    Completed::Local(_, message) => Some(message),
                    Completed::Global(message) => Some(message),
                    Completed::Prepared { id, result } => {
                        let result = result.map(|(client, page)| { prepared_client = client; page });
                        Some(Message::Profiles(profiles::Message::Prepared { id, result }))
                    }
                }
            }
        };
        if let Some(message) = message {
            let epoch = workspace.epoch;
            let auto_refresh = workspace.active.auto_refresh;
            let manual_refresh = matches!(
                &message,
                Message::Action(Action::Refresh)
                    | Message::Details(crate::details::Message::Action(Action::Refresh))
            );
            let effect = profiles::update(&mut workspace, &config, message);
            let reset_countdown = manual_refresh && effect.is_some();
            effects.extend(effect);
            if reset_countdown
                || epoch != workspace.epoch
                || auto_refresh != workspace.active.auto_refresh
            {
                refresh_timer.reset();
                countdown_timer.reset();
            }
            if epoch != workspace.epoch {
                if let Some(task) = request.take() {
                    task.abort();
                }
                if let Some(task) = detail_request.take() {
                    task.abort();
                }
                client = prepared_client;
            }
            if workspace.active.details.is_none()
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
    Page(u64, Option<Client>, Message),
    Local(u64, Message),
    Global(Message),
    Prepared {
        id: u64,
        result: Result<(Option<Client>, crate::workflow::Page), String>,
    },
}

impl Completed {
    fn is_current(&self, workspace: &Workspace) -> bool {
        match self {
            Self::Page(epoch, ..) | Self::Local(epoch, _) => *epoch == workspace.epoch,
            Self::Global(_) => true,
            Self::Prepared { id, .. } => workspace.accepts(*id),
        }
    }
}

async fn load(
    config: Arc<Config>,
    connection: Connection,
    mut client: Option<Client>,
    token: Vec<u8>,
    page_size: u16,
    query: String,
    generation: u64,
) -> (Option<Client>, Message) {
    let result = async {
        if client.is_none() {
            client = Some(temporal::connect(&connection).await?);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_pages_details_and_debounce_timers_cannot_cross_profile_epochs() {
        let config = Config::default();
        let mut workspace = Workspace::new(&config, false);
        let completed = [
            Completed::Page(
                0,
                None,
                Message::Loaded {
                    generation: 0,
                    result: Err("old page".into()),
                },
            ),
            Completed::Page(
                0,
                None,
                Message::Details(crate::details::Message::Loaded {
                    generation: 1,
                    result: Err("old details".into()),
                }),
            ),
            Completed::Local(0, Message::PreviewReady { generation: 1 }),
        ];
        assert!(completed.iter().all(|result| result.is_current(&workspace)));
        workspace.epoch = 1;
        assert!(
            completed
                .iter()
                .all(|result| !result.is_current(&workspace))
        );
        assert!(
            Completed::Page(
                1,
                None,
                Message::Loaded {
                    generation: 0,
                    result: Err("current page".into())
                }
            )
            .is_current(&workspace)
        );
    }
}
