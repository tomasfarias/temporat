use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    query::{Dialog, Focus, QueryBar},
    theme::Palette,
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

pub fn bar(frame: &mut Frame, model: &QueryBar, area: Rect, show_cursor: bool, palette: &Palette) {
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
        Paragraph::new(text).style(palette.base()).block(
            palette
                .block()
                .title(title)
                .border_style(palette.border(editing)),
        ),
        input,
    );
    frame.render_widget(
        Paragraph::new(" Load ")
            .style(palette.base())
            .block(palette.block()),
        load,
    );
    frame.render_widget(
        Paragraph::new(" Save ")
            .style(palette.base())
            .block(palette.block()),
        save,
    );
    frame.render_widget(
        Paragraph::new(" Clear ")
            .style(palette.base())
            .block(palette.block()),
        clear,
    );
    if show_cursor && editing && inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((inner.x + cursor, inner.y));
    }
}

pub fn modal(frame: &mut Frame, model: &QueryBar, palette: &Palette) {
    if let Some(Dialog::Save {
        name,
        error,
        saving,
    }) = &model.dialog
    {
        let area = popup(crate::layout::screen(frame.area())[0], 7);
        frame.render_widget(Clear, area);
        let block = palette
            .block()
            .border_style(palette.border(true))
            .title("Save query");
        let [input, status] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(block.inner(area));
        frame.render_widget(block, area);
        let inner = input.inner(Margin::new(1, 1));
        let (text, cursor) = name.visible(inner.width);
        frame.render_widget(
            Paragraph::new(text).style(palette.base()).block(
                palette
                    .block()
                    .title("Name")
                    .border_style(palette.border(!saving)),
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
            .style(if error.is_some() {
                palette.error()
            } else {
                palette.muted()
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
    let area = popup(
        crate::layout::screen(frame.area())[0],
        model.saved.len().min(20) as u16 + 4,
    );
    frame.render_widget(Clear, area);
    let block = palette
        .block()
        .border_style(palette.border(true))
        .title("Load query");
    if model.saved.is_empty() {
        frame.render_widget(
            Paragraph::new("No saved queries. Use Save or add [queries] in TOML.")
                .style(palette.base())
                .block(block),
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
            .style(palette.base())
            .block(block)
            .highlight_style(palette.selection());
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
                .draw(|frame| modal(frame, &model, &crate::theme::Theme::default().palette()))
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
                .draw(|frame| modal(frame, &model, &crate::theme::Theme::default().palette()))
                .unwrap();
            if width == 80 {
                assert!(format!("{:?}", terminal.backend().buffer()).contains("disk error"));
            }
        }
    }
}
