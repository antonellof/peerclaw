//! Web UI: chat-first assistant at `/`, node console at `/console`.
//!
//! - **Embedded (no `web/dist`)** — `/` and `/chat` serve `chat.html`; `/console` serves `dashboard.html`.
//! - **SPA (`npm run build` in `web/`)** — static assets from `web/dist` with SPA fallback; legacy UIs at
//!   `/embed/chat` and `/embed/console`.
//! - OpenAI-compatible API (`/v1/chat/completions`, `/v1/models`)
//!
//! ## Control plane (WebSocket `/ws`)
//! - Tick: `{ "type": "status", "data": { ... } }` (resource + peers + job counts)
//! - Push: `{ "type": "tasks_changed" }` when agent tasks are created or updated (console debounces `GET /api/tasks`)

pub mod openai;

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Json, Response,
    },
    routing::{delete, get, post, put},
    Router,
};
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::services::{ServeDir, ServeFile};

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
    /// When set, UTF-8 chunks are pushed here as the model generates (local GGUF path streams tokens;
    /// InferenceEngine / remote paths typically send one chunk with the full reply).
    pub stream_delta_tx: Option<mpsc::UnboundedSender<String>>,
}

/// Response to inference request
#[derive(Clone)]
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

/// One turn in a browser chat session (`/api/chat` with `session_id`).
#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Optional curated public bootstrap entries (extend when the project publishes well-known relays).
#[derive(Clone, Serialize)]
pub struct CommunityPeerEntry {
    pub label: String,
    pub multiaddr: String,
}

/// P2P settings snapshot for the console (from `config.toml` at serve time).
#[derive(Clone, Serialize)]
pub struct P2pNetworkHints {
    pub bootstrap_peers: Vec<String>,
    pub mdns_enabled: bool,
    pub kademlia_enabled: bool,
    pub community_peers: Vec<CommunityPeerEntry>,
}

impl Default for P2pNetworkHints {
    fn default() -> Self {
        Self {
            bootstrap_peers: Vec::new(),
            mdns_enabled: true,
            kademlia_enabled: true,
            community_peers: default_community_peer_directory(),
        }
    }
}

pub fn default_community_peer_directory() -> Vec<CommunityPeerEntry> {
    vec![]
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
    /// Stop flags for in-flight web agentic tasks (`POST /api/tasks/:id/stop`).
    pub agent_task_cancels: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>,
    /// Provider tracker for network LLM providers
    pub provider_tracker: Option<Arc<ProviderTracker>>,
    /// Channel to send tasks to the agent runtime
    pub agent_task_tx: Option<mpsc::Sender<AgentTaskRequest>>,
    /// Broadcast JSON control-plane events to `/ws` subscribers (tasks, future job streams).
    pub ws_control_tx: broadcast::Sender<serde_json::Value>,
    /// Skill registry when the full node runtime is wired (discovery for console / future agents).
    pub skills: Option<Arc<crate::skills::SkillRegistry>>,
    /// Bounded chat history keyed by `session_id` for `/api/chat`.
    pub chat_sessions: Arc<RwLock<HashMap<String, Vec<ChatMessage>>>>,
    /// Persistent session store (redb-backed); `None` when running without a data directory.
    pub session_store: Option<Arc<crate::agent::SessionStore>>,
    /// MCP settings (mutable from `PUT /api/mcp/config`; in-memory until you edit `config.toml`).
    pub mcp_config: Arc<RwLock<crate::mcp::McpConfig>>,
    /// Live MCP connections + tool catalog when `reload_mcp_manager` has run successfully.
    pub mcp_manager: Arc<RwLock<Option<Arc<crate::mcp::McpManager>>>>,
    /// Tool registry from the node (`peerclaw serve`); powers agentic chat ReAct loop.
    pub tools: Option<Arc<crate::tools::ToolRegistry>>,
    /// Channel to `job_submit` / `job_status` handlers on the serve loop (P2P marketplace).
    pub node_tool_tx: Option<crate::tools::NodeToolTx>,
    /// Resolved skills directory (matches `SkillRegistry` when `skills` is set).
    pub skills_dir: PathBuf,
    /// Host `config.toml` path (for UI copy hints).
    pub config_path: PathBuf,
    /// When set, `POST /api/peers/dial` queues a multiaddr for the serve loop to dial.
    pub peer_dial_tx: Option<mpsc::Sender<String>>,
    /// Bootstrap list and discovery flags (from node config when running under `serve`).
    pub p2p_network_hints: Arc<P2pNetworkHints>,
    /// Wired on `peerclaw serve` for model downloads and `/api/inference/settings`.
    pub inference: Option<Arc<crate::inference::InferenceEngine>>,
    /// Channel registry for messaging channel management.
    pub channel_registry: Option<Arc<crate::messaging::ChannelRegistry>>,
    /// Full wallet instance for balance + transactions.
    pub wallet: Option<Arc<crate::wallet::Wallet>>,
    /// Vector store for semantic search / memory.
    pub vector_store: Option<Arc<crate::vector::VectorStore>>,
}

fn new_ws_control_plane() -> broadcast::Sender<serde_json::Value> {
    let (tx, _) = broadcast::channel(256);
    tx
}

/// Notify WebSocket subscribers that task rows may have changed.
pub fn broadcast_tasks_changed(tx: &broadcast::Sender<serde_json::Value>) {
    let _ = tx.send(serde_json::json!({ "type": "tasks_changed" }));
}

/// Push agentic loop progress into a web [`WebTask`] so the UI can poll live steps.
#[derive(Clone)]
struct AgenticTaskProgressSink {
    store: Arc<RwLock<Vec<WebTask>>>,
    task_id: String,
    ws_tx: broadcast::Sender<serde_json::Value>,
}

impl AgenticTaskProgressSink {
    async fn append_log(&self, line: String) {
        let mut tasks = self.store.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == self.task_id) {
            t.logs.push(line);
        }
        broadcast_tasks_changed(&self.ws_tx);
    }

    /// ReAct pass number (1-based), not tool-call count — matches "Pass N/…" in logs.
    async fn set_react_pass(&self, pass: u32) {
        let mut tasks = self.store.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == self.task_id) {
            t.iterations = pass;
        }
        broadcast_tasks_changed(&self.ws_tx);
    }

    async fn record_tool_step(&self, line: String, tokens_so_far: u32) {
        let mut tasks = self.store.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == self.task_id) {
            t.logs.push(line);
            t.tokens_used = tokens_so_far;
        }
        broadcast_tasks_changed(&self.ws_tx);
    }

    async fn set_tokens(&self, tokens_so_far: u32) {
        let mut tasks = self.store.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == self.task_id) {
            t.tokens_used = tokens_so_far;
        }
        broadcast_tasks_changed(&self.ws_tx);
    }
}

/// Request to run a task through the agent runtime.
pub struct AgentTaskRequest {
    pub task_id: String,
    pub description: String,
    pub response_tx: tokio::sync::oneshot::Sender<crate::agent::AgentResult>,
    /// Shared task store so the agent can stream logs in real-time
    pub task_store: Arc<RwLock<Vec<WebTask>>>,
    pub ws_control_tx: broadcast::Sender<serde_json::Value>,
    /// Cooperative stop (`POST /api/tasks/:id/stop`) — checked between ReAct iterations.
    pub cancel: Arc<AtomicBool>,
    /// Session id for conversation history continuity.
    pub session_id: Option<String>,
}

/// Static UI: `PEERCLAW_WEB_DIST` if set and valid, else `web/dist` under the crate root (after `npm run build` in `web/`).
pub fn spa_dist_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PEERCLAW_WEB_DIST") {
        let path = PathBuf::from(p);
        if path.join("index.html").is_file() {
            return Some(path);
        }
    }
    let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/dist");
    dist.join("index.html").is_file().then_some(dist)
}

/// Task routes under `/api/…` via `nest`. Path params **must** use `:id` (Axum 0.7 / matchit); `{id}` never matches.
fn api_tasks_router() -> Router<Arc<WebState>> {
    Router::new()
        .route("/tasks/:id/stop", post(api_task_stop))
        .route("/tasks/:id", get(api_task_detail))
        .route("/tasks", post(api_create_task))
        .route("/tasks", get(api_list_tasks))
}

/// REST handlers under `/api/...` (nested so static `ServeDir` fallback never handles POST/OPTIONS for these paths).
fn api_router() -> Router<Arc<WebState>> {
    Router::new()
        .merge(api_tasks_router())
        .route("/status", get(api_status))
        .route("/onboarding", get(api_onboarding))
        .route("/peers/dial", post(api_peers_dial))
        .route("/peers/network", get(api_peers_network))
        .route("/peers", get(api_peers))
        .route("/jobs", get(api_jobs))
        .route("/tools", get(api_tools))
        .route("/skills/local", get(api_skills_local))
        .route("/skills/network", get(api_skills_network))
        .route("/skills/meta", get(api_skills_meta))
        .route("/skills/scan", post(api_skills_scan))
        .route("/skills/templates", get(api_skills_templates))
        .route("/skills/:name/toggle", post(api_skills_toggle))
        .route("/skills/studio/ai", post(api_skills_studio_ai))
        .route("/skills/studio", get(api_skills_studio_list))
        .route(
            "/skills/studio/:slug",
            get(api_skills_studio_get).put(api_skills_studio_put),
        )
        .route("/mcp/status", get(api_mcp_status))
        .route("/mcp/config", put(api_mcp_put_config))
        .route("/jobs/submit", post(api_submit_job))
        .route("/chat", post(api_chat))
        .route("/chat/stream", post(api_chat_stream))
        .route("/providers", get(api_list_providers))
        .route("/providers/config", get(api_get_provider_config))
        .route("/providers/config", post(api_set_provider_config))
        .route("/nodes/:id", get(api_node_detail))
        .route("/swarm/agents", get(api_swarm_agents))
        .route("/swarm/topology", get(api_swarm_topology))
        .route("/swarm/timeline", get(api_swarm_timeline))
        .route(
            "/inference/settings",
            get(api_inference_settings_get).put(api_inference_settings_put),
        )
        .route("/models/download", post(api_models_download))
        // Channel management
        .route("/channels", get(api_list_channels))
        .route("/channels", post(api_register_channel))
        .route("/channels/:id", delete(api_remove_channel))
        .route("/channels/:id/test", post(api_test_channel))
        // Wallet
        .route("/wallet", get(api_wallet_balance))
        .route("/wallet/transactions", get(api_wallet_transactions))
        // Vector memory
        .route("/vector/collections", get(api_vector_list_collections))
        .route("/vector/collections", post(api_vector_create_collection))
        .route(
            "/vector/collections/:name",
            delete(api_vector_delete_collection),
        )
        .route("/vector/search", post(api_vector_search))
        .route("/vector/insert", post(api_vector_insert))
        // Tool execution
        .route("/tools/execute", post(api_tool_execute))
        .route("/tools/:name", get(api_tool_detail))
}

/// Create the web router.
pub fn create_router(state: Arc<WebState>) -> Router {
    let spa = spa_dist_dir();
    let mut router = Router::new()
        .nest("/api", api_router())
        .route("/ws", get(ws_handler))
        // OpenAI-compatible API routes
        .route("/v1/chat/completions", post(openai::chat_completions))
        .route("/v1/models", get(openai::list_models))
        .route("/v1/embeddings", post(openai::embeddings));

    router = if let Some(dist) = spa {
        let index_path = dist.join("index.html");
        router
            .route("/embed/chat", get(assistant_index))
            .route("/embed/console", get(console_index))
            .fallback_service(ServeDir::new(dist).not_found_service(ServeFile::new(index_path)))
    } else {
        router
            .route("/", get(assistant_index))
            .route("/chat", get(assistant_index))
            .route("/console", get(console_index))
    };

    router.with_state(state)
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
        agent_task_cancels: Arc::new(RwLock::new(HashMap::new())),
        provider_tracker: None,
        agent_task_tx: None,
        ws_control_tx: new_ws_control_plane(),
        skills: None,
        chat_sessions: Arc::new(RwLock::new(HashMap::new())),
        session_store: None,
        mcp_config: Arc::new(RwLock::new(crate::mcp::McpConfig::default())),
        mcp_manager: Arc::new(RwLock::new(None)),
        tools: None,
        node_tool_tx: None,
        skills_dir: crate::bootstrap::base_dir().join("skills"),
        config_path: crate::bootstrap::base_dir().join("config.toml"),
        peer_dial_tx: None,
        p2p_network_hints: Arc::new(P2pNetworkHints::default()),
        inference: None,
        channel_registry: None,
        wallet: None,
        vector_store: None,
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
        agent_task_cancels: Arc::new(RwLock::new(HashMap::new())),
        provider_tracker: None,
        agent_task_tx: None,
        ws_control_tx: new_ws_control_plane(),
        skills: None,
        chat_sessions: Arc::new(RwLock::new(HashMap::new())),
        session_store: None,
        mcp_config: Arc::new(RwLock::new(crate::mcp::McpConfig::default())),
        mcp_manager: Arc::new(RwLock::new(None)),
        tools: None,
        node_tool_tx: None,
        skills_dir: crate::bootstrap::base_dir().join("skills"),
        config_path: crate::bootstrap::base_dir().join("config.toml"),
        peer_dial_tx: None,
        p2p_network_hints: Arc::new(P2pNetworkHints::default()),
        inference: None,
        channel_registry: None,
        wallet: None,
        vector_store: None,
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
        agent_task_cancels: Arc::new(RwLock::new(HashMap::new())),
        provider_tracker: None,
        agent_task_tx: None,
        ws_control_tx: new_ws_control_plane(),
        skills: None,
        chat_sessions: Arc::new(RwLock::new(HashMap::new())),
        session_store: None,
        mcp_config: Arc::new(RwLock::new(crate::mcp::McpConfig::default())),
        mcp_manager: Arc::new(RwLock::new(None)),
        tools: None,
        node_tool_tx: None,
        skills_dir: crate::bootstrap::base_dir().join("skills"),
        config_path: crate::bootstrap::base_dir().join("config.toml"),
        peer_dial_tx: None,
        p2p_network_hints: Arc::new(P2pNetworkHints::default()),
        inference: None,
        channel_registry: None,
        wallet: None,
        vector_store: None,
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
        agent_task_cancels: Arc::new(RwLock::new(HashMap::new())),
        provider_tracker: None,
        agent_task_tx: None,
        ws_control_tx: new_ws_control_plane(),
        skills: None,
        chat_sessions: Arc::new(RwLock::new(HashMap::new())),
        session_store: None,
        mcp_config: Arc::new(RwLock::new(crate::mcp::McpConfig::default())),
        mcp_manager: Arc::new(RwLock::new(None)),
        tools: None,
        node_tool_tx: None,
        skills_dir: crate::bootstrap::base_dir().join("skills"),
        config_path: crate::bootstrap::base_dir().join("config.toml"),
        peer_dial_tx: None,
        p2p_network_hints: Arc::new(P2pNetworkHints::default()),
        inference: None,
        channel_registry: None,
        wallet: None,
        vector_store: None,
    })
}

/// Start the web server.
pub async fn start_server(addr: SocketAddr, state: Arc<WebState>) -> anyhow::Result<()> {
    let spa = spa_dist_dir().is_some();
    // Layer order: **NormalizePath outermost** so the URI is fixed before CORS and the router.
    // If CORS is outermost, some requests no longer match `/api/tasks/:id` and fall through to SPA HTML.
    let app = create_router(state)
        .layer(
            CorsLayer::very_permissive()
                .allow_private_network(true),
        )
        .layer(NormalizePathLayer::trim_trailing_slash());

    if spa {
        tracing::info!(
            "Web: SPA from web/dist (legacy chat /embed/chat  console /embed/console) http://{}",
            addr
        );
    } else {
        tracing::info!(
            "Web: assistant http://{}  console http://{}/console",
            addr,
            addr
        );
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// === API Endpoints ===

async fn assistant_index() -> Html<&'static str> {
    Html(include_str!("chat.html"))
}

async fn console_index() -> Html<&'static str> {
    Html(include_str!("dashboard.html"))
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
struct OnboardingStep {
    id: &'static str,
    ok: bool,
    detail: String,
}

#[derive(Serialize)]
struct OnboardingResponse {
    peer_id: String,
    steps: Vec<OnboardingStep>,
}

async fn api_onboarding(State(state): State<Arc<WebState>>) -> Json<OnboardingResponse> {
    let peer_id = state.local_peer_id.to_string();
    let peer_count = state.connected_peers.read().await.len();
    let skill_count = if let Some(reg) = &state.skills {
        reg.list_all().await.len()
    } else {
        0
    };

    Json(OnboardingResponse {
        peer_id,
        steps: vec![
            OnboardingStep {
                id: "inference",
                ok: state.inference_tx.is_some(),
                detail: if state.inference_tx.is_some() {
                    "Web /api/chat can use the node inference channel (local or P2P-routed)."
                        .to_string()
                } else {
                    "Inference channel not wired to this web state; chat falls back to CLI hints."
                        .to_string()
                },
            },
            OnboardingStep {
                id: "agent_tasks",
                ok: state.agent_task_tx.is_some(),
                detail: if state.agent_task_tx.is_some() {
                    "Console Agent tab runs ReAct tasks against the loaded agent runtime.".to_string()
                } else {
                    "Start with --agent and an agent.toml to enable console agent tasks.".to_string()
                },
            },
            OnboardingStep {
                id: "p2p_peers",
                ok: peer_count > 0,
                detail: format!(
                    "{peer_count} connected peer(s). libp2p discovery needs reachable listen addresses."
                ),
            },
            OnboardingStep {
                id: "job_marketplace",
                ok: state.job_submit_tx.is_some(),
                detail: if state.job_submit_tx.is_some() {
                    "Job submission from the console is available.".to_string()
                } else {
                    "Job submission channel not attached.".to_string()
                },
            },
            OnboardingStep {
                id: "skills_registry",
                ok: state.skills.is_some(),
                detail: if state.skills.is_some() {
                    format!("Skill registry present ({skill_count} entries in list_all).")
                } else {
                    "Skill registry not attached to web state.".to_string()
                },
            },
        ],
    })
}

async fn api_skills_local(
    State(state): State<Arc<WebState>>,
) -> Json<Vec<crate::skills::SkillInfo>> {
    let Some(reg) = &state.skills else {
        return Json(vec![]);
    };
    let all = reg.list_all().await;
    Json(
        all.into_iter()
            .filter(|s| s.trust != crate::skills::SkillTrust::Network)
            .collect(),
    )
}

async fn api_skills_network(
    State(state): State<Arc<WebState>>,
) -> Json<Vec<crate::skills::SkillInfo>> {
    let Some(reg) = &state.skills else {
        return Json(vec![]);
    };
    let all = reg.list_all().await;
    Json(
        all.into_iter()
            .filter(|s| s.trust == crate::skills::SkillTrust::Network)
            .collect(),
    )
}

#[derive(Serialize)]
struct SkillsMetaResponse {
    skills_dir: String,
    config_path: String,
    registry_attached: bool,
    scan_cli: &'static str,
    list_cli: &'static str,
    /// Optional `config.toml` snippet to override the default skills directory.
    directory_toml_snippet: &'static str,
}

async fn api_skills_meta(State(state): State<Arc<WebState>>) -> Json<SkillsMetaResponse> {
    Json(SkillsMetaResponse {
        skills_dir: state.skills_dir.display().to_string(),
        config_path: state.config_path.display().to_string(),
        registry_attached: state.skills.is_some(),
        scan_cli: "peerclaw skill scan",
        list_cli: "peerclaw skill list",
        directory_toml_snippet: "# Optional — default is ~/.peerclaw/skills\n[skills]\ndirectory = \"/path/to/skills\"\n",
    })
}

async fn api_skills_scan(State(state): State<Arc<WebState>>) -> Json<serde_json::Value> {
    let Some(reg) = &state.skills else {
        return Json(serde_json::json!({
            "ok": false,
            "error": "Skill registry not attached (start full node with web, e.g. peerclaw serve --web)."
        }));
    };
    match reg.scan().await {
        Ok(n) => Json(serde_json::json!({ "ok": true, "loaded": n })),
        Err(e) => Json(serde_json::json!({
            "ok": false,
            "error": e.to_string()
        })),
    }
}

// ---- Skill templates: bundled example skills from examples/skills/ ----

#[derive(Serialize)]
struct SkillTemplate {
    name: String,
    version: String,
    description: String,
    author: String,
    keywords: Vec<String>,
    tags: Vec<String>,
    trust: &'static str,
    content: String,
}

fn load_bundled_templates() -> Vec<SkillTemplate> {
    let candidates: Vec<std::path::PathBuf> = {
        let mut v = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                v.push(parent.join("../../examples/skills"));
                v.push(parent.join("examples/skills"));
            }
        }
        v.push(std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/skills"
        )));
        v
    };

    let skills_dir = match candidates.iter().find(|p| p.is_dir()) {
        Some(d) => d.clone(),
        None => return vec![],
    };

    let mut templates = Vec::new();
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return templates;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let skill_file = if path.is_dir() {
            path.join("SKILL.md")
        } else if path.extension().is_some_and(|e| e == "md") {
            path.clone()
        } else {
            continue;
        };
        if !skill_file.exists() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&skill_file) else {
            continue;
        };
        let Ok(skill) = crate::skills::parser::parse_skill_content(
            &content,
            crate::skills::SkillSource::Bundled("examples".into()),
            crate::skills::SkillTrust::Local,
        ) else {
            continue;
        };
        templates.push(SkillTemplate {
            name: skill.manifest.name.clone(),
            version: skill.manifest.version.clone(),
            description: skill.manifest.description.clone(),
            author: skill.manifest.author.clone().unwrap_or_default(),
            keywords: skill.manifest.activation.keywords.clone(),
            tags: skill.manifest.activation.tags.clone(),
            trust: "local",
            content,
        });
    }

    templates.sort_by(|a, b| a.name.cmp(&b.name));
    templates
}

async fn api_skills_templates() -> Json<Vec<SkillTemplate>> {
    Json(load_bundled_templates())
}

async fn api_skills_toggle(
    State(state): State<Arc<WebState>>,
    Path(name): Path<String>,
) -> Json<serde_json::Value> {
    let Some(reg) = &state.skills else {
        return Json(serde_json::json!({
            "ok": false,
            "error": "Skill registry not attached."
        }));
    };
    match reg.toggle_skill(&name).await {
        Some(enabled) => Json(serde_json::json!({ "ok": true, "enabled": enabled })),
        None => Json(serde_json::json!({
            "ok": false,
            "error": format!("Skill '{}' not found.", name)
        })),
    }
}

// ---- Skill studio: edit SKILL.md on disk + optional AI assist (same inference queue as chat) ----

#[derive(Serialize)]
struct SkillStudioListEntry {
    slug: String,
    layout: &'static str,
}

#[derive(Serialize)]
struct SkillStudioGetResponse {
    slug: String,
    content: String,
    layout: &'static str,
}

#[derive(Deserialize)]
struct SkillStudioPutBody {
    content: String,
}

#[derive(Deserialize)]
struct SkillStudioAiRequest {
    content: String,
    instruction: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct SkillStudioAiResponse {
    text: String,
    tokens: u32,
}

fn studio_err_json(
    status: StatusCode,
    msg: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({ "error": msg.into() })),
    )
}

fn resolve_skill_read_path(
    skills_dir: &std::path::Path,
    slug: &str,
) -> Result<(std::path::PathBuf, &'static str), (StatusCode, Json<serde_json::Value>)> {
    if !crate::skills::validate_skill_name(slug) {
        return Err(studio_err_json(StatusCode::BAD_REQUEST, "invalid skill slug"));
    }
    let root = std::fs::canonicalize(skills_dir).map_err(|e| {
        studio_err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("skills dir: {e}"),
        )
    })?;
    let nested = root.join(slug).join("SKILL.md");
    let flat = root.join(format!("{slug}.md"));
    if nested.is_file() {
        let c = std::fs::canonicalize(&nested).map_err(|e| {
            studio_err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
        if !c.starts_with(&root) {
            return Err(studio_err_json(StatusCode::FORBIDDEN, "path escape"));
        }
        Ok((c, "nested"))
    } else if flat.is_file() {
        let c = std::fs::canonicalize(&flat).map_err(|e| {
            studio_err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
        if !c.starts_with(&root) {
            return Err(studio_err_json(StatusCode::FORBIDDEN, "path escape"));
        }
        Ok((c, "flat"))
    } else {
        Err(studio_err_json(
            StatusCode::NOT_FOUND,
            "skill file not found",
        ))
    }
}

fn resolve_skill_write_path(
    skills_dir: &std::path::Path,
    slug: &str,
) -> Result<std::path::PathBuf, (StatusCode, Json<serde_json::Value>)> {
    if !crate::skills::validate_skill_name(slug) {
        return Err(studio_err_json(StatusCode::BAD_REQUEST, "invalid skill slug"));
    }
    let root = std::fs::canonicalize(skills_dir).map_err(|e| {
        studio_err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("skills dir: {e}"),
        )
    })?;
    let dir = root.join(slug);
    if dir.exists() {
        let c = std::fs::canonicalize(&dir).map_err(|e| {
            studio_err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
        if !c.starts_with(&root) {
            return Err(studio_err_json(StatusCode::FORBIDDEN, "path escape"));
        }
        Ok(c.join("SKILL.md"))
    } else if !dir.starts_with(&root) {
        Err(studio_err_json(StatusCode::FORBIDDEN, "path escape"))
    } else {
        Ok(dir.join("SKILL.md"))
    }
}

async fn api_skills_studio_list(
    State(state): State<Arc<WebState>>,
) -> Result<Json<Vec<SkillStudioListEntry>>, (StatusCode, Json<serde_json::Value>)> {
    let root = &state.skills_dir;
    let rd = std::fs::read_dir(root).map_err(|e| {
        studio_err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        )
    })?;
    let mut items = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        if path.is_dir() {
            let sm = path.join("SKILL.md");
            if sm.is_file() {
                let slug = e.file_name().to_string_lossy().to_string();
                if crate::skills::validate_skill_name(&slug) {
                    items.push(SkillStudioListEntry {
                        slug,
                        layout: "nested",
                    });
                }
            }
        } else if path.extension().is_some_and(|x| x == "md") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if crate::skills::validate_skill_name(stem) {
                    items.push(SkillStudioListEntry {
                        slug: stem.to_string(),
                        layout: "flat",
                    });
                }
            }
        }
    }
    items.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(Json(items))
}

async fn api_skills_studio_get(
    Path(slug): Path<String>,
    State(state): State<Arc<WebState>>,
) -> Result<Json<SkillStudioGetResponse>, (StatusCode, Json<serde_json::Value>)> {
    let (path, layout) = resolve_skill_read_path(&state.skills_dir, &slug)?;
    let meta = std::fs::metadata(&path).map_err(|e| {
        studio_err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    if meta.len() > crate::skills::MAX_SKILL_SIZE {
        return Err(studio_err_json(
            StatusCode::PAYLOAD_TOO_LARGE,
            "SKILL.md too large",
        ));
    }
    let content = std::fs::read_to_string(&path).map_err(|e| {
        studio_err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    Ok(Json(SkillStudioGetResponse {
        slug,
        content,
        layout,
    }))
}

async fn api_skills_studio_put(
    Path(slug): Path<String>,
    State(state): State<Arc<WebState>>,
    Json(body): Json<SkillStudioPutBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let b = body.content.as_bytes();
    if b.len() as u64 > crate::skills::MAX_SKILL_SIZE {
        return Err(studio_err_json(
            StatusCode::PAYLOAD_TOO_LARGE,
            "content exceeds max skill size (64 KiB)",
        ));
    }
    let path = resolve_skill_write_path(&state.skills_dir, &slug)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            studio_err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
    }
    std::fs::write(&path, &body.content).map_err(|e| {
        studio_err_json(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "slug": slug,
        "path": path.display().to_string()
    })))
}

async fn run_studio_inference(
    state: &WebState,
    prompt: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
) -> Result<InferenceResponse, (StatusCode, Json<serde_json::Value>)> {
    let Some(tx) = &state.inference_tx else {
        return Err(studio_err_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "Inference not available on this node.",
        ));
    };
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let request = InferenceRequest {
        prompt,
        model,
        max_tokens,
        temperature,
        response_tx,
        stream_delta_tx: None,
    };
    tx.send(request)
        .await
        .map_err(|_| studio_err_json(StatusCode::INTERNAL_SERVER_ERROR, "failed to queue inference"))?;
    match tokio::time::timeout(Duration::from_secs(120), response_rx).await {
        Ok(Ok(r)) => Ok(r),
        Ok(Err(_)) => Err(studio_err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "inference cancelled",
        )),
        Err(_) => Err(studio_err_json(
            StatusCode::GATEWAY_TIMEOUT,
            "inference timeout",
        )),
    }
}

async fn api_skills_studio_ai(
    State(state): State<Arc<WebState>>,
    Json(req): Json<SkillStudioAiRequest>,
) -> Result<Json<SkillStudioAiResponse>, (StatusCode, Json<serde_json::Value>)> {
    let model = req.model.unwrap_or_else(|| "llama-3.2-3b".to_string());
    let max_tokens = req.max_tokens.unwrap_or(2048).min(8192);
    let temperature = req.temperature.unwrap_or(0.3);

    let system_rules = r#"You help edit PeerClaw SKILL.md files.
Each file MUST begin with YAML frontmatter between --- lines containing at least: name, version, description.
The body is markdown: instructions injected into the agent when the skill activates.

Output rules:
- Return ONLY the complete SKILL.md file contents (frontmatter + body).
- Do NOT wrap the file in markdown code fences.
- Do NOT add commentary before or after the file."#;

    let prompt = format!(
        "{system_rules}\n\nUser instruction:\n{}\n\n--- Current SKILL.md ---\n{}",
        req.instruction.trim(),
        req.content
    );

    let inf = run_studio_inference(state.as_ref(), prompt, model, max_tokens, temperature).await?;

    Ok(Json(SkillStudioAiResponse {
        text: inf.text,
        tokens: inf.tokens_generated,
    }))
}

/// Upper bound on LLM↔tool rounds (avoids unbounded memory growth on the conversation).
const AGENTIC_MAX_ITERS: u32 = 12;
/// Cap parallel tool calls per model response so one bad turn cannot flood the executor / UI.
const AGENTIC_MAX_TOOL_CALLS_PER_PASS: usize = 6;

fn default_chat_agentic() -> bool {
    true
}

async fn build_agentic_system_prefix(
    registry: Option<&crate::tools::ToolRegistry>,
    mcp: Option<&crate::mcp::McpManager>,
    include_mcp_catalog: bool,
) -> String {
    use crate::tools::ToolLocation;
    let mut s = String::from(
        "To use a tool, write:\n\
         <tool_call>\nname: TOOL_NAME\nargs: {\"param\": \"value\"}\n</tool_call>\n\n\
         Example:\n\
         <tool_call>\nname: web_search\nargs: {\"query\": \"AI agents 2026\"}\n</tool_call>\n\n\
         Rules: Use 1-3 tool calls per turn. If a tool fails, answer from knowledge. Never guess URLs.\n\n",
    );
    if let Some(registry) = registry {
        s.push_str("You are a helpful assistant with local tools. Use exact tool names.\n\nTools:\n");
        let mut infos = registry.list_tools().await;
        infos.retain(|t| matches!(t.location, ToolLocation::Local));
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        // Keep only the most useful tools for small-context models.
        let priority_tools = [
            "web_fetch", "web_search", "shell", "file_read", "file_write", "apply_patch",
            "browser", "pdf_read", "json", "memory_search", "memory_write",
            "llm_task", "agent_spawn", "job_submit", "job_status",
        ];
        let (priority, rest): (Vec<_>, Vec<_>) = infos.into_iter()
            .partition(|t| priority_tools.contains(&t.name.as_str()));
        for t in &priority {
            s.push_str(&format!("- {}: {}\n", t.name, t.description));
        }
        if !rest.is_empty() {
            s.push_str(&format!("- (and {} more: {})\n",
                rest.len(),
                rest.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
    } else {
        s.push_str(
            "You are a helpful assistant with **MCP (Model Context Protocol)** tools only (no PeerClaw local tool registry on this endpoint).\n\n\
             Tool names MUST be MCP ids: `server:tool_name` as listed below.\n\
             After tool results, continue until you can answer the user without more tools.\n",
        );
    }
    if include_mcp_catalog {
        if let Some(manager) = mcp {
            if manager.tool_count() > 0 {
                s.push_str("\n### MCP tools\n");
                let mut entries = manager.list_tools_with_ids();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                for (id, tool) in entries {
                    s.push_str(&format!(
                        "- **{}**: {}\n",
                        id,
                        tool.description.as_deref().unwrap_or("(no description)")
                    ));
                }
            }
        }
    }
    s
}

#[allow(clippy::too_many_arguments)]
async fn run_unified_agentic_inference(
    state: &WebState,
    registry: Option<Arc<crate::tools::ToolRegistry>>,
    mcp: Option<Arc<crate::mcp::McpManager>>,
    include_mcp_catalog: bool,
    conversation_body: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
    progress: Option<AgenticTaskProgressSink>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<(InferenceResponse, Vec<String>), String> {
    let Some(tx) = &state.inference_tx else {
        return Err("Inference not available".into());
    };
    let prefix = build_agentic_system_prefix(
        registry.as_deref(),
        mcp.as_deref(),
        include_mcp_catalog,
    )
    .await;
    let mut conversation = format!("{prefix}\n\n{conversation_body}");
    let prefix_len = prefix.len();
    let mut tool_logs: Vec<String> = Vec::new();
    let mut total_tokens: u32 = 0;
    let tool_session = uuid::Uuid::new_v4().to_string();

    // Estimate prompt budget: model context size (chars ≈ tokens * 4) minus output tokens.
    // For small local GGUF models (4096 ctx), this is critical.
    // For remote APIs with large contexts, 48k chars is a safe upper bound.
    let model_ctx_chars = state
        .inference
        .as_ref()
        .map(|inf| inf.config_context_size() as usize * 4)
        .unwrap_or(48_000);
    let output_budget_chars = (max_tokens as usize) * 4;
    let prompt_budget = model_ctx_chars.saturating_sub(output_budget_chars + 200);
    // At least 2k chars for the prompt, otherwise skip agentic entirely.
    let conv_max_chars = prompt_budget.max(2_000);

    /// Bail out after this many consecutive passes where every tool call fails.
    const MAX_CONSECUTIVE_FAIL_PASSES: u32 = 2;
    let mut consecutive_all_fail_passes: u32 = 0;
    /// Track how many times we auto-generated tool calls for a planning model.
    let mut auto_action_count: u32 = 0;
    /// Track URLs already fetched to avoid re-fetching.
    let mut fetched_urls: HashSet<String> = HashSet::new();

    for iter in 1..=AGENTIC_MAX_ITERS {
        // Compact conversation to fit model context using smart pruning.
        if conversation.len() > conv_max_chars {
            conversation = crate::agent::compaction::prune_string_conversation(
                &conversation,
                prefix_len,
                conv_max_chars,
            );
        }

        // On the last allowed iteration, force text: tell the model to stop using tools.
        if iter == AGENTIC_MAX_ITERS {
            conversation.push_str(
                "\n\n(System: This is your FINAL turn. Do NOT call any tools. \
                 Write your complete answer directly to the user NOW.)\n",
            );
        }
        if cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Acquire))
        {
            return Err("Stopped by user".into());
        }
        if let Some(ref sink) = progress {
            sink.set_react_pass(iter).await;
            sink.append_log(format!(
                "[{}] Pass {}/{}: requesting model…",
                chrono::Utc::now().format("%H:%M:%S"),
                iter,
                AGENTIC_MAX_ITERS
            ))
            .await;
        }
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let request = InferenceRequest {
            prompt: conversation.clone(),
            model: model.clone(),
            max_tokens,
            temperature,
            response_tx,
            stream_delta_tx: None,
        };
        tx.send(request)
            .await
            .map_err(|_| "failed to queue inference".to_string())?;
        // Wait until inference finishes — no wall-clock cap (long local/remote runs, large max_tokens).
        // Stops on engine error/cancel via channel drop or Err from oneshot.
        let inf = match response_rx.await {
            Ok(r) => r,
            Err(_) => return Err("inference cancelled".into()),
        };
        total_tokens = total_tokens.saturating_add(inf.tokens_generated);
        if let Some(ref sink) = progress {
            sink.set_tokens(total_tokens).await;
        }
        let text = inf.text;

        // Detect inference errors (e.g. context overflow, model crash) — retry with compacted context.
        if inf.location == "error" || text.starts_with("Error: Inference error:") || text.starts_with("Error: ") {
            if let Some(ref sink) = progress {
                sink.append_log(format!(
                    "[{}] Inference error on pass {}: {} — compacting and retrying",
                    chrono::Utc::now().format("%H:%M:%S"),
                    iter,
                    text.chars().take(120).collect::<String>()
                ))
                .await;
            }
            // Aggressively compact: keep only prefix + the original goal.
            let goal_start = conversation.find("Goal:").or_else(|| conversation.find("### Agent goal"))
                .or_else(|| conversation.find("### User thread"))
                .unwrap_or(prefix_len);
            let goal_end = conversation[goal_start..].find("\n\nAssistant:")
                .map(|i| goal_start + i)
                .unwrap_or(conversation.len().min(goal_start + 2000));
            let goal_section = conversation[goal_start..goal_end].to_string();
            conversation = format!(
                "{}\n\n{}\n\n(System: Earlier tool results were dropped due to context limits. Answer from your knowledge now. Do NOT call tools.)\n",
                &conversation[..prefix_len],
                goal_section
            );
            continue;
        }

        let mut calls = crate::agent::parse_tool_calls(&text);
        if calls.is_empty() {
            // Detect "plan without action" — model describes what it *would* do but
            // doesn't emit tool_call blocks. Auto-generate tool calls from context.
            // After 2 auto-actions (search + fetch), force the model to answer instead.
            if auto_action_count < 2 {
                let plan_phrases = ["let me search", "let me gather", "let me look",
                    "i'll search", "i'll research", "i'll fetch", "i'll start by",
                    "sub-questions", "let me find", "let me check", "i'll gather",
                    "let me explore", "i'll explore", "i'll investigate"];
                let lower = text.to_lowercase();
                let is_plan = plan_phrases.iter().any(|p| lower.contains(p));

                if is_plan {
                    // Try to extract a URL from search results that we haven't fetched yet.
                    let url_re = regex::Regex::new(r#""url"\s*:\s*"(https?://[^"]+)""#).ok();
                    let fresh_url = url_re.and_then(|re| {
                        re.captures_iter(&conversation)
                            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
                            .find(|u| !fetched_urls.contains(u))
                    });

                    if let Some(url) = fresh_url {
                        fetched_urls.insert(url.clone());
                        calls = vec![crate::agent::ParsedToolCall {
                            name: "web_fetch".to_string(),
                            args: serde_json::json!({ "url": url }),
                        }];
                    } else {
                        // No fresh URLs — auto-search the user's goal.
                        let goal = conversation_body.lines()
                            .find(|l| !l.starts_with('#') && !l.starts_with("Tool ") && l.len() > 10)
                            .unwrap_or(&conversation_body)
                            .trim()
                            .chars().take(200).collect::<String>();
                        calls = vec![crate::agent::ParsedToolCall {
                            name: "web_search".to_string(),
                            args: serde_json::json!({ "query": goal }),
                        }];
                    }
                    auto_action_count += 1;

                    if let Some(ref sink) = progress {
                        sink.append_log(format!(
                            "[{}] Pass {}: model planned but didn't call tools — auto-{}",
                            chrono::Utc::now().format("%H:%M:%S"),
                            iter,
                            if calls[0].name == "web_fetch" { "fetching URL" } else { "searching" },
                        )).await;
                    }
                }
            } else if auto_action_count >= 2 && calls.is_empty() {
                // We've done 2 auto-actions (search + fetch). Force the model to answer.
                conversation.push_str("\n\nAssistant:\n");
                conversation.push_str(&text);
                conversation.push_str(
                    "\n\nUser:\n(System: You have search results and page content above. \
                     Write your complete answer NOW. Do NOT call any more tools.)\n",
                );
                if let Some(ref sink) = progress {
                    sink.append_log(format!(
                        "[{}] Pass {}: forcing answer after {} auto-actions",
                        chrono::Utc::now().format("%H:%M:%S"),
                        iter, auto_action_count,
                    )).await;
                }
                continue;
            }

            if calls.is_empty() {
                let cleaned = crate::agent::extract_answer(&text);
                let text_out = if cleaned.trim().is_empty() {
                    "(No text in the model's final reply after stripping tool markup. See task steps / logs for tool results, or retry.)"
                        .to_string()
                } else {
                    cleaned
                };
                return Ok((
                    InferenceResponse {
                        text: text_out,
                        tokens_generated: total_tokens,
                        tokens_per_second: inf.tokens_per_second,
                        location: inf.location,
                        provider_peer_id: inf.provider_peer_id,
                    },
                    tool_logs,
                ));
            }
        }
        // Small models often repeat the same tool_call many times; merge before execute/log.
        let model_tool_call_count = calls.len();
        let mut seen_sig: HashSet<(String, String)> = HashSet::new();
        calls.retain(|call| {
            let sig = (call.name.clone(), call.args.to_string());
            seen_sig.insert(sig)
        });
        let duplicate_calls_merged = model_tool_call_count.saturating_sub(calls.len());

        if let Some(ref sink) = progress {
            let mut msg = format!(
                "[{}] Pass {}: {} tool call(s)",
                chrono::Utc::now().format("%H:%M:%S"),
                iter,
                model_tool_call_count
            );
            if duplicate_calls_merged > 0 {
                msg.push_str(&format!(
                    " → {} unique (merged {} duplicate(s))",
                    calls.len(),
                    duplicate_calls_merged
                ));
            }
            sink.append_log(msg).await;
        }
        let dropped_calls = if calls.len() > AGENTIC_MAX_TOOL_CALLS_PER_PASS {
            let n = calls.len() - AGENTIC_MAX_TOOL_CALLS_PER_PASS;
            calls.truncate(AGENTIC_MAX_TOOL_CALLS_PER_PASS);
            if let Some(ref sink) = progress {
                sink.append_log(format!(
                    "[{}] Pass {}: executing first {} of {} tool call(s) (max {} per turn)",
                    chrono::Utc::now().format("%H:%M:%S"),
                    iter,
                    AGENTIC_MAX_TOOL_CALLS_PER_PASS,
                    AGENTIC_MAX_TOOL_CALLS_PER_PASS + n,
                    AGENTIC_MAX_TOOL_CALLS_PER_PASS
                ))
                .await;
            }
            Some(n)
        } else {
            None
        };

        conversation.push_str("\n\nAssistant:\n");
        conversation.push_str(&text);
        conversation.push_str("\n\nUser:\n");
        if duplicate_calls_merged > 0 {
            conversation.push_str(&format!(
                "(System: {duplicate_calls_merged} repeated tool call(s) with identical name+args were merged; each unique call runs once. Prefer a single well-formed call per intent.)\n"
            ));
        }
        if let Some(d) = dropped_calls {
            conversation.push_str(&format!(
                "(System: {d} tool call(s) in this reply were skipped — max {AGENTIC_MAX_TOOL_CALLS_PER_PASS} per turn. Use fewer, complete calls.)\n"
            ));
        }
        conversation.push_str("Here are the tool results:\n");
        let mut pass_failures = 0u32;
        let call_count = calls.len();
        for call in calls {
            let summary = if call.name.contains(':') {
                match &mcp {
                    Some(m) => {
                        let res = m.call_tool(&call.name, call.args.clone()).await;
                        match res {
                            Ok(r) => serde_json::to_string(&r)
                                .unwrap_or_else(|_| "(unserializable result)".into()),
                            Err(e) => {
                                pass_failures += 1;
                                format!("ERROR: {e}")
                            }
                        }
                    }
                    None => {
                        pass_failures += 1;
                        "ERROR: MCP tool requested but MCP is not enabled or has no connected tools"
                            .to_string()
                    }
                }
            } else {
                match &registry {
                    Some(reg) => {
                        let ctx = crate::tools::ToolContext {
                            session_id: tool_session.clone(),
                            job_id: None,
                            peer_id: state.local_peer_id.to_string(),
                            working_dir: std::env::current_dir().unwrap_or_default(),
                            sandboxed: false,
                            available_secrets: vec![],
                            node_tool_tx: state.node_tool_tx.clone(),
                            egress_policy: None,
                            agent_depth: 0,
                        };
                        match reg
                            .execute_local(&call.name, call.args.clone(), &ctx)
                            .await
                        {
                            Ok(r) => serde_json::to_string(&r.output)
                                .unwrap_or_else(|_| "(unserializable)".into()),
                            Err(e) => {
                                pass_failures += 1;
                                serde_json::json!({ "error": e.to_string() }).to_string()
                            }
                        }
                    }
                    None => {
                        pass_failures += 1;
                        "ERROR: Local tool name used but only MCP tools are available; use server:tool_name from the MCP list."
                            .to_string()
                    }
                }
            };
            let preview = if summary.chars().count() > 220 {
                let short: String = summary.chars().take(217).collect();
                format!("{short}…")
            } else {
                summary.clone()
            };
            let line = format!(
                "[{}] {} → {}",
                chrono::Utc::now().format("%H:%M:%S"),
                call.name,
                preview
            );
            tool_logs.push(line.clone());
            if let Some(ref sink) = progress {
                sink.record_tool_step(line, total_tokens).await;
            }
            // Truncate large tool results to save context for the answer.
            let truncated = if summary.len() > 3000 {
                format!("{}… (truncated)", &summary[..3000])
            } else {
                summary
            };
            conversation.push_str(&format!("- {} → {}\n", call.name, truncated));
        }

        // After tool results, nudge the model to answer.
        if pass_failures < call_count as u32 {
            conversation.push_str(
                "\nNow use the tool results above to write your answer to the user. If you need more info, call another tool. Otherwise answer directly without tool calls.\n",
            );
        }

        // Track consecutive all-fail passes and bail out with a nudge.
        if pass_failures as usize >= call_count {
            consecutive_all_fail_passes += 1;
            if consecutive_all_fail_passes >= MAX_CONSECUTIVE_FAIL_PASSES {
                conversation.push_str(
                    "\n(System: All tool calls have failed for multiple consecutive passes. \
                     STOP calling tools. Answer the user's question directly from your own knowledge. \
                     Do NOT make any more tool_call blocks.)\n",
                );
                if let Some(ref sink) = progress {
                    sink.append_log(format!(
                        "[{}] {} consecutive all-fail passes — forcing answer from knowledge",
                        chrono::Utc::now().format("%H:%M:%S"),
                        MAX_CONSECUTIVE_FAIL_PASSES
                    ))
                    .await;
                }
            }
        } else {
            consecutive_all_fail_passes = 0;
        }
    }
    Err("Agentic: max tool iterations reached".into())
}

/// Disconnect existing MCP sessions and reconnect from the current [`WebState::mcp_config`].
pub async fn reload_mcp_manager(state: &WebState) {
    let cfg = state.mcp_config.read().await.clone();
    if let Some(old) = state.mcp_manager.write().await.take() {
        let _ = old.disconnect_all().await;
    }
    if !cfg.enabled || cfg.servers.is_empty() {
        tracing::info!("MCP disabled or no servers configured");
        return;
    }
    let manager = Arc::new(crate::mcp::McpManager::new(cfg));
    match manager.connect_all().await {
        Ok(()) => {
            let n_tools = manager.tool_count();
            let n_srv = manager.server_count();
            *state.mcp_manager.write().await = Some(manager);
            tracing::info!(servers = n_srv, tools = n_tools, "MCP manager ready");
        }
        Err(e) => tracing::warn!("MCP connect_all failed: {}", e),
    }
}

#[derive(Serialize)]
struct McpToolListItem {
    id: String,
    description: Option<String>,
}

#[derive(Serialize)]
struct McpStatusResponse {
    mode: &'static str,
    in_core: bool,
    config: crate::mcp::McpConfig,
    config_path: String,
    connected_servers: Vec<String>,
    tool_count: usize,
    tools: Vec<McpToolListItem>,
    /// Example `config.toml` fragment matching `McpConfig` / `McpServerConfig`.
    mcp_toml_snippet: &'static str,
    hint: &'static str,
    spec_url: &'static str,
}

async fn api_mcp_status(State(state): State<Arc<WebState>>) -> Json<McpStatusResponse> {
    let config = state.mcp_config.read().await.clone();
    let (connected_servers, tools, tool_count) = {
        let g = state.mcp_manager.read().await;
        if let Some(m) = g.as_ref() {
            let connected_servers = m.connected_server_names();
            let catalog: Vec<McpToolListItem> = m
                .list_tools_with_ids()
                .into_iter()
                .map(|(id, t)| McpToolListItem {
                    id,
                    description: t.description.clone(),
                })
                .collect();
            let tool_count = catalog.len();
            (connected_servers, catalog, tool_count)
        } else {
            (vec![], vec![], 0)
        }
    };
    Json(McpStatusResponse {
        mode: "sidecar_ready",
        in_core: false,
        config,
        config_path: state.config_path.display().to_string(),
        connected_servers,
        tool_count,
        tools,
        mcp_toml_snippet: r#"[mcp]
enabled = true
timeout_secs = 30
auto_reconnect = true

[[mcp.servers]]
name = "example"
url = "stdio://local"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
"#,
        hint: "stdio servers need command + args; HTTP MCP is not implemented in the client yet. Enable “Use MCP” in chat or agent when tools appear below.",
        spec_url: "https://modelcontextprotocol.io",
    })
}

async fn api_mcp_put_config(
    State(state): State<Arc<WebState>>,
    Json(cfg): Json<crate::mcp::McpConfig>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if cfg.servers.len() > 24 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": "too many servers (max 24)" })),
        ));
    }
    for s in &cfg.servers {
        if s.name.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "ok": false, "error": "server name must be non-empty" })),
            ));
        }
        if s.url.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "ok": false, "error": "server url must be non-empty" })),
            ));
        }
    }
    *state.mcp_config.write().await = cfg;
    reload_mcp_manager(state.as_ref()).await;
    Ok(Json(serde_json::json!({ "ok": true })))
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

#[derive(Serialize)]
struct P2pNetworkApiResponse {
    local_peer_id: String,
    bootstrap_peers: Vec<String>,
    mdns_enabled: bool,
    kademlia_enabled: bool,
    community_peers: Vec<CommunityPeerEntry>,
    dial_supported: bool,
}

async fn api_peers_network(State(state): State<Arc<WebState>>) -> Json<P2pNetworkApiResponse> {
    let h = state.p2p_network_hints.as_ref();
    Json(P2pNetworkApiResponse {
        local_peer_id: state.local_peer_id.to_string(),
        bootstrap_peers: h.bootstrap_peers.clone(),
        mdns_enabled: h.mdns_enabled,
        kademlia_enabled: h.kademlia_enabled,
        community_peers: h.community_peers.clone(),
        dial_supported: state.peer_dial_tx.is_some(),
    })
}

#[derive(Deserialize)]
struct DialPeerBody {
    multiaddr: String,
}

#[derive(Serialize)]
struct DialPeerResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn api_peers_dial(
    State(state): State<Arc<WebState>>,
    Json(body): Json<DialPeerBody>,
) -> impl IntoResponse {
    let addr = body.multiaddr.trim();
    if addr.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(DialPeerResponse {
                ok: false,
                error: Some("multiaddr required".to_string()),
            }),
        )
            .into_response();
    }
    if let Err(e) = addr.parse::<Multiaddr>() {
        return (
            StatusCode::BAD_REQUEST,
            Json(DialPeerResponse {
                ok: false,
                error: Some(format!("invalid multiaddr: {e}")),
            }),
        )
            .into_response();
    }
    let Some(tx) = &state.peer_dial_tx else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(DialPeerResponse {
                ok: false,
                error: Some("dial not available in this mode".to_string()),
            }),
        )
            .into_response();
    };
    match tx.try_send(addr.to_string()) {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(DialPeerResponse { ok: true, error: None }),
        )
            .into_response(),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(DialPeerResponse {
                ok: false,
                error: Some("dial queue full; try again shortly".to_string()),
            }),
        )
            .into_response(),
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(DialPeerResponse {
                ok: false,
                error: Some("dial channel closed".to_string()),
            }),
        )
            .into_response(),
    }
}

async fn api_jobs(State(state): State<Arc<WebState>>) -> Json<Vec<WebJobInfo>> {
    let jobs = state.job_list.read().await;
    Json(jobs.clone())
}

/// GET /api/tools — list available tools from the node's ToolRegistry.
async fn api_tools(State(state): State<Arc<WebState>>) -> Json<serde_json::Value> {
    let Some(registry) = &state.tools else {
        return Json(serde_json::json!({ "tools": [], "hint": "No tool registry (node not started with serve)" }));
    };
    let infos = registry.list_tools().await;
    let tools: Vec<serde_json::Value> = infos
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "location": format!("{:?}", t.location),
            })
        })
        .collect();
    Json(serde_json::json!({ "tools": tools, "count": tools.len() }))
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
            match tokio::time::timeout(std::time::Duration::from_secs(30), response_rx).await {
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
    /// When set, prior turns for this id are prepended (bounded) and new turns are stored server-side.
    session_id: Option<String>,
    /// When true (default), run a ReAct loop with the node's ToolRegistry (`job_submit`, shell, …) and optional MCP.
    #[serde(default = "default_chat_agentic")]
    agentic: bool,
    /// When true, include MCP tool ids in the system prefix and route `server:tool` calls to MCP.
    #[serde(default)]
    use_mcp: bool,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
    tokens: u32,
    tokens_per_second: f32,
    location: String,
    provider_peer_id: Option<String>,
}

/// Build the model prompt with recent session history prepended.
///
/// First checks the in-memory cache; if empty, falls back to the persistent [`SessionStore`].
async fn build_prompt_with_history(
    sessions: &RwLock<HashMap<String, Vec<ChatMessage>>>,
    session_id: Option<&str>,
    user_message: &str,
) -> String {
    build_prompt_with_history_persistent(sessions, None, session_id, user_message).await
}

/// Like [`build_prompt_with_history`] but also consults a persistent [`SessionStore`].
async fn build_prompt_with_history_persistent(
    sessions: &RwLock<HashMap<String, Vec<ChatMessage>>>,
    store: Option<&crate::agent::SessionStore>,
    session_id: Option<&str>,
    user_message: &str,
) -> String {
    let Some(sid) = session_id else {
        return user_message.to_string();
    };

    // Try in-memory first.
    let guard = sessions.read().await;
    let in_memory = guard.get(sid);

    // Collect messages: prefer in-memory, fall back to persistent store.
    let msgs: Vec<ChatMessage> = if let Some(cached) = in_memory {
        if !cached.is_empty() {
            cached.clone()
        } else {
            drop(guard);
            load_from_store(store, sid)
        }
    } else {
        drop(guard);
        load_from_store(store, sid)
    };

    if msgs.is_empty() {
        return user_message.to_string();
    }

    // Keep recent turns but cap total chars to ~6k so we don't blow context for small models.
    const MAX_HISTORY_CHARS: usize = 6_000;
    let mut prefix = String::new();
    for m in msgs.iter().rev() {
        let line = format!("{}: {}\n", m.role, m.content);
        if prefix.len() + line.len() > MAX_HISTORY_CHARS {
            break;
        }
        prefix.insert_str(0, &line);
    }
    if prefix.is_empty() {
        user_message.to_string()
    } else {
        format!("Previous turns:\n{prefix}\nUser: {user_message}")
    }
}

/// Load turns from the persistent store and convert to [`ChatMessage`].
fn load_from_store(
    store: Option<&crate::agent::SessionStore>,
    session_id: &str,
) -> Vec<ChatMessage> {
    let Some(store) = store else {
        return Vec::new();
    };
    match store.load_session(session_id, 40) {
        Ok(turns) => turns
            .into_iter()
            .map(|t| ChatMessage {
                role: t.role,
                content: t.content,
            })
            .collect(),
        Err(e) => {
            tracing::warn!(session = session_id, "Failed to load session from store: {}", e);
            Vec::new()
        }
    }
}

/// Append a user+assistant turn to the in-memory session cache and cap at MAX_MSGS.
/// Also persists to the [`SessionStore`] if available.
async fn save_session_turn(
    sessions: &RwLock<HashMap<String, Vec<ChatMessage>>>,
    session_id: &str,
    user_message: &str,
    assistant_text: &str,
) {
    save_session_turn_persistent(sessions, None, session_id, user_message, assistant_text).await;
}

/// Like [`save_session_turn`] but also persists to a [`SessionStore`].
async fn save_session_turn_persistent(
    sessions: &RwLock<HashMap<String, Vec<ChatMessage>>>,
    store: Option<&crate::agent::SessionStore>,
    session_id: &str,
    user_message: &str,
    assistant_text: &str,
) {
    const MAX_MSGS: usize = 80;
    let mut guard = sessions.write().await;
    let entry = guard.entry(session_id.to_string()).or_default();
    entry.push(ChatMessage {
        role: "user".into(),
        content: user_message.to_string(),
    });
    entry.push(ChatMessage {
        role: "assistant".into(),
        content: assistant_text.to_string(),
    });
    if entry.len() > MAX_MSGS {
        let drain = entry.len() - MAX_MSGS;
        entry.drain(0..drain);
    }

    // Persist to disk.
    if let Some(store) = store {
        if let Err(e) = store.save_turn(session_id, "user", user_message) {
            tracing::warn!(session = session_id, "Failed to persist user turn: {}", e);
        }
        if let Err(e) = store.save_turn(session_id, "assistant", assistant_text) {
            tracing::warn!(session = session_id, "Failed to persist assistant turn: {}", e);
        }
    }
}

async fn api_chat(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ChatRequest>,
) -> Json<ChatResponse> {
    let model = req.model.unwrap_or_else(|| "llama-3.2-3b".to_string());
    let max_tokens = req.max_tokens.unwrap_or(500);
    let temperature = req.temperature.unwrap_or(0.7);
    let user_message = req.message.clone();

    let store_ref = state.session_store.as_deref();
    let prompt_for_model = build_prompt_with_history_persistent(
        &state.chat_sessions,
        store_ref,
        req.session_id.as_deref(),
        &user_message,
    )
    .await;

    let mcp_arc = state.mcp_manager.read().await.clone();
    let mcp_for_agentic = if req.use_mcp {
        mcp_arc.clone().filter(|m| m.tool_count() > 0)
    } else {
        None
    };
    let include_mcp_catalog = req.use_mcp && mcp_for_agentic.is_some();

    if req.agentic {
        if let Some(registry) = state.tools.clone() {
            if state.inference_tx.is_some() {
                // Auto-inject matching skill prompt if a skill registry is available.
                let skill_inject = if let Some(ref skills) = state.skills {
                    skills.select_best(&user_message).await
                        .filter(|s| s.is_available())
                        .map(|s| {
                            let body = s.prompt();
                            let clipped = if body.chars().count() > 6_000 {
                                format!("{}…(truncated)", body.chars().take(6_000).collect::<String>())
                            } else {
                                body.to_string()
                            };
                            format!("### Active Skill: {}\n{}\n\n", s.name(), clipped)
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                let body = format!("{skill_inject}### User thread\n{prompt_for_model}");
                match run_unified_agentic_inference(
                    state.as_ref(),
                    Some(registry),
                    mcp_for_agentic.clone(),
                    include_mcp_catalog,
                    body,
                    model.clone(),
                    max_tokens,
                    temperature,
                    None,
                    None,
                )
                .await
                {
                    Ok((response, _)) => {
                        if let Some(ref sid) = req.session_id {
                            save_session_turn_persistent(
                                &state.chat_sessions,
                                store_ref,
                                sid,
                                &user_message,
                                &response.text,
                            )
                            .await;
                        }
                        return Json(ChatResponse {
                            response: response.text,
                            tokens: response.tokens_generated,
                            tokens_per_second: response.tokens_per_second,
                            location: response.location,
                            provider_peer_id: response.provider_peer_id,
                        });
                    }
                    Err(e) => {
                        return Json(ChatResponse {
                            response: format!("Agentic: {e}"),
                            tokens: 0,
                            tokens_per_second: 0.0,
                            location: "error".to_string(),
                            provider_peer_id: None,
                        });
                    }
                }
            }
        }
    }

    if req.use_mcp {
        if let Some(mcp) = mcp_arc {
            if mcp.tool_count() > 0 && state.inference_tx.is_some() {
                let body = format!("### User thread\n{prompt_for_model}");
                match run_unified_agentic_inference(
                    state.as_ref(),
                    None,
                    Some(mcp),
                    true,
                    body,
                    model.clone(),
                    max_tokens,
                    temperature,
                    None,
                    None,
                )
                .await
                {
                    Ok((response, _)) => {
                        if let Some(ref sid) = req.session_id {
                            save_session_turn(
                                &state.chat_sessions,
                                sid,
                                &user_message,
                                &response.text,
                            )
                            .await;
                        }
                        return Json(ChatResponse {
                            response: response.text,
                            tokens: response.tokens_generated,
                            tokens_per_second: response.tokens_per_second,
                            location: response.location,
                            provider_peer_id: response.provider_peer_id,
                        });
                    }
                    Err(e) => {
                        return Json(ChatResponse {
                            response: format!("Agentic: {e}"),
                            tokens: 0,
                            tokens_per_second: 0.0,
                            location: "error".to_string(),
                            provider_peer_id: None,
                        });
                    }
                }
            }
        }
    }

    // If we have an inference channel, use it
    if let Some(tx) = &state.inference_tx {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request = InferenceRequest {
            prompt: prompt_for_model,
            model: model.clone(),
            max_tokens,
            temperature,
            response_tx,
            stream_delta_tx: None,
        };

        if tx.send(request).await.is_ok() {
            match tokio::time::timeout(std::time::Duration::from_secs(60), response_rx).await {
                Ok(Ok(response)) => {
                    if let Some(ref sid) = req.session_id {
                        save_session_turn(
                            &state.chat_sessions,
                            sid,
                            &user_message,
                            &response.text,
                        )
                        .await;
                    }
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

/// SSE chat: `data:` lines are JSON — `{ "type": "delta", "text": "..." }` then
/// `{ "type": "done", "response", "tokens", "tokens_per_second", "location", "provider_peer_id" }`.
async fn api_chat_stream(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Response, (axum::http::StatusCode, Json<ChatResponse>)> {
    let model = req.model.unwrap_or_else(|| "llama-3.2-3b".to_string());
    let max_tokens = req.max_tokens.unwrap_or(500);
    let temperature = req.temperature.unwrap_or(0.7);
    let user_message = req.message.clone();

    let prompt_for_model = build_prompt_with_history(
        &state.chat_sessions,
        req.session_id.as_deref(),
        &user_message,
    )
    .await;

    let mcp_arc = state.mcp_manager.read().await.clone();
    let mcp_for_agentic = if req.use_mcp {
        mcp_arc.clone().filter(|m| m.tool_count() > 0)
    } else {
        None
    };
    let include_mcp_catalog = req.use_mcp && mcp_for_agentic.is_some();

    if req.agentic {
        if let Some(registry) = state.tools.clone() {
            if state.inference_tx.is_some() {
                let body = format!("### User thread\n{prompt_for_model}");
                let state_spawn = state.clone();
                let sessions = state.chat_sessions.clone();
                let session_store_spawn = state.session_store.clone();
                let session_id = req.session_id.clone();
                let user_message_for_session = user_message.clone();
                let model_spawn = model.clone();
                let mcp_clone = mcp_for_agentic.clone();
                let (sse_tx, sse_rx) = mpsc::channel::<Result<Event, Infallible>>(128);
                tokio::spawn(async move {
                    match run_unified_agentic_inference(
                        state_spawn.as_ref(),
                        Some(registry),
                        mcp_clone,
                        include_mcp_catalog,
                        body,
                        model_spawn,
                        max_tokens,
                        temperature,
                        None,
                        None,
                    )
                    .await
                    {
                        Ok((response, _)) => {
                            let payload =
                                serde_json::json!({ "type": "delta", "text": response.text }).to_string();
                            if sse_tx
                                .send(Ok(Event::default().data(payload)))
                                .await
                                .is_err()
                            {
                                return;
                            }
                            if let Some(ref sid) = session_id {
                                save_session_turn_persistent(
                                    &sessions,
                                    session_store_spawn.as_deref(),
                                    sid,
                                    &user_message_for_session,
                                    &response.text,
                                )
                                .await;
                            }
                            let done = serde_json::json!({
                                "type": "done",
                                "response": response.text,
                                "tokens": response.tokens_generated,
                                "tokens_per_second": response.tokens_per_second,
                                "location": response.location,
                                "provider_peer_id": response.provider_peer_id,
                            })
                            .to_string();
                            let _ = sse_tx.send(Ok(Event::default().data(done))).await;
                        }
                        Err(e) => {
                            let txt = format!("Agentic: {e}");
                            let payload = serde_json::json!({ "type": "delta", "text": txt }).to_string();
                            if sse_tx
                                .send(Ok(Event::default().data(payload)))
                                .await
                                .is_err()
                            {
                                return;
                            }
                            let done = serde_json::json!({
                                "type": "done",
                                "response": txt,
                                "tokens": 0,
                                "tokens_per_second": 0.0,
                                "location": "error",
                                "provider_peer_id": null,
                            })
                            .to_string();
                            let _ = sse_tx.send(Ok(Event::default().data(done))).await;
                        }
                    }
                });
                let stream = ReceiverStream::new(sse_rx);
                return Ok(Sse::new(stream)
                    .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
                    .into_response());
            }
        }
    }

    if req.use_mcp {
        if let Some(mcp) = mcp_arc {
            if mcp.tool_count() > 0 && state.inference_tx.is_some() {
                let body = format!("### User thread\n{prompt_for_model}");
                let state_spawn = state.clone();
                let sessions = state.chat_sessions.clone();
                let session_store_spawn = state.session_store.clone();
                let session_id = req.session_id.clone();
                let user_message_for_session = user_message.clone();
                let model_spawn = model.clone();
                let (sse_tx, sse_rx) = mpsc::channel::<Result<Event, Infallible>>(128);
                tokio::spawn(async move {
                    match run_unified_agentic_inference(
                        state_spawn.as_ref(),
                        None,
                        Some(mcp),
                        true,
                        body,
                        model_spawn,
                        max_tokens,
                        temperature,
                        None,
                        None,
                    )
                    .await
                    {
                        Ok((response, _)) => {
                            let payload =
                                serde_json::json!({ "type": "delta", "text": response.text }).to_string();
                            if sse_tx
                                .send(Ok(Event::default().data(payload)))
                                .await
                                .is_err()
                            {
                                return;
                            }
                            if let Some(ref sid) = session_id {
                                save_session_turn_persistent(
                                    &sessions,
                                    session_store_spawn.as_deref(),
                                    sid,
                                    &user_message_for_session,
                                    &response.text,
                                )
                                .await;
                            }
                            let done = serde_json::json!({
                                "type": "done",
                                "response": response.text,
                                "tokens": response.tokens_generated,
                                "tokens_per_second": response.tokens_per_second,
                                "location": response.location,
                                "provider_peer_id": response.provider_peer_id,
                            })
                            .to_string();
                            let _ = sse_tx.send(Ok(Event::default().data(done))).await;
                        }
                        Err(e) => {
                            let txt = format!("Agentic: {e}");
                            let payload = serde_json::json!({ "type": "delta", "text": txt }).to_string();
                            if sse_tx
                                .send(Ok(Event::default().data(payload)))
                                .await
                                .is_err()
                            {
                                return;
                            }
                            let done = serde_json::json!({
                                "type": "done",
                                "response": txt,
                                "tokens": 0,
                                "tokens_per_second": 0.0,
                                "location": "error",
                                "provider_peer_id": null,
                            })
                            .to_string();
                            let _ = sse_tx.send(Ok(Event::default().data(done))).await;
                        }
                    }
                });
                let stream = ReceiverStream::new(sse_rx);
                return Ok(Sse::new(stream)
                    .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
                    .into_response());
            }
        }
    }

    let Some(tx) = &state.inference_tx else {
        return Err((
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
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
            }),
        ));
    };

    let (delta_tx, mut delta_rx) = mpsc::unbounded_channel::<String>();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    let request = InferenceRequest {
        prompt: prompt_for_model,
        model: model.clone(),
        max_tokens,
        temperature,
        response_tx,
        stream_delta_tx: Some(delta_tx),
    };

    if tx.send(request).await.is_err() {
        return Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ChatResponse {
                response: "Failed to queue inference request".to_string(),
                tokens: 0,
                tokens_per_second: 0.0,
                location: "error".to_string(),
                provider_peer_id: None,
            }),
        ));
    }

    let sessions = state.chat_sessions.clone();
    let session_store_spawn = state.session_store.clone();
    let session_id = req.session_id.clone();
    let user_message_for_session = user_message.clone();

    let (sse_tx, sse_rx) = mpsc::channel::<Result<Event, Infallible>>(128);

    tokio::spawn(async move {
        while let Some(chunk) = delta_rx.recv().await {
            let payload = serde_json::json!({ "type": "delta", "text": chunk }).to_string();
            if sse_tx
                .send(Ok(Event::default().data(payload)))
                .await
                .is_err()
            {
                return;
            }
        }

        let response = match tokio::time::timeout(Duration::from_secs(120), response_rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => InferenceResponse {
                text: "Error: Inference task cancelled".to_string(),
                tokens_generated: 0,
                tokens_per_second: 0.0,
                location: "error".to_string(),
                provider_peer_id: None,
            },
            Err(_) => InferenceResponse {
                text: "Error: Inference timeout (120s)".to_string(),
                tokens_generated: 0,
                tokens_per_second: 0.0,
                location: "error".to_string(),
                provider_peer_id: None,
            },
        };

        if let Some(ref sid) = session_id {
            save_session_turn_persistent(
                &sessions,
                session_store_spawn.as_deref(),
                sid,
                &user_message_for_session,
                &response.text,
            )
            .await;
        }

        let done = serde_json::json!({
            "type": "done",
            "response": response.text,
            "tokens": response.tokens_generated,
            "tokens_per_second": response.tokens_per_second,
            "location": response.location,
            "provider_peer_id": response.provider_peer_id,
        })
        .to_string();

        let _ = sse_tx.send(Ok(Event::default().data(done))).await;
    });

    let stream = ReceiverStream::new(sse_rx);
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<WebState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_ws_control_message(state: &WebState, text: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(t) = v.get("type").and_then(|x| x.as_str()) else {
        return;
    };
    if t == "ping" {
        let _ = state
            .ws_control_tx
            .send(serde_json::json!({ "type": "pong", "ts": chrono::Utc::now().timestamp() }));
    }
}

async fn handle_socket(mut socket: WebSocket, state: Arc<WebState>) {
    let mut sub = state.ws_control_tx.subscribe();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    interval.tick().await;

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(t))) => {
                        handle_ws_control_message(&state, &t).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            recv = sub.recv() => {
                match recv {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg.to_string())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = interval.tick() => {
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
        return Json(SwarmAgentsResponse {
            agents: vec![],
            total: 0,
        });
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
    Json(SwarmAgentsResponse {
        agents: agent_infos,
        total,
    })
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

// === Inference & model download API ===

#[derive(Serialize)]
struct InferenceSettingsResponse {
    use_local_gguf: bool,
    use_ollama: bool,
    ollama_url: String,
    remote_api_enabled: bool,
    remote_api_base_url: String,
    remote_api_model: String,
    api_key_configured: bool,
    models_directory: String,
    gguf_presets: Vec<GgufPresetRow>,
    hint: &'static str,
}

#[derive(Serialize)]
struct GgufPresetRow {
    id: &'static str,
    repo: &'static str,
}

#[derive(Deserialize)]
struct InferenceSettingsPut {
    use_local_gguf: bool,
    use_ollama: bool,
    ollama_url: String,
    remote_api_enabled: bool,
    remote_api_base_url: String,
    remote_api_model: String,
    /// When omitted, existing API key is left unchanged.
    #[serde(default)]
    remote_api_key: Option<String>,
}

#[derive(Deserialize)]
struct ModelDownloadBody {
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    quant: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    filename: Option<String>,
}

#[derive(Serialize)]
struct ModelDownloadResponse {
    success: bool,
    path: Option<String>,
    bytes: Option<u64>,
    error: Option<String>,
}

fn inference_settings_from_live(
    live: &crate::inference::InferenceLiveSettings,
    models_directory: String,
) -> InferenceSettingsResponse {
    let gguf_presets = crate::models_hf::KNOWN_GGUF_PRESETS
        .iter()
        .map(|(id, repo, _, _)| GgufPresetRow { id, repo })
        .collect();
    InferenceSettingsResponse {
        use_local_gguf: live.use_local_gguf,
        use_ollama: live.use_ollama,
        ollama_url: live.ollama_url.clone(),
        remote_api_enabled: live.remote_api_enabled,
        remote_api_base_url: live.remote_api_base_url.clone(),
        remote_api_model: live.remote_api_model.clone(),
        api_key_configured: !live.remote_api_key.trim().is_empty(),
        models_directory,
        gguf_presets,
        hint: "Remote API: OpenAI-compatible Chat Completions (Bearer key). Saving updates ~/.peerclaw/config.toml.",
    }
}

async fn api_inference_settings_get(
    State(state): State<Arc<WebState>>,
) -> Result<Json<InferenceSettingsResponse>, (StatusCode, String)> {
    let Some(inf) = &state.inference else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Inference engine not attached (run peerclaw serve with --web).".into(),
        ));
    };
    let live_arc = inf.live_settings();
    let live = live_arc.read().await;
    let models_directory = inf.models_directory().display().to_string();
    Ok(Json(inference_settings_from_live(&live, models_directory)))
}

async fn api_inference_settings_put(
    State(state): State<Arc<WebState>>,
    Json(body): Json<InferenceSettingsPut>,
) -> Result<Json<InferenceSettingsResponse>, (StatusCode, String)> {
    let Some(inf) = &state.inference else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "Inference engine not attached.".into(),
        ));
    };
    {
        let live_arc = inf.live_settings();
        let mut live = live_arc.write().await;
        live.use_local_gguf = body.use_local_gguf;
        live.use_ollama = body.use_ollama;
        live.ollama_url = body.ollama_url;
        live.remote_api_enabled = body.remote_api_enabled;
        live.remote_api_base_url = body.remote_api_base_url;
        live.remote_api_model = body.remote_api_model;
        if let Some(k) = body.remote_api_key {
            live.remote_api_key = k;
        }
        let mut cfg = crate::config::Config::load().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("config load: {e}"),
            )
        })?;
        live.apply_to_config(&mut cfg.inference);
        cfg.save().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("config save: {e}"),
            )
        })?;
    }
    let live_arc = inf.live_settings();
    let live = live_arc.read().await;
    let models_directory = inf.models_directory().display().to_string();
    Ok(Json(inference_settings_from_live(&live, models_directory)))
}

async fn api_models_download(
    State(state): State<Arc<WebState>>,
    Json(body): Json<ModelDownloadBody>,
) -> Json<ModelDownloadResponse> {
    let Some(inf) = &state.inference else {
        return Json(ModelDownloadResponse {
            success: false,
            path: None,
            bytes: None,
            error: Some("Inference engine not attached.".into()),
        });
    };

    let models_dir = inf.models_directory().clone();
    let (url, dest) = if let Some(u) = body.url.filter(|s| !s.trim().is_empty()) {
        let u = u.trim().to_string();
        let path = crate::models_hf::dest_for_custom_url(&models_dir, &u, body.filename.as_deref());
        (u, path)
    } else if let Some(preset) = body.preset.filter(|s| !s.trim().is_empty()) {
        let quant = body
            .quant
            .as_deref()
            .filter(|q| !q.is_empty())
            .unwrap_or("q4_k_m");
        match crate::models_hf::preset_to_hf_url(&preset, quant) {
            Some((url, name)) => (url, models_dir.join(name)),
            None => {
                return Json(ModelDownloadResponse {
                    success: false,
                    path: None,
                    bytes: None,
                    error: Some(format!("Unknown preset '{preset}'. See gguf_presets in GET /api/inference/settings.")),
                });
            }
        }
    } else {
        return Json(ModelDownloadResponse {
            success: false,
            path: None,
            bytes: None,
            error: Some("Provide \"url\" (Hugging Face resolve link to a .gguf) or \"preset\" + optional \"quant\".".into()),
        });
    };

    if dest.exists() {
        return Json(ModelDownloadResponse {
            success: false,
            path: Some(dest.display().to_string()),
            bytes: None,
            error: Some("File already exists; remove it first or pick another name.".into()),
        });
    }

    match crate::models_hf::download_url_to_path(&url, &dest).await {
        Ok(n) => {
            let _ = inf.scan_models().await;
            Json(ModelDownloadResponse {
                success: true,
                path: Some(dest.display().to_string()),
                bytes: Some(n),
                error: None,
            })
        }
        Err(e) => Json(ModelDownloadResponse {
            success: false,
            path: Some(dest.display().to_string()),
            bytes: None,
            error: Some(e),
        }),
    }
}

// === Task Management API ===

#[derive(Deserialize)]
struct CreateTaskPayload {
    task_type: String,
    description: String,
    model: Option<String>,
    budget: Option<f64>,
    /// When true, run the goal through the MCP tool loop instead of the loaded agent runtime.
    #[serde(default)]
    use_mcp: bool,
    /// Optional session id for conversation continuity across agent tasks.
    #[serde(default)]
    session_id: Option<String>,
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
        logs: vec![format!(
            "[{}] Task created",
            chrono::Utc::now().format("%H:%M:%S")
        )],
        model: req.model.clone(),
        budget: req.budget.unwrap_or(5.0),
        tokens_used: 0,
        iterations: 0,
    };

    state.task_store.write().await.push(task);
    broadcast_tasks_changed(&state.ws_control_tx);

    let task_skill_section: Option<String> = if let Some(reg) = &state.skills {
        reg.get(&req.task_type)
            .await
            .filter(|s| s.is_available())
            .map(|s| {
                let body = s.prompt();
                let clipped = if body.chars().count() > 12_000 {
                    format!(
                        "{}…\n(truncated for context limit)",
                        body.chars().take(12_000).collect::<String>()
                    )
                } else {
                    body.to_string()
                };
                format!(
                    "### Skill `{}` (task type `{}`)\n{}\n\n",
                    s.name(),
                    req.task_type,
                    clipped
                )
            })
    } else {
        None
    };
    let skill_block = task_skill_section.unwrap_or_default();

    let agentic_ready = state.inference_tx.is_some() && state.tools.is_some();

    let mcp_ready = req.use_mcp
        && state.inference_tx.is_some()
        && state
            .mcp_manager
            .read()
            .await
            .as_ref()
            .map(|m| m.tool_count() > 0)
            .unwrap_or(false);

    // Build conversation-enriched description when a session_id is provided.
    let enriched_description = if req.session_id.is_some() {
        build_prompt_with_history(
            &state.chat_sessions,
            req.session_id.as_deref(),
            &req.description,
        )
        .await
    } else {
        req.description.clone()
    };

    if let Some(tx) = &state.agent_task_tx {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut m = state.agent_task_cancels.write().await;
            m.insert(task_id.clone(), cancel.clone());
        }
        let store = state.task_store.clone();
        let tid = task_id.clone();
        let description = enriched_description.clone();
        let tx = tx.clone();
        let ws_tx = state.ws_control_tx.clone();
        let state_spawn = state.clone();
        let session_id = req.session_id.clone();
        let raw_description = req.description.clone();

        tokio::spawn(async move {
            // Mark as running
            {
                let mut tasks = store.write().await;
                if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                    t.status = "running".to_string();
                    t.logs.push(format!(
                        "[{}] Agent started execution",
                        chrono::Utc::now().format("%H:%M:%S")
                    ));
                }
            }
            broadcast_tasks_changed(&ws_tx);

            // Send to agent runtime via channel
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            let request = AgentTaskRequest {
                task_id: tid.clone(),
                description,
                response_tx,
                task_store: store.clone(),
                ws_control_tx: ws_tx.clone(),
                cancel: cancel.clone(),
                session_id: session_id.clone(),
            };

            tracing::info!(task_id = %tid, "Sending task to agent runtime");
            if tx.send(request).await.is_ok() {
                tracing::info!(task_id = %tid, "Task sent, waiting for result...");
                match tokio::time::timeout(std::time::Duration::from_secs(300), response_rx).await {
                    Ok(Ok(result)) => {
                        tracing::info!(task_id = %tid, success = result.success, "Task result received, updating store");
                        let user_stop = result.error.as_deref() == Some("Stopped by user");
                        let answer_text = if user_stop {
                            "Stopped by user.".to_string()
                        } else {
                            result.answer.clone()
                        };
                        let mut tasks = store.write().await;
                        if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                            t.status = if result.success {
                                "completed".to_string()
                            } else if user_stop {
                                "cancelled".to_string()
                            } else {
                                "failed".to_string()
                            };
                            t.completed_at = Some(chrono::Utc::now().to_rfc3339());
                            t.result = Some(answer_text.clone());
                            t.tokens_used = result.total_tokens;
                            t.iterations = result.iterations;
                            if let Some(err) = &result.error {
                                if !user_stop {
                                    t.logs.push(format!(
                                        "[{}] Error: {}",
                                        chrono::Utc::now().format("%H:%M:%S"),
                                        err
                                    ));
                                } else {
                                    t.logs.push(format!(
                                        "[{}] {}",
                                        chrono::Utc::now().format("%H:%M:%S"),
                                        err
                                    ));
                                }
                            }
                            if !user_stop {
                                t.logs.push(format!(
                                    "[{}] Completed: {} iterations, {} tokens, {:.4} PCLAW spent",
                                    chrono::Utc::now().format("%H:%M:%S"),
                                    result.iterations,
                                    result.total_tokens,
                                    result.budget_spent,
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
                        broadcast_tasks_changed(&ws_tx);
                        // Persist turn into chat session so follow-up messages have context.
                        if let Some(sid) = &session_id {
                            save_session_turn(
                                &state_spawn.chat_sessions,
                                sid,
                                &raw_description,
                                &answer_text,
                            )
                            .await;
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::error!(task_id = %tid, error = %e, "Agent task channel closed");
                        let mut tasks = store.write().await;
                        if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                            t.status = "failed".to_string();
                            t.result = Some(format!("Agent channel error: {}", e));
                        }
                        broadcast_tasks_changed(&ws_tx);
                    }
                    Err(_) => {
                        tracing::error!(task_id = %tid, "Agent task timed out after 300s");
                        let mut tasks = store.write().await;
                        if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                            t.status = "failed".to_string();
                            t.result = Some("Agent task timed out (300s)".to_string());
                        }
                        broadcast_tasks_changed(&ws_tx);
                    }
                }
            } else {
                tracing::error!(task_id = %tid, "Failed to send task to agent runtime channel");
                let mut tasks = store.write().await;
                if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                    t.status = "failed".to_string();
                    t.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    t.result = Some("Could not queue task to agent runtime (channel closed)".to_string());
                    t.logs.push(format!(
                        "[{}] Failed to queue task",
                        chrono::Utc::now().format("%H:%M:%S")
                    ));
                }
                broadcast_tasks_changed(&ws_tx);
            }
            state_spawn.agent_task_cancels.write().await.remove(&tid);
        });
    } else if agentic_ready {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut m = state.agent_task_cancels.write().await;
            m.insert(task_id.clone(), cancel.clone());
        }
        let registry = state.tools.clone().expect("agentic_ready implies tools");
        let mcp_for = if req.use_mcp {
            state
                .mcp_manager
                .read()
                .await
                .clone()
                .filter(|m| m.tool_count() > 0)
        } else {
            None
        };
        let include_mcp_catalog = req.use_mcp && mcp_for.is_some();
        let store = state.task_store.clone();
        let tid = task_id.clone();
        let description = enriched_description.clone();
        let raw_description = req.description.clone();
        let session_id = req.session_id.clone();
        let skill_block = skill_block.clone();
        let model = req
            .model
            .clone()
            .unwrap_or_else(|| "llama-3.2-3b".to_string());
        let ws_tx = state.ws_control_tx.clone();
        let state_spawn = state.clone();

        tokio::spawn(async move {
            {
                let mut tasks = store.write().await;
                if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                    t.status = "running".to_string();
                    t.logs.push(format!(
                        "[{}] Agentic run started (local tools + optional MCP)",
                        chrono::Utc::now().format("%H:%M:%S")
                    ));
                }
            }
            broadcast_tasks_changed(&ws_tx);

            let body = format!(
                "Goal: {description}\n\n{skill_block}\
                 Use tools only when needed. If a tool fails, answer from your knowledge.\n\
                 Give a complete, useful answer.\n"
            );

            let progress_sink = AgenticTaskProgressSink {
                store: store.clone(),
                task_id: tid.clone(),
                ws_tx: ws_tx.clone(),
            };

            let infer_outcome = run_unified_agentic_inference(
                state_spawn.as_ref(),
                Some(registry),
                mcp_for,
                include_mcp_catalog,
                body,
                model,
                1024,
                0.35,
                Some(progress_sink),
                Some(cancel),
            )
            .await;

            state_spawn.agent_task_cancels.write().await.remove(&tid);

            match infer_outcome {
                Ok((resp, _tool_logs)) => {
                    let answer_text = resp.text.clone();
                    let mut tasks = store.write().await;
                    if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                        // Pass / tool lines already streamed into `t.logs` during the run.
                        t.status = "completed".to_string();
                        t.completed_at = Some(chrono::Utc::now().to_rfc3339());
                        t.result = Some(resp.text);
                        t.tokens_used = resp.tokens_generated;
                        // `t.iterations` is ReAct pass count from `set_react_pass`, not tool-step count.
                        if t.iterations == 0 {
                            t.iterations = 1;
                        }
                    }
                    drop(tasks);
                    broadcast_tasks_changed(&ws_tx);
                    // Persist turn into chat session for follow-up continuity.
                    if let Some(sid) = &session_id {
                        save_session_turn(
                            &state_spawn.chat_sessions,
                            sid,
                            &raw_description,
                            &answer_text,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    let user_stop = e == "Stopped by user";
                    let mut tasks = store.write().await;
                    if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                        t.status = if user_stop {
                            "cancelled".to_string()
                        } else {
                            "failed".to_string()
                        };
                        t.completed_at = Some(chrono::Utc::now().to_rfc3339());
                        t.result = Some(if user_stop {
                            "Stopped by user.".to_string()
                        } else {
                            format!("Agentic: {e}")
                        });
                        t.logs.push(format!(
                            "[{}] {}",
                            chrono::Utc::now().format("%H:%M:%S"),
                            e
                        ));
                    }
                    broadcast_tasks_changed(&ws_tx);
                }
            }
        });
    } else if mcp_ready {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut m = state.agent_task_cancels.write().await;
            m.insert(task_id.clone(), cancel.clone());
        }
        let mcp = state
            .mcp_manager
            .read()
            .await
            .as_ref()
            .cloned()
            .expect("mcp_ready implies Some");
        let store = state.task_store.clone();
        let tid = task_id.clone();
        let description = enriched_description.clone();
        let raw_description = req.description.clone();
        let session_id = req.session_id.clone();
        let skill_block = skill_block.clone();
        let model = req
            .model
            .clone()
            .unwrap_or_else(|| "llama-3.2-3b".to_string());
        let ws_tx = state.ws_control_tx.clone();
        let state_spawn = state.clone();

        tokio::spawn(async move {
            {
                let mut tasks = store.write().await;
                if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                    t.status = "running".to_string();
                    t.logs.push(format!(
                        "[{}] MCP-only agentic run started",
                        chrono::Utc::now().format("%H:%M:%S")
                    ));
                }
            }
            broadcast_tasks_changed(&ws_tx);

            let body = format!(
                "### Agent goal\n{description}\n\n\
                 {skill_block}\
                 Use MCP tools when they help; pass the arguments each tool expects (see MCP server docs if unsure).\n\
                 Finish with a concise, substantive answer for the user (not commentary about tools).\n"
            );

            let progress_sink = AgenticTaskProgressSink {
                store: store.clone(),
                task_id: tid.clone(),
                ws_tx: ws_tx.clone(),
            };

            let infer_outcome = run_unified_agentic_inference(
                state_spawn.as_ref(),
                None,
                Some(mcp),
                true,
                body,
                model,
                1024,
                0.35,
                Some(progress_sink),
                Some(cancel),
            )
            .await;

            state_spawn.agent_task_cancels.write().await.remove(&tid);

            match infer_outcome {
                Ok((resp, _tool_logs)) => {
                    let answer_text = resp.text.clone();
                    let mut tasks = store.write().await;
                    if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                        // Pass / tool lines already streamed into `t.logs` during the run.
                        t.status = "completed".to_string();
                        t.completed_at = Some(chrono::Utc::now().to_rfc3339());
                        t.result = Some(resp.text);
                        t.tokens_used = resp.tokens_generated;
                        if t.iterations == 0 {
                            t.iterations = 1;
                        }
                    }
                    drop(tasks);
                    broadcast_tasks_changed(&ws_tx);
                    if let Some(sid) = &session_id {
                        save_session_turn(
                            &state_spawn.chat_sessions,
                            sid,
                            &raw_description,
                            &answer_text,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    let user_stop = e == "Stopped by user";
                    let mut tasks = store.write().await;
                    if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                        t.status = if user_stop {
                            "cancelled".to_string()
                        } else {
                            "failed".to_string()
                        };
                        t.completed_at = Some(chrono::Utc::now().to_rfc3339());
                        t.result = Some(if user_stop {
                            "Stopped by user.".to_string()
                        } else {
                            format!("Agentic: {e}")
                        });
                        t.logs.push(format!(
                            "[{}] {}",
                            chrono::Utc::now().format("%H:%M:%S"),
                            e
                        ));
                    }
                    broadcast_tasks_changed(&ws_tx);
                }
            }
        });
    } else if req.use_mcp {
        let mut tasks = state.task_store.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id) {
            t.status = "failed".to_string();
            t.completed_at = Some(chrono::Utc::now().to_rfc3339());
            t.result = Some(
                "MCP mode requires inference and at least one connected MCP tool. Without MCP, use a full `peerclaw serve` node (agentic tools) or `--agent` for a spec-driven runtime."
                    .to_string(),
            );
            t.logs.push(format!(
                "[{}] MCP unavailable (no tools connected or inference off)",
                chrono::Utc::now().format("%H:%M:%S")
            ));
        }
        broadcast_tasks_changed(&state.ws_control_tx);
    } else {
        let mut tasks = state.task_store.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id) {
            t.status = "failed".to_string();
            t.completed_at = Some(chrono::Utc::now().to_rfc3339());
            t.result = Some(
                "No execution path for this task. Use `peerclaw serve` with inference, or `--agent` for a spec-driven agent, or enable MCP in settings when tools are connected."
                    .to_string(),
            );
            t.logs.push(format!(
                "[{}] No agent runtime, no tool registry, and no MCP path matched",
                chrono::Utc::now().format("%H:%M:%S")
            ));
        }
        broadcast_tasks_changed(&state.ws_control_tx);
    }

    Json(CreateTaskResponse {
        success: true,
        task_id: Some(task_id),
        error: None,
    })
}

/// Signal an in-flight web agentic task to stop (honoured between ReAct iterations).
async fn api_task_stop(
    State(state): State<Arc<WebState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    let flag = {
        let guard = state.agent_task_cancels.read().await;
        guard.get(&id).cloned()
    };
    if let Some(f) = flag {
        f.store(true, Ordering::Release);
        let mut tasks = state.task_store.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
            if t.status == "running" || t.status == "pending" {
                t.logs.push(format!(
                    "[{}] Stop requested — exits after the current model or tool step",
                    chrono::Utc::now().format("%H:%M:%S")
                ));
            }
        }
        broadcast_tasks_changed(&state.ws_control_tx);
        return Json(serde_json::json!({
            "ok": true,
            "message": "stop signaled",
        }));
    }
    Json(serde_json::json!({
        "ok": false,
        "message": "task is not running or cannot be stopped (only in-flight agentic/MCP web tasks)",
    }))
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
        match serde_json::to_value(task) {
            Ok(v) => Json(v),
            Err(e) => {
                tracing::warn!(error = %e, task_id = %id, "task detail JSON serialization failed");
                Json(serde_json::json!({
                    "error": "task_serialize_failed",
                    "message": "Could not serialize task (e.g. invalid float in stored fields).",
                }))
            }
        }
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
            models: m
                .models
                .iter()
                .map(|mo| ProviderModelInfo {
                    model_name: mo.model_name.clone(),
                    context_size: mo.context_size,
                    price_per_1k_tokens: mo.price_per_1k_tokens,
                    backend: format!("{}", mo.backend),
                })
                .collect(),
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

async fn api_get_provider_config(
    State(state): State<Arc<WebState>>,
) -> Json<ProviderConfigResponse> {
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
    let (name, agent_state, action_count, success_rate, is_local) =
        if let Some(swarm) = &state.swarm_manager {
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
                (
                    "This Node (local)".to_string(),
                    "active".to_string(),
                    0,
                    1.0,
                    true,
                )
            } else {
                (
                    format!("Peer ...{}", &id[id.len().saturating_sub(8)..]),
                    "connected".to_string(),
                    0,
                    0.0,
                    false,
                )
            }
        } else if is_local_node {
            (
                "This Node (local)".to_string(),
                "active".to_string(),
                0,
                1.0,
                true,
            )
        } else {
            (
                format!("Peer ...{}", &id[id.len().saturating_sub(8)..]),
                "connected".to_string(),
                0,
                0.0,
                false,
            )
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
            .map(|m| {
                m.models
                    .iter()
                    .map(|mo| ProviderModelInfo {
                        model_name: mo.model_name.clone(),
                        context_size: mo.context_size,
                        price_per_1k_tokens: mo.price_per_1k_tokens,
                        backend: format!("{}", mo.backend),
                    })
                    .collect()
            })
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

// === Channel Management Endpoints ===

#[derive(Serialize)]
struct ChannelInfoResponse {
    id: String,
    platform: String,
    name: String,
    enabled: bool,
    connected: bool,
    messages_sent: u64,
    messages_received: u64,
}

async fn api_list_channels(State(state): State<Arc<WebState>>) -> Json<serde_json::Value> {
    let Some(registry) = &state.channel_registry else {
        return Json(serde_json::json!({
            "channels": [],
            "hint": "No channel registry (node not started with messaging)"
        }));
    };

    let handles = registry.list().await;
    let mut channels = Vec::new();

    for handle in &handles {
        let connected = handle.is_connected().await;
        let stats = handle.stats();
        let config = handle.config();
        channels.push(ChannelInfoResponse {
            id: handle.id().to_string(),
            platform: config.platform.to_string(),
            name: config.name.clone(),
            enabled: config.enabled,
            connected,
            messages_sent: stats.messages_sent,
            messages_received: stats.messages_received,
        });
    }

    Json(serde_json::json!({
        "channels": channels,
        "count": channels.len()
    }))
}

#[derive(Deserialize)]
struct RegisterChannelPayload {
    platform: String,
    name: String,
    enabled: Option<bool>,
    settings: Option<serde_json::Value>,
}

async fn api_register_channel(
    State(state): State<Arc<WebState>>,
    Json(payload): Json<RegisterChannelPayload>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(registry) = &state.channel_registry else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "No channel registry available" })),
        );
    };

    let platform = match payload.platform.as_str() {
        "repl" => crate::messaging::Platform::Repl,
        "webhook" => crate::messaging::Platform::Webhook,
        "websocket" => crate::messaging::Platform::WebSocket,
        "telegram" => crate::messaging::Platform::Telegram,
        "discord" => crate::messaging::Platform::Discord,
        "slack" => crate::messaging::Platform::Slack,
        "matrix" => crate::messaging::Platform::Matrix,
        "p2p" => crate::messaging::Platform::P2p,
        "wasm" => crate::messaging::Platform::Wasm,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("Unknown platform: {}", payload.platform)
                })),
            );
        }
    };

    let config = crate::messaging::ChannelConfig {
        platform,
        name: payload.name.clone(),
        enabled: payload.enabled.unwrap_or(true),
        settings: payload.settings.unwrap_or(serde_json::Value::Null),
        ..Default::default()
    };

    registry
        .add_config(payload.name.clone(), config.clone())
        .await;

    let channel_id = crate::messaging::ChannelId::from_parts(&payload.platform, &payload.name);

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": channel_id.to_string(),
            "platform": payload.platform,
            "name": payload.name,
            "status": "config_registered"
        })),
    )
}

async fn api_remove_channel(
    State(state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(registry) = &state.channel_registry else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "No channel registry available" })),
        );
    };

    let channel_id = crate::messaging::ChannelId(id.clone());
    let removed = registry.remove(&channel_id).await;

    if removed {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "removed": true, "id": id })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Channel not found", "id": id })),
        )
    }
}

async fn api_test_channel(
    State(state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(registry) = &state.channel_registry else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "No channel registry available" })),
        );
    };

    let channel_id = crate::messaging::ChannelId(id.clone());
    let handle = registry.get(&channel_id).await;

    let Some(handle) = handle else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Channel not found", "id": id })),
        );
    };

    let test_message = crate::messaging::ChannelMessage::response(
        channel_id.clone(),
        "PeerClaw test message".to_string(),
    );

    match handle.send(test_message).await {
        Ok(msg_id) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "message_id": msg_id.to_string(),
                "channel_id": id
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
                "channel_id": id
            })),
        ),
    }
}

// === Wallet Endpoints ===

async fn api_wallet_balance(State(state): State<Arc<WebState>>) -> Json<serde_json::Value> {
    if let Some(wallet) = &state.wallet {
        let snapshot = wallet.balance().await;
        Json(serde_json::json!({
            "available_micro": snapshot.available,
            "escrowed_micro": snapshot.in_escrow,
            "staked_micro": snapshot.staked,
            "total_micro": snapshot.total,
            "available": from_micro(snapshot.available),
            "escrowed": from_micro(snapshot.in_escrow),
            "staked": from_micro(snapshot.staked),
            "total": from_micro(snapshot.total),
            "currency": "PCLAW"
        }))
    } else {
        let balance_micro = *state.wallet_balance.read().await;
        Json(serde_json::json!({
            "available_micro": balance_micro,
            "escrowed_micro": 0,
            "staked_micro": 0,
            "total_micro": balance_micro,
            "available": from_micro(balance_micro),
            "escrowed": 0.0,
            "staked": 0.0,
            "total": from_micro(balance_micro),
            "currency": "PCLAW"
        }))
    }
}

#[derive(Deserialize)]
struct TransactionQuery {
    limit: Option<usize>,
}

async fn api_wallet_transactions(
    State(state): State<Arc<WebState>>,
    axum::extract::Query(query): axum::extract::Query<TransactionQuery>,
) -> Json<serde_json::Value> {
    let Some(wallet) = &state.wallet else {
        return Json(serde_json::json!({
            "transactions": [],
            "hint": "Full wallet not available (no transaction history)"
        }));
    };

    let limit = query.limit.unwrap_or(50).min(500);
    let txs = wallet.transactions(limit).await;

    let items: Vec<serde_json::Value> = txs
        .iter()
        .map(|tx| {
            serde_json::json!({
                "id": tx.id.to_string(),
                "type": tx.tx_type.to_string(),
                "amount_micro": tx.amount,
                "amount": from_micro(tx.amount),
                "direction": tx.direction.to_string(),
                "timestamp": tx.timestamp.to_rfc3339(),
            })
        })
        .collect();

    Json(serde_json::json!({
        "transactions": items,
        "count": items.len()
    }))
}

// === Vector Memory Endpoints ===

async fn api_vector_list_collections(
    State(state): State<Arc<WebState>>,
) -> Json<serde_json::Value> {
    let store = state
        .vector_store
        .as_ref()
        .cloned()
        .or_else(|| crate::vector::get_vector_store());

    let Some(store) = store else {
        return Json(serde_json::json!({
            "collections": [],
            "hint": "No vector store initialized"
        }));
    };

    let collections = store.list_collections();
    let items: Vec<serde_json::Value> = collections
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "count": c.count,
                "dimension": c.dimension
            })
        })
        .collect();

    Json(serde_json::json!({
        "collections": items,
        "count": items.len()
    }))
}

#[derive(Deserialize)]
struct CreateCollectionPayload {
    name: String,
    dimension: Option<usize>,
}

async fn api_vector_create_collection(
    State(state): State<Arc<WebState>>,
    Json(payload): Json<CreateCollectionPayload>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = state
        .vector_store
        .as_ref()
        .cloned()
        .or_else(|| crate::vector::get_vector_store());

    let Some(store) = store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "No vector store initialized" })),
        );
    };

    let result = if let Some(dim) = payload.dimension {
        store.create_collection_with_dim(&payload.name, dim)
    } else {
        store.create_collection(&payload.name)
    };

    match result {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "name": payload.name,
                "dimension": payload.dimension.unwrap_or(crate::vector::DEFAULT_EMBEDDING_DIM),
                "created": true
            })),
        ),
        Err(e) => {
            let status = if matches!(e, crate::vector::VectorError::CollectionExists(_)) {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

async fn api_vector_delete_collection(
    State(state): State<Arc<WebState>>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = state
        .vector_store
        .as_ref()
        .cloned()
        .or_else(|| crate::vector::get_vector_store());

    let Some(store) = store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "No vector store initialized" })),
        );
    };

    match store.delete_collection(&name) {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({ "deleted": true, "name": name })),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Collection not found", "name": name })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

#[derive(Deserialize)]
struct VectorSearchPayload {
    collection: String,
    query: Vec<f32>,
    top_k: Option<usize>,
}

async fn api_vector_search(
    State(state): State<Arc<WebState>>,
    Json(payload): Json<VectorSearchPayload>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = state
        .vector_store
        .as_ref()
        .cloned()
        .or_else(|| crate::vector::get_vector_store());

    let Some(store) = store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "No vector store initialized" })),
        );
    };

    let top_k = payload.top_k.unwrap_or(10).min(100);

    match store.search(&payload.collection, payload.query, top_k) {
        Ok(results) => {
            let items: Vec<serde_json::Value> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.id,
                        "score": r.score,
                        "text": r.text,
                        "payload": r.payload,
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "results": items,
                    "count": items.len(),
                    "collection": payload.collection
                })),
            )
        }
        Err(e) => {
            let status = if matches!(e, crate::vector::VectorError::CollectionNotFound(_)) {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

#[derive(Deserialize)]
struct VectorInsertPayload {
    collection: String,
    id: String,
    vector: Vec<f32>,
    text: Option<String>,
    payload: Option<serde_json::Value>,
}

async fn api_vector_insert(
    State(state): State<Arc<WebState>>,
    Json(payload): Json<VectorInsertPayload>,
) -> (StatusCode, Json<serde_json::Value>) {
    let store = state
        .vector_store
        .as_ref()
        .cloned()
        .or_else(|| crate::vector::get_vector_store());

    let Some(store) = store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "No vector store initialized" })),
        );
    };

    let result = if let Some(text) = &payload.text {
        store.upsert_text(
            &payload.collection,
            &payload.id,
            text,
            payload.vector,
            payload.payload,
        )
    } else {
        store.upsert(
            &payload.collection,
            &payload.id,
            payload.vector,
            payload.payload,
        )
    };

    match result {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "inserted": true,
                "id": payload.id,
                "collection": payload.collection
            })),
        ),
        Err(e) => {
            let status = match &e {
                crate::vector::VectorError::CollectionNotFound(_) => StatusCode::NOT_FOUND,
                crate::vector::VectorError::InvalidDimension { .. } => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

// === Tool Execution Endpoints ===

#[derive(Deserialize)]
struct ToolExecutePayload {
    name: String,
    args: Option<serde_json::Value>,
}

async fn api_tool_execute(
    State(state): State<Arc<WebState>>,
    Json(payload): Json<ToolExecutePayload>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(registry) = &state.tools else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "No tool registry (node not started with serve)"
            })),
        );
    };

    let params = payload.args.unwrap_or(serde_json::json!({}));
    let ctx = crate::tools::ToolContext::local(state.local_peer_id.to_string());

    match registry.execute_local(&payload.name, params, &ctx).await {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": result.output.success,
                "data": result.output.data,
                "message": result.output.message,
                "duration_ms": result.execution_time_ms,
                "executed_by": result.executed_by,
                "warnings": result.output.warnings,
            })),
        ),
        Err(e) => {
            let status = match &e {
                crate::tools::ToolError::NotFound(_) => StatusCode::NOT_FOUND,
                crate::tools::ToolError::InvalidParameters(_) => StatusCode::BAD_REQUEST,
                crate::tools::ToolError::NotAuthorized(_) => StatusCode::FORBIDDEN,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

async fn api_tool_detail(
    State(state): State<Arc<WebState>>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(registry) = &state.tools else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "No tool registry (node not started with serve)"
            })),
        );
    };

    let infos = registry.list_tools().await;
    let tool_info = infos.iter().find(|t| t.name == name);

    let Some(info) = tool_info else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Tool not found", "name": name })),
        );
    };

    let schema = registry
        .get(&name)
        .map(|t| t.parameters_schema())
        .unwrap_or(serde_json::json!({}));

    let stats = registry.get_stats(&name).await;

    let mut response = serde_json::json!({
        "name": info.name,
        "description": info.description,
        "domain": format!("{:?}", info.domain),
        "location": format!("{:?}", info.location),
        "price": info.price,
        "peer_id": info.peer_id,
        "parameters_schema": schema,
    });

    if let Some(stats) = stats {
        response["stats"] = serde_json::json!({
            "total_calls": stats.total_calls,
            "successful_calls": stats.successful_calls,
            "failed_calls": stats.failed_calls,
            "total_time_ms": stats.total_time_ms,
        });
    }

    (StatusCode::OK, Json(response))
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use libp2p::identity::Keypair;
    use tower::ServiceExt;

    #[tokio::test]
    async fn axum_matches_api_tasks_param_route() {
        let app = Router::new()
            .route("/", get(|| async { "root" }))
            .route("/tasks/:id", get(|| async { axum::Json("ok") }));
        let root = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(root.status(), StatusCode::OK);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/tasks/550e8400-e29b-41d4-a716-446655440000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    fn test_state() -> Arc<WebState> {
        let peer_id = Keypair::generate_ed25519().public().to_peer_id();
        create_web_state(peer_id, Arc::new(ResourceMonitor::with_defaults()))
    }

    #[tokio::test]
    async fn nested_api_tasks_detail_returns_json() {
        let app = Router::new()
            .nest("/api", api_tasks_router())
            .with_state(test_state());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/tasks/550e8400-e29b-41d4-a716-446655440000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "Task not found");
    }

    #[tokio::test]
    async fn create_router_task_detail_hits_json_handler_with_spa_dist() {
        let app = create_router(test_state());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/tasks/550e8400-e29b-41d4-a716-446655440000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    /// Full router + SPA + middleware stack matching [`start_server`]. Do not `clone()` the
    /// layered `Router` before `oneshot` — that can yield 404 for param routes.
    #[tokio::test]
    async fn get_api_task_detail_is_json_when_spa_enabled() {
        let app = create_router(test_state())
            .layer(
                CorsLayer::very_permissive()
                    .allow_private_network(true),
            )
            .layer(NormalizePathLayer::trim_trailing_slash());

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/tasks/550e8400-e29b-41d4-a716-446655440000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let text = std::str::from_utf8(&bytes).expect("utf8");
        assert!(
            text.trim_start().starts_with('{'),
            "expected JSON from api_task_detail, got: {}",
            &text.chars().take(240).collect::<String>()
        );
        let v: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(v["error"], "Task not found");
    }
}
