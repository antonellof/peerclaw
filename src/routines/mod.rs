//! Routine engine for background automation.
//!
//! Provides:
//! - Cron-based scheduled tasks
//! - Event-triggered routines
//! - Heartbeat system for proactive execution
//! - Webhook handlers

pub mod cron;
pub mod heartbeat;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub use cron::{CronScheduler, CronTask};
pub use heartbeat::{Heartbeat, HeartbeatConfig};

/// Routine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineConfig {
    /// Enable routine engine
    pub enabled: bool,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_secs: u64,
    /// Maximum concurrent routines
    pub max_concurrent: usize,
    /// Default timeout for routines
    pub default_timeout_secs: u64,
}

impl Default for RoutineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            heartbeat_interval_secs: 300, // 5 minutes
            max_concurrent: 10,
            default_timeout_secs: 60,
        }
    }
}

/// Routine trigger types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoutineTrigger {
    /// Cron schedule (e.g., "0 * * * *" for hourly)
    Cron { schedule: String },
    /// Interval in seconds
    Interval { seconds: u64 },
    /// Event-based trigger
    Event { event_type: String },
    /// Webhook trigger
    Webhook { path: String },
    /// On startup
    Startup,
    /// Manual trigger only
    Manual,
}

/// Routine status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutineStatus {
    Idle,
    Running,
    Completed,
    Failed,
    Disabled,
}

/// Routine definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routine {
    /// Unique identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Trigger configuration
    pub trigger: RoutineTrigger,
    /// Action to execute (prompt or command)
    pub action: RoutineAction,
    /// Timeout in seconds
    pub timeout_secs: Option<u64>,
    /// Enabled flag
    pub enabled: bool,
}

/// Routine action
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoutineAction {
    /// Execute a prompt
    Prompt { prompt: String },
    /// Execute a tool
    Tool { name: String, params: serde_json::Value },
    /// Execute a command
    Command { command: String },
    /// Read and process a file
    ReadFile { path: String },
    /// Custom action
    Custom { handler: String, data: serde_json::Value },
}

/// Routine execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineResult {
    /// Routine ID
    pub routine_id: String,
    /// Execution status
    pub status: RoutineStatus,
    /// Result output
    pub output: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Event for triggering routines
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineEvent {
    /// Event type
    pub event_type: String,
    /// Event source
    pub source: String,
    /// Event data
    pub data: serde_json::Value,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Routine engine for managing background tasks
pub struct RoutineEngine {
    config: RoutineConfig,
    routines: Arc<RwLock<HashMap<String, Routine>>>,
    status: Arc<RwLock<HashMap<String, RoutineStatus>>>,
    last_run: Arc<RwLock<HashMap<String, chrono::DateTime<chrono::Utc>>>>,
    cron_scheduler: CronScheduler,
    heartbeat: Heartbeat,
    event_tx: mpsc::Sender<RoutineEvent>,
    event_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<RoutineEvent>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl RoutineEngine {
    /// Create a new routine engine
    pub fn new(config: RoutineConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(100);

        Self {
            cron_scheduler: CronScheduler::new(),
            heartbeat: Heartbeat::new(HeartbeatConfig {
                interval_secs: config.heartbeat_interval_secs,
                ..Default::default()
            }),
            config,
            routines: Arc::new(RwLock::new(HashMap::new())),
            status: Arc::new(RwLock::new(HashMap::new())),
            last_run: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            event_rx: Arc::new(tokio::sync::Mutex::new(event_rx)),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Register a routine
    pub fn register(&self, routine: Routine) {
        let id = routine.id.clone();
        let mut routines = self.routines.write();
        routines.insert(id.clone(), routine.clone());

        let mut status = self.status.write();
        status.insert(id, RoutineStatus::Idle);

        tracing::info!(routine = %routine.name, "Registered routine");
    }

    /// Unregister a routine
    pub fn unregister(&self, id: &str) -> Option<Routine> {
        let mut routines = self.routines.write();
        let routine = routines.remove(id);

        if routine.is_some() {
            let mut status = self.status.write();
            status.remove(id);
            tracing::info!(routine = %id, "Unregistered routine");
        }

        routine
    }

    /// Get a routine by ID
    pub fn get(&self, id: &str) -> Option<Routine> {
        let routines = self.routines.read();
        routines.get(id).cloned()
    }

    /// List all routines
    pub fn list(&self) -> Vec<Routine> {
        let routines = self.routines.read();
        routines.values().cloned().collect()
    }

    /// Get routine status
    pub fn get_status(&self, id: &str) -> Option<RoutineStatus> {
        let status = self.status.read();
        status.get(id).copied()
    }

    /// Send an event to trigger matching routines
    pub async fn send_event(&self, event: RoutineEvent) {
        let _ = self.event_tx.send(event).await;
    }

    /// Get event sender for external use
    pub fn event_sender(&self) -> mpsc::Sender<RoutineEvent> {
        self.event_tx.clone()
    }

    /// Start the routine engine
    pub async fn start<F>(&self, executor: F)
    where
        F: Fn(Routine) -> futures::future::BoxFuture<'static, RoutineResult> + Send + Sync + Clone + 'static,
    {
        if !self.config.enabled {
            tracing::info!("Routine engine disabled");
            return;
        }

        self.running.store(true, std::sync::atomic::Ordering::SeqCst);
        tracing::info!("Starting routine engine");

        // Run startup routines
        self.run_startup_routines(executor.clone()).await;

        // Start interval routines
        self.start_interval_routines(executor.clone());

        // Start event listener
        self.start_event_listener(executor.clone());

        // Start cron scheduler
        self.start_cron_routines(executor.clone());

        // Start heartbeat
        self.heartbeat.start().await;
    }

    /// Stop the routine engine
    pub async fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
        self.heartbeat.stop().await;
        tracing::info!("Stopped routine engine");
    }

    /// Run startup routines
    async fn run_startup_routines<F>(&self, executor: F)
    where
        F: Fn(Routine) -> futures::future::BoxFuture<'static, RoutineResult> + Send + Sync + Clone + 'static,
    {
        let routines: Vec<_> = {
            let routines = self.routines.read();
            routines
                .values()
                .filter(|r| {
                    r.enabled && matches!(r.trigger, RoutineTrigger::Startup)
                })
                .cloned()
                .collect()
        };

        for routine in routines {
            let id = routine.id.clone();
            self.set_status(&id, RoutineStatus::Running);

            let result = executor(routine).await;

            self.set_status(&id, result.status);
            self.update_last_run(&id);

            tracing::debug!(routine = %id, status = ?result.status, "Startup routine completed");
        }
    }

    /// Start interval-based routines
    fn start_interval_routines<F>(&self, executor: F)
    where
        F: Fn(Routine) -> futures::future::BoxFuture<'static, RoutineResult> + Send + Sync + Clone + 'static,
    {
        let routines: Vec<_> = {
            let routines = self.routines.read();
            routines
                .values()
                .filter(|r| {
                    r.enabled && matches!(r.trigger, RoutineTrigger::Interval { .. })
                })
                .cloned()
                .collect()
        };

        for routine in routines {
            if let RoutineTrigger::Interval { seconds } = routine.trigger {
                let running = self.running.clone();
                let status_map = self.status.clone();
                let last_run_map = self.last_run.clone();
                let executor = executor.clone();
                let routine = routine.clone();

                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(seconds));

                    while running.load(std::sync::atomic::Ordering::SeqCst) {
                        interval.tick().await;

                        if !running.load(std::sync::atomic::Ordering::SeqCst) {
                            break;
                        }

                        let id = routine.id.clone();

                        // Update status
                        {
                            let mut status = status_map.write();
                            status.insert(id.clone(), RoutineStatus::Running);
                        }

                        let result = executor(routine.clone()).await;

                        // Update status and last run
                        {
                            let mut status = status_map.write();
                            status.insert(id.clone(), result.status);
                        }
                        {
                            let mut last_run = last_run_map.write();
                            last_run.insert(id.clone(), chrono::Utc::now());
                        }

                        tracing::debug!(routine = %id, status = ?result.status, "Interval routine completed");
                    }
                });
            }
        }
    }

    /// Start event listener
    fn start_event_listener<F>(&self, executor: F)
    where
        F: Fn(Routine) -> futures::future::BoxFuture<'static, RoutineResult> + Send + Sync + Clone + 'static,
    {
        let running = self.running.clone();
        let routines = self.routines.clone();
        let status_map = self.status.clone();
        let last_run_map = self.last_run.clone();
        let event_rx = self.event_rx.clone();

        tokio::spawn(async move {
            let mut rx = event_rx.lock().await;

            while running.load(std::sync::atomic::Ordering::SeqCst) {
                match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
                    Ok(Some(event)) => {
                        // Find matching routines
                        let matching: Vec<_> = {
                            let routines = routines.read();
                            routines
                                .values()
                                .filter(|r| {
                                    r.enabled
                                        && matches!(&r.trigger, RoutineTrigger::Event { event_type } if event_type == &event.event_type)
                                })
                                .cloned()
                                .collect()
                        };

                        for routine in matching {
                            let id = routine.id.clone();
                            let executor = executor.clone();
                            let status_map = status_map.clone();
                            let last_run_map = last_run_map.clone();

                            tokio::spawn(async move {
                                {
                                    let mut status = status_map.write();
                                    status.insert(id.clone(), RoutineStatus::Running);
                                }

                                let result = executor(routine).await;

                                {
                                    let mut status = status_map.write();
                                    status.insert(id.clone(), result.status);
                                }
                                {
                                    let mut last_run = last_run_map.write();
                                    last_run.insert(id.clone(), chrono::Utc::now());
                                }

                                tracing::debug!(routine = %id, status = ?result.status, "Event routine completed");
                            });
                        }
                    }
                    Ok(None) => break,
                    Err(_) => continue, // Timeout, check running flag
                }
            }
        });
    }

    /// Start cron-based routines
    fn start_cron_routines<F>(&self, executor: F)
    where
        F: Fn(Routine) -> futures::future::BoxFuture<'static, RoutineResult> + Send + Sync + Clone + 'static,
    {
        let routines: Vec<_> = {
            let routines = self.routines.read();
            routines
                .values()
                .filter(|r| {
                    r.enabled && matches!(r.trigger, RoutineTrigger::Cron { .. })
                })
                .cloned()
                .collect()
        };

        for routine in routines {
            if let RoutineTrigger::Cron { ref schedule } = routine.trigger {
                let task = CronTask {
                    id: routine.id.clone(),
                    schedule: schedule.clone(),
                    enabled: true,
                };

                self.cron_scheduler.add_task(task);

                let running = self.running.clone();
                let status_map = self.status.clone();
                let last_run_map = self.last_run.clone();
                let executor = executor.clone();
                let routine = routine.clone();
                let schedule = schedule.clone();

                tokio::spawn(async move {
                    // Parse cron schedule
                    let Ok(cron_schedule) = schedule.parse::<::cron::Schedule>() else {
                        tracing::error!(routine = %routine.id, schedule = %schedule, "Invalid cron schedule");
                        return;
                    };

                    while running.load(std::sync::atomic::Ordering::SeqCst) {
                        // Find next execution time
                        let now = chrono::Utc::now();
                        let Some(next) = cron_schedule.upcoming(chrono::Utc).next() else {
                            break;
                        };

                        let wait_duration = (next - now).to_std().unwrap_or(Duration::from_secs(60));

                        tokio::time::sleep(wait_duration).await;

                        if !running.load(std::sync::atomic::Ordering::SeqCst) {
                            break;
                        }

                        let id = routine.id.clone();

                        {
                            let mut status = status_map.write();
                            status.insert(id.clone(), RoutineStatus::Running);
                        }

                        let result = executor(routine.clone()).await;

                        {
                            let mut status = status_map.write();
                            status.insert(id.clone(), result.status);
                        }
                        {
                            let mut last_run = last_run_map.write();
                            last_run.insert(id.clone(), chrono::Utc::now());
                        }

                        tracing::debug!(routine = %id, status = ?result.status, "Cron routine completed");
                    }
                });
            }
        }
    }

    fn set_status(&self, id: &str, new_status: RoutineStatus) {
        let mut status = self.status.write();
        status.insert(id.to_string(), new_status);
    }

    fn update_last_run(&self, id: &str) {
        let mut last_run = self.last_run.write();
        last_run.insert(id.to_string(), chrono::Utc::now());
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routine_config_default() {
        let config = RoutineConfig::default();
        assert!(config.enabled);
        assert_eq!(config.heartbeat_interval_secs, 300);
    }

    #[test]
    fn test_routine_registration() {
        let engine = RoutineEngine::new(RoutineConfig::default());

        let routine = Routine {
            id: "test".to_string(),
            name: "Test Routine".to_string(),
            description: "A test routine".to_string(),
            trigger: RoutineTrigger::Manual,
            action: RoutineAction::Prompt {
                prompt: "Hello".to_string(),
            },
            timeout_secs: None,
            enabled: true,
        };

        engine.register(routine.clone());

        assert!(engine.get("test").is_some());
        assert_eq!(engine.get_status("test"), Some(RoutineStatus::Idle));

        engine.unregister("test");
        assert!(engine.get("test").is_none());
    }
}
