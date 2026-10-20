use std::time::Duration;

use anyhow::{Context, Result};
use temporalio_client::{Client, grpc::WorkflowService, tonic::Request};
use temporalio_common::protos::temporal::api::{
    common::v1::WorkflowExecution,
    workflowservice::v1::{DescribeWorkflowExecutionRequest, DescribeWorkflowExecutionResponse},
};

use crate::{
    config::Column,
    details::Snapshot,
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
    let response = tokio::time::timeout(
        Duration::from_secs(30),
        client
            .clone()
            .describe_workflow_execution(Request::new(request)),
    )
    .await
    .context("describing workflow timed out")?
    .context("describing workflow")?
    .into_inner();
    Ok(snapshot(response))
}

fn snapshot(response: DescribeWorkflowExecutionResponse) -> Snapshot {
    let columns = [
        Column::WorkflowId,
        Column::RunId,
        Column::WorkflowType,
        Column::Status,
        Column::StartTime,
        Column::CloseTime,
        Column::TaskQueue,
        Column::HistoryLength,
    ];
    let labels = [
        "Workflow ID",
        "Run ID",
        "Type",
        "Status",
        "Started (UTC)",
        "Closed (UTC)",
        "Task queue",
        "History events",
    ];
    let cells = workflow::cells(
        response.workflow_execution_info.unwrap_or_default(),
        &columns,
    );
    let mut lines: Vec<_> = labels
        .into_iter()
        .zip(cells)
        .map(|(label, value)| format!("{label}: {value}"))
        .collect();
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
