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

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
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
    routing::{get, post},
    Router,
};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;
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
    /// Broadcast JSON control-plane events to `/ws` subscribers (tasks, future job streams).
    pub ws_control_tx: broadcast::Sender<serde_json::Value>,
    /// Skill registry when the full node runtime is wired (discovery for console / future agents).
    pub skills: Option<Arc<crate::skills::SkillRegistry>>,
    /// Bounded chat history keyed by `session_id` for `/api/chat`.
    pub chat_sessions: Arc<RwLock<HashMap<String, Vec<ChatMessage>>>>,
    /// MCP client configuration (sidecar / stdio servers documented in `GET /api/mcp/status`).
    pub mcp_config: crate::mcp::McpConfig,
    /// Resolved skills directory (matches `SkillRegistry` when `skills` is set).
    pub skills_dir: PathBuf,
    /// Host `config.toml` path (for UI copy hints).
    pub config_path: PathBuf,
}

fn new_ws_control_plane() -> broadcast::Sender<serde_json::Value> {
    let (tx, _) = broadcast::channel(256);
    tx
}

/// Notify WebSocket subscribers that task rows may have changed.
pub fn broadcast_tasks_changed(tx: &broadcast::Sender<serde_json::Value>) {
    let _ = tx.send(serde_json::json!({ "type": "tasks_changed" }));
}

/// Request to run a task through the agent runtime.
pub struct AgentTaskRequest {
    pub task_id: String,
    pub description: String,
    pub response_tx: tokio::sync::oneshot::Sender<crate::agent::AgentResult>,
    /// Shared task store so the agent can stream logs in real-time
    pub task_store: Arc<RwLock<Vec<WebTask>>>,
    pub ws_control_tx: broadcast::Sender<serde_json::Value>,
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

/// REST handlers under `/api/...` (nested so static `ServeDir` fallback never handles POST/OPTIONS for these paths).
fn api_router() -> Router<Arc<WebState>> {
    Router::new()
        .route("/status", get(api_status))
        .route("/onboarding", get(api_onboarding))
        .route("/peers", get(api_peers))
        .route("/jobs", get(api_jobs))
        .route("/skills/local", get(api_skills_local))
        .route("/skills/network", get(api_skills_network))
        .route("/skills/meta", get(api_skills_meta))
        .route("/skills/scan", post(api_skills_scan))
        .route("/skills/studio/ai", post(api_skills_studio_ai))
        .route("/skills/studio", get(api_skills_studio_list))
        .route(
            "/skills/studio/{slug}",
            get(api_skills_studio_get).put(api_skills_studio_put),
        )
        .route("/mcp/status", get(api_mcp_status))
        .route("/jobs/submit", post(api_submit_job))
        .route("/chat", post(api_chat))
        .route("/chat/stream", post(api_chat_stream))
        .route("/tasks", post(api_create_task))
        .route("/tasks", get(api_list_tasks))
        .route("/tasks/{id}", get(api_task_detail))
        .route("/providers", get(api_list_providers))
        .route("/providers/config", get(api_get_provider_config))
        .route("/providers/config", post(api_set_provider_config))
        .route("/nodes/{id}", get(api_node_detail))
        .route("/swarm/agents", get(api_swarm_agents))
        .route("/swarm/topology", get(api_swarm_topology))
        .route("/swarm/timeline", get(api_swarm_timeline))
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
        provider_tracker: None,
        agent_task_tx: None,
        ws_control_tx: new_ws_control_plane(),
        skills: None,
        chat_sessions: Arc::new(RwLock::new(HashMap::new())),
        mcp_config: crate::mcp::McpConfig::default(),
        skills_dir: crate::bootstrap::base_dir().join("skills"),
        config_path: crate::bootstrap::base_dir().join("config.toml"),
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
        ws_control_tx: new_ws_control_plane(),
        skills: None,
        chat_sessions: Arc::new(RwLock::new(HashMap::new())),
        mcp_config: crate::mcp::McpConfig::default(),
        skills_dir: crate::bootstrap::base_dir().join("skills"),
        config_path: crate::bootstrap::base_dir().join("config.toml"),
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
        ws_control_tx: new_ws_control_plane(),
        skills: None,
        chat_sessions: Arc::new(RwLock::new(HashMap::new())),
        mcp_config: crate::mcp::McpConfig::default(),
        skills_dir: crate::bootstrap::base_dir().join("skills"),
        config_path: crate::bootstrap::base_dir().join("config.toml"),
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
        ws_control_tx: new_ws_control_plane(),
        skills: None,
        chat_sessions: Arc::new(RwLock::new(HashMap::new())),
        mcp_config: crate::mcp::McpConfig::default(),
        skills_dir: crate::bootstrap::base_dir().join("skills"),
        config_path: crate::bootstrap::base_dir().join("config.toml"),
    })
}

/// Start the web server.
pub async fn start_server(addr: SocketAddr, state: Arc<WebState>) -> anyhow::Result<()> {
    let spa = spa_dist_dir().is_some();
    let app = create_router(state).layer(CorsLayer::permissive());

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

#[derive(Serialize)]
struct McpStatusResponse {
    mode: &'static str,
    in_core: bool,
    config: crate::mcp::McpConfig,
    config_path: String,
    /// Example `config.toml` fragment matching `McpConfig` / `McpServerConfig`.
    mcp_toml_snippet: &'static str,
    hint: &'static str,
    spec_url: &'static str,
}

async fn api_mcp_status(State(state): State<Arc<WebState>>) -> Json<McpStatusResponse> {
    Json(McpStatusResponse {
        mode: "sidecar_ready",
        in_core: false,
        config: state.mcp_config.clone(),
        config_path: state.config_path.display().to_string(),
        mcp_toml_snippet: r#"[mcp]
enabled = true
timeout_secs = 30
auto_reconnect = true

[[mcp.servers]]
name = "example"
url = "http://127.0.0.1:3000/mcp"
"#,
        hint: "Run MCP servers as separate OS processes (stdio or HTTP) and bridge tools into agents from the host; core types are in src/mcp for optional embedding.",
        spec_url: "https://modelcontextprotocol.io",
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
    let user_message = req.message.clone();

    let prompt_for_model = if let Some(ref sid) = req.session_id {
        let sessions = state.chat_sessions.read().await;
        let mut prefix = String::new();
        if let Some(msgs) = sessions.get(sid) {
            let skip = msgs.len().saturating_sub(40);
            for m in msgs.iter().skip(skip) {
                prefix.push_str(&m.role);
                prefix.push_str(": ");
                prefix.push_str(&m.content);
                prefix.push('\n');
            }
        }
        if prefix.is_empty() {
            user_message.clone()
        } else {
            format!("Previous turns (compact):\n{prefix}\nUser: {user_message}")
        }
    } else {
        user_message.clone()
    };

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
                        let mut sessions = state.chat_sessions.write().await;
                        let entry = sessions.entry(sid.clone()).or_default();
                        entry.push(ChatMessage {
                            role: "user".into(),
                            content: user_message,
                        });
                        entry.push(ChatMessage {
                            role: "assistant".into(),
                            content: response.text.clone(),
                        });
                        const MAX_MSGS: usize = 80;
                        if entry.len() > MAX_MSGS {
                            let drain = entry.len() - MAX_MSGS;
                            entry.drain(0..drain);
                        }
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

    let prompt_for_model = if let Some(ref sid) = req.session_id {
        let sessions = state.chat_sessions.read().await;
        let mut prefix = String::new();
        if let Some(msgs) = sessions.get(sid) {
            let skip = msgs.len().saturating_sub(40);
            for m in msgs.iter().skip(skip) {
                prefix.push_str(&m.role);
                prefix.push_str(": ");
                prefix.push_str(&m.content);
                prefix.push('\n');
            }
        }
        if prefix.is_empty() {
            user_message.clone()
        } else {
            format!("Previous turns (compact):\n{prefix}\nUser: {user_message}")
        }
    } else {
        user_message.clone()
    };

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
            let mut map = sessions.write().await;
            let entry = map.entry(sid.clone()).or_default();
            entry.push(ChatMessage {
                role: "user".into(),
                content: user_message_for_session,
            });
            entry.push(ChatMessage {
                role: "assistant".into(),
                content: response.text.clone(),
            });
            const MAX_MSGS: usize = 80;
            if entry.len() > MAX_MSGS {
                let drain = entry.len() - MAX_MSGS;
                entry.drain(0..drain);
            }
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

    // If we have an agent channel, spawn execution
    if let Some(tx) = &state.agent_task_tx {
        let store = state.task_store.clone();
        let tid = task_id.clone();
        let description = req.description.clone();
        let tx = tx.clone();
        let ws_tx = state.ws_control_tx.clone();

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
            };

            tracing::info!(task_id = %tid, "Sending task to agent runtime");
            if tx.send(request).await.is_ok() {
                tracing::info!(task_id = %tid, "Task sent, waiting for result...");
                match tokio::time::timeout(std::time::Duration::from_secs(300), response_rx).await {
                    Ok(Ok(result)) => {
                        tracing::info!(task_id = %tid, success = result.success, "Task result received, updating store");
                        let mut tasks = store.write().await;
                        if let Some(t) = tasks.iter_mut().find(|t| t.id == tid) {
                            t.status = if result.success {
                                "completed".to_string()
                            } else {
                                "failed".to_string()
                            };
                            t.completed_at = Some(chrono::Utc::now().to_rfc3339());
                            t.result = Some(result.answer);
                            t.tokens_used = result.total_tokens;
                            t.iterations = result.iterations;
                            if let Some(err) = &result.error {
                                t.logs.push(format!(
                                    "[{}] Error: {}",
                                    chrono::Utc::now().format("%H:%M:%S"),
                                    err
                                ));
                            }
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
                        broadcast_tasks_changed(&ws_tx);
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
            }
        });
    } else {
        // No agent runtime - mark as failed
        let mut tasks = state.task_store.write().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id) {
            t.status = "failed".to_string();
            t.result = Some("No agent runtime loaded. Start with --agent flag.".to_string());
        }
        broadcast_tasks_changed(&state.ws_control_tx);
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
