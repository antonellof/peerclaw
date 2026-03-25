//! Workspace filesystem for persistent memory and identity.
//!
//! Provides:
//! - Structured file-based memory (MEMORY.md, IDENTITY.md, etc.)
//! - Document storage and retrieval
//! - Identity files for consistent personality
//! - Daily logs and project organization

pub mod files;
pub mod identity;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use identity::{Identity, IdentityConfig};

/// Workspace configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Root directory for workspace
    pub root: PathBuf,
    /// Enable auto-creation of default files
    pub auto_create: bool,
    /// Enable daily log files
    pub daily_logs: bool,
    /// Maximum file size (bytes)
    pub max_file_size: usize,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        let root = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("peerclaw")
            .join("workspace");

        Self {
            root,
            auto_create: true,
            daily_logs: true,
            max_file_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

/// Workspace error
#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("File not found: {0}")]
    NotFound(String),

    #[error("File too large: {size} bytes (max {max})")]
    FileTooLarge { size: usize, max: usize },

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Parse error: {0}")]
    ParseError(String),
}

pub type Result<T> = std::result::Result<T, WorkspaceError>;

/// Standard workspace files
pub mod standard_files {
    pub const README: &str = "README.md";
    pub const MEMORY: &str = "MEMORY.md";
    pub const IDENTITY: &str = "IDENTITY.md";
    pub const SOUL: &str = "SOUL.md";
    pub const AGENTS: &str = "AGENTS.md";
    pub const USER: &str = "USER.md";
    pub const TOOLS: &str = "TOOLS.md";
    pub const HEARTBEAT: &str = "HEARTBEAT.md";
    pub const BOOTSTRAP: &str = "BOOTSTRAP.md";
}

/// Workspace manager
pub struct Workspace {
    config: WorkspaceConfig,
    identity: Option<Identity>,
}

impl Workspace {
    /// Create a new workspace
    pub fn new(config: WorkspaceConfig) -> Result<Self> {
        // Create root directory if it doesn't exist
        if !config.root.exists() {
            std::fs::create_dir_all(&config.root)?;
        }

        let mut workspace = Self {
            config,
            identity: None,
        };

        // Auto-create default files
        if workspace.config.auto_create {
            workspace.ensure_defaults()?;
        }

        // Load identity
        workspace.load_identity()?;

        Ok(workspace)
    }

    /// Ensure default files exist
    fn ensure_defaults(&self) -> Result<()> {
        // Create directories
        for dir in &["context", "daily", "projects", "channels"] {
            let path = self.config.root.join(dir);
            if !path.exists() {
                std::fs::create_dir_all(&path)?;
            }
        }

        // Create README if missing
        let readme_path = self.config.root.join(standard_files::README);
        if !readme_path.exists() {
            std::fs::write(&readme_path, DEFAULT_README)?;
        }

        // Create IDENTITY if missing
        let identity_path = self.config.root.join(standard_files::IDENTITY);
        if !identity_path.exists() {
            std::fs::write(&identity_path, DEFAULT_IDENTITY)?;
        }

        // Create MEMORY if missing
        let memory_path = self.config.root.join(standard_files::MEMORY);
        if !memory_path.exists() {
            std::fs::write(&memory_path, DEFAULT_MEMORY)?;
        }

        // Create HEARTBEAT if missing
        let heartbeat_path = self.config.root.join(standard_files::HEARTBEAT);
        if !heartbeat_path.exists() {
            std::fs::write(&heartbeat_path, DEFAULT_HEARTBEAT)?;
        }

        Ok(())
    }

    /// Load identity from IDENTITY.md
    fn load_identity(&mut self) -> Result<()> {
        let path = self.config.root.join(standard_files::IDENTITY);
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            self.identity = Some(Identity::parse(&content)?);
        }
        Ok(())
    }

    /// Get identity
    pub fn identity(&self) -> Option<&Identity> {
        self.identity.as_ref()
    }

    /// Read a file from workspace
    pub fn read(&self, path: &str) -> Result<String> {
        let full_path = self.resolve_path(path)?;

        if !full_path.exists() {
            return Err(WorkspaceError::NotFound(path.to_string()));
        }

        let metadata = std::fs::metadata(&full_path)?;
        if metadata.len() as usize > self.config.max_file_size {
            return Err(WorkspaceError::FileTooLarge {
                size: metadata.len() as usize,
                max: self.config.max_file_size,
            });
        }

        Ok(std::fs::read_to_string(&full_path)?)
    }

    /// Write a file to workspace
    pub fn write(&self, path: &str, content: &str) -> Result<()> {
        if content.len() > self.config.max_file_size {
            return Err(WorkspaceError::FileTooLarge {
                size: content.len(),
                max: self.config.max_file_size,
            });
        }

        let full_path = self.resolve_path(path)?;

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&full_path, content)?;
        Ok(())
    }

    /// Append to a file
    pub fn append(&self, path: &str, content: &str) -> Result<()> {
        let full_path = self.resolve_path(path)?;

        // Check size
        let current_size = if full_path.exists() {
            std::fs::metadata(&full_path)?.len() as usize
        } else {
            0
        };

        if current_size + content.len() > self.config.max_file_size {
            return Err(WorkspaceError::FileTooLarge {
                size: current_size + content.len(),
                max: self.config.max_file_size,
            });
        }

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&full_path)?;

        writeln!(file, "{}", content)?;
        Ok(())
    }

    /// Delete a file
    pub fn delete(&self, path: &str) -> Result<bool> {
        let full_path = self.resolve_path(path)?;

        if full_path.exists() {
            std::fs::remove_file(&full_path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if file exists
    pub fn exists(&self, path: &str) -> bool {
        self.resolve_path(path).map(|p| p.exists()).unwrap_or(false)
    }

    /// List files in a directory
    pub fn list(&self, path: &str) -> Result<Vec<String>> {
        let full_path = self.resolve_path(path)?;

        if !full_path.exists() {
            return Err(WorkspaceError::NotFound(path.to_string()));
        }

        let mut files = Vec::new();
        for entry in std::fs::read_dir(&full_path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            files.push(name);
        }

        files.sort();
        Ok(files)
    }

    /// Get file tree
    pub fn tree(&self, path: &str, depth: usize) -> Result<FileTree> {
        let full_path = self.resolve_path(path)?;
        self.build_tree(&full_path, depth)
    }

    /// Build file tree recursively
    fn build_tree(&self, path: &Path, depth: usize) -> Result<FileTree> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());

        if path.is_file() {
            let metadata = std::fs::metadata(path)?;
            return Ok(FileTree {
                name,
                is_dir: false,
                size: Some(metadata.len()),
                children: Vec::new(),
            });
        }

        let mut children = Vec::new();
        if depth > 0 {
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let child = self.build_tree(&entry.path(), depth - 1)?;
                children.push(child);
            }
            children.sort_by(|a, b| {
                // Directories first, then alphabetical
                match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.cmp(&b.name),
                }
            });
        }

        Ok(FileTree {
            name,
            is_dir: true,
            size: None,
            children,
        })
    }

    /// Resolve a relative path to absolute
    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        // Security: prevent path traversal
        if path.contains("..") {
            return Err(WorkspaceError::InvalidPath(
                "Path traversal not allowed".to_string(),
            ));
        }

        let path = path.trim_start_matches('/');
        Ok(self.config.root.join(path))
    }

    /// Get daily log path for today
    pub fn daily_log_path(&self) -> String {
        let today = chrono::Local::now().format("%Y-%m-%d");
        format!("daily/{}.md", today)
    }

    /// Write to today's daily log
    pub fn log_daily(&self, entry: &str) -> Result<()> {
        if !self.config.daily_logs {
            return Ok(());
        }

        let path = self.daily_log_path();
        let timestamp = chrono::Local::now().format("%H:%M:%S");
        let formatted = format!("- [{}] {}", timestamp, entry);

        self.append(&path, &formatted)
    }

    /// Get memory content
    pub fn memory(&self) -> Result<String> {
        self.read(standard_files::MEMORY)
    }

    /// Get heartbeat content
    pub fn heartbeat(&self) -> Result<String> {
        self.read(standard_files::HEARTBEAT)
    }

    /// Get workspace root
    pub fn root(&self) -> &Path {
        &self.config.root
    }
}

/// File tree structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTree {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub children: Vec<FileTree>,
}

impl FileTree {
    /// Format as string tree
    pub fn to_string_tree(&self, prefix: &str) -> String {
        let mut result = String::new();
        self.format_tree(&mut result, prefix, true);
        result
    }

    fn format_tree(&self, output: &mut String, prefix: &str, is_last: bool) {
        let connector = if is_last { "└── " } else { "├── " };
        let icon = if self.is_dir { "📁 " } else { "📄 " };

        output.push_str(&format!("{}{}{}{}\n", prefix, connector, icon, self.name));

        let child_prefix = format!("{}{}   ", prefix, if is_last { " " } else { "│" });

        for (i, child) in self.children.iter().enumerate() {
            let is_last_child = i == self.children.len() - 1;
            child.format_tree(output, &child_prefix, is_last_child);
        }
    }
}

// Default file contents
const DEFAULT_README: &str = r#"# Workspace

This is your PeerClaw workspace for persistent memory and context.

## Structure

- `MEMORY.md` - Long-term curated memories (injected in system prompt)
- `IDENTITY.md` - Agent name, personality, and vibe
- `SOUL.md` - Core values and principles
- `HEARTBEAT.md` - Periodic checklist for proactive execution
- `daily/` - Daily log files (YYYY-MM-DD.md)
- `projects/` - Project-specific context
- `context/` - Identity-related documents

## Usage

Use memory tools to read, write, and search workspace files.
"#;

const DEFAULT_IDENTITY: &str = r#"# Identity

name: PeerClaw
nature: Helpful AI assistant
vibe: Professional yet friendly

## Traits

- Helpful and knowledgeable
- Concise but thorough
- Security-conscious
- Respects user privacy

## Communication Style

- Clear and direct
- Uses examples when helpful
- Admits uncertainty honestly
- Asks clarifying questions when needed
"#;

const DEFAULT_MEMORY: &str = r#"# Memory

## User Preferences

(Add user preferences here)

## Important Facts

(Add important facts here)

## Lessons Learned

(Add lessons learned here)
"#;

const DEFAULT_HEARTBEAT: &str = r#"# Heartbeat Checklist

## Regular Checks

- [ ] Check for pending tasks
- [ ] Review recent conversations
- [ ] Update memory with new learnings

## Periodic Tasks

- [ ] Summarize daily activity (daily)
- [ ] Clean up old logs (weekly)
- [ ] Review and update preferences (monthly)

## Notes

If all checks pass with no action needed, respond with HEARTBEAT_OK.
Otherwise, report findings and actions taken.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_creation() {
        let temp = tempfile::tempdir().unwrap();
        let config = WorkspaceConfig {
            root: temp.path().to_path_buf(),
            auto_create: true,
            daily_logs: true,
            max_file_size: 1024 * 1024,
        };

        let workspace = Workspace::new(config).unwrap();

        // Check default files created
        assert!(workspace.exists(standard_files::README));
        assert!(workspace.exists(standard_files::IDENTITY));
        assert!(workspace.exists(standard_files::MEMORY));
    }

    #[test]
    fn test_read_write() {
        let temp = tempfile::tempdir().unwrap();
        let config = WorkspaceConfig {
            root: temp.path().to_path_buf(),
            ..Default::default()
        };

        let workspace = Workspace::new(config).unwrap();

        // Write
        workspace.write("test.txt", "Hello, world!").unwrap();

        // Read
        let content = workspace.read("test.txt").unwrap();
        assert_eq!(content, "Hello, world!");

        // Delete
        assert!(workspace.delete("test.txt").unwrap());
        assert!(!workspace.exists("test.txt"));
    }

    #[test]
    fn test_path_traversal_blocked() {
        let temp = tempfile::tempdir().unwrap();
        let config = WorkspaceConfig {
            root: temp.path().to_path_buf(),
            ..Default::default()
        };

        let workspace = Workspace::new(config).unwrap();

        // Should fail
        let result = workspace.read("../../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_daily_log() {
        let temp = tempfile::tempdir().unwrap();
        let config = WorkspaceConfig {
            root: temp.path().to_path_buf(),
            daily_logs: true,
            ..Default::default()
        };

        let workspace = Workspace::new(config).unwrap();

        workspace.log_daily("Test entry").unwrap();

        let path = workspace.daily_log_path();
        assert!(workspace.exists(&path));
    }
}
