use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Paragraph, Row, Table, TableState},
};

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

pub fn areas(area: ratatui::layout::Rect) -> [ratatui::layout::Rect; 4] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area)
}

pub fn view(frame: &mut Frame, model: &Model, config: &Config) {
    let [status, query, body, help] = areas(frame.area());
    let editing = model.query.focus == crate::query::Focus::Input;
    crate::query_view::bar(frame, &model.query, query);
    frame.render_widget(
        Paragraph::new(format!(
            "{} | {}",
            config.connection.namespace, model.status
        )),
        status,
    );
    let columns = &model.columns;
    let rows = model
        .rows
        .iter()
        .skip(model.row_offset)
        .take(model.visible_rows)
        .map(|row| Row::new(row.iter().map(String::as_str)));
    let table = Table::new(
        rows,
        columns
            .iter()
            .map(|column| Constraint::Length(column.width())),
    )
    .header(
        Row::new(columns.iter().map(|column| column.title()))
            .style(Style::default().fg(Color::Cyan)),
    )
    .block(Block::bordered().title("Workflows"))
    .row_highlight_style(Style::default().bg(Color::DarkGray))
    .column_highlight_style(Style::default().bg(Color::Blue))
    .cell_highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black));
    // Widget state is local: rendering never changes the application model.
    let mut state = TableState::default()
        .with_selected((!model.rows.is_empty()).then_some(model.selected - model.row_offset))
        .with_selected_column(model.selected_column);
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
            }),
            message,
        );
    }
    frame.render_widget(
        Paragraph::new(if editing {
            "Enter apply  Esc cancel  Ctrl-U clear  Tab focus".into()
        } else {
            config.keybindings.help()
        }),
        help,
    );
    crate::query_view::modal(frame, &model.query, &config.keybindings);
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
        assert_eq!(before[(1, 6)].bg, Color::Cyan);
        assert_eq!(after[(1, 6)].bg, Color::DarkGray);
        assert!(
            after
                .content
                .iter()
                .zip(&before.content)
                .any(|(after, before)| after.bg == Color::Cyan && before.bg != Color::Cyan)
        );
    }

    #[test]
    fn shrinking_hides_cached_rows_and_growing_reveals_them() {
        let config = Config::default();
        let mut model = Model::preview(&config);
        for (height, last_visible) in [(11, "Preview 3"), (9, "Preview 1"), (11, "Preview 3")] {
            crate::app::update(&mut model, crate::app::Message::Resize(height));
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
    fn renders_empty_results_and_survives_tiny_terminals() {
        let config = Config::default();
        let mut model = Model::new(&config, false);
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal.draw(|frame| view(frame, &model, &config)).unwrap();
        assert!(format!("{:?}", terminal.backend().buffer()).contains("No workflows found."));
        for (width, height) in [(0, 0), (1, 1), (5, 3)] {
            crate::app::update(&mut model, crate::app::Message::Resize(height));
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| view(frame, &model, &config)).unwrap();
        }
    }
}
