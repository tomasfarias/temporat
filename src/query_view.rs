use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    input::Keybindings,
    query::{Dialog, Focus, QueryBar},
};

pub fn controls(area: Rect) -> [Rect; 4] {
    Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(9),
    ])
    .areas(area)
}

pub fn bar(frame: &mut Frame, model: &QueryBar, area: Rect) {
    let [input, load, save, clear] = controls(area);
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
        Paragraph::new(" Load ").block(Block::bordered().border_style(focus_style(false))),
        load,
    );
    frame.render_widget(
        Paragraph::new(" Save ").block(Block::bordered().border_style(focus_style(false))),
        save,
    );
    frame.render_widget(
        Paragraph::new(" Clear ").block(Block::bordered().border_style(focus_style(false))),
        clear,
    );
    if editing && inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((inner.x + cursor, inner.y));
    }
}

fn focus_style(focused: bool) -> Style {
    Style::default().fg(if focused { Color::Cyan } else { Color::Reset })
}

pub fn modal(frame: &mut Frame, model: &QueryBar, keys: &Keybindings) {
    if let Some(Dialog::Save {
        name,
        error,
        saving,
    }) = &model.dialog
    {
        let area = popup(frame.area(), 7);
        frame.render_widget(Clear, area);
        let block = Block::bordered()
            .title("Save query")
            .title_bottom("Enter saves/replaces name · Esc cancels");
        let [input, status] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(block.inner(area));
        frame.render_widget(block, area);
        let inner = input.inner(Margin::new(1, 1));
        let (text, cursor) = name.visible(inner.width);
        frame.render_widget(
            Paragraph::new(text).block(
                Block::bordered()
                    .title("Name")
                    .border_style(focus_style(!saving)),
            ),
            input,
        );
        frame.render_widget(
            Paragraph::new(if *saving {
                "Saving…"
            } else {
                error
                    .as_deref()
                    .unwrap_or("Saves the query text shown in the bar.")
            })
            .wrap(Wrap { trim: false }),
            status,
        );
        if !saving && inner.width > 0 && inner.height > 0 {
            frame.set_cursor_position((inner.x + cursor, inner.y));
        }
        return;
    }
    let Some(Dialog::Load { selected }) = &model.dialog else {
        return;
    };
    let area = popup(frame.area(), model.saved.len().min(20) as u16 + 4);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title("Load query").title_bottom(format!(
        "{}/{} select · Enter load · Esc cancel",
        keys.down, keys.up
    ));
    if model.saved.is_empty() {
        frame.render_widget(
            Paragraph::new("No saved queries. Use Save or add [queries] in TOML.").block(block),
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

fn popup(area: Rect, height: u16) -> Rect {
    let width = area.width.saturating_sub(4).min(100);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
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

    #[test]
    fn save_dialog_renders_errors_and_handles_small_terminals() {
        let mut model = QueryBar::new("", Default::default());
        model.dialog = Some(Dialog::Save {
            name: crate::text_input::TextInput::new("Example"),
            error: Some("disk error".into()),
            saving: false,
        });
        for (width, height) in [(80, 12), (1, 1), (0, 0)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| modal(frame, &model, &Keybindings::default()))
                .unwrap();
            if width == 80 {
                assert!(format!("{:?}", terminal.backend().buffer()).contains("disk error"));
            }
        }
    }
}
