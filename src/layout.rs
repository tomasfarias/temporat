use ratatui::layout::{Constraint, Layout, Rect};

pub const SPLIT_MIN_WIDTH: u16 = 140;

pub fn screen(area: Rect) -> [Rect; 2] {
    Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area)
}

pub fn panes(area: Rect) -> (Rect, Option<Rect>) {
    let split = area.width >= SPLIT_MIN_WIDTH && area.height >= 10;
    let [area, _] = screen(area);
    if !split {
        return (area, None);
    }
    let [table, details] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
            .spacing(1)
            .areas(area);
    (table, Some(details))
}

pub fn table(area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .areas(area)
}

pub fn workflow_rows(screen: Rect) -> usize {
    let (pane, _) = panes(screen);
    usize::from(table(pane)[2].height.saturating_sub(3)).max(1)
}

pub fn details(area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_is_outside_panes_and_viewport_sizes_match_widgets() {
        for (width, height) in [(160, 24), (80, 24), (140, 10), (5, 3), (1, 1), (0, 0)] {
            let area = Rect::new(0, 0, width, height);
            let [content, footer] = screen(area);
            let (left, right) = panes(area);
            assert_eq!(content.height + footer.height, height);
            assert_eq!(left.bottom(), footer.y);
            if let Some(right) = right {
                assert_eq!(right.bottom(), footer.y);
            }
            assert_eq!(
                workflow_rows(area),
                usize::from(table(left)[2].height.saturating_sub(3)).max(1)
            );
        }
    }
}
