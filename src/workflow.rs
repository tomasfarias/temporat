use chrono::{DateTime, Utc};
use temporalio_common::protos::temporal::api::{
    enums::v1::WorkflowExecutionStatus, workflow::v1::WorkflowExecutionInfo,
};

use crate::config::Column;

pub struct Page {
    pub rows: Vec<WorkflowRow>,
    pub next_page_token: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Execution {
    pub workflow_id: String,
    pub run_id: String,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct WorkflowRow {
    pub execution: Option<Execution>,
    pub cells: Vec<String>,
}

impl WorkflowRow {
    pub fn from_info(info: WorkflowExecutionInfo, columns: &[Column]) -> Self {
        let execution = info
            .execution
            .as_ref()
            .filter(|execution| !execution.workflow_id.is_empty() && !execution.run_id.is_empty())
            .map(|execution| Execution {
                workflow_id: execution.workflow_id.clone(),
                run_id: execution.run_id.clone(),
            });
        Self {
            execution,
            cells: cells(info, columns),
        }
    }
}

// Keep SDK types at the boundary. The UI receives only the requested display cells.
pub fn cells(info: WorkflowExecutionInfo, columns: &[Column]) -> Vec<String> {
    columns
        .iter()
        .map(|column| match column {
            Column::WorkflowId => info
                .execution
                .as_ref()
                .map(|execution| execution.workflow_id.clone())
                .unwrap_or_else(missing),
            Column::RunId => info
                .execution
                .as_ref()
                .map(|execution| execution.run_id.clone())
                .unwrap_or_else(missing),
            Column::WorkflowType => info
                .r#type
                .as_ref()
                .map(|kind| kind.name.clone())
                .unwrap_or_else(missing),
            Column::Status => status(info.status),
            Column::StartTime => info
                .start_time
                .as_ref()
                .map(|time| timestamp(time.seconds, time.nanos))
                .unwrap_or_else(missing),
            Column::CloseTime => info
                .close_time
                .as_ref()
                .map(|time| timestamp(time.seconds, time.nanos))
                .unwrap_or_else(missing),
            Column::TaskQueue => info.task_queue.clone(),
            Column::HistoryLength => info.history_length.to_string(),
        })
        .collect()
}

fn missing() -> String {
    "—".into()
}

pub fn timestamp(seconds: i64, nanos: i32) -> String {
    u32::try_from(nanos)
        .ok()
        .filter(|nanos| *nanos < 1_000_000_000)
        .and_then(|nanos| DateTime::<Utc>::from_timestamp(seconds, nanos))
        .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(missing)
}

fn status(value: i32) -> String {
    match WorkflowExecutionStatus::try_from(value) {
        Ok(WorkflowExecutionStatus::Running) => "Running",
        Ok(WorkflowExecutionStatus::Completed) => "Completed",
        Ok(WorkflowExecutionStatus::Failed) => "Failed",
        Ok(WorkflowExecutionStatus::Canceled) => "Canceled",
        Ok(WorkflowExecutionStatus::Terminated) => "Terminated",
        Ok(WorkflowExecutionStatus::ContinuedAsNew) => "Continued as new",
        Ok(WorkflowExecutionStatus::TimedOut) => "Timed out",
        _ => return format!("Unknown ({value})"),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use temporalio_common::protos::temporal::api::common::v1::WorkflowExecution;

    #[test]
    fn maps_only_requested_columns_in_order_and_handles_absent_fields() {
        let info = WorkflowExecutionInfo {
            execution: Some(WorkflowExecution {
                workflow_id: "order-42".into(),
                run_id: "run".into(),
            }),
            status: 999,
            ..Default::default()
        };
        assert_eq!(
            cells(
                info,
                &[
                    Column::CloseTime,
                    Column::Status,
                    Column::WorkflowId,
                    Column::WorkflowType
                ]
            ),
            ["—", "Unknown (999)", "order-42", "—"]
        );
    }

    #[test]
    fn timestamps_are_utc_and_invalid_values_are_missing() {
        assert_eq!(timestamp(0, 0), "1970-01-01 00:00:00");
        assert_eq!(timestamp(i64::MAX, 0), "—");
        assert_eq!(timestamp(0, -1), "—");
        assert_eq!(timestamp(0, 1_000_000_000), "—");
    }

    #[test]
    fn retains_exact_run_identity_when_id_columns_are_hidden() {
        let row = WorkflowRow::from_info(
            WorkflowExecutionInfo {
                execution: Some(WorkflowExecution {
                    workflow_id: "order".into(),
                    run_id: "older-run".into(),
                }),
                ..Default::default()
            },
            &[Column::Status],
        );
        assert_eq!(row.cells.len(), 1);
        assert_eq!(
            row.execution.unwrap(),
            Execution {
                workflow_id: "order".into(),
                run_id: "older-run".into()
            }
        );
        assert!(
            WorkflowRow::from_info(WorkflowExecutionInfo::default(), &[])
                .execution
                .is_none()
        );
    }
}
