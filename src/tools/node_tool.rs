//! Commands from agent tools to the running node (P2P jobs, job status, etc.).
//!
//! The `peerclaw serve` loop owns [`crate::runtime::Runtime`]; tools receive a channel sender
//! in [`crate::tools::ToolContext`] and block on a oneshot for the result.

use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

/// Result of submitting a marketplace job (GossipSub + local JobManager).
#[derive(Debug, Clone, Serialize)]
pub struct P2pJobSubmitResult {
    pub success: bool,
    pub job_id: Option<String>,
    pub error: Option<String>,
}

/// Async messages handled on the node runtime task.
#[derive(Debug)]
pub enum NodeToolCommand {
    SubmitP2pJob {
        job_type: String,
        budget: f64,
        payload: String,
        reply: oneshot::Sender<P2pJobSubmitResult>,
    },
    DescribeP2pJob {
        job_id: String,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    },
}

pub type NodeToolTx = mpsc::Sender<NodeToolCommand>;

/// Send a job submit command and wait for the node response.
pub async fn submit_p2p_job_via_node(
    tx: &NodeToolTx,
    job_type: String,
    budget: f64,
    payload: String,
) -> Result<P2pJobSubmitResult, String> {
    let (reply, rx) = oneshot::channel();
    tx.send(NodeToolCommand::SubmitP2pJob {
        job_type,
        budget,
        payload,
        reply,
    })
    .await
    .map_err(|_| "node tool channel closed".to_string())?;
    rx.await.map_err(|_| "node dropped job submit reply".to_string())
}

/// Look up a job by id across pending requests, active jobs, and recent completed.
pub async fn describe_p2p_job_via_node(
    tx: &NodeToolTx,
    job_id: String,
) -> Result<serde_json::Value, String> {
    let (reply, rx) = oneshot::channel();
    tx.send(NodeToolCommand::DescribeP2pJob { job_id, reply })
        .await
        .map_err(|_| "node tool channel closed".to_string())?;
    rx.await
        .map_err(|_| "node dropped job status reply".to_string())?
}
