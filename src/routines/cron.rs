//! Cron scheduler for timed tasks.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Cron task definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronTask {
    /// Task ID
    pub id: String,
    /// Cron schedule expression
    pub schedule: String,
    /// Enabled flag
    pub enabled: bool,
}

impl CronTask {
    /// Create a new cron task
    pub fn new(id: &str, schedule: &str) -> Self {
        Self {
            id: id.to_string(),
            schedule: schedule.to_string(),
            enabled: true,
        }
    }

    /// Validate the cron schedule
    pub fn is_valid(&self) -> bool {
        self.schedule.parse::<::cron::Schedule>().is_ok()
    }

    /// Get next execution time
    pub fn next_run(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.schedule
            .parse::<::cron::Schedule>()
            .ok()
            .and_then(|s| s.upcoming(chrono::Utc).next())
    }
}

/// Cron scheduler
pub struct CronScheduler {
    tasks: Arc<RwLock<HashMap<String, CronTask>>>,
}

impl CronScheduler {
    /// Create a new cron scheduler
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a task
    pub fn add_task(&self, task: CronTask) {
        let mut tasks = self.tasks.write();
        tasks.insert(task.id.clone(), task);
    }

    /// Remove a task
    pub fn remove_task(&self, id: &str) -> Option<CronTask> {
        let mut tasks = self.tasks.write();
        tasks.remove(id)
    }

    /// Get a task
    pub fn get_task(&self, id: &str) -> Option<CronTask> {
        let tasks = self.tasks.read();
        tasks.get(id).cloned()
    }

    /// List all tasks
    pub fn list_tasks(&self) -> Vec<CronTask> {
        let tasks = self.tasks.read();
        tasks.values().cloned().collect()
    }

    /// Enable a task
    pub fn enable_task(&self, id: &str) -> bool {
        let mut tasks = self.tasks.write();
        if let Some(task) = tasks.get_mut(id) {
            task.enabled = true;
            true
        } else {
            false
        }
    }

    /// Disable a task
    pub fn disable_task(&self, id: &str) -> bool {
        let mut tasks = self.tasks.write();
        if let Some(task) = tasks.get_mut(id) {
            task.enabled = false;
            true
        } else {
            false
        }
    }

    /// Get tasks due to run
    pub fn get_due_tasks(&self) -> Vec<CronTask> {
        let tasks = self.tasks.read();
        let now = chrono::Utc::now();

        tasks
            .values()
            .filter(|task| {
                if !task.enabled {
                    return false;
                }

                task.schedule
                    .parse::<::cron::Schedule>()
                    .ok()
                    .and_then(|s| s.upcoming(chrono::Utc).next())
                    .map(|next| next <= now + chrono::Duration::seconds(1))
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    /// Get task count
    pub fn task_count(&self) -> usize {
        self.tasks.read().len()
    }
}

impl Default for CronScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Common cron patterns (7-field format: sec min hour day month dow year)
pub mod patterns {
    /// Every minute
    pub const EVERY_MINUTE: &str = "0 * * * * * *";
    /// Every 5 minutes
    pub const EVERY_5_MINUTES: &str = "0 */5 * * * * *";
    /// Every 15 minutes
    pub const EVERY_15_MINUTES: &str = "0 */15 * * * * *";
    /// Every 30 minutes
    pub const EVERY_30_MINUTES: &str = "0 */30 * * * * *";
    /// Every hour
    pub const HOURLY: &str = "0 0 * * * * *";
    /// Every day at midnight
    pub const DAILY: &str = "0 0 0 * * * *";
    /// Every day at 9am
    pub const DAILY_9AM: &str = "0 0 9 * * * *";
    /// Every Monday at midnight
    pub const WEEKLY: &str = "0 0 0 * * Mon *";
    /// First of month at midnight
    pub const MONTHLY: &str = "0 0 0 1 * * *";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_task() {
        let task = CronTask::new("test", patterns::HOURLY);
        assert!(task.is_valid());
        assert!(task.next_run().is_some());
    }

    #[test]
    fn test_invalid_schedule() {
        let task = CronTask::new("test", "invalid");
        assert!(!task.is_valid());
        assert!(task.next_run().is_none());
    }

    #[test]
    fn test_scheduler() {
        let scheduler = CronScheduler::new();

        scheduler.add_task(CronTask::new("task1", patterns::HOURLY));
        scheduler.add_task(CronTask::new("task2", patterns::DAILY));

        assert_eq!(scheduler.task_count(), 2);
        assert!(scheduler.get_task("task1").is_some());

        scheduler.disable_task("task1");
        assert!(!scheduler.get_task("task1").unwrap().enabled);

        scheduler.remove_task("task1");
        assert_eq!(scheduler.task_count(), 1);
    }
}
