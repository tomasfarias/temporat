use ratatui::{Frame, widgets::Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::Model,
    details::Tab,
    input::Keybindings,
    query::{Dialog, Focus},
    theme::Palette,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Workflows,
    Query,
    LoadQuery,
    SaveQuery,
    SavingQuery,
    Details,
    History,
    Event,
    Profiles,
    Connecting,
    Error,
}

impl Context {
    pub fn for_model(model: &Model) -> Self {
        if model.error.is_some() {
            Self::Error
        } else if model.details_focused {
            match model.details.as_ref() {
                Some(details) if details.tab == Tab::History && details.event_open => Self::Event,
                Some(details) if details.tab == Tab::History => Self::History,
                _ => Self::Details,
            }
        } else {
            match &model.query.dialog {
                Some(Dialog::Load { .. }) => Self::LoadQuery,
                Some(Dialog::Save { saving: true, .. }) => Self::SavingQuery,
                Some(Dialog::Save { .. }) => Self::SaveQuery,
                None if model.query.focus == Focus::Input => Self::Query,
                None => Self::Workflows,
            }
        }
    }

    fn hints(self, keys: &Keybindings) -> Vec<String> {
        let scroll = || format!("{}/{} scroll", keys.down, keys.up);
        let select = || format!("{}/{} select", keys.down, keys.up);
        let quit = || format!("{} quit", keys.quit);
        match self {
            Self::Workflows => vec![
                "Enter details".into(),
                format!("{}/{} rows", keys.down, keys.up),
                format!("{} top", keys.top),
                format!("{}/{} headers", keys.left, keys.right),
                format!("{} query", keys.query),
                format!("{} refresh", keys.refresh),
                quit(),
                format!("{} toggle auto-refresh", keys.auto_refresh),
                format!("{} load", keys.load_query),
                format!("{} save", keys.save_query),
                format!("{} namespaces", keys.profiles),
            ],
            Self::Details | Self::History | Self::Event => {
                let mut hints = vec![
                    if self == Self::Event {
                        "Esc events"
                    } else {
                        "Esc back"
                    }
                    .into(),
                    if self == Self::History {
                        select()
                    } else {
                        scroll()
                    },
                ];
                if self == Self::History {
                    hints.push("Enter event".into());
                }
                hints.extend([
                    format!("{}/{} tabs", keys.left, keys.right),
                    format!("{} list top", keys.top),
                    format!("{} refresh", keys.refresh),
                    quit(),
                    format!("{} toggle auto-refresh", keys.auto_refresh),
                    format!("{} namespaces", keys.profiles),
                ]);
                hints
            }
            Self::Query => vec![
                "Enter apply".into(),
                "Esc cancel".into(),
                "Ctrl+o load".into(),
                "Ctrl+s save".into(),
                "Ctrl+u clear".into(),
                "Ctrl+c quit".into(),
            ],
            Self::LoadQuery => vec![
                "Enter load".into(),
                "Esc cancel".into(),
                select(),
                "Ctrl+c quit".into(),
            ],
            Self::SaveQuery => vec![
                "Enter save/replace".into(),
                "Esc cancel".into(),
                "Ctrl+u clear".into(),
                "Ctrl+c quit".into(),
            ],
            Self::SavingQuery => vec!["Ctrl+c quit".into()],
            Self::Profiles => vec![
                "Enter switch".into(),
                "Esc close".into(),
                "↑/↓ or Ctrl+j/k select".into(),
                "Type to search".into(),
                "Ctrl+c quit".into(),
            ],
            Self::Connecting => vec!["Esc cancel".into(), "Ctrl+c quit".into()],
            Self::Error => vec![
                "Enter/Esc close".into(),
                scroll(),
                "PgUp/PgDn page".into(),
                quit(),
                "Home/End".into(),
            ],
        }
    }

    fn text(self, keys: &Keybindings, width: u16) -> String {
        let hints = self.hints(keys);
        let mut text = String::new();
        for hint in &hints {
            let separator = if text.is_empty() { "" } else { " │ " };
            if text.width() + separator.width() + hint.width() <= usize::from(width) {
                text.push_str(separator);
                text.push_str(hint);
            }
        }
        // Even the primary hint may not fit on a tiny terminal; let the widget clip it.
        if text.is_empty() {
            hints.into_iter().next().unwrap_or_default()
        } else {
            text
        }
    }
}

pub fn view(frame: &mut Frame, context: Context, keys: &Keybindings, palette: &Palette) {
    let [_, area] = crate::layout::screen(frame.area());
    frame.render_widget(
        Paragraph::new(context.text(keys, area.width)).style(palette.muted().bg(palette.surface)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_match_focus_and_only_offer_event_inspection_in_history() {
        let config = crate::config::Config::default();
        let mut model = Model::preview(&config);
        assert_eq!(Context::for_model(&model), Context::Workflows);
        model.query.focus = Focus::Input;
        assert_eq!(Context::for_model(&model), Context::Query);
        model.query.dialog = Some(Dialog::Load { selected: 0 });
        assert_eq!(Context::for_model(&model), Context::LoadQuery);
        model.details_focused = true;
        assert_eq!(Context::for_model(&model), Context::Details);
        model.error = Some(crate::error_popup::Popup::new("error".into()));
        assert_eq!(Context::for_model(&model), Context::Error);
        assert!(
            !Context::Details
                .text(&config.keybindings, 240)
                .contains("Enter event")
        );
        assert!(
            Context::History
                .text(&config.keybindings, 240)
                .contains("Enter event")
        );
        assert!(
            Context::Event
                .text(&config.keybindings, 240)
                .contains("Esc events")
        );
    }

    #[test]
    fn narrow_footer_prioritizes_navigation_and_uses_custom_bindings() {
        let keys = Keybindings {
            down: 'J',
            up: 'K',
            ..Default::default()
        };
        let text = Context::Details.text(&keys, 40);
        assert!(text.contains("Esc back"));
        assert!(text.contains("J/K scroll"));
        assert!(!text.contains("auto-refresh"));
        assert!(text.width() <= 40);
        assert!(!Context::SavingQuery.text(&keys, 80).contains("Esc"));
    }
}
