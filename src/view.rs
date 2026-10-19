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

pub fn view(frame: &mut Frame, model: &Model, config: &Config) {
    let [status, body, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Paragraph::new(format!(
            "{} | {}",
            config.connection.namespace, model.status
        )),
        status,
    );
    let columns = &model.columns[model.column_offset..];
    let rows = model
        .rows
        .iter()
        .skip(model.row_offset)
        .take(model.visible_rows)
        .map(|row| Row::new(row.iter().skip(model.column_offset).map(String::as_str)));
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
    .row_highlight_style(Style::default().bg(Color::DarkGray));
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
            }),
            message,
        );
    }
    frame.render_widget(Paragraph::new(config.keybindings.help()), help);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_configured_columns_in_order_and_scrolls_horizontally() {
        let mut config = Config::default();
        config.table.columns = vec![Column::Status, Column::WorkflowId];
        let mut model = Model::preview(&config);
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal.draw(|frame| view(frame, &model, &config)).unwrap();
        let text = format!("{:?}", terminal.backend().buffer());
        assert!(text.find("Status").unwrap() < text.find("Workflow ID").unwrap());
        crate::app::update(
            &mut model,
            crate::app::Message::Action(crate::input::Action::Right),
        );
        terminal.draw(|frame| view(frame, &model, &config)).unwrap();
        let text = format!("{:?}", terminal.backend().buffer());
        assert!(!text.contains("Status"));
        assert!(text.contains("Workflow ID"));
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
