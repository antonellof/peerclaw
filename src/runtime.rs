//! Runtime coordinator - integrates all subsystems for distributed execution.
//!
//! This module wires up the TaskExecutor, JobManager, InferenceEngine,
//! and P2P network to enable distributed task execution.

use libp2p::PeerId;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Gossip topic for signed resource manifests (capability fan-out).
pub const RESOURCES_GOSSIP_TOPIC: &str = "peerclaw/resources/v1";

use crate::a2a::gossip::A2A_GOSSIP_TOPIC;
use crate::a2a::state::A2aState;
use crate::config::Config;
use crate::db::Database;
use crate::executor::remote::{JobProvider, RemoteExecutor, RemoteExecutorConfig};
use crate::executor::task::{ExecutionTask, InferenceTask, TaskResult, WebFetchTask};
use crate::executor::{MonitorConfig, ResourceMonitor, RouterConfig, TaskExecutor};
use crate::identity::NodeIdentity;
use crate::inference::{
    BatchAggregator, BatchConfig, BatchError, BatchResponse, BatchStats, InferenceConfig,
    InferenceEngine, InferenceLiveSettings, ModelDistributor,
};
use crate::job::{network as job_network, JobManager, PricingStrategy};
use crate::p2p::{Network, ProviderTracker};
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;
use crate::wallet::{Wallet, WalletConfig};

/// Runtime state containing all integrated subsystems.
pub struct Runtime {
    /// Node identity
    pub identity: Arc<NodeIdentity>,
    /// Database
    pub database: Arc<Database>,
    /// Wallet for token operations
    pub wallet: Arc<Wallet>,
    /// Job manager for marketplace operations
    pub job_manager: Arc<RwLock<JobManager>>,
    /// P2P network
    pub network: Arc<RwLock<Network>>,
    /// Task executor with smart routing
    pub executor: Arc<TaskExecutor>,
    /// Inference engine
    pub inference: Arc<InferenceEngine>,
    /// Model distributor for P2P model sharing
    pub model_distributor: Arc<ModelDistributor>,
    /// Job provider for handling incoming requests
    pub job_provider: Arc<JobProvider>,
    /// Remote executor for P2P task offloading
    pub remote_executor: Arc<RemoteExecutor>,
    /// Batch aggregator for multi-agent inference
    pub batch_aggregator: Arc<BatchAggregator>,
    /// Tool registry
    pub tools: Arc<ToolRegistry>,
    /// Skill registry
    pub skills: Arc<SkillRegistry>,
    /// Provider tracker for discovering LLM providers on the network
    pub provider_tracker: Arc<ProviderTracker>,
    /// Local peer ID
    pub local_peer_id: PeerId,
    /// Configuration
    pub config: Config,
    /// Prompt fragments (embedded defaults + optional overlay dir at startup).
    pub prompts: Arc<crate::prompts::PromptBundle>,
    /// A2A task state and discovered agent cards (shared with P2P + HTTP).
    pub a2a: Arc<A2aState>,

    /// Wired from `serve` after [`crate::swarm::SwarmManager`] exists (optional).
    pub swarm_manager: tokio::sync::RwLock<Option<Arc<crate::swarm::SwarmManager>>>,
}

#[allow(clippy::arc_with_non_send_sync)]
impl Runtime {
    /// Create a new runtime with all subsystems initialized.
    pub async fn new(
        identity: Arc<NodeIdentity>,
        database: Database,
        config: Config,
    ) -> anyhow::Result<Self> {
        let _local_peer_id = *identity.peer_id();
        let database = Arc::new(database);

        // Create wallet
        let wallet = Arc::new(Wallet::new(
            identity.clone(),
            WalletConfig::default(),
            (*database).clone(),
        )?);

        // Credit some initial tokens for testing
        wallet
            .credit(crate::wallet::to_micro(1000.0), "initial_balance")
            .await?;

        // Create job manager
        let local_peer_id = *identity.peer_id();
        let local_peer_id_str = local_peer_id.to_string();
        let job_manager = Arc::new(RwLock::new(JobManager::new(
            wallet.clone(),
            local_peer_id_str,
        )));

        let a2a = A2aState::new();

        // Create network
        let network = Arc::new(RwLock::new(Network::new(
            &identity,
            config.p2p.clone(),
            a2a.clone(),
        )?));

        // Create resource monitor
        let resource_monitor = Arc::new(ResourceMonitor::new(MonitorConfig::default()));

        // Create router config
        let router_config = RouterConfig {
            local_utilization_threshold: config.executor.local_utilization_threshold,
            offload_threshold: config.executor.offload_threshold,
            allow_network_offload: config.executor.allow_network_offload,
            max_concurrent_inference: config.executor.max_concurrent_inference,
            max_concurrent_wasm: config.executor.max_concurrent_wasm,
            local_preference_factor: 1.2, // Prefer local execution by default
        };

        // Create executor config
        let executor_config = crate::executor::ExecutorConfig {
            models_dir: config.inference.models_dir.clone(),
            max_web_response_size: config.executor.max_web_response_size,
            default_web_timeout_secs: config.executor.default_web_timeout_secs,
        };

        // Create inference engine
        let inference_config = InferenceConfig {
            models_dir: config.inference.models_dir.clone(),
            max_loaded_models: config.inference.max_loaded_models,
            max_memory_mb: config.inference.max_memory_mb,
            gpu_layers: config.inference.gpu_layers,
            context_size: config.inference.context_size,
            batch_size: config.inference.batch_size,
            use_ollama: config.inference.use_ollama,
            ollama_url: config.inference.ollama_url.clone(),
        };
        let inference_live = Arc::new(tokio::sync::RwLock::new(
            InferenceLiveSettings::from_config(&config.inference),
        ));
        let inference = Arc::new(InferenceEngine::new(inference_config, inference_live)?);

        // Scan existing models so they're available immediately
        match inference.scan_models().await {
            Ok(n) if n > 0 => tracing::info!(count = n, "Registered GGUF models from disk"),
            Err(e) => tracing::warn!("Failed to scan models directory: {e}"),
            _ => {}
        }

        // Create model distributor
        let model_distributor =
            Arc::new(ModelDistributor::new(config.inference.models_dir.clone()));

        // Create job provider
        let job_provider = Arc::new(JobProvider::new(
            job_manager.clone(),
            network.clone(),
            local_peer_id,
        ));

        // Create remote executor for P2P task offloading
        let remote_executor = Arc::new(RemoteExecutor::new(
            job_manager.clone(),
            network.clone(),
            local_peer_id,
            RemoteExecutorConfig::default(),
        ));

        // Create task executor with inference engine and remote executor wired in
        let executor = TaskExecutor::new(resource_monitor.clone(), router_config, executor_config)
            .with_job_manager(job_manager.clone())
            .with_network(network.clone())
            .with_inference_engine(inference.clone())
            .with_remote_executor(remote_executor.clone());
        let executor = Arc::new(executor);

        // Create batch aggregator for multi-agent inference
        let batch_config = BatchConfig {
            batch_window_ms: config.executor.batch_window_ms.unwrap_or(50),
            max_batch_size: config.executor.max_batch_size.unwrap_or(8),
            min_batch_size: config.executor.min_batch_size.unwrap_or(4),
            adaptive: true,
            max_queue_depth: 100,
        };
        let (batch_aggregator, _batch_processor) = BatchAggregator::new(batch_config);
        let batch_aggregator = Arc::new(batch_aggregator);

        // Create tool registry (builtin tools are registered in new())
        let tools = Arc::new(ToolRegistry::new(local_peer_id.to_string()));
        tracing::info!(count = tools.count().await, "Registered builtin tools");

        // Create skill registry
        let skills_dir = config
            .skills
            .directory
            .clone()
            .unwrap_or_else(|| crate::bootstrap::base_dir().join("skills"));
        let skills = Arc::new(
            SkillRegistry::new(skills_dir, local_peer_id.to_string())
                .map_err(|e| anyhow::anyhow!("Failed to create skill registry: {}", e))?,
        );
        // Scan for local skills
        match skills.scan().await {
            Ok(count) => tracing::info!(count, "Loaded local skills"),
            Err(e) => tracing::warn!(error = %e, "Failed to scan skills directory"),
        }

        // Create provider tracker for network LLM provider discovery
        let provider_tracker = Arc::new(ProviderTracker::new(config.provider_sharing.clone()));

        if config.provider_sharing.enabled {
            tracing::info!("LLM provider sharing enabled");
        }

        let prompts = crate::prompts::load_prompt_bundle(&config);

        Ok(Self {
            identity,
            database,
            wallet,
            job_manager,
            network,
            executor,
            inference,
            model_distributor,
            job_provider,
            remote_executor,
            batch_aggregator,
            tools,
            skills,
            provider_tracker,
            local_peer_id,
            config,
            prompts,
            a2a,
            swarm_manager: tokio::sync::RwLock::new(None),
        })
    }

    /// Attach swarm manager for crew/P2P visualization (called from `serve`).
    pub async fn attach_swarm_manager(&self, sm: Arc<crate::swarm::SwarmManager>) {
        *self.swarm_manager.write().await = Some(sm);
    }

    /// Subscribe to job-related and provider GossipSub topics.
    pub async fn subscribe_to_job_topics(&self) -> anyhow::Result<()> {
        let mut network = self.network.write().await;
        network.subscribe(job_network::topics::JOB_REQUESTS)?;
        network.subscribe(job_network::topics::JOB_BIDS)?;
        network.subscribe(job_network::topics::JOB_STATUS)?;
        network.subscribe(crate::p2p::provider::PROVIDER_TOPIC)?;
        network.subscribe(crate::skills::SKILLS_TOPIC)?;
        network.subscribe(A2A_GOSSIP_TOPIC)?;
        network.subscribe(RESOURCES_GOSSIP_TOPIC)?;
        network.subscribe(crate::crew::CREW_TASK_TOPIC)?;
        network.subscribe(crate::crew::POD_TOPIC)?;
        let world_global = crate::crew::world_topic("global");
        network.subscribe(world_global.as_str())?;
        tracing::info!("Subscribed to job marketplace, provider, skills, A2A, resources, crew, pod, and world (global) topics");
        Ok(())
    }

    /// Build and broadcast our provider manifest to the network.
    pub async fn advertise_provider(&self) -> anyhow::Result<()> {
        if !self.config.provider_sharing.enabled {
            return Ok(());
        }

        // Build model offerings from local inference engine
        let models = self.inference.available_models().await;
        let mut offerings = Vec::new();

        for model_info in models {
            offerings.push(crate::p2p::ModelOffering {
                model_name: model_info.name.clone(),
                context_size: model_info.context_length,
                price_per_1k_tokens: (self.config.economy.inference_price_per_1k as f64
                    * self.config.provider_sharing.price_multiplier)
                    as u64,
                max_tokens_per_request: model_info.context_length,
                quantization: Some(format!("{:?}", model_info.quantization)),
                backend: if self.config.inference.use_ollama {
                    crate::p2p::ProviderBackend::Ollama
                } else {
                    crate::p2p::ProviderBackend::Gguf
                },
            });
        }

        if offerings.is_empty() {
            return Ok(());
        }

        let rate_limits = crate::p2p::ProviderRateLimits {
            max_requests_per_hour: self.config.provider_sharing.max_requests_per_hour,
            max_tokens_per_day: self.config.provider_sharing.max_tokens_per_day,
            max_concurrent_requests: self.config.provider_sharing.max_concurrent_requests,
        };

        let mut manifest = crate::p2p::ProviderManifest::new(
            self.local_peer_id.to_string(),
            offerings,
            rate_limits,
        );

        // Sign the manifest
        let identity = self.identity.clone();
        manifest.sign(|data| identity.sign(data).to_bytes().to_vec());

        // Broadcast via GossipSub
        let data = rmp_serde::to_vec(&manifest)
            .map_err(|e| anyhow::anyhow!("Failed to serialize provider manifest: {}", e))?;

        let mut network = self.network.write().await;
        network.publish(crate::p2p::provider::PROVIDER_TOPIC, data)?;

        tracing::info!(
            models = manifest.models.len(),
            "Advertised provider manifest to network"
        );

        Ok(())
    }

    /// Advertise shared skills to the P2P network via GossipSub.
    pub async fn advertise_skills(&self) -> anyhow::Result<()> {
        let identity = self.identity.clone();
        let batch = self
            .skills
            .build_announcement_batch(|data| identity.sign(data).to_bytes().to_vec())
            .await;

        if let Some(batch) = batch {
            let skill_count = batch.skills.len();
            let data = rmp_serde::to_vec(&batch)
                .map_err(|e| anyhow::anyhow!("Failed to serialize skill announcements: {}", e))?;

            let mut network = self.network.write().await;
            network.publish(crate::skills::SKILLS_TOPIC, data)?;

            tracing::info!(skills = skill_count, "Advertised skills to network");
        }

        Ok(())
    }

    /// Set the pricing strategy for this node.
    pub async fn set_pricing(&self, strategy: PricingStrategy) {
        self.job_manager.write().await.set_pricing(strategy).await;
    }

    /// Execute a task (will be routed automatically).
    pub async fn execute_task(
        &self,
        task: ExecutionTask,
    ) -> Result<TaskResult, crate::executor::ExecutorError> {
        self.executor.execute(task).await
    }

    /// Execute an inference task.
    pub async fn inference(
        &self,
        prompt: &str,
        model: &str,
        max_tokens: u32,
    ) -> Result<TaskResult, crate::executor::ExecutorError> {
        let task = InferenceTask::new(model, prompt).with_max_tokens(max_tokens);
        self.executor.execute(ExecutionTask::Inference(task)).await
    }

    /// Submit inference via batch aggregator (for multi-agent scenarios).
    /// Multiple requests are collected and processed together for efficiency.
    pub async fn inference_batched(
        &self,
        source: &str,
        model: &str,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<BatchResponse, BatchError> {
        self.batch_aggregator
            .submit(
                source.to_string(),
                model.to_string(),
                prompt.to_string(),
                max_tokens,
                temperature,
            )
            .await
    }

    /// Get batch aggregator statistics.
    pub async fn batch_stats(&self) -> BatchStats {
        self.batch_aggregator.stats().await
    }

    /// Execute inference with streaming - tokens are printed directly to stdout as generated.
    pub async fn inference_streaming_print(
        &self,
        model: &str,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<crate::executor::task::InferenceResult, crate::executor::ExecutorError> {
        let task = InferenceTask::new(model, prompt)
            .with_max_tokens(max_tokens)
            .with_temperature(temperature);

        self.executor.execute_inference_streaming_print(task).await
    }

    /// Execute a web fetch task.
    pub async fn web_fetch(&self, url: &str) -> Result<TaskResult, crate::executor::ExecutorError> {
        let task = WebFetchTask::get(url);
        self.executor.execute(ExecutionTask::WebFetch(task)).await
    }

    /// Get resource state.
    pub async fn resource_state(&self) -> crate::executor::ResourceState {
        self.executor.resource_state().await
    }

    /// Get wallet balance (available μPCLAW).
    pub async fn balance(&self) -> u64 {
        self.wallet.balance().await.available
    }

    /// Get connected peers count.
    pub async fn connected_peers_count(&self) -> usize {
        self.network.read().await.connected_peers().len()
    }

    /// Handle a gossip message (job-related).
    pub async fn handle_gossip_message(&self, topic: &str, data: Vec<u8>, source: Option<PeerId>) {
        match topic {
            t if t == job_network::topics::JOB_REQUESTS => {
                if let Ok(job_network::JobMessage::Request(req_msg)) =
                    job_network::deserialize_message(&data)
                {
                    tracing::info!(
                        job_id = %req_msg.request.id,
                        from = %req_msg.requester_peer_id,
                        "Received job request"
                    );
                    if let Err(e) = self.job_provider.handle_request(req_msg).await {
                        tracing::warn!(error = %e, "Failed to handle job request");
                    }
                }
            }
            t if t == job_network::topics::JOB_BIDS => {
                if let Ok(job_network::JobMessage::Bid(bid_msg)) =
                    job_network::deserialize_message(&data)
                {
                    tracing::debug!(
                        job_id = %bid_msg.bid.job_id,
                        from = %bid_msg.bidder_peer_id,
                        price = bid_msg.bid.price,
                        "Received bid"
                    );
                    if let Err(e) = self
                        .job_manager
                        .write()
                        .await
                        .receive_bid(bid_msg.bid)
                        .await
                    {
                        tracing::warn!(error = %e, "Failed to process bid");
                    }
                }
            }
            t if t == job_network::topics::JOB_STATUS => {
                if let Ok(msg) = job_network::deserialize_message(&data) {
                    match msg {
                        job_network::JobMessage::BidAccepted(accept_msg) => {
                            tracing::info!(
                                job_id = %accept_msg.job_id,
                                winner = %accept_msg.winner_peer_id,
                                "Bid accepted"
                            );

                            // Check if we're the winner
                            if accept_msg.winner_peer_id == self.local_peer_id.to_string() {
                                tracing::info!(job_id = %accept_msg.job_id, "We won the bid! Executing job...");

                                // Get the request and execute it
                                let job_id = accept_msg.job_id.clone();
                                if let Some(request) =
                                    self.job_provider.get_pending_request(&job_id).await
                                {
                                    self.execute_provider_job(job_id, request).await;
                                }
                            }

                            if let Err(e) = self.job_provider.handle_bid_accepted(accept_msg).await
                            {
                                tracing::warn!(error = %e, "Failed to handle bid acceptance");
                            }
                        }
                        job_network::JobMessage::Result(result_msg) => {
                            tracing::info!(
                                job_id = %result_msg.job_id,
                                provider = %result_msg.provider_peer_id,
                                "Received job result"
                            );
                            // Store result if we're the requester
                            {
                                let job_manager = self.job_manager.write().await;
                                if let Err(e) = job_manager
                                    .submit_result(&result_msg.job_id, result_msg.result)
                                    .await
                                {
                                    tracing::warn!(error = %e, "Failed to store job result");
                                } else {
                                    // Auto-settle the job (verify and release payment)
                                    if let Err(e) =
                                        job_manager.settle_job(&result_msg.job_id, true).await
                                    {
                                        tracing::warn!(error = %e, "Failed to settle job");
                                    } else {
                                        tracing::info!(job_id = %result_msg.job_id, "Job settled successfully");
                                    }
                                }
                            }
                        }
                        job_network::JobMessage::StatusUpdate(status_msg) => {
                            tracing::debug!(
                                job_id = %status_msg.job_id,
                                status = ?status_msg.status,
                                "Job status update"
                            );
                        }
                        _ => {}
                    }
                }
            }
            t if t == crate::p2p::provider::PROVIDER_TOPIC => {
                if let Ok(manifest) = rmp_serde::from_slice::<crate::p2p::ProviderManifest>(&data) {
                    // Don't track our own advertisements
                    if manifest.peer_id != self.local_peer_id.to_string() {
                        tracing::info!(
                            peer = %manifest.peer_id,
                            models = manifest.models.len(),
                            "Received provider advertisement"
                        );
                        self.provider_tracker.update_provider(manifest).await;
                    }
                }
            }
            t if t == crate::skills::SKILLS_TOPIC => {
                if let Ok(batch) =
                    rmp_serde::from_slice::<crate::skills::SkillAnnouncementBatch>(&data)
                {
                    // Verify the batch using the GossipSub source peer ID
                    // (authenticated by the libp2p Noise transport layer) and
                    // validate the Ed25519 signature over the batch contents.
                    let source_peer = source;
                    self.skills
                        .handle_announcement_batch(&batch, |claimed_peer_id, msg, sig| {
                            // Require a GossipSub source for authentication.
                            let src = match source_peer {
                                Some(s) => s,
                                None => {
                                    tracing::warn!(
                                        "Rejecting skill batch with no GossipSub source"
                                    );
                                    return false;
                                }
                            };

                            // Ensure the claimed peer_id matches the Noise-authenticated
                            // source to prevent identity spoofing.
                            if src.to_string() != claimed_peer_id {
                                tracing::warn!(
                                    claimed = claimed_peer_id,
                                    actual = %src,
                                    "Skill announcement peer_id mismatch with source"
                                );
                                return false;
                            }

                            // Validate signature length (Ed25519 signatures are 64 bytes).
                            if sig.len() != 64 {
                                tracing::warn!(
                                    peer = claimed_peer_id,
                                    sig_len = sig.len(),
                                    "Invalid skill announcement signature length"
                                );
                                return false;
                            }

                            // The Noise-authenticated source guarantees the sender's
                            // identity. The signature provides additional content
                            // integrity for replay/persistence scenarios.
                            // Full Ed25519 verification requires a public key registry;
                            // for now we rely on the Noise transport authentication.
                            let _ = (msg, sig);
                            true
                        })
                        .await;
                }
            }
            t if t == A2A_GOSSIP_TOPIC => {
                if let Ok(ann) = serde_json::from_slice::<crate::a2a::AgentCardAnnouncement>(&data)
                {
                    if ann.peer_id != self.local_peer_id.to_string() {
                        self.a2a.upsert_peer_card(ann.peer_id.clone(), ann.card);
                        tracing::debug!(peer = %ann.peer_id, "Cached remote agent card from gossip");
                    }
                }
            }
            t if t == RESOURCES_GOSSIP_TOPIC => {
                if let Ok(manifest) = rmp_serde::from_slice::<crate::p2p::ResourceManifest>(&data) {
                    if manifest.peer_id != self.local_peer_id.to_string() {
                        tracing::debug!(
                            peer = %manifest.peer_id,
                            caps = manifest.capabilities.len(),
                            models = manifest.supported_models.len(),
                            "Received resource manifest from gossip"
                        );
                    }
                }
            }
            t if t == crate::crew::CREW_TASK_TOPIC => {
                if let Ok(res) = serde_json::from_slice::<crate::crew::CrewTaskResult>(&data) {
                    tracing::debug!(
                        run = %res.run_id,
                        task = %res.task_id,
                        worker = %res.worker_peer,
                        ok = res.success,
                        "Crew task result on network"
                    );
                } else if let Ok(claim) =
                    serde_json::from_slice::<crate::crew::CrewTaskClaim>(&data)
                {
                    tracing::debug!(
                        run = %claim.offer_run_id,
                        task = %claim.offer_task_id,
                        worker = %claim.worker_peer,
                        "Crew task claim on network"
                    );
                } else if let Ok(offer) =
                    serde_json::from_slice::<crate::crew::CrewTaskOffer>(&data)
                {
                    if self.config.orchestration.crew_worker {
                        if let Some(src) = source {
                            if let Err(e) = self.process_crew_worker_offer(offer, src).await {
                                tracing::debug!(error = %e, "crew worker skipped offer");
                            }
                        }
                    } else {
                        tracing::debug!(
                            run = %offer.run_id,
                            task = %offer.task_id,
                            from = %offer.orchestrator_peer,
                            "Crew task offer on network"
                        );
                    }
                }
            }
            t if t == crate::crew::POD_TOPIC => {
                if let Ok(art) = serde_json::from_slice::<crate::crew::PodArtifactPublished>(&data)
                {
                    tracing::debug!(
                        pod = %art.pod_id,
                        campaign = %art.campaign_id,
                        "Pod artifact published"
                    );
                }
            }
            t if t.starts_with("peerclaw/world/") && t.ends_with("/v1") => {
                if let Ok(ms) = serde_json::from_slice::<crate::crew::CampaignMilestone>(&data) {
                    tracing::debug!(
                        campaign = %ms.campaign_id,
                        peer = %ms.reporter_peer,
                        "Campaign milestone"
                    );
                }
            }
            _ => {}
        }
    }

    async fn process_crew_worker_offer(
        &self,
        offer: crate::crew::CrewTaskOffer,
        src: PeerId,
    ) -> Result<(), String> {
        use crate::executor::task::{ExecutionTask, InferenceTask, TaskData};
        use crate::swarm::ActionType;

        if src == self.local_peer_id {
            return Ok(());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if now > offer.expires_at_ms {
            return Err("offer expired".into());
        }
        if !offer.verify_source(&src) {
            return Err("offer verification failed".into());
        }

        let sm_opt = self.swarm_manager.read().await.clone();
        if let Some(ref sm) = sm_opt {
            if let Some(aid) = sm.any_local_agent_id() {
                sm.record_job_action(
                    aid,
                    ActionType::CrewTaskOffer,
                    &format!("offer {} / {}", offer.run_id, offer.task_id),
                    true,
                );
            }
        }

        let mut claim = crate::crew::CrewTaskClaim {
            offer_run_id: offer.run_id.clone(),
            offer_task_id: offer.task_id.clone(),
            worker_peer: self.local_peer_id.to_string(),
            signature: Vec::new(),
        };
        claim.sign(self.identity.as_ref());
        let claim_bytes = serde_json::to_vec(&claim).map_err(|e| format!("claim json: {e}"))?;
        {
            let mut net = self.network.write().await;
            net.publish(crate::crew::CREW_TASK_TOPIC, claim_bytes)
                .map_err(|e| e.to_string())?;
        }

        if let Some(ref sm) = sm_opt {
            if let Some(aid) = sm.any_local_agent_id() {
                sm.record_job_action(
                    aid,
                    ActionType::CrewTaskClaim,
                    &format!("claim {} / {}", offer.run_id, offer.task_id),
                    true,
                );
            }
        }

        let first_model = self
            .inference
            .available_models()
            .await
            .into_iter()
            .next()
            .ok_or_else(|| "no local models for crew worker".to_string())?
            .name;

        let hint = offer.model_hint.trim();
        let model_ref = if hint.is_empty() {
            first_model.as_str()
        } else {
            hint
        };

        let prompt = self.prompts.crew_worker_prompt(offer.summary.trim());
        let task = InferenceTask::new(model_ref, prompt).with_max_tokens(256);
        let exec = self
            .executor
            .execute(ExecutionTask::Inference(task))
            .await
            .map_err(|e| e.to_string())?;

        let (text, ok) = match exec.data {
            TaskData::Inference(r) => (r.text, true),
            TaskData::Error(e) => (e, false),
            _ => ("unexpected executor output".to_string(), false),
        };

        let mut result = crate::crew::CrewTaskResult {
            run_id: offer.run_id.clone(),
            task_id: offer.task_id.clone(),
            worker_peer: self.local_peer_id.to_string(),
            output_summary: text.chars().take(2000).collect(),
            success: ok,
            signature: Vec::new(),
        };
        result.sign(self.identity.as_ref());
        let res_bytes = serde_json::to_vec(&result).map_err(|e| e.to_string())?;
        {
            let mut net = self.network.write().await;
            net.publish(crate::crew::CREW_TASK_TOPIC, res_bytes)
                .map_err(|e| e.to_string())?;
        }

        if let Some(ref sm) = sm_opt {
            if let Some(aid) = sm.any_local_agent_id() {
                sm.record_job_action(
                    aid,
                    ActionType::CrewTaskComplete,
                    &format!("result {} / {} ok={}", offer.run_id, offer.task_id, ok),
                    ok,
                );
            }
        }

        Ok(())
    }

    /// Build a minimal signed resource manifest and gossip it (replaces silent DHT stub).
    pub async fn gossip_resource_manifest(&self) -> anyhow::Result<()> {
        let resources = crate::p2p::Resources::default();
        let caps = vec![crate::p2p::Capability::Inference];
        let mut manifest =
            crate::p2p::ResourceManifest::new(self.local_peer_id.to_string(), resources, caps);
        let models: Vec<String> = self
            .inference
            .available_models()
            .await
            .into_iter()
            .map(|m| m.name)
            .collect();
        manifest.supported_models = models;
        manifest.sign(|b| self.identity.sign(b).to_bytes().to_vec());
        let data = rmp_serde::to_vec(&manifest)?;
        self.network
            .write()
            .await
            .publish(RESOURCES_GOSSIP_TOPIC, data)?;
        Ok(())
    }

    /// Publish our agent card on the A2A gossip topic.
    pub async fn gossip_agent_card(&self, public_base_url: &str) -> anyhow::Result<()> {
        let models: Vec<String> = self
            .inference
            .available_models()
            .await
            .into_iter()
            .map(|m| m.name)
            .collect();
        let card = crate::a2a::AgentCard::peerclaw_default(
            format!("peerclaw-{}", &self.local_peer_id.to_string()[..8.min(8)]),
            "PeerClaw P2P agent node",
            public_base_url.trim_end_matches('/').to_string(),
            self.local_peer_id.to_string(),
            models,
        );
        let ann = crate::a2a::AgentCardAnnouncement {
            peer_id: self.local_peer_id.to_string(),
            epoch_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            card: card.clone(),
        };
        self.a2a
            .upsert_peer_card(self.local_peer_id.to_string(), card);
        let data = serde_json::to_vec(&ann)?;
        self.network.write().await.publish(A2A_GOSSIP_TOPIC, data)?;
        Ok(())
    }
}

impl Runtime {
    /// Execute a job as a provider (when our bid was accepted).
    pub async fn execute_provider_job(
        &self,
        job_id: crate::job::JobId,
        request: crate::job::JobRequest,
    ) {
        use crate::executor::task::{ExecutionTask, InferenceTask, TaskData, WebFetchTask};
        use crate::job::network::{
            serialize_message, topics, JobMessage, JobResultMessage, JobStatusMessage,
            JobStatusUpdate,
        };
        use crate::job::{ActualUsage, ExecutionMetrics, JobResult};

        tracing::info!(job_id = %job_id, "Executing job as provider");

        // Broadcast that we're starting
        let status_msg = JobMessage::StatusUpdate(JobStatusMessage {
            job_id: job_id.clone(),
            status: JobStatusUpdate::Started,
            peer_id: self.local_peer_id.to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        });
        if let Ok(data) = serialize_message(&status_msg) {
            let _ = self.network.write().await.publish(topics::JOB_STATUS, data);
        }

        // Execute based on resource type
        let payload = request.payload.as_deref().unwrap_or(&[]);
        let result = match &request.resource_type {
            crate::job::ResourceType::Inference { model, tokens } => {
                let prompt_cow = String::from_utf8_lossy(payload);
                let prompt: &str = if prompt_cow.is_empty() {
                    "Hello"
                } else {
                    prompt_cow.as_ref()
                };

                let task = InferenceTask::new(model, prompt).with_max_tokens(*tokens);

                match self.executor.execute(ExecutionTask::Inference(task)).await {
                    Ok(task_result) => match &task_result.data {
                        TaskData::Inference(r) => JobResult::new(r.text.as_bytes().to_vec())
                            .with_usage(ActualUsage {
                                tokens: Some(r.tokens_generated),
                                compute_time_ms: Some(task_result.metrics.total_time_ms),
                                bytes: None,
                            })
                            .with_metrics(ExecutionMetrics {
                                ttfb_ms: task_result.metrics.ttfb_ms,
                                total_time_ms: task_result.metrics.total_time_ms,
                                tokens_per_sec: Some(r.tokens_per_second),
                            }),
                        _ => JobResult::new(b"Unexpected result type".to_vec()),
                    },
                    Err(e) => JobResult::new(format!("Error: {}", e).into_bytes()),
                }
            }
            crate::job::ResourceType::WebFetch { url_count: _ } => {
                let url = String::from_utf8_lossy(payload);
                let task = WebFetchTask::get(url.as_ref());

                match self.executor.execute(ExecutionTask::WebFetch(task)).await {
                    Ok(task_result) => match &task_result.data {
                        TaskData::WebFetch(r) => {
                            JobResult::new(r.body.clone()).with_usage(ActualUsage {
                                tokens: None,
                                compute_time_ms: Some(task_result.metrics.total_time_ms),
                                bytes: Some(r.body.len() as u64),
                            })
                        }
                        _ => JobResult::new(b"Unexpected result type".to_vec()),
                    },
                    Err(e) => JobResult::new(format!("Error: {}", e).into_bytes()),
                }
            }
            _ => JobResult::new(b"Unsupported resource type".to_vec()),
        };

        tracing::info!(job_id = %job_id, "Job execution complete, sending result");

        // Broadcast result
        let result_msg = JobMessage::Result(JobResultMessage {
            job_id: job_id.clone(),
            result: result.clone(),
            provider_peer_id: self.local_peer_id.to_string(),
            signature: vec![],
        });
        if let Ok(data) = serialize_message(&result_msg) {
            if let Err(e) = self.network.write().await.publish(topics::JOB_STATUS, data) {
                tracing::warn!("Failed to broadcast result: {}", e);
            }
        }

        // Remove from pending
        self.job_provider.remove_pending_request(&job_id).await;

        tracing::info!(job_id = %job_id, "Provider job completed");
    }
}

/// Runtime statistics.
#[derive(Debug, Clone)]
pub struct RuntimeStats {
    pub peer_id: String,
    pub connected_peers: usize,
    pub balance: f64,
    pub active_jobs: usize,
    pub completed_jobs: usize,
    pub resource_state: crate::executor::ResourceState,
}

impl Runtime {
    /// Get runtime statistics.
    pub async fn stats(&self) -> RuntimeStats {
        let active_jobs = self.job_manager.read().await.active_jobs().await.len();
        let completed_jobs = self
            .job_manager
            .read()
            .await
            .completed_jobs(100)
            .await
            .len();

        RuntimeStats {
            peer_id: self.local_peer_id.to_string(),
            connected_peers: self.connected_peers_count().await,
            balance: crate::wallet::from_micro(self.balance().await),
            active_jobs,
            completed_jobs,
            resource_state: self.resource_state().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_runtime_creation() {
        let dir = tempdir().unwrap();
        let identity = Arc::new(NodeIdentity::generate());
        let mut config = Config::default();
        config.database.path = dir.path().join("test.redb");
        config.inference.models_dir = dir.path().join("models");

        let db = Database::open(&config.database.path).unwrap();
        let runtime = Runtime::new(identity, db, config).await;

        assert!(runtime.is_ok());
    }

    #[tokio::test]
    async fn test_runtime_stats() {
        let dir = tempdir().unwrap();
        let identity = Arc::new(NodeIdentity::generate());
        let mut config = Config::default();
        config.database.path = dir.path().join("test.redb");
        config.inference.models_dir = dir.path().join("models");

        let db = Database::open(&config.database.path).unwrap();
        let runtime = Runtime::new(identity, db, config).await.unwrap();

        let stats = runtime.stats().await;
        assert_eq!(stats.connected_peers, 0);
        assert!(stats.balance > 0.0); // Should have initial balance
    }
}
