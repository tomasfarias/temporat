use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, List, ListItem, ListState, Paragraph, Tabs},
};

use crate::{
    details::{Model, Tab},
    input::Keybindings,
};

pub fn view(frame: &mut Frame, model: &Model, keys: &Keybindings) {
    let [status, tabs, body, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let status_text = if model.tab == Tab::History {
        format!(
            "{} | {} events loaded",
            model.status,
            model
                .history
                .as_ref()
                .map(|page| page.events.len())
                .unwrap_or(0)
        )
    } else {
        model.status.clone()
    };
    frame.render_widget(Paragraph::new(status_text), status);
    frame.render_widget(
        Tabs::new(["Overview", "Input / Result", "History"])
            .select(model.tab as usize)
            .highlight_style(Style::default().fg(Color::Cyan)),
        tabs,
    );
    let block = Block::bordered().title(format!(
        "{} · {}",
        model.request.execution.workflow_id, model.request.execution.run_id
    ));
    if model.tab == Tab::History && !model.event_open {
        let items = model
            .history
            .iter()
            .flat_map(|page| &page.events)
            .skip(model.event_offset)
            .take(model.visible_rows)
            .map(|event| ListItem::new(format!("{:>6}  {}  {}", event.id, event.time, event.kind)));
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black));
        let selected = model
            .history
            .as_ref()
            .is_some_and(|page| !page.events.is_empty())
            .then_some(model.event_selected - model.event_offset);
        frame.render_stateful_widget(
            list,
            body,
            &mut ListState::default().with_selected(selected),
        );
    } else {
        let text = model
            .lines()
            .into_iter()
            .skip(model.scroll)
            .take(model.visible_rows)
            .collect::<Vec<_>>()
            .join("\n");
        frame.render_widget(Paragraph::new(text).block(block), body);
    }
    frame.render_widget(
        Paragraph::new(format!(
            "{}/{} scroll  {}/{} tabs  Enter event  {} refresh  Esc back  {} quit",
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
