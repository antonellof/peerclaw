//! Runtime coordinator - integrates all subsystems for distributed execution.
//!
//! This module wires up the TaskExecutor, JobManager, InferenceEngine,
//! and P2P network to enable distributed task execution.

use std::sync::Arc;
use std::time::Duration;

use libp2p::PeerId;
use tokio::sync::{mpsc, RwLock};

use crate::config::Config;
use crate::db::Database;
use crate::executor::{
    ExecutorConfig, MonitorConfig, ResourceMonitor, RouterConfig, TaskExecutor,
};
use crate::executor::remote::{JobProvider, RemoteExecutor, RemoteExecutorConfig};
use crate::executor::task::{ExecutionTask, InferenceTask, TaskResult, WebFetchTask};
use crate::identity::NodeIdentity;
use crate::inference::{InferenceConfig, InferenceEngine, ModelDistributor};
use crate::job::{JobManager, PricingStrategy, network as job_network};
use crate::p2p::{Network, NetworkEvent};
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
    /// Local peer ID
    pub local_peer_id: PeerId,
    /// Configuration
    pub config: Config,
}

impl Runtime {
    /// Create a new runtime with all subsystems initialized.
    pub async fn new(
        identity: Arc<NodeIdentity>,
        database: Database,
        config: Config,
    ) -> anyhow::Result<Self> {
        let local_peer_id = *identity.peer_id();
        let database = Arc::new(database);

        // Create wallet
        let wallet = Arc::new(Wallet::new(
            identity.clone(),
            WalletConfig::default(),
            (*database).clone(),
        )?);

        // Credit some initial tokens for testing
        wallet.credit(crate::wallet::to_micro(1000.0), "initial_balance").await?;

        // Create job manager
        let job_manager = Arc::new(RwLock::new(JobManager::new(wallet.clone())));

        // Create network
        let network = Arc::new(RwLock::new(Network::new(&identity, config.p2p.clone())?));

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

        // Create task executor
        let executor = TaskExecutor::new(resource_monitor.clone(), router_config, executor_config)
            .with_job_manager(job_manager.clone())
            .with_network(network.clone());
        let executor = Arc::new(executor);

        // Create inference engine
        let inference_config = InferenceConfig {
            models_dir: config.inference.models_dir.clone(),
            max_loaded_models: config.inference.max_loaded_models,
            max_memory_mb: config.inference.max_memory_mb,
            gpu_layers: config.inference.gpu_layers,
            context_size: config.inference.context_size,
            batch_size: config.inference.batch_size,
        };
        let inference = Arc::new(InferenceEngine::new(inference_config)?);

        // Create model distributor
        let model_distributor = Arc::new(ModelDistributor::new(config.inference.models_dir.clone()));

        // Create job provider
        let job_provider = Arc::new(JobProvider::new(
            job_manager.clone(),
            network.clone(),
            local_peer_id,
        ));

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
            local_peer_id,
            config,
        })
    }

    /// Subscribe to job-related GossipSub topics.
    pub async fn subscribe_to_job_topics(&self) -> anyhow::Result<()> {
        let mut network = self.network.write().await;
        network.subscribe(job_network::topics::JOB_REQUESTS)?;
        network.subscribe(job_network::topics::JOB_BIDS)?;
        network.subscribe(job_network::topics::JOB_STATUS)?;
        tracing::info!("Subscribed to job marketplace topics");
        Ok(())
    }

    /// Set the pricing strategy for this node.
    pub async fn set_pricing(&self, strategy: PricingStrategy) {
        self.job_manager.write().await.set_pricing(strategy).await;
    }

    /// Execute a task (will be routed automatically).
    pub async fn execute_task(&self, task: ExecutionTask) -> Result<TaskResult, crate::executor::ExecutorError> {
        self.executor.execute(task).await
    }

    /// Execute an inference task.
    pub async fn inference(&self, prompt: &str, model: &str, max_tokens: u32) -> Result<TaskResult, crate::executor::ExecutorError> {
        let task = InferenceTask::new(model, prompt).with_max_tokens(max_tokens);
        self.executor.execute(ExecutionTask::Inference(task)).await
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
                if let Ok(msg) = job_network::deserialize_message(&data) {
                    if let job_network::JobMessage::Request(req_msg) = msg {
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
            }
            t if t == job_network::topics::JOB_BIDS => {
                if let Ok(msg) = job_network::deserialize_message(&data) {
                    if let job_network::JobMessage::Bid(bid_msg) = msg {
                        tracing::debug!(
                            job_id = %bid_msg.bid.job_id,
                            from = %bid_msg.bidder_peer_id,
                            price = bid_msg.bid.price,
                            "Received bid"
                        );
                        if let Err(e) = self.job_manager.write().await.receive_bid(bid_msg.bid).await {
                            tracing::warn!(error = %e, "Failed to process bid");
                        }
                    }
                }
            }
            t if t == job_network::topics::JOB_STATUS => {
                if let Ok(msg) = job_network::deserialize_message(&data) {
                    if let job_network::JobMessage::BidAccepted(accept_msg) = msg {
                        tracing::info!(
                            job_id = %accept_msg.job_id,
                            winner = %accept_msg.winner_peer_id,
                            "Bid accepted"
                        );
                        if let Err(e) = self.job_provider.handle_bid_accepted(accept_msg).await {
                            tracing::warn!(error = %e, "Failed to handle bid acceptance");
                        }
                    }
                }
            }
            _ => {}
        }
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
        let completed_jobs = self.job_manager.read().await.completed_jobs(100).await.len();

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
