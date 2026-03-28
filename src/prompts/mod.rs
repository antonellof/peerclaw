//! User-editable prompt templates loaded at startup.
//!
//! Defaults ship in the `prompts/` directory at the repo root (embedded in the binary).
//! Override any fragment by placing a same-named `*.txt` file. Resolution (first existing directory wins):
//! 1. `[prompts].directory` in `config.toml` (also set from `PEERCLAW_PROMPTS_DIR` when the config is loaded)
//! 2. `PEERCLAW_PROMPTS_DIR` if set and the directory exists
//! 3. `~/.peerclaw/prompts/` (under the data directory from bootstrap)
//!
//! File names match the stem keys (e.g. `agentic_system_intro.txt`). Restart the node after edits.

mod bundle;

pub use bundle::PromptBundle;

use std::path::{Path, PathBuf};

use crate::config::Config;

/// Resolve overlay directory for prompt `.txt` files.
pub fn resolve_prompt_overlay_dir(config: &Config) -> Option<PathBuf> {
    if let Some(ref p) = config.prompts.directory {
        let path = expand_home(p);
        if path.is_dir() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "prompts.directory is not a directory; ignoring"
        );
    }
    if let Ok(env) = std::env::var("PEERCLAW_PROMPTS_DIR") {
        let path = PathBuf::from(env.trim());
        if path.is_dir() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "PEERCLAW_PROMPTS_DIR is not a directory; ignoring"
        );
    }
    let default = crate::bootstrap::base_dir().join("prompts");
    if default.is_dir() {
        return Some(default);
    }
    None
}

/// Load prompts: embedded defaults, then optional per-file overrides from disk.
pub fn load_prompt_bundle(config: &Config) -> std::sync::Arc<PromptBundle> {
    let overlay = resolve_prompt_overlay_dir(config);
    if let Some(ref p) = overlay {
        tracing::info!(path = %p.display(), "Prompt overlay directory active (same-named .txt files override embedded defaults)");
    } else {
        tracing::info!("Using embedded prompt defaults only (set PEERCLAW_PROMPTS_DIR or [prompts].directory to override)");
    }
    std::sync::Arc::new(PromptBundle::load(overlay.as_deref()))
}

fn expand_home(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    }
    path.to_path_buf()
}

/// Replace `{key}` placeholders (simple, non-recursive).
pub fn subst(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut s = template.to_string();
    for (k, v) in pairs {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}
