//! Smart task routing - decides local vs. network execution.

use super::resource::{ResourceMonitor, ResourceState};
use super::task::{ExecutionTask, ResourceRequirements};
use std::sync::Arc;

/// Decides where to execute tasks based on local resources.
pub struct TaskRouter {
    resource_monitor: Arc<ResourceMonitor>,
    config: RouterConfig,
}

impl TaskRouter {
    /// Create a new task router.
    pub fn new(resource_monitor: Arc<ResourceMonitor>, config: RouterConfig) -> Self {
        Self {
            resource_monitor,
            config,
        }
    }

    /// Decide where to execute a task.
    pub async fn route(&self, task: &ExecutionTask) -> RoutingDecision {
        let requirements = task.estimate_requirements();
        let state = self.resource_monitor.current_state().await;

        // Check if we can handle it locally
        if self.should_execute_locally(&state, &requirements, task) {
            return RoutingDecision::ExecuteLocally;
        }

        // Check if we should offload to network
        if self.should_offload(&state, &requirements) {
            return RoutingDecision::OffloadToNetwork {
                requirements: self.to_job_requirements(&requirements, task),
            };
        }

        // Neither local nor network viable
        RoutingDecision::InsufficientResources {
            reason: self.build_insufficient_reason(&state, &requirements),
        }
    }

    /// Check if task should be executed locally.
    fn should_execute_locally(
        &self,
        state: &ResourceState,
        requirements: &ResourceRequirements,
        task: &ExecutionTask,
    ) -> bool {
        // First, check if we have the basic resources
        if !state.can_handle(requirements) {
            return false;
        }

        // For inference, check if the model is loaded (prefer local if already loaded)
        if let ExecutionTask::Inference(inference_task) = task {
            if state.has_model(&inference_task.model) {
                return true;
            }
        }

        // Check utilization threshold
        let utilization = 1.0 - state.available_capacity();
        if utilization > self.config.local_utilization_threshold {
            return false;
        }

        // Check concurrent task limits
        match task {
            ExecutionTask::Inference(_) => {
                if state.active_inference_tasks >= self.config.max_concurrent_inference {
                    return false;
                }
            }
            ExecutionTask::WasmExecution(_) => {
                if state.active_wasm_tasks >= self.config.max_concurrent_wasm {
                    return false;
                }
            }
            _ => {}
        }

        true
    }

    /// Check if task should be offloaded to the network.
    fn should_offload(&self, state: &ResourceState, requirements: &ResourceRequirements) -> bool {
        // If we're at high utilization, consider offloading
        let utilization = 1.0 - state.available_capacity();

        // Check if local resources are insufficient
        if !state.can_handle(requirements) {
            // We can't handle it locally, so offload if network is allowed
            return self.config.allow_network_offload;
        }

        // Even if we could handle it, offload if we're busy
        if utilization > self.config.offload_threshold {
            return self.config.allow_network_offload;
        }

        false
    }

    /// Build explanation for why resources are insufficient.
    fn build_insufficient_reason(
        &self,
        state: &ResourceState,
        requirements: &ResourceRequirements,
    ) -> String {
        let mut reasons = Vec::new();

        if state.ram_available_mb < requirements.ram_mb {
            reasons.push(format!(
                "need {}MB RAM, have {}MB",
                requirements.ram_mb, state.ram_available_mb
            ));
        }

        if let Some(required_vram) = requirements.vram_mb {
            match state.vram_available_mb {
                None => reasons.push("GPU required but not available".to_string()),
                Some(available) if available < required_vram => {
                    reasons.push(format!(
                        "need {}MB VRAM, have {}MB",
                        required_vram, available
                    ));
                }
                _ => {}
            }
        }

        if !self.config.allow_network_offload {
            reasons.push("network offload disabled".to_string());
        }

        if reasons.is_empty() {
            "unknown resource constraint".to_string()
        } else {
            reasons.join("; ")
        }
    }

    /// Convert task requirements to job requirements for network offload.
    fn to_job_requirements(
        &self,
        requirements: &ResourceRequirements,
        task: &ExecutionTask,
    ) -> PeerFilter {
        PeerFilter {
            min_ram_mb: Some(requirements.ram_mb),
            min_vram_mb: requirements.vram_mb,
            min_cpu_cores: Some(requirements.cpu_cores),
            required_model: match task {
                ExecutionTask::Inference(t) => Some(t.model.clone()),
                _ => None,
            },
            required_capabilities: match task {
                ExecutionTask::Inference(_) => vec!["inference".to_string()],
                ExecutionTask::WasmExecution(_) => vec!["wasm".to_string()],
                ExecutionTask::WebFetch(_) | ExecutionTask::WebSearch(_) => {
                    vec!["web_proxy".to_string()]
                }
            },
        }
    }
}

/// Configuration for task routing.
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// CPU/RAM utilization threshold for local execution (0.0 - 1.0)
    /// Above this, prefer network offload
    pub local_utilization_threshold: f64,
    /// Utilization threshold above which to offload even if we could handle locally
    pub offload_threshold: f64,
    /// Allow offloading tasks to network peers
    pub allow_network_offload: bool,
    /// Maximum concurrent inference tasks before offloading
    pub max_concurrent_inference: u32,
    /// Maximum concurrent WASM tasks before offloading
    pub max_concurrent_wasm: u32,
    /// Prefer local execution even if slightly busier
    /// (1.0 = no preference, 1.2 = prefer local up to 20% more load)
    pub local_preference_factor: f64,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            local_utilization_threshold: 0.8,
            offload_threshold: 0.9,
            allow_network_offload: true,
            max_concurrent_inference: 2,
            max_concurrent_wasm: 10,
            local_preference_factor: 1.2,
        }
    }
}

/// The decision from the router.
#[derive(Debug, Clone)]
pub enum RoutingDecision {
    /// Execute the task on this node
    ExecuteLocally,
    /// Offload to network peers
    OffloadToNetwork {
        /// Requirements for finding a suitable peer
        requirements: PeerFilter,
    },
    /// Cannot execute - insufficient resources everywhere
    InsufficientResources {
        /// Explanation of why
        reason: String,
    },
}

/// Filter for finding suitable network peers to handle a task.
#[derive(Debug, Clone, Default)]
pub struct PeerFilter {
    /// Minimum RAM needed
    pub min_ram_mb: Option<u32>,
    /// Minimum VRAM needed
    pub min_vram_mb: Option<u32>,
    /// Minimum CPU cores
    pub min_cpu_cores: Option<u16>,
    /// Required model to be available
    pub required_model: Option<String>,
    /// Required capabilities
    pub required_capabilities: Vec<String>,
}

impl PeerFilter {
    /// Check if a peer's resources satisfy these requirements.
    pub fn satisfied_by(&self, resources: &ResourceState) -> bool {
        if let Some(min_ram) = self.min_ram_mb {
            if resources.ram_available_mb < min_ram {
                return false;
            }
        }

        if let Some(min_vram) = self.min_vram_mb {
            match resources.vram_available_mb {
                Some(vram) if vram >= min_vram => {}
                _ => return false,
            }
        }

        if let Some(ref model) = self.required_model {
            if !resources.has_model(model) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::resource::MonitorConfig;
    use crate::executor::task::InferenceTask;

    fn create_router() -> TaskRouter {
        let monitor = Arc::new(ResourceMonitor::new(MonitorConfig::default()));
        TaskRouter::new(monitor, RouterConfig::default())
    }

    #[tokio::test]
    async fn test_route_lightweight_task_locally() {
        let router = create_router();

        let task = ExecutionTask::WebFetch(super::super::task::WebFetchTask::get(
            "https://example.com",
        ));
        let decision = router.route(&task).await;

        matches!(decision, RoutingDecision::ExecuteLocally);
    }

    #[test]
    fn test_job_requirements_satisfied() {
        let requirements = PeerFilter {
            min_ram_mb: Some(4000),
            min_vram_mb: Some(6000),
            min_cpu_cores: None,
            required_model: Some("llama-7b".to_string()),
            required_capabilities: vec![],
        };

        let state = ResourceState {
            ram_available_mb: 8000,
            vram_available_mb: Some(8000),
            loaded_models: vec!["llama-7b".to_string()],
            ..Default::default()
        };

        assert!(requirements.satisfied_by(&state));
    }

    #[test]
    fn test_job_requirements_not_satisfied_missing_model() {
        let requirements = PeerFilter {
            min_ram_mb: None,
            min_vram_mb: None,
            min_cpu_cores: None,
            required_model: Some("llama-70b".to_string()),
            required_capabilities: vec![],
        };

        let state = ResourceState {
            loaded_models: vec!["llama-7b".to_string()],
            ..Default::default()
        };

        assert!(!requirements.satisfied_by(&state));
    }
}
