use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};

use crate::{
    input::Keybindings,
    query::{Dialog, Focus, QueryBar},
};

pub fn controls(area: Rect) -> [Rect; 2] {
    Layout::horizontal([Constraint::Min(0), Constraint::Length(8)]).areas(area)
}

pub fn bar(frame: &mut Frame, model: &QueryBar, area: Rect) {
    let [input, load] = controls(area);
    let editing = model.focus == Focus::Input && model.dialog.is_none();
    let inner = input.inner(Margin::new(1, 1));
    let (text, cursor) = model.input.visible(inner.width);
    let title = if model.input.text == model.applied {
        "Query"
    } else {
        "Query (edited)"
    };
    frame.render_widget(
        Paragraph::new(text).block(
            Block::bordered()
                .title(title)
                .border_style(focus_style(editing)),
        ),
        input,
    );
    frame.render_widget(
        Paragraph::new(" Load ")
            .block(Block::bordered().border_style(focus_style(model.focus == Focus::Load))),
        load,
    );
    if editing && inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((inner.x + cursor, inner.y));
    }
}

fn focus_style(focused: bool) -> Style {
    Style::default().fg(if focused { Color::Cyan } else { Color::Reset })
}

pub fn modal(frame: &mut Frame, model: &QueryBar, keys: &Keybindings) {
    let Some(Dialog::Load { selected }) = &model.dialog else {
        return;
    };
    let area = frame.area();
    let width = area.width.saturating_sub(4).min(100);
    let height = (model.saved.len().min(20) as u16 + 4).min(area.height);
    let area = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let block = Block::bordered().title("Load query").title_bottom(format!(
        "{}/{} select · Enter load · Esc cancel",
        keys.down, keys.up
    ));
    if model.saved.is_empty() {
        frame.render_widget(
            Paragraph::new("No saved queries. Add entries under [queries] in TOML.").block(block),
            area,
        );
    } else {
        let items = model.saved.iter().map(|(name, query)| {
            ListItem::new(format!(
                "{name}: {}",
                if query.is_empty() {
                    "(all workflows)"
                } else {
                    query
                }
            ))
        });
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black));
        frame.render_stateful_widget(
            list,
            area,
            &mut ListState::default().with_selected(Some(*selected)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn picker_scrolls_to_selected_query_and_handles_small_terminals() {
        let saved = (0..30)
            .map(|n| (format!("Query {n:02}"), format!("WorkflowId = '{n}'")))
            .collect();
        let mut model = QueryBar::new("", saved);
        model.dialog = Some(Dialog::Load { selected: 29 });
        for (width, height) in [(80, 12), (1, 1), (0, 0)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| modal(frame, &model, &Keybindings::default()))
                .unwrap();
            if width == 80 {
                assert!(format!("{:?}", terminal.backend().buffer()).contains("Query 29"));
            }
        }
    }
}
