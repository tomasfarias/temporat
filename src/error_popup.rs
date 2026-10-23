use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Clear, Paragraph},
};

use crate::theme::Palette;

pub struct Popup {
    pub text: String,
    scroll: usize,
}

pub enum Message {
    Scroll(isize),
    Page(isize),
    Close,
}

impl Popup {
    pub fn new(text: String) -> Self {
        Self { text, scroll: 0 }
    }

    fn layout(&self, screen: Rect) -> (Rect, Vec<String>) {
        let [screen, _] = crate::layout::screen(screen);
        let width = screen.width.saturating_sub(4).min(110);
        let lines: Vec<_> = self
            .text
            .lines()
            .flat_map(|line| {
                textwrap::wrap(line, usize::from(width.saturating_sub(2).max(1)))
                    .into_iter()
                    .map(|line| line.into_owned())
            })
            .collect();
        let height = (lines
            .len()
            .saturating_add(2)
            .min(usize::from(screen.height.saturating_sub(4)))) as u16;
        let area = Rect::new(
            screen.x + (screen.width - width) / 2,
            screen.y + (screen.height - height) / 2,
            width,
            height,
        );
        (area, lines)
    }

    pub fn scroll(&mut self, message: Message, screen: Rect) {
        let (area, lines) = self.layout(screen);
        let visible = usize::from(area.height.saturating_sub(2)).max(1);
        let delta = match message {
            Message::Scroll(delta) => delta,
            Message::Page(delta) => delta.saturating_mul(visible as isize),
            Message::Close => 0,
        };
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(lines.len().saturating_sub(visible));
    }
}

pub fn view(frame: &mut Frame, popup: &Popup, palette: &Palette) {
    let (area, lines) = popup.layout(frame.area());
    let visible = usize::from(area.height.saturating_sub(2));
    let start = popup.scroll.min(lines.len().saturating_sub(visible.max(1)));
    let end = (start + visible).min(lines.len());
    let block = palette
        .block()
        .border_style(palette.error())
        .title_style(palette.error())
        .title(format!(
            "Workflow error · {}–{end}/{}",
            (start + 1).min(end),
            lines.len()
        ));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines[start..end].join("\n"))
            .style(palette.base())
            .block(block),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn wraps_and_scrolls_to_the_full_error_then_clamps_on_resize() {
        let mut popup = Popup::new(format!(
            "{}\nLAST CAUSE",
            "long diagnostic message ".repeat(100)
        ));
        let area = Rect::new(0, 0, 60, 14);
        popup.scroll(Message::Scroll(isize::MAX), area);
        assert!(popup.scroll > 0);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| view(frame, &popup, &crate::theme::Theme::default().palette()))
            .unwrap();
        assert!(format!("{:?}", terminal.backend().buffer()).contains("LAST CAUSE"));
        popup.scroll(Message::Scroll(0), Rect::new(0, 0, 200, 200));
        assert_eq!(popup.scroll, 0);
        for (width, height) in [(1, 1), (0, 0)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| view(frame, &popup, &crate::theme::Theme::default().palette()))
                .unwrap();
        }
    }
}
