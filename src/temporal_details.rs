use std::time::Duration;

use anyhow::{Context, Result, bail};
use prost::Message;
use temporalio_client::{Client, grpc::WorkflowService, tonic::Request};
use temporalio_common::protos::temporal::api::{
    common::v1::WorkflowExecution,
    enums::v1::{EncodingType, HistoryEventFilterType},
    history::v1::{History, HistoryEvent, history_event::Attributes},
    workflowservice::v1::{
        DescribeWorkflowExecutionRequest, DescribeWorkflowExecutionResponse,
        GetWorkflowExecutionHistoryRequest, GetWorkflowExecutionHistoryResponse,
        GetWorkflowExecutionHistoryReverseRequest,
    },
};

use crate::{
    debug_log,
    details::Snapshot,
    payload,
    workflow::{self, Execution},
};

pub async fn load(client: &Client, execution: &Execution) -> Result<Snapshot> {
    let request = DescribeWorkflowExecutionRequest {
        namespace: client.options().namespace.clone(),
        execution: Some(WorkflowExecution {
            workflow_id: execution.workflow_id.clone(),
            run_id: execution.run_id.clone(),
        }),
    };
    let response = debug_log::request("DescribeWorkflowExecution", async {
        tokio::time::timeout(
            Duration::from_secs(30),
            client
                .clone()
                .describe_workflow_execution(Request::new(request)),
        )
        .await
        .context("describing workflow timed out")?
        .context("describing workflow")
    })
    .await?
    .into_inner();
    Ok(snapshot(response))
}

pub async fn input_result(client: &Client, execution: &Execution) -> Result<String> {
    let (first, close) = tokio::try_join!(
        history_page(
            client,
            execution,
            Vec::new(),
            HistoryEventFilterType::AllEvent
        ),
        latest_event(client, execution),
    )?;
    let input = first
        .0
        .iter()
        .find_map(|event| match &event.attributes {
            Some(Attributes::WorkflowExecutionStartedEventAttributes(attributes)) => {
                Some(payload::payloads(attributes.input.as_ref()))
            }
            _ => None,
        })
        .unwrap_or_else(|| "Start event unavailable in the first history page.".into());
    let result = close
        .as_ref()
        .map(outcome)
        .unwrap_or_else(|| "No history events available.".into());
    Ok(format!("Input\n{input}\n\nResult / Failure\n{result}"))
}

async fn latest_event(client: &Client, execution: &Execution) -> Result<Option<HistoryEvent>> {
    let request = GetWorkflowExecutionHistoryReverseRequest {
        namespace: client.options().namespace.clone(),
        execution: Some(WorkflowExecution {
            workflow_id: execution.workflow_id.clone(),
            run_id: execution.run_id.clone(),
        }),
        maximum_page_size: 1,
        next_page_token: Vec::new(),
    };
    // A close-event request may wait on an open execution. Reverse history gives
    // its latest available event immediately, whether it has closed or not.
    let response = debug_log::request("GetWorkflowExecutionHistoryReverse", async {
        tokio::time::timeout(
            Duration::from_secs(30),
            client
                .clone()
                .get_workflow_execution_history_reverse(Request::new(request)),
        )
        .await
        .context("fetching latest workflow event timed out")?
        .context("fetching latest workflow event")
    })
    .await?
    .into_inner();
    Ok(response
        .history
        .and_then(|history| history.events.into_iter().next()))
}

pub async fn history_page(
    client: &Client,
    execution: &Execution,
    next_page_token: Vec<u8>,
    filter: HistoryEventFilterType,
) -> Result<(Vec<HistoryEvent>, Vec<u8>)> {
    let request = GetWorkflowExecutionHistoryRequest {
        namespace: client.options().namespace.clone(),
        execution: Some(WorkflowExecution {
            workflow_id: execution.workflow_id.clone(),
            run_id: execution.run_id.clone(),
        }),
        maximum_page_size: 50,
        next_page_token,
        history_event_filter_type: filter.into(),
        wait_new_event: false,
        ..Default::default()
    };
    let response = debug_log::request("GetWorkflowExecutionHistory", async {
        tokio::time::timeout(
            Duration::from_secs(30),
            client
                .clone()
                .get_workflow_execution_history(Request::new(request)),
        )
        .await
        .context("fetching workflow history timed out")?
        .context("fetching workflow history")
    })
    .await?
    .into_inner();
    let token = response.next_page_token.clone();
    Ok((events(response)?, token))
}

fn events(response: GetWorkflowExecutionHistoryResponse) -> Result<Vec<HistoryEvent>> {
    if let Some(history) = response.history {
        return Ok(history.events);
    }
    let mut events = Vec::new();
    for blob in response.raw_history {
        if blob.encoding_type != i32::from(EncodingType::Proto3) {
            bail!("unsupported raw history encoding {}", blob.encoding_type);
        }
        events.extend(
            History::decode(blob.data.as_slice())
                .context("decoding raw history")?
                .events,
        );
    }
    Ok(events)
}

fn outcome(event: &HistoryEvent) -> String {
    match &event.attributes {
        Some(Attributes::WorkflowExecutionCompletedEventAttributes(attributes)) => {
            payload::payloads(attributes.result.as_ref())
        }
        Some(Attributes::WorkflowExecutionFailedEventAttributes(attributes)) => {
            format!("Failed\n{}", pretty(&attributes.failure))
        }
        Some(Attributes::WorkflowExecutionCanceledEventAttributes(attributes)) => format!(
            "Canceled\n{}",
            payload::payloads(attributes.details.as_ref())
        ),
        Some(Attributes::WorkflowExecutionTerminatedEventAttributes(attributes)) => format!(
            "Terminated: {}\n{}",
            attributes.reason,
            payload::payloads(attributes.details.as_ref())
        ),
        Some(Attributes::WorkflowExecutionTimedOutEventAttributes(_)) => "Timed out".into(),
        Some(Attributes::WorkflowExecutionContinuedAsNewEventAttributes(attributes)) => {
            format!("Continued as new: {}", attributes.new_execution_run_id)
        }
        _ => "No close event; the execution may still be running.".into(),
    }
}

fn pretty(value: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|error| format!("Unable to format data: {error}"))
}

fn snapshot(response: DescribeWorkflowExecutionResponse) -> Snapshot {
    let mut lines = vec![workflow::overview(
        &response.workflow_execution_info.unwrap_or_default(),
    )];
    lines.push(format!(
        "Pending activities: {}",
        response.pending_activities.len()
    ));
    lines.push(format!(
        "Pending child workflows: {}",
        response.pending_children.len()
    ));
    Snapshot {
        overview: lines.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temporalio_common::protos::temporal::api::{
        common::v1::DataBlob, history::v1::WorkflowExecutionContinuedAsNewEventAttributes,
    };

    #[test]
    fn decodes_raw_history_without_losing_event_identity() {
        let history = History {
            events: vec![HistoryEvent {
                event_id: 123,
                ..Default::default()
            }],
        };
        let response = GetWorkflowExecutionHistoryResponse {
            raw_history: vec![DataBlob {
                encoding_type: EncodingType::Proto3.into(),
                data: history.encode_to_vec(),
            }],
            ..Default::default()
        };
        assert_eq!(events(response).unwrap()[0].event_id, 123);
    }

    #[test]
    fn continued_as_new_links_next_run_without_following_it() {
        let event = HistoryEvent {
            attributes: Some(Attributes::WorkflowExecutionContinuedAsNewEventAttributes(
                WorkflowExecutionContinuedAsNewEventAttributes {
                    new_execution_run_id: "next-run".into(),
                    ..Default::default()
                },
            )),
            ..Default::default()
        };
        assert_eq!(outcome(&event), "Continued as new: next-run");
    }
}
