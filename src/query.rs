use crate::text_input::{Edit, TextInput};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Table,
    Input,
    Load,
    Save,
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
    Load {
        selected: usize,
    },
    Save {
        name: TextInput,
        error: Option<String>,
        saving: bool,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Apply,
    Save { name: String, query: String },
}

pub enum Message {
    Focus(Focus),
    CycleFocus(bool),
    Edit(Edit),
    Activate,
    Cancel,
    OpenLoad,
    OpenSave,
    MoveSelection(isize),
    Saved {
        name: String,
        result: Result<BTreeMap<String, String>, String>,
    },
}

impl QueryBar {
    pub fn editing(&self) -> bool {
        matches!(self.dialog, Some(Dialog::Save { saving: false, .. }))
            || (self.dialog.is_none() && self.focus == Focus::Input)
    }

    pub fn new(query: &str, saved: BTreeMap<String, String>) -> Self {
        let active_name = saved
            .iter()
            .find_map(|(name, value)| (value == query).then(|| name.clone()));
        Self {
            input: TextInput::new(query),
            applied: query.into(),
            focus: Focus::Table,
            saved,
            dialog: None,
            active_name,
        }
    }
}

pub fn update(model: &mut QueryBar, message: Message) -> Option<Command> {
    match message {
        Message::Focus(focus) if model.dialog.is_none() => model.focus = focus,
        Message::CycleFocus(backwards) if model.dialog.is_none() => {
            let order = [Focus::Table, Focus::Input, Focus::Load, Focus::Save];
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
        Message::OpenSave => {
            model.dialog = Some(Dialog::Save {
                name: TextInput::new(model.active_name.as_deref().unwrap_or_default()),
                error: None,
                saving: false,
            });
        }
        Message::Saved { name, result } => match result {
            Ok(saved) => {
                model.saved = saved;
                model.active_name = Some(name);
                model.dialog = None;
                model.focus = Focus::Table;
            }
            Err(message) => {
                if let Some(Dialog::Save { error, saving, .. }) = &mut model.dialog {
                    *error = Some(message);
                    *saving = false;
                }
            }
        },
        Message::MoveSelection(delta) => {
            if let Some(Dialog::Load { selected }) = &mut model.dialog {
                *selected = selected
                    .saturating_add_signed(delta)
                    .min(model.saved.len().saturating_sub(1));
            }
        }
        Message::Activate if matches!(model.dialog, Some(Dialog::Save { .. })) => {
            let Some(Dialog::Save {
                name,
                error,
                saving,
            }) = &mut model.dialog
            else {
                return None;
            };
            if *saving {
                return None;
            }
            if name.text.trim().is_empty() {
                *error = Some("Enter a query name.".into());
                return None;
            }
            *error = None;
            *saving = true;
            return Some(Command::Save {
                name: name.text.trim().into(),
                query: model.input.text.clone(),
            });
        }
        Message::Activate if matches!(model.dialog, Some(Dialog::Load { .. })) => {
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
        Message::Edit(edit) => {
            if let Some(Dialog::Save {
                name,
                saving: false,
                ..
            }) = &mut model.dialog
            {
                name.update(edit);
            }
        }
        Message::Activate if model.focus == Focus::Input => {
            model.applied = model.input.text.clone();
            model.focus = Focus::Table;
            return Some(Command::Apply);
        }
        Message::Activate if model.focus == Focus::Load => return update(model, Message::OpenLoad),
        Message::Activate if model.focus == Focus::Save => return update(model, Message::OpenSave),
        Message::Cancel if matches!(model.dialog, Some(Dialog::Save { saving: true, .. })) => {}
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

    #[test]
    fn saving_uses_draft_and_only_updates_catalog_after_success() {
        let mut model = QueryBar::new("original", BTreeMap::new());
        model.input = TextInput::new("WorkflowId = 'new'");
        update(&mut model, Message::OpenSave);
        assert_eq!(update(&mut model, Message::Activate), None);
        update(
            &mut model,
            Message::Edit(Edit::Insert("Named query".into())),
        );
        let save = Command::Save {
            name: "Named query".into(),
            query: "WorkflowId = 'new'".into(),
        };
        assert_eq!(update(&mut model, Message::Activate), Some(save));
        assert_eq!(update(&mut model, Message::Activate), None);
        assert!(model.saved.is_empty());
        update(
            &mut model,
            Message::Saved {
                name: "Named query".into(),
                result: Err("disk error".into()),
            },
        );
        assert!(
            matches!(&model.dialog, Some(Dialog::Save { name, error: Some(error), saving: false }) if name.text == "Named query" && error == "disk error")
        );
        update(&mut model, Message::Activate);
        update(
            &mut model,
            Message::Saved {
                name: "Named query".into(),
                result: Ok(BTreeMap::from([(
                    "Named query".into(),
                    "WorkflowId = 'new'".into(),
                )])),
            },
        );
        assert_eq!(model.saved["Named query"], model.input.text);
        assert_eq!(model.applied, "original");
        assert!(model.dialog.is_none());
    }

    #[test]
    fn focus_cycles_through_input_and_both_buttons() {
        let mut model = QueryBar::new("", BTreeMap::new());
        for focus in [Focus::Input, Focus::Load, Focus::Save, Focus::Table] {
            update(&mut model, Message::CycleFocus(false));
            assert_eq!(model.focus, focus);
        }
        update(&mut model, Message::CycleFocus(true));
        assert_eq!(model.focus, Focus::Save);
        update(&mut model, Message::Activate);
        assert!(matches!(model.dialog, Some(Dialog::Save { .. })));
    }
}
