//! Persistent session storage for agent conversations.
//!
//! Uses redb to store conversation turns keyed by `(session_id, turn_index)`,
//! with MessagePack serialization for compact storage. Sessions survive restarts.

use std::path::PathBuf;
use std::sync::Arc;

use redb::{Database as RedbDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

/// Table: key = `"session_id\0turn_index_zero_padded"`, value = msgpack bytes of [`SessionTurn`].
const SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("sessions");

/// Metadata table: key = session_id, value = msgpack bytes of [`SessionMeta`].
const SESSION_META: TableDefinition<&str, &[u8]> = TableDefinition::new("session_meta");

/// A single conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTurn {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

/// Metadata about a session (stored separately for fast listing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub turn_count: u64,
    /// First user message (for display).
    pub title: String,
}

/// Summary returned by `list_sessions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub turn_count: u64,
    pub title: String,
}

impl From<SessionMeta> for SessionInfo {
    fn from(m: SessionMeta) -> Self {
        Self {
            session_id: m.session_id,
            created_at: m.created_at,
            updated_at: m.updated_at,
            turn_count: m.turn_count,
            title: m.title,
        }
    }
}

/// Persistent session store backed by redb.
pub struct SessionStore {
    db: Arc<RedbDatabase>,
}

/// Build the composite key `"session_id\0NNNNNNNNNN"` for a turn.
fn turn_key(session_id: &str, index: u64) -> String {
    format!("{}\0{:010}", session_id, index)
}

/// Return the key prefix for all turns in a session (up to but not including the separator after it).
fn session_prefix(session_id: &str) -> String {
    format!("{}\0", session_id)
}

/// Inclusive upper bound for range scans: session_id + '\0' + '9' * 10 covers all 10-digit indices.
fn session_upper_bound(session_id: &str) -> String {
    format!("{}\0{}", session_id, "9999999999")
}

impl SessionStore {
    /// Open or create the session database at `db_path`.
    pub fn new(db_path: PathBuf) -> Result<Self, SessionStoreError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(SessionStoreError::Io)?;
        }
        let db = RedbDatabase::create(&db_path)?;

        // Initialize tables.
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(SESSIONS)?;
            let _ = write_txn.open_table(SESSION_META)?;
        }
        write_txn.commit()?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Append a turn to a session.
    pub fn save_turn(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> Result<(), SessionStoreError> {
        let now = chrono::Utc::now().timestamp();

        let write_txn = self.db.begin_write()?;
        {
            let mut meta_table = write_txn.open_table(SESSION_META)?;
            let mut sessions_table = write_txn.open_table(SESSIONS)?;

            // Load or create metadata.
            let mut meta = match meta_table.get(session_id)? {
                Some(bytes) => rmp_serde::from_slice::<SessionMeta>(bytes.value())
                    .map_err(|e| SessionStoreError::Serde(e.to_string()))?,
                None => SessionMeta {
                    session_id: session_id.to_string(),
                    created_at: now,
                    updated_at: now,
                    turn_count: 0,
                    title: String::new(),
                },
            };

            let turn = SessionTurn {
                role: role.to_string(),
                content: content.to_string(),
                timestamp: now,
            };

            let key = turn_key(session_id, meta.turn_count);
            let bytes =
                rmp_serde::to_vec(&turn).map_err(|e| SessionStoreError::Serde(e.to_string()))?;
            sessions_table.insert(key.as_str(), bytes.as_slice())?;

            meta.turn_count += 1;
            meta.updated_at = now;
            if meta.title.is_empty() && role == "user" {
                // Use first user message as title (truncated).
                meta.title = content.chars().take(120).collect();
            }

            let meta_bytes =
                rmp_serde::to_vec(&meta).map_err(|e| SessionStoreError::Serde(e.to_string()))?;
            meta_table.insert(session_id, meta_bytes.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load the most recent `max_turns` turns for a session.
    pub fn load_session(
        &self,
        session_id: &str,
        max_turns: usize,
    ) -> Result<Vec<SessionTurn>, SessionStoreError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SESSIONS)?;

        let lower = session_prefix(session_id);
        let upper = session_upper_bound(session_id);

        let mut turns = Vec::new();
        // Range scan for all turns in this session.
        for entry in table.range(lower.as_str()..=upper.as_str())? {
            let (_key, value) = entry?;
            let turn: SessionTurn = rmp_serde::from_slice(value.value())
                .map_err(|e| SessionStoreError::Serde(e.to_string()))?;
            turns.push(turn);
        }

        // Keep only the last `max_turns`.
        if turns.len() > max_turns {
            turns = turns.split_off(turns.len() - max_turns);
        }

        Ok(turns)
    }

    /// Delete all turns and metadata for a session.
    pub fn clear_session(&self, session_id: &str) -> Result<(), SessionStoreError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut sessions_table = write_txn.open_table(SESSIONS)?;
            let mut meta_table = write_txn.open_table(SESSION_META)?;

            // Collect keys to remove (can't mutate while iterating).
            let lower = session_prefix(session_id);
            let upper = session_upper_bound(session_id);
            let keys: Vec<String> = {
                let mut keys = Vec::new();
                for entry in sessions_table.range(lower.as_str()..=upper.as_str())? {
                    let (key, _) = entry?;
                    keys.push(key.value().to_string());
                }
                keys
            };

            for key in &keys {
                sessions_table.remove(key.as_str())?;
            }

            meta_table.remove(session_id)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// List all sessions with metadata.
    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>, SessionStoreError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SESSION_META)?;

        let mut sessions = Vec::new();
        for entry in table.iter()? {
            let (_key, value) = entry?;
            let meta: SessionMeta = rmp_serde::from_slice(value.value())
                .map_err(|e| SessionStoreError::Serde(e.to_string()))?;
            sessions.push(SessionInfo::from(meta));
        }

        // Sort by most recently updated.
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    /// Remove sessions whose last update is older than `max_age_secs` seconds ago.
    pub fn prune_old(&self, max_age_secs: i64) -> Result<u32, SessionStoreError> {
        let cutoff = chrono::Utc::now().timestamp() - max_age_secs;
        let sessions_to_prune: Vec<String> = {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(SESSION_META)?;
            let mut ids = Vec::new();
            for entry in table.iter()? {
                let (_key, value) = entry?;
                let meta: SessionMeta = rmp_serde::from_slice(value.value())
                    .map_err(|e| SessionStoreError::Serde(e.to_string()))?;
                if meta.updated_at <= cutoff {
                    ids.push(meta.session_id);
                }
            }
            ids
        };

        let count = sessions_to_prune.len() as u32;
        for sid in &sessions_to_prune {
            self.clear_session(sid)?;
        }
        Ok(count)
    }
}

impl Clone for SessionStore {
    fn clone(&self) -> Self {
        Self {
            db: Arc::clone(&self.db),
        }
    }
}

/// Errors from session store operations.
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    #[error("redb error: {0}")]
    Redb(#[from] redb::Error),

    #[error("redb database error: {0}")]
    Database(#[from] redb::DatabaseError),

    #[error("redb table error: {0}")]
    Table(#[from] redb::TableError),

    #[error("redb storage error: {0}")]
    Storage(#[from] redb::StorageError),

    #[error("redb transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),

    #[error("redb commit error: {0}")]
    Commit(#[from] redb::CommitError),

    #[error("serialization error: {0}")]
    Serde(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load_turns() {
        let dir = tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("sessions.redb")).unwrap();

        store.save_turn("s1", "user", "Hello").unwrap();
        store.save_turn("s1", "assistant", "Hi there!").unwrap();
        store.save_turn("s1", "user", "How are you?").unwrap();
        store
            .save_turn("s1", "assistant", "I'm doing well!")
            .unwrap();

        let turns = store.load_session("s1", 100).unwrap();
        assert_eq!(turns.len(), 4);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[0].content, "Hello");
        assert_eq!(turns[3].content, "I'm doing well!");
    }

    #[test]
    fn test_load_with_max_turns() {
        let dir = tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("sessions.redb")).unwrap();

        for i in 0..10 {
            store
                .save_turn("s1", "user", &format!("Message {}", i))
                .unwrap();
        }

        let turns = store.load_session("s1", 3).unwrap();
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].content, "Message 7");
        assert_eq!(turns[2].content, "Message 9");
    }

    #[test]
    fn test_clear_session() {
        let dir = tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("sessions.redb")).unwrap();

        store.save_turn("s1", "user", "Hello").unwrap();
        store.save_turn("s1", "assistant", "Hi").unwrap();

        store.clear_session("s1").unwrap();

        let turns = store.load_session("s1", 100).unwrap();
        assert!(turns.is_empty());

        let sessions = store.list_sessions().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_list_sessions() {
        let dir = tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("sessions.redb")).unwrap();

        store
            .save_turn("s1", "user", "First session message")
            .unwrap();
        store
            .save_turn("s2", "user", "Second session message")
            .unwrap();
        store.save_turn("s2", "assistant", "Reply").unwrap();

        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);

        // Both sessions should have titles from first user message.
        let s1 = sessions.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.title, "First session message");
        assert_eq!(s1.turn_count, 1);

        let s2 = sessions.iter().find(|s| s.session_id == "s2").unwrap();
        assert_eq!(s2.title, "Second session message");
        assert_eq!(s2.turn_count, 2);
    }

    #[test]
    fn test_multiple_sessions_isolated() {
        let dir = tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("sessions.redb")).unwrap();

        store.save_turn("s1", "user", "Hello from s1").unwrap();
        store.save_turn("s2", "user", "Hello from s2").unwrap();

        let s1_turns = store.load_session("s1", 100).unwrap();
        let s2_turns = store.load_session("s2", 100).unwrap();

        assert_eq!(s1_turns.len(), 1);
        assert_eq!(s1_turns[0].content, "Hello from s1");
        assert_eq!(s2_turns.len(), 1);
        assert_eq!(s2_turns[0].content, "Hello from s2");
    }

    #[test]
    fn test_persistence_across_reopen() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("sessions.redb");

        {
            let store = SessionStore::new(db_path.clone()).unwrap();
            store.save_turn("s1", "user", "Persistent msg").unwrap();
        }

        // Reopen the database.
        let store = SessionStore::new(db_path).unwrap();
        let turns = store.load_session("s1", 100).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].content, "Persistent msg");
    }

    #[test]
    fn test_prune_old() {
        let dir = tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("sessions.redb")).unwrap();

        store.save_turn("s1", "user", "Recent").unwrap();

        // Prune sessions older than 0 seconds (nothing should be old enough yet).
        // Actually, since save_turn uses `now`, pruning with max_age=999999 should keep everything.
        let pruned = store.prune_old(999_999).unwrap();
        assert_eq!(pruned, 0);

        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);

        // Prune with max_age=0 removes everything.
        let pruned = store.prune_old(0).unwrap();
        assert_eq!(pruned, 1);

        let sessions = store.list_sessions().unwrap();
        assert!(sessions.is_empty());
    }
}
