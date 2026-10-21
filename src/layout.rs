use ratatui::layout::{Constraint, Layout, Rect};

pub const SPLIT_MIN_WIDTH: u16 = 140;

pub fn panes(area: Rect) -> (Rect, Option<Rect>) {
    if area.width < SPLIT_MIN_WIDTH || area.height < 10 {
        return (area, None);
    }
    let [table, details] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
            .spacing(1)
            .areas(area);
    (table, Some(details))
}
