use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::{Clear, List, ListItem, ListState, Paragraph},
};

use crate::{
    app::{self, Effect},
    config::Config,
    error_popup,
    input::Action,
    text_input::{Edit, TextInput},
    workflow::Page,
};

pub struct Workspace {
    pub active: app::Model,
    pub epoch: u64,
    pub picker: Option<Picker>,
    inactive: BTreeMap<String, app::Model>,
    pending: Option<(u64, String)>,
    next_id: u64,
    width: u16,
    height: u16,
    offline: bool,
}

pub struct Picker {
    search: TextInput,
    selected: usize,
}

pub enum Message {
    Edit(Edit),
    Move(isize),
    Activate,
    Cancel,
    Prepared {
        id: u64,
        result: Result<Page, String>,
    },
    Saved {
        profile: String,
        name: String,
        result: Result<BTreeMap<String, String>, String>,
    },
}

impl Workspace {
    pub fn new(config: &Config, offline: bool) -> Self {
        Self {
            active: app::Model::new(config, offline),
            epoch: 0,
            picker: None,
            inactive: BTreeMap::new(),
            pending: None,
            next_id: 0,
            width: 80,
            height: 24,
            offline,
        }
    }

    pub fn accepts(&self, id: u64) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(pending, _)| *pending == id)
    }

    fn matches<'a>(&self, config: &'a Config) -> Vec<&'a str> {
        let filter = self
            .picker
            .as_ref()
            .map(|picker| picker.search.text.to_lowercase())
            .unwrap_or_default();
        config
            .profiles
            .iter()
            .filter(|(name, connection)| {
                name.to_lowercase().contains(&filter)
                    || connection.namespace.to_lowercase().contains(&filter)
            })
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

// The workspace owns profile transitions; ordinary app updates remain profile-local.
pub fn update(workspace: &mut Workspace, config: &Config, message: app::Message) -> Option<Effect> {
    let message = match message {
        app::Message::AutoRefresh | app::Message::AnimateCell
            if workspace.picker.is_some() || workspace.pending.is_some() =>
        {
            return None;
        }
        app::Message::Action(Action::Profiles) => {
            let selected = config
                .profiles
                .keys()
                .position(|name| name == &workspace.active.profile)
                .unwrap_or(0);
            workspace.picker = Some(Picker {
                search: TextInput::new(""),
                selected,
            });
            return None;
        }
        app::Message::Resize { width, height } => {
            workspace.width = width;
            workspace.height = height;
            return app::update(
                &mut workspace.active,
                app::Message::Resize { width, height },
            );
        }
        app::Message::Profiles(message) => message,
        message => return app::update(&mut workspace.active, message),
    };
    match message {
        Message::Cancel => {
            workspace.picker = None;
            workspace.pending = None;
            return Some(Effect::CancelSwitch);
        }
        Message::Prepared { id, result } => {
            if !workspace.accepts(id) {
                return None;
            }
            let (_, name) = workspace.pending.take().unwrap();
            let page = match result {
                Ok(page) => page,
                Err(error) => {
                    workspace.active.error = Some(error_popup::Popup::new(format!(
                        "Could not switch to profile {name:?}.\n\n{error}"
                    )));
                    return None;
                }
            };
            let cached = workspace.inactive.remove(&name);
            let returning = cached.as_ref().is_some_and(app::Model::has_cached_page);
            let mut target = cached.unwrap_or_else(|| app::Model::new(config, workspace.offline));
            target.profile = name;
            target.auto_refresh = workspace.active.auto_refresh;
            target.query.saved = workspace.active.query.saved.clone();
            workspace.active.suspend();
            let previous = std::mem::replace(&mut workspace.active, target);
            workspace
                .inactive
                .insert(previous.profile.clone(), previous);
            workspace.epoch += 1;
            workspace.picker = None;
            let mut effects: Vec<_> = app::update(
                &mut workspace.active,
                app::Message::Resize {
                    width: workspace.width,
                    height: workspace.height,
                },
            )
            .into_iter()
            .collect();
            if returning {
                effects.extend(app::update(&mut workspace.active, app::Message::Resume));
            } else if !workspace.offline {
                effects.extend(workspace.active.initial_page(page));
            }
            return Some(Effect::Batch(effects));
        }
        Message::Saved {
            profile,
            name,
            result,
        } => {
            if let Ok(saved) = &result {
                workspace.active.query.saved = saved.clone();
                for model in workspace.inactive.values_mut() {
                    model.query.saved = saved.clone();
                }
            }
            let model = if profile == workspace.active.profile {
                Some(&mut workspace.active)
            } else {
                workspace.inactive.get_mut(&profile)
            };
            if let Some(model) = model {
                // Saving is global and may complete after leaving the originating profile.
                crate::query::update(
                    &mut model.query,
                    crate::query::Message::Saved { name, result },
                );
            }
        }
        _ if workspace.pending.is_some() => {}
        Message::Edit(edit) => {
            if let Some(picker) = &mut workspace.picker {
                picker.search.update(edit);
                picker.selected = 0;
            }
        }
        Message::Move(delta) => {
            let count = workspace.matches(config).len();
            if let Some(picker) = &mut workspace.picker {
                picker.selected = picker
                    .selected
                    .saturating_add_signed(delta)
                    .min(count.saturating_sub(1));
            }
        }
        Message::Activate => {
            let picker = workspace.picker.as_ref()?;
            let names = workspace.matches(config);
            let name = *names.get(picker.selected)?;
            if name == workspace.active.profile {
                workspace.picker = None;
                return None;
            }
            let query = workspace
                .inactive
                .get(name)
                .map(|model| model.query.applied.as_str())
                .unwrap_or(config.initial_query())
                .to_owned();
            workspace.next_id += 1;
            workspace.pending = Some((workspace.next_id, name.to_owned()));
            return Some(Effect::PrepareProfile {
                id: workspace.next_id,
                name: name.to_owned(),
                query,
                page_size: config.table.page_size.unwrap_or_else(|| {
                    crate::layout::workflow_rows(ratatui::layout::Rect::new(
                        0,
                        0,
                        workspace.width,
                        workspace.height,
                    ))
                    .clamp(1, 1000) as u16
                }),
            });
        }
    }
    None
}

pub fn input(key: KeyEvent) -> Option<app::Message> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
        return Some(app::Message::Action(Action::Quit));
    }
    let message = match key.code {
        KeyCode::Esc => Message::Cancel,
        KeyCode::Enter => Message::Activate,
        KeyCode::Up => Message::Move(-1),
        KeyCode::Down => Message::Move(1),
        KeyCode::Char('k') if key.modifiers == KeyModifiers::CONTROL => Message::Move(-1),
        KeyCode::Char('j') if key.modifiers == KeyModifiers::CONTROL => Message::Move(1),
        _ => Message::Edit(crate::input::edit(key)?),
    };
    Some(app::Message::Profiles(message))
}

pub fn view(frame: &mut Frame, workspace: &Workspace, config: &Config) {
    crate::view::view(frame, &workspace.active, config);
    picker_view(frame, workspace, config);
    // Match input dispatch: errors take precedence over the picker and app focus.
    let context = if workspace.active.error.is_none() && workspace.picker.is_some() {
        if workspace.pending.is_some() {
            crate::footer::Context::Connecting
        } else {
            crate::footer::Context::Profiles
        }
    } else {
        crate::footer::Context::for_model(&workspace.active)
    };
    crate::footer::view(frame, context, &config.keybindings, &config.theme.palette());
}

fn picker_view(frame: &mut Frame, workspace: &Workspace, config: &Config) {
    let Some(picker) = &workspace.picker else {
        return;
    };
    let palette = config.theme.palette();
    let [area, _] = crate::layout::screen(frame.area());
    let [_, area, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(16.min(area.height)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, area, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(80.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(Clear, area);
    let block = palette.block().title("Namespaces / profiles");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [search, list, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);
    let (text, cursor) = picker.search.visible(search.width);
    frame.render_widget(Paragraph::new(text).style(palette.surface()), search);
    let names = workspace.matches(config);
    let items: Vec<_> = names
        .iter()
        .map(|name| {
            ListItem::new(format!(
                "{}{} · {}",
                if *name == workspace.active.profile {
                    "● "
                } else {
                    "  "
                },
                name,
                config.profiles[*name].namespace
            ))
        })
        .collect();
    let mut state =
        ListState::default().with_selected((!names.is_empty()).then_some(picker.selected));
    frame.render_stateful_widget(
        List::new(items)
            .style(palette.base())
            .highlight_style(palette.selection()),
        list,
        &mut state,
    );
    let status_text = if let Some((_, name)) = &workspace.pending {
        format!("Connecting to {name}…")
    } else if names.is_empty() {
        "No matching profiles".into()
    } else {
        String::new()
    };
    frame.render_widget(Paragraph::new(status_text).style(palette.muted()), status);
    if workspace.pending.is_none()
        && workspace.active.error.is_none()
        && search.width > 0
        && search.height > 0
    {
        frame.set_cursor_position((search.x + cursor, search.y));
    }
    if let Some(error) = &workspace.active.error {
        error_popup::view(frame, error, &palette);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        details,
        workflow::{Execution, WorkflowRow},
    };

    fn config() -> Config {
        let mut config = Config::default();
        config.profiles.insert(
            "staging".into(),
            crate::config::Connection {
                namespace: "staging.account".into(),
                tls: false,
                ..Default::default()
            },
        );
        config
    }

    fn rendered_lines(
        workspace: &Workspace,
        config: &Config,
        width: u16,
        height: u16,
    ) -> Vec<String> {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| view(frame, workspace, config))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn single_global_footer_tracks_panes_dialogs_and_overlay_precedence() {
        let config = config();
        let mut workspace = Workspace::new(&config, false);
        workspace.active.rows = vec![WorkflowRow {
            execution: Some(Execution {
                workflow_id: "order".into(),
                run_id: "run".into(),
            }),
            overview: "Status: Running".into(),
            ..Default::default()
        }];
        update(
            &mut workspace,
            &config,
            app::Message::Resize {
                width: 160,
                height: 24,
            },
        );
        let lines = rendered_lines(&workspace, &config, 160, 24);
        assert!(lines[23].contains("Enter details"));
        assert!(lines[23].contains("a toggle auto-refresh"));
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("Enter details"))
                .count(),
            1
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("Enter focuses details"))
        );
        // The footer is not constrained to either pane's width.
        let (left, _) = crate::layout::panes(ratatui::layout::Rect::new(0, 0, 160, 24));
        assert!(
            !lines[23]
                .chars()
                .skip(usize::from(left.width))
                .collect::<String>()
                .trim()
                .is_empty()
        );

        update(
            &mut workspace,
            &config,
            app::Message::Action(Action::OpenDetails),
        );
        let lines = rendered_lines(&workspace, &config, 160, 24);
        assert!(lines[23].contains("Esc back"));
        assert!(!lines[23].contains("Enter event"));
        assert!(!lines[23].contains("headers"));
        workspace.active.details.as_mut().unwrap().tab = details::Tab::History;
        assert!(rendered_lines(&workspace, &config, 160, 24)[23].contains("Enter event"));
        workspace.active.details.as_mut().unwrap().event_open = true;
        assert!(rendered_lines(&workspace, &config, 160, 24)[23].contains("Esc events"));

        update(
            &mut workspace,
            &config,
            app::Message::Resize {
                width: 80,
                height: 24,
            },
        );
        assert!(rendered_lines(&workspace, &config, 80, 24)[23].contains("Esc events"));
        workspace.active.details_focused = false;
        workspace.active.query.focus = crate::query::Focus::Input;
        assert!(rendered_lines(&workspace, &config, 80, 24)[23].contains("Enter apply"));
        workspace.active.query.dialog = Some(crate::query::Dialog::Load { selected: 0 });
        let lines = rendered_lines(&workspace, &config, 80, 24);
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("Enter load"))
                .count(),
            1
        );
        assert!(lines[23].contains("Enter load"));

        update(
            &mut workspace,
            &config,
            app::Message::Action(Action::Profiles),
        );
        assert!(rendered_lines(&workspace, &config, 80, 24)[23].contains("Enter switch"));
        workspace.pending = Some((1, "staging".into()));
        assert!(rendered_lines(&workspace, &config, 80, 24)[23].contains("Esc cancel"));
        workspace.active.error = Some(error_popup::Popup::new("failed".into()));
        let lines = rendered_lines(&workspace, &config, 80, 24);
        assert!(lines[23].contains("Enter/Esc close"));
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("Enter/Esc close"))
                .count(),
            1
        );
        for (width, height) in [(5, 3), (1, 1), (0, 0)] {
            rendered_lines(&workspace, &config, width, height);
        }
    }

    fn page(label: &str) -> Page {
        Page {
            rows: (0..4)
                .map(|index| WorkflowRow {
                    execution: Some(Execution {
                        workflow_id: format!("workflow-{index}"),
                        run_id: "same-run".into(),
                    }),
                    cells: vec![format!("{label}-{index}")],
                    overview: label.into(),
                })
                .collect(),
            next_page_token: Vec::new(),
        }
    }

    fn send(workspace: &mut Workspace, config: &Config, message: Message) -> Option<Effect> {
        update(workspace, config, app::Message::Profiles(message))
    }

    fn prepare(workspace: &mut Workspace, config: &Config, name: &str) -> (u64, String) {
        update(workspace, config, app::Message::Action(Action::Profiles));
        send(workspace, config, Message::Edit(Edit::Insert(name.into())));
        let Some(Effect::PrepareProfile { id, query, .. }) =
            send(workspace, config, Message::Activate)
        else {
            panic!("expected preparation")
        };
        (id, query)
    }

    #[test]
    fn cell_scrolling_pauses_in_picker_and_restarts_after_profile_switches() {
        let config = config();
        let mut workspace = Workspace::new(&config, false);
        let long_page = || page("long-workflow-name-with-an-overflowing-tail");
        workspace.active.initial_page(long_page());
        for _ in 0..15 {
            update(&mut workspace, &config, app::Message::AnimateCell);
        }
        let offset = workspace.active.cell_scroll.offset();
        assert!(offset > 0);
        update(
            &mut workspace,
            &config,
            app::Message::Action(Action::Profiles),
        );
        for _ in 0..20 {
            update(&mut workspace, &config, app::Message::AnimateCell);
        }
        assert_eq!(workspace.active.cell_scroll.offset(), offset);
        send(&mut workspace, &config, Message::Cancel);
        update(&mut workspace, &config, app::Message::AnimateCell);
        assert!(workspace.active.cell_scrolling());
        for name in ["staging", "development"] {
            let (id, _) = prepare(&mut workspace, &config, name);
            for _ in 0..20 {
                update(&mut workspace, &config, app::Message::AnimateCell);
            }
            send(
                &mut workspace,
                &config,
                Message::Prepared {
                    id,
                    result: Ok(long_page()),
                },
            );
            assert_eq!(workspace.active.cell_scroll.offset(), 0);
            for _ in 0..10 {
                update(&mut workspace, &config, app::Message::AnimateCell);
            }
            assert!(workspace.active.cell_scroll.offset() > 0);
        }
    }

    #[test]
    fn polling_pauses_in_picker_and_toggle_state_follows_profile_switches() {
        let config = config();
        let mut workspace = Workspace::new(&config, false);
        update(
            &mut workspace,
            &config,
            app::Message::Action(Action::Profiles),
        );
        assert!(update(&mut workspace, &config, app::Message::AutoRefresh).is_none());
        let (id, _) = prepare(&mut workspace, &config, "staging");
        assert!(update(&mut workspace, &config, app::Message::AutoRefresh).is_none());
        update(
            &mut workspace,
            &config,
            app::Message::Action(Action::ToggleAutoRefresh),
        );
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id,
                result: Ok(page("staging")),
            },
        );
        assert!(!workspace.active.auto_refresh);
        assert!(update(&mut workspace, &config, app::Message::AutoRefresh).is_none());
        update(
            &mut workspace,
            &config,
            app::Message::Action(Action::ToggleAutoRefresh),
        );
        let (id, _) = prepare(&mut workspace, &config, "development");
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id,
                result: Ok(page("development")),
            },
        );
        assert!(workspace.active.auto_refresh);
        assert!(matches!(
            update(&mut workspace, &config, app::Message::AutoRefresh),
            Some(Effect::LoadPage { .. })
        ));
    }

    #[test]
    fn failure_and_cancel_preserve_the_view_and_reject_late_preparations() {
        let config = config();
        let mut workspace = Workspace::new(&config, false);
        workspace.active.initial_page(page("original"));
        workspace.active.selected = 2;
        workspace.active.query.input = TextInput::new("unfinished draft");
        let (id, _) = prepare(&mut workspace, &config, "staging");
        assert_eq!(workspace.active.profile, "development");
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id,
                result: Err("permission denied".into()),
            },
        );
        assert_eq!(workspace.epoch, 0);
        assert_eq!(workspace.active.selected, 2);
        assert_eq!(workspace.active.rows[0].cells[0], "original-0");
        assert_eq!(workspace.active.query.input.text, "unfinished draft");
        assert!(workspace.active.error.is_some());
        update(
            &mut workspace,
            &config,
            app::Message::Error(error_popup::Message::Close),
        );
        let Some(Effect::PrepareProfile { id, .. }) =
            send(&mut workspace, &config, Message::Activate)
        else {
            panic!()
        };
        assert_eq!(
            send(&mut workspace, &config, Message::Cancel),
            Some(Effect::CancelSwitch)
        );
        let (new_id, _) = prepare(&mut workspace, &config, "staging");
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id,
                result: Ok(page("late")),
            },
        );
        assert!(workspace.accepts(new_id));
        assert_eq!(workspace.active.profile, "development");
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id: new_id,
                result: Ok(page("staging")),
            },
        );
        assert_eq!(workspace.active.profile, "staging");
        assert_eq!(workspace.active.rows[0].cells[0], "staging-0");
    }

    #[test]
    fn return_restores_query_position_and_profile_specific_metadata() {
        let config = config();
        let mut workspace = Workspace::new(&config, false);
        update(
            &mut workspace,
            &config,
            app::Message::Resize {
                width: 160,
                height: 10,
            },
        );
        workspace.active.initial_page(page("development"));
        update(&mut workspace, &config, app::Message::Action(Action::Down));
        update(&mut workspace, &config, app::Message::Action(Action::Down));
        workspace.active.selected_column = 2;
        workspace.active.query.applied = "WorkflowId = 'remembered'".into();
        workspace.active.query.input = TextInput::new("draft");
        let generation = workspace
            .active
            .details
            .as_ref()
            .unwrap()
            .request
            .generation;
        update(
            &mut workspace,
            &config,
            app::Message::Details(details::Message::Loaded {
                generation,
                result: Ok(details::Content::Overview(details::Snapshot {
                    overview: "private dev metadata".into(),
                })),
            }),
        );
        let (id, _) = prepare(&mut workspace, &config, "staging");
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id,
                result: Ok(page("staging")),
            },
        );
        update(&mut workspace, &config, app::Message::Action(Action::Down));
        update(&mut workspace, &config, app::Message::Action(Action::Down));
        assert_eq!(workspace.active.details.as_ref().unwrap().text(), "staging");
        let (id, query) = prepare(&mut workspace, &config, "development");
        assert_eq!(query, "WorkflowId = 'remembered'");
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id,
                result: Ok(page("validation only")),
            },
        );
        assert_eq!(workspace.epoch, 2);
        assert_eq!(workspace.active.selected, 2);
        assert_eq!(workspace.active.row_offset, 1);
        assert_eq!(workspace.active.selected_column, 2);
        assert_eq!(workspace.active.rows[0].cells[0], "development-0");
        assert_eq!(workspace.active.query.input.text, "draft");
        assert_eq!(
            workspace.active.details.as_ref().unwrap().text(),
            "private dev metadata"
        );
    }

    #[test]
    fn interrupted_query_does_not_poison_pagination_on_return() {
        let config = config();
        let mut workspace = Workspace::new(&config, false);
        update(
            &mut workspace,
            &config,
            app::Message::Resize {
                width: 80,
                height: 12,
            },
        );
        let mut first = page("original");
        first.next_page_token = vec![42];
        workspace.active.initial_page(first);
        workspace.active.query.input = TextInput::new("new query");
        workspace.active.query.focus = crate::query::Focus::Input;
        update(
            &mut workspace,
            &config,
            app::Message::Query(crate::query::Message::Activate),
        );
        let (id, _) = prepare(&mut workspace, &config, "staging");
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id,
                result: Ok(page("staging")),
            },
        );
        let (id, query) = prepare(&mut workspace, &config, "development");
        assert!(query.is_empty());
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id,
                result: Ok(page("validation")),
            },
        );
        // A late pre-switch response must not replace the cached table, even after returning.
        update(
            &mut workspace,
            &config,
            app::Message::Loaded {
                generation: 1,
                result: Ok(page("stale")),
            },
        );
        assert_eq!(workspace.active.rows[0].cells[0], "original-0");
        update(&mut workspace, &config, app::Message::Action(Action::Down));
        let effect = update(&mut workspace, &config, app::Message::Action(Action::Down));
        assert!(
            matches!(effect, Some(Effect::LoadPage { token, query, .. }) if token == [42] && query.is_empty())
        );
    }

    #[test]
    fn return_initializes_an_interrupted_first_load_but_preserves_a_cached_empty_page() {
        let config = config();
        for completed in [false, true] {
            let mut workspace = Workspace::new(&config, false);
            update(
                &mut workspace,
                &config,
                app::Message::Action(Action::Refresh),
            );
            if completed {
                workspace.active.initial_page(Page {
                    rows: Vec::new(),
                    next_page_token: Vec::new(),
                });
            }
            let (id, _) = prepare(&mut workspace, &config, "staging");
            send(
                &mut workspace,
                &config,
                Message::Prepared {
                    id,
                    result: Ok(page("staging")),
                },
            );
            let (id, _) = prepare(&mut workspace, &config, "development");
            send(
                &mut workspace,
                &config,
                Message::Prepared {
                    id,
                    result: Ok(page("validated")),
                },
            );
            assert_eq!(workspace.active.rows.len(), if completed { 0 } else { 4 });
            assert!(workspace.active.has_cached_page());
        }
    }

    #[test]
    fn saved_queries_remain_global_when_a_save_finishes_after_switching() {
        let config = config();
        let mut workspace = Workspace::new(&config, false);
        let (id, _) = prepare(&mut workspace, &config, "staging");
        send(
            &mut workspace,
            &config,
            Message::Prepared {
                id,
                result: Ok(page("staging")),
            },
        );
        let saved = BTreeMap::from([("New".into(), "query".into())]);
        send(
            &mut workspace,
            &config,
            Message::Saved {
                profile: "development".into(),
                name: "New".into(),
                result: Ok(saved.clone()),
            },
        );
        assert_eq!(workspace.active.query.saved, saved);
        assert_eq!(workspace.inactive["development"].query.saved, saved);
        assert!(workspace.inactive["development"].query.dialog.is_none());
    }

    #[test]
    fn picker_searches_namespaces_accepts_text_and_scrolls_in_small_terminals() {
        let config = config();
        let mut workspace = Workspace::new(&config, true);
        update(
            &mut workspace,
            &config,
            app::Message::Action(Action::Profiles),
        );
        for character in "ACCOUNT".chars() {
            update(
                &mut workspace,
                &config,
                input(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)).unwrap(),
            );
        }
        assert_eq!(workspace.matches(&config), ["staging"]);
        send(&mut workspace, &config, Message::Edit(Edit::Clear));
        update(
            &mut workspace,
            &config,
            input(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)).unwrap(),
        );
        assert_eq!(workspace.picker.as_ref().unwrap().selected, 1);
        for (width, height) in [(100, 24), (10, 4), (1, 1)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| view(frame, &workspace, &config))
                .unwrap();
        }
        send(
            &mut workspace,
            &config,
            Message::Edit(Edit::Insert("no matches".into())),
        );
        assert!(send(&mut workspace, &config, Message::Activate).is_none());
        assert_eq!(workspace.active.profile, "development");
    }
}
