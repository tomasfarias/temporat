use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Paragraph, Tabs},
};

use crate::{details::Model, input::Keybindings};

pub fn view(frame: &mut Frame, model: &Model, keys: &Keybindings) {
    let [status, tabs, body, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(Paragraph::new(model.status.as_str()), status);
    frame.render_widget(
        Tabs::new(["Overview", "Input / Result"])
            .select(model.tab as usize)
            .highlight_style(Style::default().fg(Color::Cyan)),
        tabs,
    );
    let text = model
        .lines()
        .into_iter()
        .skip(model.scroll)
        .take(model.visible_rows)
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(
        Paragraph::new(text).block(Block::bordered().title(format!(
            "{} · {}",
            model.request.execution.workflow_id, model.request.execution.run_id
        ))),
        body,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{}/{} scroll  {}/{} tabs  {} refresh  Esc back  {} quit",
            keys.down, keys.up, keys.left, keys.right, keys.refresh, keys.quit
        )),
        help,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        details::{Request, Snapshot},
        workflow::Execution,
    };
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_execution_identity_and_survives_tiny_terminals() {
        let mut model = Model::new(
            Request {
                generation: 1,
                execution: Execution {
                    workflow_id: "order".into(),
                    run_id: "old-run".into(),
                },
                kind: crate::details::Kind::Overview,
            },
            80,
            12,
        );
        model.snapshot = Some(Snapshot {
            overview: "Status: Running".into(),
        });
        for (width, height) in [(80, 12), (1, 1), (0, 0)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| view(frame, &model, &Keybindings::default()))
                .unwrap();
            if width == 80 {
                let text = format!("{:?}", terminal.backend().buffer());
                assert!(text.contains("old-run"));
                assert!(text.contains("Status: Running"));
            }
        }
    }
}
