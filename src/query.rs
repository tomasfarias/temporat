use crate::text_input::{Edit, TextInput};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Table,
    Input,
    Load,
}

pub struct QueryBar {
    pub input: TextInput,
    pub applied: String,
    pub focus: Focus,
    pub saved: BTreeMap<String, String>,
    pub dialog: Option<Dialog>,
    pub active_name: Option<String>,
}

pub enum Dialog {
    Load { selected: usize },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Apply,
}

pub enum Message {
    Focus(Focus),
    CycleFocus(bool),
    Edit(Edit),
    Activate,
    Cancel,
    OpenLoad,
    MoveSelection(isize),
}

impl QueryBar {
    pub fn new(query: &str, saved: BTreeMap<String, String>) -> Self {
        Self {
            input: TextInput::new(query),
            applied: query.into(),
            focus: Focus::Table,
            saved,
            dialog: None,
            active_name: None,
        }
    }
}

pub fn update(model: &mut QueryBar, message: Message) -> Option<Command> {
    match message {
        Message::Focus(focus) if model.dialog.is_none() => model.focus = focus,
        Message::CycleFocus(backwards) if model.dialog.is_none() => {
            let order = [Focus::Table, Focus::Input, Focus::Load];
            let index = order
                .iter()
                .position(|focus| *focus == model.focus)
                .unwrap();
            model.focus =
                order[(index + if backwards { order.len() - 1 } else { 1 }) % order.len()];
        }
        Message::OpenLoad => {
            let selected = model
                .active_name
                .as_ref()
                .and_then(|name| model.saved.keys().position(|key| key == name))
                .unwrap_or(0);
            model.dialog = Some(Dialog::Load { selected });
        }
        Message::MoveSelection(delta) => {
            if let Some(Dialog::Load { selected }) = &mut model.dialog {
                *selected = selected
                    .saturating_add_signed(delta)
                    .min(model.saved.len().saturating_sub(1));
            }
        }
        Message::Activate if model.dialog.is_some() => {
            let Some(Dialog::Load { selected }) = model.dialog else {
                return None;
            };
            if let Some((name, query)) = model.saved.iter().nth(selected) {
                model.active_name = Some(name.clone());
                model.input = TextInput::new(query);
                model.applied = query.clone();
                model.dialog = None;
                model.focus = Focus::Table;
                return Some(Command::Apply);
            }
        }
        Message::Edit(edit) if model.dialog.is_none() => model.input.update(edit),
        Message::Activate if model.focus == Focus::Input => {
            model.applied = model.input.text.clone();
            model.focus = Focus::Table;
            return Some(Command::Apply);
        }
        Message::Activate if model.focus == Focus::Load => return update(model, Message::OpenLoad),
        Message::Cancel if model.dialog.is_some() => model.dialog = None,
        Message::Cancel => {
            model.input = TextInput::new(&model.applied);
            model.focus = Focus::Table;
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_restores_applied_query_and_empty_query_can_be_submitted() {
        let mut model = QueryBar::new("WorkflowType = 'Test'", BTreeMap::new());
        update(&mut model, Message::Focus(Focus::Input));
        update(&mut model, Message::Edit(Edit::Clear));
        update(&mut model, Message::Cancel);
        assert_eq!(model.input.text, model.applied);
        update(&mut model, Message::Focus(Focus::Input));
        update(&mut model, Message::Edit(Edit::Clear));
        assert_eq!(update(&mut model, Message::Activate), Some(Command::Apply));
        assert_eq!(model.applied, "");
    }

    #[test]
    fn picker_lists_saved_queries_and_applies_selected_query() {
        let mut model = QueryBar::new(
            "",
            BTreeMap::from([
                ("All".into(), "".into()),
                ("Failed".into(), "ExecutionStatus = 'Failed'".into()),
            ]),
        );
        update(&mut model, Message::OpenLoad);
        update(&mut model, Message::MoveSelection(1));
        assert_eq!(update(&mut model, Message::Activate), Some(Command::Apply));
        assert_eq!(model.applied, "ExecutionStatus = 'Failed'");
        assert_eq!(model.input.text, model.applied);
        assert!(model.dialog.is_none());
    }

    #[test]
    fn empty_picker_and_cancel_preserve_draft() {
        let mut model = QueryBar::new("draft", BTreeMap::new());
        update(&mut model, Message::OpenLoad);
        update(&mut model, Message::MoveSelection(1));
        assert_eq!(update(&mut model, Message::Activate), None);
        update(&mut model, Message::Cancel);
        assert_eq!(model.input.text, "draft");
    }
}
