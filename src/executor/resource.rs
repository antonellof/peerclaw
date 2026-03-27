//! Real-time system resource monitoring.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tokio::sync::RwLock;
use tokio::time::interval;

/// Monitors system resources in real-time.
pub struct ResourceMonitor {
    state: Arc<RwLock<ResourceState>>,
    config: MonitorConfig,
}

impl ResourceMonitor {
    /// Create a new resource monitor.
    pub fn new(config: MonitorConfig) -> Self {
        let initial_state = ResourceState::default();
        Self {
            state: Arc::new(RwLock::new(initial_state)),
            config,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(MonitorConfig::default())
    }

    /// Start background monitoring task.
    pub fn start_background_updates(&self) -> tokio::task::JoinHandle<()> {
        let state = self.state.clone();
        let update_interval = self.config.update_interval;

        tokio::spawn(async move {
            let mut sys = System::new_with_specifics(
                RefreshKind::new()
                    .with_cpu(CpuRefreshKind::everything())
                    .with_memory(MemoryRefreshKind::everything()),
            );

            let mut ticker = interval(update_interval);

            loop {
                ticker.tick().await;

                // Refresh system info
                sys.refresh_cpu_usage();
                sys.refresh_memory();

                // Calculate CPU usage (average across all cores)
                let cpu_usage = sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>()
                    / sys.cpus().len() as f32
                    / 100.0;

                let ram_total_mb = (sys.total_memory() / 1_000_000) as u32;
                let ram_used_mb = (sys.used_memory() / 1_000_000) as u32;
                let ram_available_mb = ram_total_mb.saturating_sub(ram_used_mb);

                // Update state
                let mut current_state = state.write().await;
                current_state.cpu_usage = cpu_usage as f64;
                current_state.ram_total_mb = ram_total_mb;
                current_state.ram_available_mb = ram_available_mb;

                // GPU monitoring — best-effort, no external crate needed.
                current_state.gpu_usage = probe_gpu_usage();
            }
        })
    }

    /// Get the current resource state.
    pub async fn current_state(&self) -> ResourceState {
        self.state.read().await.clone()
    }

    /// Check if we have enough resources for a task.
    pub async fn can_handle(&self, requirements: &super::task::ResourceRequirements) -> bool {
        let state = self.state.read().await;
        state.can_handle(requirements)
    }

    /// Get available capacity as a ratio (0.0 - 1.0).
    pub async fn available_capacity(&self) -> f64 {
        let state = self.state.read().await;
        state.available_capacity()
    }

    /// Register a model as loaded.
    pub async fn register_loaded_model(&self, model_id: String) {
        let mut state = self.state.write().await;
        if !state.loaded_models.contains(&model_id) {
            state.loaded_models.push(model_id);
        }
    }

    /// Unregister a model.
    pub async fn unregister_model(&self, model_id: &str) {
        let mut state = self.state.write().await;
        state.loaded_models.retain(|m| m != model_id);
    }

    /// Increment active task count.
    pub async fn task_started(&self, task_type: TaskType) {
        let mut state = self.state.write().await;
        match task_type {
            TaskType::Inference => state.active_inference_tasks += 1,
            TaskType::Wasm => state.active_wasm_tasks += 1,
            TaskType::Web => state.active_web_tasks += 1,
        }
    }

    /// Decrement active task count.
    pub async fn task_completed(&self, task_type: TaskType) {
        let mut state = self.state.write().await;
        match task_type {
            TaskType::Inference => {
                state.active_inference_tasks = state.active_inference_tasks.saturating_sub(1)
            }
            TaskType::Wasm => state.active_wasm_tasks = state.active_wasm_tasks.saturating_sub(1),
            TaskType::Web => state.active_web_tasks = state.active_web_tasks.saturating_sub(1),
        }
    }
}

/// Task type for tracking active tasks.
#[derive(Debug, Clone, Copy)]
pub enum TaskType {
    Inference,
    Wasm,
    Web,
}

/// Configuration for resource monitoring.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// How often to update resource metrics
    pub update_interval: Duration,
    /// Minimum RAM to keep available (MB)
    pub reserve_ram_mb: u32,
    /// Maximum concurrent inference tasks
    pub max_inference_tasks: u32,
    /// Maximum concurrent WASM tasks
    pub max_wasm_tasks: u32,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            update_interval: Duration::from_secs(1),
            reserve_ram_mb: 500,
            max_inference_tasks: 2,
            max_wasm_tasks: 10,
        }
    }
}

/// Current resource state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceState {
    /// CPU usage (0.0 - 1.0)
    pub cpu_usage: f64,
    /// Total RAM in MB
    pub ram_total_mb: u32,
    /// Available RAM in MB
    pub ram_available_mb: u32,
    /// GPU usage (0.0 - 1.0), None if no GPU
    pub gpu_usage: Option<f64>,
    /// Total VRAM in MB, None if no GPU
    pub vram_total_mb: Option<u32>,
    /// Available VRAM in MB, None if no GPU
    pub vram_available_mb: Option<u32>,
    /// Currently loaded model IDs
    pub loaded_models: Vec<String>,
    /// Active inference tasks
    pub active_inference_tasks: u32,
    /// Active WASM tasks
    pub active_wasm_tasks: u32,
    /// Active web tasks
    pub active_web_tasks: u32,
}

impl ResourceState {
    /// Check if we can handle the given resource requirements.
    pub fn can_handle(&self, requirements: &super::task::ResourceRequirements) -> bool {
        // Check RAM
        if self.ram_available_mb < requirements.ram_mb {
            return false;
        }

        // Check VRAM if required
        if let Some(required_vram) = requirements.vram_mb {
            match self.vram_available_mb {
                Some(available) if available >= required_vram => {}
                _ => return false,
            }
        }

        true
    }

    /// Calculate available capacity ratio.
    pub fn available_capacity(&self) -> f64 {
        let cpu_available = 1.0 - self.cpu_usage;
        let ram_ratio = self.ram_available_mb as f64 / self.ram_total_mb.max(1) as f64;

        // Weight CPU and RAM equally
        (cpu_available + ram_ratio) / 2.0
    }

    /// Check if a model is loaded.
    pub fn has_model(&self, model_id: &str) -> bool {
        self.loaded_models.iter().any(|m| m == model_id)
    }
}

/// Best-effort GPU utilization probe (no external crate).
fn probe_gpu_usage() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("ioreg")
            .args(["-r", "-d", "1", "-c", "IOAccelerator"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let line = line.trim();
            if line.contains("GPU Activity(%)") || line.contains("Device Utilization %") {
                if let Some(eq) = line.find('=') {
                    let val = line[eq + 1..].trim().trim_matches('"');
                    if let Ok(pct) = val.parse::<f64>() {
                        return Some((pct / 100.0).clamp(0.0, 1.0));
                    }
                }
            }
        }
        None
    }
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=utilization.gpu",
                "--format=csv,noheader,nounits",
            ])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let val = text.trim().lines().next()?.trim().parse::<f64>().ok()?;
        Some((val / 100.0).clamp(0.0, 1.0))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::task::ResourceRequirements;

    #[test]
    fn test_can_handle_sufficient_resources() {
        let state = ResourceState {
            ram_available_mb: 8000,
            vram_available_mb: Some(6000),
            ..Default::default()
        };

        let requirements = ResourceRequirements {
            ram_mb: 4000,
            vram_mb: Some(4000),
            cpu_cores: 2,
            estimated_duration: Duration::from_secs(10),
        };

        assert!(state.can_handle(&requirements));
    }

    #[test]
    fn test_can_handle_insufficient_ram() {
        let state = ResourceState {
            ram_available_mb: 2000,
            ..Default::default()
        };

        let requirements = ResourceRequirements {
            ram_mb: 4000,
            vram_mb: None,
            cpu_cores: 1,
            estimated_duration: Duration::from_secs(10),
        };

        assert!(!state.can_handle(&requirements));
    }

    #[test]
    fn test_can_handle_no_gpu_required() {
        let state = ResourceState {
            ram_available_mb: 8000,
            vram_available_mb: None, // No GPU
            ..Default::default()
        };

        let requirements = ResourceRequirements {
            ram_mb: 4000,
            vram_mb: None, // CPU-only task
            cpu_cores: 1,
            estimated_duration: Duration::from_secs(10),
        };

        assert!(state.can_handle(&requirements));
    }

    #[test]
    fn test_available_capacity() {
        let state = ResourceState {
            cpu_usage: 0.5,
            ram_total_mb: 16000,
            ram_available_mb: 8000, // 50% available
            ..Default::default()
        };

        let capacity = state.available_capacity();
        assert!((capacity - 0.5).abs() < 0.01);
    }
}
