//! Hugging Face GGUF presets and download helpers (shared by CLI and web API).
//!
//! Model presets and aliases are loaded from JSON files at compile time:
//! - `templates/models/gguf-presets.json` — downloadable GGUF models
//! - `templates/models/aliases.json` — short name → preset ID mapping

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::Deserialize;

/// A GGUF model preset from `templates/models/gguf-presets.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct GgufPreset {
    pub id: String,
    pub repo: String,
    pub stem: String,
    #[serde(default = "default_sep")]
    pub sep: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub recommended: bool,
}

fn default_sep() -> String {
    "-".into()
}

/// All GGUF presets, loaded from the embedded JSON at compile time.
static GGUF_PRESETS: LazyLock<Vec<GgufPreset>> = LazyLock::new(|| {
    const RAW: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/models/gguf-presets.json"
    ));
    serde_json::from_str(RAW).unwrap_or_default()
});

/// Model name aliases, loaded from the embedded JSON at compile time.
static MODEL_ALIASES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    const RAW: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates/models/aliases.json"
    ));
    serde_json::from_str(RAW).unwrap_or_default()
});

/// Get the list of all GGUF presets.
pub fn gguf_presets() -> &'static [GgufPreset] {
    &GGUF_PRESETS
}

/// Resolve a model name alias (e.g., "llama" → "llama-3.2-3b").
/// Returns the input unchanged if no alias matches.
pub fn resolve_model_alias(name: &str) -> String {
    let lower = name.to_lowercase();
    MODEL_ALIASES
        .get(&lower)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

/// Resolve a named preset + quantization to a Hugging Face `resolve` URL and output filename.
pub fn preset_to_hf_url(preset: &str, quant: &str) -> Option<(String, String)> {
    let p = GGUF_PRESETS.iter().find(|p| p.id == preset)?;

    // Per-repo quant tag conventions
    let quant_tag = if preset.starts_with("phi-3") {
        match quant.to_lowercase().as_str() {
            "q4_k_m" | "q4_k" | "q4" => "q4".to_string(),
            "q8_0" | "q8" => "q8_0".to_string(),
            "fp16" | "f16" => "fp16".to_string(),
            other => other.to_string(),
        }
    } else if preset.starts_with("qwen") {
        quant.to_lowercase().replace('-', "_")
    } else if p.sep == "." {
        // TheBloke style: lowercase with dots
        quant.to_uppercase().replace('-', "_")
    } else {
        quant.to_uppercase().replace('-', "_")
    };

    let file = format!("{}{}{}.gguf", p.stem, p.sep, quant_tag);
    let url = format!("https://huggingface.co/{}/resolve/main/{file}", p.repo);
    let out_name = format!("{}-{}.gguf", preset, quant);
    Some((url, out_name))
}

/// Download a URL to a path with streaming progress.
/// `on_progress(downloaded_bytes, total_bytes_opt)` is called periodically (~every 500KB).
pub async fn download_url_to_path<F: Fn(u64, Option<u64>)>(
    url: &str,
    dest: &Path,
    on_progress: Option<F>,
) -> Result<u64, String> {
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(7200))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let total = response.content_length();

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("create file: {e}"))?;

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_report: u64 = 0;

    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download stream: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("write: {e}"))?;
        downloaded += chunk.len() as u64;

        // Report progress every ~500KB
        if downloaded - last_report > 512_000 || downloaded == total.unwrap_or(u64::MAX) {
            if let Some(ref cb) = on_progress {
                cb(downloaded, total);
            }
            last_report = downloaded;
        }
    }

    file.flush().await.map_err(|e| format!("flush: {e}"))?;
    Ok(downloaded)
}

/// Filename from a HF URL path, or `model.gguf` fallback.
pub fn filename_from_hf_url(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    path.rsplit('/')
        .next()
        .filter(|s| s.ends_with(".gguf"))
        .map(String::from)
        .unwrap_or_else(|| "model.gguf".to_string())
}

/// Destination path under `models_dir` for a custom URL download.
pub fn dest_for_custom_url(
    models_dir: &Path,
    url: &str,
    filename_override: Option<&str>,
) -> PathBuf {
    let name = filename_override
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| filename_from_hf_url(url));
    models_dir.join(name)
}
