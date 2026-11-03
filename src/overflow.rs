use std::time::Duration;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::workflow::Execution;

pub const TICK: Duration = Duration::from_millis(100);
const WAIT: Duration = Duration::from_secs(1);
const STEP: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Scroll {
    target: Option<Target>,
    offset: usize,
    elapsed: Duration,
}

struct Target {
    row: usize,
    column: usize,
    execution: Option<Execution>,
    text: String,
    width: u16,
}

impl Scroll {
    pub fn sync(
        &mut self,
        row: usize,
        column: usize,
        execution: Option<&Execution>,
        text: &str,
        width: u16,
    ) {
        // Very small cells have no room for both a marker and a wide grapheme.
        if width < 3 || text.width() <= usize::from(width) {
            *self = Self::default();
            return;
        }
        if self.target.as_ref().is_some_and(|target| {
            target.row == row
                && target.column == column
                && target.execution.as_ref() == execution
                && target.text == text
                && target.width == width
        }) {
            return;
        }
        *self = Self {
            target: Some(Target {
                row,
                column,
                execution: execution.cloned(),
                text: text.to_owned(),
                width,
            }),
            ..Self::default()
        };
    }

    pub fn active(&self) -> bool {
        self.target.is_some()
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn tick(&mut self) {
        let Some(target) = &self.target else { return };
        let tail = &target.text[self.offset..];
        let left = leading_marker(tail, usize::from(target.width), self.offset);
        let at_end = self.offset > 0 && tail.width() <= usize::from(target.width) - left;
        let delay = if self.offset == 0 || at_end {
            WAIT
        } else {
            STEP
        };
        self.elapsed += TICK;
        if self.elapsed < delay {
            return;
        }
        self.elapsed -= delay;
        if at_end {
            self.offset = 0;
        } else if let Some(grapheme) = target.text[self.offset..].graphemes(true).next() {
            self.offset += grapheme.len();
        }
    }
}

/// A single-line viewport, with ellipses wherever text is hidden. Offsets are
/// grapheme boundaries, so combining marks and emoji sequences stay together.
pub fn cell(text: &str, width: u16, offset: usize) -> String {
    let width = usize::from(width);
    if width == 0 {
        return String::new();
    }
    let text = &text[offset..];
    let left = leading_marker(text, width, offset);
    let right = usize::from(text.width() > width.saturating_sub(left));
    let available = width.saturating_sub(left + right);
    let mut result = String::new();
    if left > 0 {
        result.push('…');
    }
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let size = grapheme.width();
        if used + size > available {
            break;
        }
        result.push_str(grapheme);
        used += size;
    }
    if right > 0 && left < width {
        // Keep the marker at the edge even when a wide grapheme did not fit.
        result.extend(std::iter::repeat_n(' ', available - used));
        result.push('…');
    }
    result
}

fn leading_marker(text: &str, width: usize, offset: usize) -> usize {
    // In a narrow cell, prioritize the next whole grapheme and trailing marker.
    usize::from(
        offset > 0
            && text
                .graphemes(true)
                .next()
                .is_some_and(|g| g.width() + 2 <= width),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{self, Effect, Message, Model},
        config::{Column, Config},
        input::Action,
        query::{self, Focus},
        workflow::{Page, WorkflowRow},
    };

    #[test]
    fn markers_use_display_width_and_preserve_whole_graphemes() {
        for (text, width, expected) in [
            ("abcdef", 0, ""),
            ("abcdef", 1, "…"),
            ("abcdef", 2, "a…"),
            ("abcdef", 6, "abcdef"),
            ("abcdef", 5, "abcd…"),
            ("界界界", 4, "界 …"),
            ("e\u{301}e\u{301}e\u{301}", 2, "e\u{301}…"),
            ("👩‍💻👩‍💻👩‍💻", 4, "👩‍💻 …"),
        ] {
            assert_eq!(cell(text, width, 0), expected);
        }
        assert_eq!(cell("abcdef", 4, 1), "…bc…");
        assert_eq!(cell("abcdef", 4, 3), "…def");
        assert_eq!(cell("界天地", 3, "界".len()), "天…");
    }

    #[test]
    fn scroll_waits_at_both_ends_and_exposes_every_grapheme() {
        for text in [
            "abcdefgh",
            "界界界界",
            "e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}",
            "👩‍💻👨‍💻👩‍🚀",
        ] {
            let mut scroll = Scroll::default();
            scroll.sync(0, 0, None, text, 4);
            for _ in 0..9 {
                scroll.tick();
            }
            assert_eq!(scroll.offset(), 0);
            scroll.tick();
            assert!(scroll.offset() > 0);
            let mut seen = cell(text, 4, 0);
            for _ in 0..100 {
                let displayed = cell(text, 4, scroll.offset());
                assert!(displayed.width() <= 4);
                seen.push_str(&displayed);
                if text[scroll.offset()..].width() < 4 {
                    let end = scroll.offset();
                    for _ in 0..9 {
                        scroll.tick();
                    }
                    assert_eq!(scroll.offset(), end);
                    scroll.tick();
                    assert_eq!(scroll.offset(), 0);
                    break;
                }
                scroll.tick();
            }
            assert_eq!(scroll.offset(), 0, "must complete the cycle");
            for grapheme in text.graphemes(true) {
                assert!(seen.contains(grapheme), "missing {grapheme} from {seen}");
            }
        }
    }

    fn row(run: &str) -> WorkflowRow {
        WorkflowRow {
            execution: Some(Execution {
                workflow_id: "workflow".into(),
                run_id: run.into(),
            }),
            cells: vec!["a-long-workflow-id-with-a-distinct-tail".into(); 2],
            ..Default::default()
        }
    }

    fn model(config: &Config) -> Model {
        let mut model = Model::new(config, false);
        app::update(
            &mut model,
            Message::Resize {
                width: 100,
                height: 12,
            },
        );
        model.initial_page(Page {
            rows: vec![row("run-1"), row("run-2")],
            next_page_token: Vec::new(),
        });
        model
    }

    fn tick(model: &mut Model, count: usize) {
        for _ in 0..count {
            app::update(model, Message::AnimateCell);
        }
    }

    #[test]
    fn navigation_content_and_resize_reset_but_unchanged_refresh_preserves_progress() {
        let mut config = Config::default();
        config.table.columns = vec![Column::WorkflowId, Column::WorkflowType];
        let mut model = model(&config);
        tick(&mut model, 15);
        let offset = model.cell_scroll.offset();
        assert!(offset > 0);
        let Some(Effect::LoadPage { generation, .. }) =
            app::update(&mut model, Message::AutoRefresh)
        else {
            panic!()
        };
        app::update(
            &mut model,
            Message::Loaded {
                generation,
                result: Ok(Page {
                    rows: vec![row("run-1"), row("run-2")],
                    next_page_token: Vec::new(),
                }),
            },
        );
        assert_eq!(model.cell_scroll.offset(), offset);
        for action in [Action::Down, Action::Up, Action::Right, Action::Left] {
            app::update(&mut model, Message::Action(action));
            assert_eq!(model.cell_scroll.offset(), 0);
            tick(&mut model, 10);
            assert!(model.cell_scroll.offset() > 0);
        }
        model.rows[0].cells[0].push_str("-changed");
        app::update(&mut model, Message::RefreshCountdown(10));
        assert_eq!(model.cell_scroll.offset(), 0);
        tick(&mut model, 10);
        model.rows[0].execution.as_mut().unwrap().run_id = "new-run".into();
        app::update(&mut model, Message::RefreshCountdown(9));
        assert_eq!(model.cell_scroll.offset(), 0);
        tick(&mut model, 10);
        app::update(
            &mut model,
            Message::Resize {
                width: 30,
                height: 12,
            },
        );
        assert_eq!(model.cell_scroll.offset(), 0);
        tick(&mut model, 10);
        assert!(model.cell_scroll.active());
        model.suspend();
        assert!(!model.cell_scroll.active());
    }

    #[test]
    fn editing_dialogs_errors_and_focused_details_pause_without_losing_progress() {
        let mut model = model(&Config::default());
        tick(&mut model, 15);
        let offset = model.cell_scroll.offset();
        assert!(offset > 0);
        app::update(&mut model, Message::Action(Action::Query));
        tick(&mut model, 20);
        assert_eq!(model.cell_scroll.offset(), offset);
        app::update(
            &mut model,
            Message::Query(query::Message::Focus(Focus::Table)),
        );
        app::update(&mut model, Message::Action(Action::LoadQuery));
        tick(&mut model, 20);
        assert_eq!(model.cell_scroll.offset(), offset);
        model.query.dialog = None;
        model.error = Some(crate::error_popup::Popup::new("test".into()));
        tick(&mut model, 20);
        assert_eq!(model.cell_scroll.offset(), offset);
        app::update(
            &mut model,
            Message::Error(crate::error_popup::Message::Close),
        );
        app::update(&mut model, Message::Action(Action::OpenDetails));
        assert!(model.details_focused);
        tick(&mut model, 20);
        assert_eq!(model.cell_scroll.offset(), offset);
        app::update(
            &mut model,
            Message::Details(crate::details::Message::Action(Action::Back)),
        );
        tick(&mut model, 3);
        assert!(model.cell_scroll.offset() > offset);
    }

    #[test]
    fn disabled_scrolling_keeps_markers_and_no_animation_work() {
        let mut config = Config::default();
        config.table.overflow.scroll_selected = false;
        let mut model = model(&config);
        tick(&mut model, 50);
        assert!(!model.cell_scrolling());
        assert_eq!(model.cell_scroll.offset(), 0);
        assert!(cell(&model.rows[0].cells[0], 10, model.cell_scroll.offset()).ends_with('…'));
    }
}
