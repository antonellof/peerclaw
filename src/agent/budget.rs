//! Budget tracking for agent execution.
//!
//! Enforces per-request, per-hour, per-day, and total budget limits.

use std::time::Instant;

/// Tracks spending and enforces budget limits for an agent.
#[derive(Debug, Clone)]
pub struct BudgetTracker {
    /// Maximum spend per single task/request (in PCLAW)
    pub per_request_limit: f64,
    /// Maximum spend per hour
    pub per_hour_limit: f64,
    /// Maximum spend per day
    pub per_day_limit: f64,
    /// Maximum total lifetime spend
    pub total_limit: f64,

    // Current spend tracking
    spent_this_request: f64,
    spent_this_hour: f64,
    spent_this_day: f64,
    spent_total: f64,

    // Time tracking
    hour_start: Instant,
    day_start: Instant,
}

impl BudgetTracker {
    /// Create a new budget tracker with the given limits.
    pub fn new(per_request: f64, per_hour: f64, per_day: f64, total: f64) -> Self {
        let now = Instant::now();
        Self {
            per_request_limit: per_request,
            per_hour_limit: per_hour,
            per_day_limit: per_day,
            total_limit: total,
            spent_this_request: 0.0,
            spent_this_hour: 0.0,
            spent_this_day: 0.0,
            spent_total: 0.0,
            hour_start: now,
            day_start: now,
        }
    }

    /// Reset counters if time periods have elapsed.
    fn reset_if_needed(&mut self) {
        let now = Instant::now();

        if now.duration_since(self.hour_start).as_secs() >= 3600 {
            self.spent_this_hour = 0.0;
            self.hour_start = now;
        }

        if now.duration_since(self.day_start).as_secs() >= 86400 {
            self.spent_this_day = 0.0;
            self.day_start = now;
        }
    }

    /// Check if we can spend the given amount without exceeding limits.
    pub fn can_spend(&mut self, amount: f64) -> bool {
        self.reset_if_needed();

        self.spent_this_request + amount <= self.per_request_limit
            && self.spent_this_hour + amount <= self.per_hour_limit
            && self.spent_this_day + amount <= self.per_day_limit
            && self.spent_total + amount <= self.total_limit
    }

    /// Record spending.
    pub fn spend(&mut self, amount: f64) {
        self.reset_if_needed();
        self.spent_this_request += amount;
        self.spent_this_hour += amount;
        self.spent_this_day += amount;
        self.spent_total += amount;
    }

    /// Reset per-request spending (called at the start of each new task).
    pub fn new_request(&mut self) {
        self.spent_this_request = 0.0;
    }

    /// Get remaining budget for this request.
    pub fn remaining_request(&self) -> f64 {
        self.per_request_limit - self.spent_this_request
    }

    /// Get total spent.
    pub fn total_spent(&self) -> f64 {
        self.spent_total
    }
}

impl Default for BudgetTracker {
    fn default() -> Self {
        Self::new(2.0, 20.0, 100.0, 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_tracking() {
        let mut budget = BudgetTracker::new(5.0, 50.0, 200.0, 1000.0);

        assert!(budget.can_spend(3.0));
        budget.spend(3.0);
        assert!(budget.can_spend(1.5));
        assert!(!budget.can_spend(3.0)); // Would exceed per-request limit

        budget.new_request();
        assert!(budget.can_spend(3.0)); // New request, per-request reset
    }

    #[test]
    fn test_total_limit() {
        let mut budget = BudgetTracker::new(100.0, 100.0, 100.0, 5.0);

        budget.spend(3.0);
        assert!(budget.can_spend(1.5));
        assert!(!budget.can_spend(3.0)); // Would exceed total limit
    }
}
