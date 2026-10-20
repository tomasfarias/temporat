use temporalio_common::protos::temporal::api::{
    enums::v1::EventType,
    history::v1::{HistoryEvent, history_event::Attributes},
};

use crate::{payload, workflow};

pub struct Event {
    pub id: i64,
    pub time: String,
    pub kind: String,
    pub details: String,
}

pub struct Page {
    pub events: Vec<Event>,
    pub next_page_token: Vec<u8>,
}

impl Event {
    pub fn from_proto(event: HistoryEvent) -> Self {
        let kind = EventType::try_from(event.event_type)
            .map(|kind| format!("{kind:?}"))
            .unwrap_or_else(|_| format!("Unknown ({})", event.event_type));
        let time = event
            .event_time
            .as_ref()
            .map(|time| workflow::timestamp(time.seconds, time.nanos))
            .unwrap_or_else(|| "—".into());
        let payloads = match &event.attributes {
            Some(Attributes::WorkflowExecutionStartedEventAttributes(attrs)) => {
                attrs.input.as_ref()
            }
            Some(Attributes::WorkflowExecutionCompletedEventAttributes(attrs)) => {
                attrs.result.as_ref()
            }
            Some(Attributes::WorkflowExecutionSignaledEventAttributes(attrs)) => {
                attrs.input.as_ref()
            }
            Some(Attributes::ActivityTaskScheduledEventAttributes(attrs)) => attrs.input.as_ref(),
            Some(Attributes::ActivityTaskCompletedEventAttributes(attrs)) => attrs.result.as_ref(),
            _ => None,
        };
        let json = serde_json::to_string_pretty(&event)
            .unwrap_or_else(|error| format!("Unable to format event: {error}"));
        let details = match payloads {
            Some(payloads) => format!(
                "Payloads\n{}\n\nEvent JSON\n{json}",
                payload::payloads(Some(payloads))
            ),
            None => json,
        };
        let details = format!(
            "Event {}: {kind}\nTime (UTC): {time}\n\n{details}",
            event.event_id
        );
        Self {
            id: event.event_id,
            time,
            kind,
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unknown_event_types_and_full_event_data() {
        let event = Event::from_proto(HistoryEvent {
            event_id: 7,
            event_type: 99999,
            task_id: 42,
            ..Default::default()
        });
        assert_eq!(event.id, 7);
        assert_eq!(event.kind, "Unknown (99999)");
        assert_eq!(event.time, "—");
        assert!(event.details.contains("42"));
    }
}
