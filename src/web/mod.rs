//! Web dashboard with real-time monitoring and chat interface.
//!
//! Provides a web UI for:
//! - Network topology visualization
//! - CPU/GPU/RAM monitoring
//! - Job marketplace status
//! - AI chat interface
//! - OpenAI-compatible API (/v1/chat/completions, /v1/models)

pub mod openai;

use std::sync::Arc;
use std::net::SocketAddr;


use axum::{
    extract::{State, ws::{WebSocket, WebSocketUpgrade, Message}},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};
use libp2p::PeerId;

use crate::executor::ResourceMonitor;
use crate::p2p::ProviderTracker;
use crate::swarm::SwarmManager;
use crate::wallet::from_micro;

/// Request for inference from web UI
pub struct InferenceRequest {
    pub prompt: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub response_tx: tokio::sync::oneshot::Sender<InferenceResponse>,
}

/// Response to inference request
pub struct InferenceResponse {
    pub text: String,
    pub tokens_generated: u32,
    pub tokens_per_second: f32,
    pub location: String,
    pub provider_peer_id: Option<String>,
}

/// Request for job submission from web UI
pub struct JobSubmitRequest {
    pub job_type: String,
    pub budget: f64,
    pub payload: String,
    pub response_tx: tokio::sync::oneshot::Sender<JobSubmitResponse>,
}

/// Response to job submission
pub struct JobSubmitResponse {
    pub success: bool,
    pub job_id: Option<String>,
    pub error: Option<String>,
}

/// Detailed job information for display.
#[derive(Clone, Serialize)]
pub struct WebJobInfo {
    pub id: String,
    pub job_type: String,
    pub status: String,
    pub provider: Option<String>,
    pub requester: String,
    pub price_micro: u64,
    pub created_at: u64,
    pub location: Option<String>,
}

/// A task created from the web UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebTask {
    pub id: String,
    pub task_type: String,
    pub description: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub result: Option<String>,
    pub logs: Vec<String>,
    pub model: Option<String>,
    pub budget: f64,
    pub tokens_used: u32,
    pub iterations: u32,
}

/// Web server state - contains only Send/Sync safe components.
pub struct WebState {
    pub local_peer_id: PeerId,
    pub resource_monitor: Arc<ResourceMonitor>,
    pub wallet_balance: Arc<RwLock<u64>>,
    /// Connected peers (updated periodically)
    pub connected_peers: Arc<RwLock<Vec<PeerId>>>,
    /// Active jobs count
    pub active_jobs: Arc<RwLock<usize>>,
    /// Completed jobs count
    pub completed_jobs: Arc<RwLock<usize>>,
    /// Detailed job list for display
    pub job_list: Arc<RwLock<Vec<WebJobInfo>>>,
    /// Channel for receiving inference requests from web UI
    pub inference_tx: Option<mpsc::Sender<InferenceRequest>>,
    /// Channel for receiving job submission requests from web UI
    pub job_submit_tx: Option<mpsc::Sender<JobSubmitRequest>>,
    /// Swarm manager for agent visualization
    pub swarm_manager: Option<Arc<SwarmManager>>,
    /// Task store for web-created tasks
    pub task_store: Arc<RwLock<Vec<WebTask>>>,
    /// Provider tracker for network LLM providers
    pub provider_tracker: Option<Arc<ProviderTracker>>,
    /// Channel to send tasks to the agent runtime
    pub agent_task_tx: Option<mpsc::Sender<AgentTaskRequest>>,
}

/// Request to run a task through the agent runtime.
pub struct AgentTaskRequest {
    pub task_id: String,
    pub description: String,
    pub response_tx: tokio::sync::oneshot::Sender<crate::agent::AgentResult>,
    /// Shared task store so the agent can stream logs in real-time
    pub task_store: Arc<RwLock<Vec<WebTask>>>,
}

/// Create the web router.
pub fn create_router(state: Arc<WebState>) -> Router {
    Router::new()
        // Dashboard routes
        .route("/", get(index))
        .route("/chat", get(index)) // Redirect to main dashboard (has Chat tab)
        .route("/api/status", get(api_status))
        .route("/api/peers", get(api_peers))
        .route("/api/jobs", get(api_jobs))
        .route("/api/jobs/submit", post(api_submit_job))
        .route("/api/chat", post(api_chat))
        .route("/ws", get(ws_handler))
        // Task management routes
        .route("/api/tasks", post(api_create_task))
        .route("/api/tasks", get(api_list_tasks))
        .route("/api/tasks/{id}", get(api_task_detail))
        // Provider routes
        .route("/api/providers", get(api_list_providers))
        .route("/api/providers/config", get(api_get_provider_config))
        .route("/api/providers/config", post(api_set_provider_config))
        // Node detail route
        .route("/api/nodes/{id}", get(api_node_detail))
        // Swarm visualization routes
        .route("/api/swarm/agents", get(api_swarm_agents))
        .route("/api/swarm/topology", get(api_swarm_topology))
        .route("/api/swarm/timeline", get(api_swarm_timeline))
        // OpenAI-compatible API routes
        .route("/v1/chat/completions", post(openai::chat_completions))
        .route("/v1/models", get(openai::list_models))
        .route("/v1/embeddings", post(openai::embeddings))
        .with_state(state)
}

/// Create WebState for the dashboard (basic version without inference).
pub fn create_web_state(
    local_peer_id: PeerId,
    resource_monitor: Arc<ResourceMonitor>,
) -> Arc<WebState> {
    Arc::new(WebState {
        local_peer_id,
        resource_monitor,
        wallet_balance: Arc::new(RwLock::new(0)),
        connected_peers: Arc::new(RwLock::new(Vec::new())),
        active_jobs: Arc::new(RwLock::new(0)),
        completed_jobs: Arc::new(RwLock::new(0)),
        job_list: Arc::new(RwLock::new(Vec::new())),
        inference_tx: None,
        job_submit_tx: None,
        swarm_manager: None,
        task_store: Arc::new(RwLock::new(Vec::new())),
        provider_tracker: None,
        agent_task_tx: None,
    })
}

/// Create WebState with inference and job submission channels.
pub fn create_web_state_with_channels(
    local_peer_id: PeerId,
    resource_monitor: Arc<ResourceMonitor>,
    inference_tx: mpsc::Sender<InferenceRequest>,
    job_submit_tx: mpsc::Sender<JobSubmitRequest>,
) -> Arc<WebState> {
    Arc::new(WebState {
        local_peer_id,
        resource_monitor,
        wallet_balance: Arc::new(RwLock::new(0)),
        connected_peers: Arc::new(RwLock::new(Vec::new())),
        active_jobs: Arc::new(RwLock::new(0)),
        completed_jobs: Arc::new(RwLock::new(0)),
        job_list: Arc::new(RwLock::new(Vec::new())),
        inference_tx: Some(inference_tx),
        job_submit_tx: Some(job_submit_tx),
        swarm_manager: None,
        task_store: Arc::new(RwLock::new(Vec::new())),
        provider_tracker: None,
        agent_task_tx: None,
    })
}

/// Create WebState with inference channel only (legacy).
pub fn create_web_state_with_inference(
    local_peer_id: PeerId,
    resource_monitor: Arc<ResourceMonitor>,
    inference_tx: mpsc::Sender<InferenceRequest>,
) -> Arc<WebState> {
    Arc::new(WebState {
        local_peer_id,
        resource_monitor,
        wallet_balance: Arc::new(RwLock::new(0)),
        connected_peers: Arc::new(RwLock::new(Vec::new())),
        active_jobs: Arc::new(RwLock::new(0)),
        completed_jobs: Arc::new(RwLock::new(0)),
        job_list: Arc::new(RwLock::new(Vec::new())),
        inference_tx: Some(inference_tx),
        job_submit_tx: None,
        swarm_manager: None,
        task_store: Arc::new(RwLock::new(Vec::new())),
        provider_tracker: None,
        agent_task_tx: None,
    })
}

/// Create WebState with swarm manager for agent visualization.
pub fn create_web_state_with_swarm(
    local_peer_id: PeerId,
    resource_monitor: Arc<ResourceMonitor>,
    swarm_manager: Arc<SwarmManager>,
) -> Arc<WebState> {
    Arc::new(WebState {
        local_peer_id,
        resource_monitor,
        wallet_balance: Arc::new(RwLock::new(0)),
        connected_peers: Arc::new(RwLock::new(Vec::new())),
        active_jobs: Arc::new(RwLock::new(0)),
        completed_jobs: Arc::new(RwLock::new(0)),
        job_list: Arc::new(RwLock::new(Vec::new())),
        inference_tx: None,
        job_submit_tx: None,
        swarm_manager: Some(swarm_manager),
        task_store: Arc::new(RwLock::new(Vec::new())),
        provider_tracker: None,
        agent_task_tx: None,
    })
}

/// Start the web server.
pub async fn start_server(
    addr: SocketAddr,
    state: Arc<WebState>,
) -> anyhow::Result<()> {
    let app = create_router(state);

    tracing::info!("Web UI starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// === API Endpoints ===

async fn index() -> Html<&'static str> {
    Html(include_str!("dashboard.html"))
}

async fn chat_page() -> Html<&'static str> {
    Html(include_str!("chat.html"))
}

#[derive(Serialize)]
struct StatusResponse {
    peer_id: String,
    connected_peers: usize,
    balance: f64,
    cpu_usage: f64,
    ram_used_mb: u32,
    ram_total_mb: u32,
    gpu_usage: Option<f64>,
    active_jobs: usize,
    completed_jobs: usize,
    active_inference: u32,
    active_web: u32,
    active_wasm: u32,
}

async fn api_status(State(state): State<Arc<WebState>>) -> Json<StatusResponse> {
    let resource_state = state.resource_monitor.current_state().await;
    let balance = from_micro(*state.wallet_balance.read().await);
    let connected_peers = state.connected_peers.read().await.len();
    let active_jobs = *state.active_jobs.read().await;
    let completed_jobs = *state.completed_jobs.read().await;

    Json(StatusResponse {
        peer_id: state.local_peer_id.to_string(),
        connected_peers,
        balance,
        cpu_usage: resource_state.cpu_usage,
        ram_used_mb: resource_state.ram_total_mb - resource_state.ram_available_mb,
        ram_total_mb: resource_state.ram_total_mb,
        gpu_usage: resource_state.gpu_usage,
        active_jobs,
        completed_jobs,
        active_inference: resource_state.active_inference_tasks,
        active_web: resource_state.active_web_tasks,
        active_wasm: resource_state.active_wasm_tasks,
    })
}

#[derive(Serialize)]
struct PeerInfo {
    id: String,
    connected: bool,
}

async fn api_peers(State(state): State<Arc<WebState>>) -> Json<Vec<PeerInfo>> {
    let peers = state.connected_peers.read().await;
    let peer_infos: Vec<PeerInfo> = peers
        .iter()
        .map(|p| PeerInfo {
            id: p.to_string(),
            connected: true,
        })
        .collect();
    Json(peer_infos)
}

async fn api_jobs(State(state): State<Arc<WebState>>) -> Json<Vec<WebJobInfo>> {
    let jobs = state.job_list.read().await;
    Json(jobs.clone())
}

#[derive(Deserialize)]
struct JobSubmitPayload {
    job_type: String,
    budget: f64,
    payload: String,
}

#[derive(Serialize)]
struct JobSubmitResult {
    success: bool,
    job_id: Option<String>,
    error: Option<String>,
}

async fn api_submit_job(
    State(state): State<Arc<WebState>>,
    Json(req): Json<JobSubmitPayload>,
) -> Json<JobSubmitResult> {
    // If we have a job submission channel, use it
    if let Some(tx) = &state.job_submit_tx {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request = JobSubmitRequest {
            job_type: req.job_type,
            budget: req.budget,
            payload: req.payload,
            response_tx,
        };

        if tx.send(request).await.is_ok() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(30),
                response_rx,
            ).await {
                Ok(Ok(response)) => {
                    return Json(JobSubmitResult {
                        success: response.success,
                        job_id: response.job_id,
                        error: response.error,
                    });
                }
                Ok(Err(_)) => {
                    return Json(JobSubmitResult {
                        success: false,
                        job_id: None,
                        error: Some("Job submission cancelled".to_string()),
                    });
                }
                Err(_) => {
                    return Json(JobSubmitResult {
                        success: false,
                        job_id: None,
                        error: Some("Job submission timeout".to_string()),
                    });
                }
            }
        }
    }

    // Fallback: no job submission channel
    Json(JobSubmitResult {
        success: false,
        job_id: None,
        error: Some("Job submission not available. Restart node with full features.".to_string()),
    })
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    model: Option<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
    tokens: u32,
    tokens_per_second: f32,
    location: String,
    provider_peer_id: Option<String>,
}

async fn api_chat(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ChatRequest>,
) -> Json<ChatResponse> {
    let model = req.model.unwrap_or_else(|| "llama-3.2-3b".to_string());
    let max_tokens = req.max_tokens.unwrap_or(500);
    let temperature = req.temperature.unwrap_or(0.7);

    // If we have an inference channel, use it
    if let Some(tx) = &state.inference_tx {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request = InferenceRequest {
            prompt: req.message,
            model: model.clone(),
            max_tokens,
            temperature,
            response_tx,
        };

        if tx.send(request).await.is_ok() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(60),
                response_rx,
            ).await {
                Ok(Ok(response)) => {
                    return Json(ChatResponse {
                        response: response.text,
                        tokens: response.tokens_generated,
                        tokens_per_second: response.tokens_per_second,
                        location: response.location,
                        provider_peer_id: response.provider_peer_id,
                    });
                }
                Ok(Err(_)) => {
                    return Json(ChatResponse {
                        response: "Error: Inference task cancelled".to_string(),
                        tokens: 0,
                        tokens_per_second: 0.0,
                        location: "error".to_string(),
                        provider_peer_id: None,
                    });
                }
                Err(_) => {
                    return Json(ChatResponse {
                        response: "Error: Inference timeout (60s)".to_string(),
                        tokens: 0,
                        tokens_per_second: 0.0,
                        location: "error".to_string(),
                        provider_peer_id: None,
                    });
                }
            }
        }
    }

    // Fallback: direct users to CLI
    Json(ChatResponse {
        response: format!(
            "Chat is available via CLI: peerclaw chat --model {}\n\n\
            To enable web chat, restart the node with inference support.",
            model
        ),
        tokens: 0,
        tokens_per_second: 0.0,
        location: "none".to_string(),
        provider_peer_id: None,
    })
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<WebState>) {
    // Send status updates every second
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

    loop {
        interval.tick().await;

        let resource_state = state.resource_monitor.current_state().await;
        let connected_peers = state.connected_peers.read().await.len();
        let active_jobs = *state.active_jobs.read().await;

        let status = serde_json::json!({
            "type": "status",
            "data": {
                "cpu_usage": resource_state.cpu_usage,
                "ram_used_mb": resource_state.ram_total_mb - resource_state.ram_available_mb,
                "ram_total_mb": resource_state.ram_total_mb,
                "connected_peers": connected_peers,
                "active_jobs": active_jobs,
            }
        });

        if socket.send(Message::Text(status.to_string())).await.is_err() {
            break;
        }
    }
}

// === Swarm API Endpoints ===

#[derive(Serialize)]
struct SwarmAgentInfo {
    id: String,
    name: String,
    state: String,
    is_local: bool,
    action_count: u64,
    jobs_completed: u64,
    jobs_failed: u64,
    success_rate: f64,
    created_at: String,
    last_active_at: String,
}

#[derive(Serialize)]
struct SwarmAgentsResponse {
    agents: Vec<SwarmAgentInfo>,
    total: usize,
}

async fn api_swarm_agents(State(state): State<Arc<WebState>>) -> Json<SwarmAgentsResponse> {
    let Some(swarm) = &state.swarm_manager else {
        return Json(SwarmAgentsResponse { agents: vec![], total: 0 });
    };

    let agents = swarm.get_agents();
    let agent_infos: Vec<SwarmAgentInfo> = agents
        .into_iter()
        .map(|a| SwarmAgentInfo {
            id: a.id.to_string(),
            name: a.name.clone(),
            state: a.state_display().to_string(),
            is_local: a.peer_id.is_none(),
            action_count: a.action_count,
            jobs_completed: a.jobs_completed,
            jobs_failed: a.jobs_failed,
            success_rate: a.success_rate(),
            created_at: a.created_at.to_rfc3339(),
            last_active_at: a.last_active_at.to_rfc3339(),
        })
        .collect();

    let total = agent_infos.len();
    Json(SwarmAgentsResponse { agents: agent_infos, total })
}

#[derive(Serialize)]
struct TopologyNode {
    id: String,
    name: String,
    state: String,
    is_local: bool,
    action_count: u64,
    success_rate: f64,
}

#[derive(Serialize)]
struct TopologyEdge {
    source: String,
    target: String,
}

#[derive(Serialize)]
struct SwarmTopologyResponse {
    nodes: Vec<TopologyNode>,
    edges: Vec<TopologyEdge>,
    timestamp: String,
}

async fn api_swarm_topology(State(state): State<Arc<WebState>>) -> Json<SwarmTopologyResponse> {
    let Some(swarm) = &state.swarm_manager else {
        return Json(SwarmTopologyResponse {
            nodes: vec![],
            edges: vec![],
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    };

    let agents = swarm.get_agents();
    let nodes: Vec<TopologyNode> = agents
        .iter()
        .map(|a| TopologyNode {
            id: a.id.to_string(),
            name: a.name.clone(),
            state: a.state_display().to_string(),
            is_local: a.peer_id.is_none(),
            action_count: a.action_count,
            success_rate: a.success_rate(),
        })
        .collect();

    // Build edges: connect local agents to remote agents
    let mut edges = Vec::new();
    let local_agents: Vec<_> = agents.iter().filter(|a| a.peer_id.is_none()).collect();
    let remote_agents: Vec<_> = agents.iter().filter(|a| a.peer_id.is_some()).collect();

    for local in &local_agents {
        for remote in &remote_agents {
            edges.push(TopologyEdge {
                source: local.id.to_string(),
                target: remote.id.to_string(),
            });
        }
    }

    Json(SwarmTopologyResponse {
        nodes,
        edges,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

#[derive(Serialize)]
struct SwarmActionInfo {
    id: String,
    agent_id: String,
    agent_name: String,
    action_type: String,
    details: String,
    timestamp: String,
}

#[derive(Serialize)]
struct SwarmTimelineResponse {
    actions: Vec<SwarmActionInfo>,
    total: usize,
    has_more: bool,
}

async fn api_swarm_timeline(State(state): State<Arc<WebState>>) -> Json<SwarmTimelineResponse> {
    let Some(swarm) = &state.swarm_manager else {
        return Json(SwarmTimelineResponse {
            actions: vec![],
            total: 0,
            has_more: false,
        });
    };

    let actions = swarm.get_actions(50, 0);
    let action_infos: Vec<SwarmActionInfo> = actions
        .into_iter()
        .map(|a| SwarmActionInfo {
            id: a.id.to_string(),
            agent_id: a.agent_id.to_string(),
            agent_name: a.agent_name,
            action_type: format!("{:?}", a.action_type),
            details: a.description,
            timestamp: a.timestamp.to_rfc3339(),
        })
        .collect();

    let total = action_infos.len();
    Json(SwarmTimelineResponse {
        actions: action_infos,
        total,
        has_more: false,
    })
}

// === Task Management API ===

#[derive(Deserialize)]
struct CreateTaskPayload {
    task_type: String,
    description: String,
    model: Option<String>,
    budget: Option<f64>,
}

#[derive(Serialize)]
struct CreateTaskResponse {
    success: bool,
    task_id: Option<String>,
    error: Option<String>,
}

async fn api_create_task(
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateTaskPayload>,
) -> Json<CreateTaskResponse> {
    let task_id = uuid::Uuid::new_v4().to_string();

    let task = WebTask {
        id: task_id.clone(),
        task_type: req.task_type.clone(),
        description: req.description.clone(),
        status: "pending".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        completed_at: None,
        result: None,
        logs: vec![format!("[{}] Task created", chrono::Utc::now().format("%H:%M:%S"))],
        model: req.model.clone(),
        budget: req.budget.unwrap_or(5.0),
        tokens_used: 0,
        iterations: 0,
    };

    state.task_store.write().await.push(task);

    // If we have an agent channel, spawn execution
    if let Some(tx) = &state.agent_task_tx {
        let store = state.task_store.clone();
        let tid = task_id.clone();
        let description = req.description.clone();
        let tx = tx.clone();

        tokio::spawn(async move {
            // Mark as running
            {
                let mut tasks = store.write().await;
                if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                    t.status = "running".to_string();
                    t.logs.push(format!("[{}] Agent started execution", chrono::Utc::now().format("%H:%M:%S")));
                }
            }

            // Send to agent runtime via channel
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            let request = AgentTaskRequest {
                task_id: tid.clone(),
                description,
                response_tx,
                task_store: store.clone(),
            };

            tracing::info!(task_id = %tid, "Sending task to agent runtime");
            if tx.send(request).await.is_ok() {
                tracing::info!(task_id = %tid, "Task sent, waiting for result...");
                match tokio::time::timeout(std::time::Duration::from_secs(300), response_rx).await {
                    Ok(Ok(result)) => {
                        tracing::info!(task_id = %tid, success = result.success, "Task result received, updating store");
                        let mut tasks = store.write().await;
                        if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                            t.status = if result.success { "completed".to_string() } else { "failed".to_string() };
                            t.completed_at = Some(chrono::Utc::now().to_rfc3339());
                            t.result = Some(result.answer);
                            t.tokens_used = result.total_tokens;
                            t.iterations = result.iterations;
                            if let Some(err) = &result.error {
                                t.logs.push(format!("[{}] Error: {}", chrono::Utc::now().format("%H:%M:%S"), err));
                            }
                            t.logs.push(format!(
                                "[{}] Completed: {} iterations, {} tokens, {:.4} PCLAW spent",
                                chrono::Utc::now().format("%H:%M:%S"),
                                result.iterations, result.total_tokens, result.budget_spent,
                            ));
                            for tc in &result.tool_calls {
                                t.logs.push(format!(
                                    "[tool] {} -> {} ({} ms)",
                                    tc.tool_name,
                                    if tc.success { "ok" } else { "failed" },
                                    tc.duration_ms,
                                ));
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::error!(task_id = %tid, error = %e, "Agent task channel closed");
                        let mut tasks = store.write().await;
                        if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                            t.status = "failed".to_string();
                            t.result = Some(format!("Agent channel error: {}", e));
                        }
                    }
                    Err(_) => {
                        tracing::error!(task_id = %tid, "Agent task timed out after 300s");
                        let mut tasks = store.write().await;
                        if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                            t.status = "failed".to_string();
                            t.result = Some("Agent task timed out (300s)".to_string());
                        }
                    }
                }
            }
        });
    } else {
        // No agent runtime - mark as failed
        let mut tasks = state.task_store.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id) {
            t.status = "failed".to_string();
            t.result = Some("No agent runtime loaded. Start with --agent flag.".to_string());
        }
    }

    Json(CreateTaskResponse {
        success: true,
        task_id: Some(task_id),
        error: None,
    })
}

async fn api_list_tasks(State(state): State<Arc<WebState>>) -> Json<Vec<WebTask>> {
    let tasks = state.task_store.read().await;
    Json(tasks.clone())
}

async fn api_task_detail(
    State(state): State<Arc<WebState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    let tasks = state.task_store.read().await;
    if let Some(task) = tasks.iter().find(|t| t.id == id) {
        Json(serde_json::to_value(task).unwrap_or_default())
    } else {
        Json(serde_json::json!({"error": "Task not found"}))
    }
}

// === Provider API ===

#[derive(Serialize)]
struct ProviderInfo {
    peer_id: String,
    models: Vec<ProviderModelInfo>,
    max_requests_per_hour: u32,
    max_tokens_per_day: u64,
}

#[derive(Serialize)]
struct ProviderModelInfo {
    model_name: String,
    context_size: u32,
    price_per_1k_tokens: u64,
    backend: String,
}

async fn api_list_providers(State(state): State<Arc<WebState>>) -> Json<Vec<ProviderInfo>> {
    let Some(tracker) = &state.provider_tracker else {
        return Json(vec![]);
    };

    let manifests = tracker.all_providers().await;
    let providers: Vec<ProviderInfo> = manifests
        .into_iter()
        .map(|m| ProviderInfo {
            peer_id: m.peer_id,
            models: m.models.iter().map(|mo| ProviderModelInfo {
                model_name: mo.model_name.clone(),
                context_size: mo.context_size,
                price_per_1k_tokens: mo.price_per_1k_tokens,
                backend: format!("{}", mo.backend),
            }).collect(),
            max_requests_per_hour: m.rate_limits.max_requests_per_hour,
            max_tokens_per_day: m.rate_limits.max_tokens_per_day,
        })
        .collect();

    Json(providers)
}

#[derive(Serialize)]
struct ProviderConfigResponse {
    enabled: bool,
    max_requests_per_hour: u32,
    max_tokens_per_day: u64,
    max_concurrent_requests: u32,
    price_multiplier: f64,
}

async fn api_get_provider_config(State(state): State<Arc<WebState>>) -> Json<ProviderConfigResponse> {
    let Some(tracker) = &state.provider_tracker else {
        return Json(ProviderConfigResponse {
            enabled: false,
            max_requests_per_hour: 0,
            max_tokens_per_day: 0,
            max_concurrent_requests: 0,
            price_multiplier: 1.0,
        });
    };

    let config = tracker.local_config().await;
    Json(ProviderConfigResponse {
        enabled: config.enabled,
        max_requests_per_hour: config.max_requests_per_hour,
        max_tokens_per_day: config.max_tokens_per_day,
        max_concurrent_requests: config.max_concurrent_requests,
        price_multiplier: config.price_multiplier,
    })
}

#[derive(Deserialize)]
struct SetProviderConfigPayload {
    enabled: Option<bool>,
    max_requests_per_hour: Option<u32>,
    max_tokens_per_day: Option<u64>,
    max_concurrent_requests: Option<u32>,
    price_multiplier: Option<f64>,
}

async fn api_set_provider_config(
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetProviderConfigPayload>,
) -> Json<serde_json::Value> {
    let Some(tracker) = &state.provider_tracker else {
        return Json(serde_json::json!({"error": "Provider tracker not available"}));
    };

    let mut config = tracker.local_config().await;
    if let Some(enabled) = req.enabled {
        config.enabled = enabled;
    }
    if let Some(v) = req.max_requests_per_hour {
        config.max_requests_per_hour = v;
    }
    if let Some(v) = req.max_tokens_per_day {
        config.max_tokens_per_day = v;
    }
    if let Some(v) = req.max_concurrent_requests {
        config.max_concurrent_requests = v;
    }
    if let Some(v) = req.price_multiplier {
        config.price_multiplier = v;
    }

    tracker.set_local_config(config).await;
    Json(serde_json::json!({"success": true}))
}

// === Node Detail API ===

#[derive(Serialize)]
struct NodeDetailResponse {
    id: String,
    is_local: bool,
    name: String,
    state: String,
    tasks: Vec<WebTask>,
    models: Vec<ProviderModelInfo>,
    action_count: u64,
    success_rate: f64,
}

async fn api_node_detail(
    State(state): State<Arc<WebState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<NodeDetailResponse> {
    let local_pid = state.local_peer_id.to_string();
    // Determine if this is the local node (by peer ID match)
    let is_local_node = id == local_pid || id == "local";

    // Check if this is a known swarm agent (match by UUID or by peer ID)
    let (name, agent_state, action_count, success_rate, is_local) = if let Some(swarm) = &state.swarm_manager {
        let agents = swarm.get_agents();
        let found = agents.iter().find(|a| {
            a.id.to_string() == id
            || a.peer_id.as_ref().map(|p| p.to_string()) == Some(id.clone())
            || (is_local_node && a.peer_id.is_none())
        });
        if let Some(agent) = found {
            (
                agent.name.clone(),
                agent.state_display().to_string(),
                agent.action_count,
                agent.success_rate(),
                agent.peer_id.is_none(),
            )
        } else if is_local_node {
            ("This Node (local)".to_string(), "active".to_string(), 0, 1.0, true)
        } else {
            (format!("Peer ...{}", &id[id.len().saturating_sub(8)..]), "connected".to_string(), 0, 0.0, false)
        }
    } else if is_local_node {
        ("This Node (local)".to_string(), "active".to_string(), 0, 1.0, true)
    } else {
        (format!("Peer ...{}", &id[id.len().saturating_sub(8)..]), "connected".to_string(), 0, 0.0, false)
    };

    // Get tasks for this node (local node gets all tasks)
    let tasks = if is_local || is_local_node {
        state.task_store.read().await.clone()
    } else {
        vec![]
    };

    // Get model offerings if this is a known provider
    let models = if let Some(tracker) = &state.provider_tracker {
        let all = tracker.all_providers().await;
        all.into_iter()
            .find(|m| m.peer_id == id || (is_local && m.peer_id == state.local_peer_id.to_string()))
            .map(|m| m.models.iter().map(|mo| ProviderModelInfo {
                model_name: mo.model_name.clone(),
                context_size: mo.context_size,
                price_per_1k_tokens: mo.price_per_1k_tokens,
                backend: format!("{}", mo.backend),
            }).collect())
            .unwrap_or_default()
    } else {
        vec![]
    };

    Json(NodeDetailResponse {
        id,
        is_local,
        name,
        state: agent_state,
        tasks,
        models,
        action_count,
        success_rate,
    })
}
