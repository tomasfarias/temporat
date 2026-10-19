use crate::text_input::{Edit, TextInput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Table,
    Input,
}

pub struct QueryBar {
    pub input: TextInput,
    pub applied: String,
    pub focus: Focus,
}

pub enum Message {
    Focus(Focus),
    CycleFocus(bool),
    Edit(Edit),
    Activate,
    Cancel,
}

impl QueryBar {
    pub fn new(query: &str) -> Self {
        Self {
            input: TextInput::new(query),
            applied: query.into(),
            focus: Focus::Table,
        }
    }
}

pub fn update(model: &mut QueryBar, message: Message) -> Option<String> {
    match message {
        Message::Focus(focus) => model.focus = focus,
        Message::CycleFocus(_backwards) => {
            model.focus = if model.focus == Focus::Table {
                Focus::Input
            } else {
                Focus::Table
            }
        }
        Message::Edit(edit) => model.input.update(edit),
        Message::Activate if model.focus == Focus::Input => {
            model.applied = model.input.text.clone();
            model.focus = Focus::Table;
            return Some(model.applied.clone());
        }
        Message::Cancel => {
            model.input = TextInput::new(&model.applied);
            model.focus = Focus::Table;
        }
        Message::Activate => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_restores_applied_query_and_empty_query_can_be_submitted() {
        let mut model = QueryBar::new("WorkflowType = 'Test'");
        update(&mut model, Message::Focus(Focus::Input));
        update(&mut model, Message::Edit(Edit::Clear));
        update(&mut model, Message::Cancel);
        assert_eq!(model.input.text, model.applied);
        update(&mut model, Message::Focus(Focus::Input));
        update(&mut model, Message::Edit(Edit::Clear));
        assert_eq!(update(&mut model, Message::Activate), Some(String::new()));
    }
}
