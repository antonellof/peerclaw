//! In-memory crew run tracking (dashboard / API).

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::orchestrator::CrewOutput;
use super::spec::CrewSpec;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewRunRecord {
    pub id: String,
    pub status: String,
    pub crew_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<CrewOutput>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs: Option<Vec<String>>,
}

pub struct CrewRunStore {
    runs: RwLock<HashMap<String, CrewRunRecord>>,
    pub cancels: RwLock<HashMap<String, Arc<AtomicBool>>>,
}

impl CrewRunStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            runs: RwLock::new(HashMap::new()),
            cancels: RwLock::new(HashMap::new()),
        })
    }

    pub fn insert_pending(&self, id: impl Into<String>, spec: &CrewSpec) -> String {
        let id = id.into();
        let rec = CrewRunRecord {
            id: id.clone(),
            status: "pending".to_string(),
            crew_name: spec.name.clone(),
            error: None,
            output: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            logs: Some(vec!["[crew] queued".to_string()]),
        };
        self.runs.write().insert(id.clone(), rec);
        self.cancels.write().insert(id.clone(), Arc::new(AtomicBool::new(false)));
        id
    }

    pub fn update_status(&self, id: &str, status: &str) {
        if let Some(r) = self.runs.write().get_mut(id) {
            r.status = status.to_string();
        }
    }

    pub fn push_log(&self, id: &str, line: impl Into<String>) {
        if let Some(r) = self.runs.write().get_mut(id) {
            r.logs.get_or_insert_with(Vec::new).push(line.into());
        }
    }

    pub fn complete_ok(&self, id: &str, out: CrewOutput) {
        if let Some(r) = self.runs.write().get_mut(id) {
            r.status = "completed".to_string();
            r.output = Some(out);
            r.completed_at = Some(chrono::Utc::now().to_rfc3339());
            r.error = None;
        }
    }

    pub fn complete_err(&self, id: &str, err: impl Into<String>) {
        if let Some(r) = self.runs.write().get_mut(id) {
            r.status = "failed".to_string();
            r.error = Some(err.into());
            r.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }
    }

    pub fn get(&self, id: &str) -> Option<CrewRunRecord> {
        self.runs.read().get(id).cloned()
    }

    pub fn list(&self) -> Vec<CrewRunRecord> {
        let mut v: Vec<_> = self.runs.read().values().cloned().collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    pub fn cancel_flag(&self, id: &str) -> Option<Arc<AtomicBool>> {
        self.cancels.read().get(id).cloned()
    }
}
