use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::{Block, Cell, Paragraph, Row, Table, TableState},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::Model,
    config::{Column, Config},
};

impl Column {
    fn title(self) -> &'static str {
        match self {
            Self::WorkflowId => "Workflow ID",
            Self::RunId => "Run ID",
            Self::WorkflowType => "Type",
            Self::Status => "Status",
            Self::StartTime => "Started (UTC)",
            Self::CloseTime => "Closed (UTC)",
            Self::TaskQueue => "Task queue",
            Self::HistoryLength => "History length",
        }
    }

    fn width(self) -> u16 {
        match self {
            Self::RunId => 36,
            Self::WorkflowId | Self::WorkflowType | Self::TaskQueue => 28,
            Self::StartTime | Self::CloseTime => 20,
            Self::Status | Self::HistoryLength => 16,
        }
    }
}

pub fn view(frame: &mut Frame, model: &Model, config: &Config) {
    let palette = config.theme.palette();
    frame.render_widget(Block::default().style(palette.base()), frame.area());
    let (table_area, detail_area) = crate::layout::panes(frame.area());
    if detail_area.is_none()
        && model.details_focused
        && let Some(details) = &model.details
    {
        crate::details_view::view(frame, details, &palette, table_area, true);
        let label = model.refresh_label();
        let [_, indicator] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(label.width() as u16)])
                .areas(ratatui::layout::Rect {
                    height: frame.area().height.min(1),
                    ..frame.area()
                });
        frame.render_widget(Paragraph::new(label).style(palette.surface()), indicator);
        if let Some(error) = &model.error {
            crate::error_popup::view(frame, error, &palette);
        }
        return;
    }
    let [status, query, body] = crate::layout::table(table_area);
    let label = model.refresh_label();
    let [status, indicator] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(label.width() as u16 + 1),
    ])
    .areas(status);
    frame.render_widget(
        Paragraph::new(format!(" {label}")).style(palette.surface()),
        indicator,
    );
    let editing = !model.details_focused && model.query.focus == crate::query::Focus::Input;
    crate::query_view::bar(
        frame,
        &model.query,
        query,
        model.error.is_none() && !model.details_focused,
        &palette,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} · {} | {}",
            model.profile, config.profiles[&model.profile].namespace, model.status
        ))
        .style(if model.status.starts_with("Error") {
            palette.error()
        } else {
            palette.surface()
        }),
        status,
    );
    let columns = &model.columns;
    let rows = model
        .rows
        .iter()
        .enumerate()
        .skip(model.row_offset)
        .take(model.visible_rows)
        .map(|(index, row)| {
            let cells = row.cells.iter().enumerate().map(|(column, text)| {
                let cell = Cell::from(text.as_str());
                if columns.get(column) == Some(&Column::Status) {
                    cell.style(palette.workflow_status(text))
                } else {
                    cell
                }
            });
            // Style the row before its cells so selection doesn't overwrite status colors.
            Row::new(cells).style(if index == model.selected {
                palette.selection()
            } else {
                palette.base()
            })
        });
    let table = Table::new(
        rows,
        columns
            .iter()
            .map(|column| Constraint::Length(column.width())),
    )
    .header(
        Row::new(columns.iter().enumerate().map(|(index, column)| {
            let cell = Cell::from(column.title());
            if index == model.selected_column {
                cell.style(palette.selected_header())
            } else {
                cell
            }
        }))
        .style(palette.heading()),
    )
    .style(palette.base())
    .block(
        palette
            .block()
            .border_style(palette.border(!model.details_focused && !editing))
            .title("Workflows"),
    );
    // Widget state is local: rendering never changes the application model.
    let mut state = TableState::default()
        .with_selected((!model.rows.is_empty()).then_some(model.selected - model.row_offset));
    frame.render_stateful_widget(table, body, &mut state);
    if model.rows.is_empty() {
        let area = body.inner(ratatui::layout::Margin::new(1, 1));
        let [_, message] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        frame.render_widget(
            Paragraph::new(if model.status.starts_with("Loading") {
                "Loading…"
            } else if model.status.starts_with("Error") {
                "Unable to load workflows; refresh to retry."
            } else {
                "No workflows found."
            })
            .style(palette.muted()),
            message,
        );
    }
    if let Some(area) = detail_area {
        if let Some(details) = &model.details {
            crate::details_view::view(frame, details, &palette, area, model.details_focused);
        } else {
            frame.render_widget(
                Paragraph::new("Select a workflow to preview its execution.")
                    .style(palette.muted())
                    .block(palette.block().title("Execution details")),
                area,
            );
        }
    }
    if let Some(error) = &model.error {
        crate::error_popup::view(frame, error, &palette);
    } else {
        crate::query_view::modal(frame, &model.query, &palette);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn column_navigation_moves_highlight_without_moving_content() {
        let mut config = Config::default();
        config.table.columns = vec![Column::Status, Column::WorkflowId];
        let mut model = Model::preview(&config);
        let mut terminal = Terminal::new(TestBackend::new(80, 13)).unwrap();
        terminal.draw(|frame| view(frame, &model, &config)).unwrap();
        let text = format!("{:?}", terminal.backend().buffer());
        assert!(text.find("Status").unwrap() < text.find("Workflow ID").unwrap());
        let before = terminal.backend().buffer().clone();
        crate::app::update(
            &mut model,
            crate::app::Message::Action(crate::input::Action::Right),
        );
        terminal.draw(|frame| view(frame, &model, &config)).unwrap();
        let text = format!("{:?}", terminal.backend().buffer());
        assert!(text.contains("Status"));
        assert!(text.contains("Workflow ID"));
        let after = terminal.backend().buffer();
        for (before, after) in before.content.iter().zip(&after.content) {
            assert_eq!(before.symbol(), after.symbol());
        }
        let palette = config.theme.palette();
        assert_eq!(before[(1, 5)].bg, palette.surface);
        assert_eq!(before[(1, 5)].fg, palette.accent);
        assert_eq!(after[(1, 5)].bg, palette.background);
        assert_eq!(before[(1, 6)].bg, palette.surface);
        assert_eq!(after[(1, 6)].bg, palette.surface);
        for y in 6..before.area.height {
            for x in 0..before.area.width {
                assert_eq!(before[(x, y)], after[(x, y)]);
            }
        }
        assert!(
            after
                .content
                .iter()
                .zip(&before.content)
                .any(|(after, before)| after.bg == palette.surface && before.bg != palette.surface)
        );
    }

    #[test]
    fn shrinking_hides_cached_rows_and_growing_reveals_them() {
        let config = Config::default();
        let mut model = Model::preview(&config);
        for (height, last_visible) in [(11, "Preview 3"), (9, "Preview 1"), (11, "Preview 3")] {
            crate::app::update(
                &mut model,
                crate::app::Message::Resize { width: 140, height },
            );
            let mut terminal = Terminal::new(TestBackend::new(140, height)).unwrap();
            terminal.draw(|frame| view(frame, &model, &config)).unwrap();
            let text = format!("{:?}", terminal.backend().buffer());
            assert!(text.contains(last_visible));
            assert!(!text.contains("Preview 4"));
            if height == 9 {
                assert!(!text.contains("Preview 2"));
            }
            assert_eq!(model.rows.len(), 25);
            assert_eq!(model.selected, 0);
        }
    }

    #[test]
    fn status_colors_survive_row_selection_in_every_theme() {
        use crate::{theme::Name, workflow::WorkflowRow};
        for name in [
            Name::SolarizedDark,
            Name::SolarizedLight,
            Name::Nord,
            Name::Terminal,
        ] {
            let mut config = Config::default();
            config.theme.name = name;
            config.table.columns = vec![Column::WorkflowId, Column::Status];
            let palette = config.theme.palette();
            let statuses = [
                ("Completed", palette.success),
                ("Continued as new", palette.success),
                ("Running", palette.running),
                ("Timed out", palette.error),
                ("Failed", palette.error),
                ("Canceled", palette.canceled),
                ("Terminated", palette.warning),
                ("Unknown (999)", palette.foreground),
            ];
            let mut model = Model::preview(&config);
            model.visible_rows = statuses.len();
            model.rows = statuses
                .iter()
                .map(|(status, _)| WorkflowRow {
                    cells: vec!["Failed".into(), (*status).into()],
                    ..Default::default()
                })
                .collect();
            let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
            for selected in 0..statuses.len() {
                model.selected = selected;
                terminal.draw(|frame| view(frame, &model, &config)).unwrap();
                let buffer = terminal.backend().buffer();
                for (index, (_, color)) in statuses.iter().enumerate() {
                    let y = 6 + index as u16;
                    assert_eq!(buffer[(30, y)].fg, *color);
                    assert_eq!(
                        buffer[(30, y)].bg,
                        if index == selected {
                            palette.surface
                        } else {
                            palette.background
                        }
                    );
                    // A workflow ID that happens to be a status name stays neutral.
                    assert_eq!(
                        buffer[(1, y)].fg,
                        if index == selected {
                            palette.selected_text
                        } else {
                            palette.foreground
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn renders_empty_results_and_survives_tiny_terminals() {
        let config = Config::default();
        let mut model = Model::new(&config, false);
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal.draw(|frame| view(frame, &model, &config)).unwrap();
        assert!(format!("{:?}", terminal.backend().buffer()).contains("No workflows found."));
        for (width, height) in [(0, 0), (1, 1), (5, 3)] {
            crate::app::update(&mut model, crate::app::Message::Resize { width, height });
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| view(frame, &model, &config)).unwrap();
        }
    }

    #[test]
    fn wide_preview_stays_in_the_right_pane_and_keeps_query_controls_on_the_left() {
        let mut config = Config::default();
        config.table.columns = vec![Column::WorkflowId, Column::Status];
        let mut model = Model::new(&config, false);
        model.rows = vec![crate::workflow::WorkflowRow {
            execution: Some(crate::workflow::Execution {
                workflow_id: "preview-order".into(),
                run_id: "exact-run".into(),
            }),
            cells: vec!["preview-order".into(), "Running".into()],
            overview: "Workflow ID: preview-order\nStatus: Running".into(),
        }];
        crate::app::update(
            &mut model,
            crate::app::Message::Resize {
                width: 160,
                height: 24,
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(160, 24)).unwrap();
        terminal.draw(|frame| view(frame, &model, &config)).unwrap();
        let (left, right) = crate::layout::panes(ratatui::layout::Rect::new(0, 0, 160, 24));
        let right = right.unwrap();
        let buffer = terminal.backend().buffer();
        let row = |area: ratatui::layout::Rect, y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        assert!(row(left, 1).contains("Query"));
        assert!(row(left, 2).contains("Clear"));
        assert!(row(left, 6).contains("preview-order"));
        assert!(row(right, 3).contains("Workflow ID: preview-order"));
        assert!(row(right, 23).trim().is_empty());
        assert_eq!(buffer[(left.right(), 6)].symbol(), " ");
    }
}
