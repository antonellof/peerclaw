//! Hugging Face GGUF presets and download helpers (shared by CLI and web API).

use std::path::{Path, PathBuf};

/// Preset id, HF repo, file stem, and quant separator (`-` or `.`).
/// URL: `https://huggingface.co/{repo}/resolve/main/{stem}{sep}{quant_tag}.gguf`
pub const KNOWN_GGUF_PRESETS: &[(&str, &str, &str, &str)] = &[
    (
        "llama-3.2-1b",
        "bartowski/Llama-3.2-1B-Instruct-GGUF",
        "Llama-3.2-1B-Instruct",
        "-",
    ),
    (
        "llama-3.2-3b",
        "bartowski/Llama-3.2-3B-Instruct-GGUF",
        "Llama-3.2-3B-Instruct",
        "-",
    ),
    (
        "phi-3-mini",
        "microsoft/Phi-3-mini-4k-instruct-gguf",
        "Phi-3-mini-4k-instruct",
        "-",
    ),
    (
        "qwen2.5-0.5b",
        "Qwen/Qwen2.5-0.5B-Instruct-GGUF",
        "qwen2.5-0.5b-instruct",
        "-",
    ),
    (
        "qwen2.5-1.5b",
        "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
        "qwen2.5-1.5b-instruct",
        "-",
    ),
    (
        "qwen2.5-3b",
        "Qwen/Qwen2.5-3B-Instruct-GGUF",
        "qwen2.5-3b-instruct",
        "-",
    ),
    (
        "gemma-2-2b",
        "bartowski/gemma-2-2b-it-GGUF",
        "gemma-2-2b-it",
        "-",
    ),
    (
        "tinyllama-1.1b",
        "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
        "tinyllama-1.1b-chat-v1.0",
        ".",
    ),
];

/// Resolve a named preset + quantization to a Hugging Face `resolve` URL and output filename.
///
/// HF GGUF repos use `STEM-QUANT.gguf` where QUANT is uppercase with underscores (`Q4_K_M`).
/// Some repos (Phi-3 Microsoft) use a shorter quant tag (`q4` instead of `Q4_K_M`); those are
/// handled via a per-preset override table.
pub fn preset_to_hf_url(preset: &str, quant: &str) -> Option<(String, String)> {
    let (_, repo, stem, sep) = KNOWN_GGUF_PRESETS.iter().find(|(n, _, _, _)| *n == preset)?;

    // Per-repo quant tag conventions:
    // - Phi-3 Microsoft: short tags (q4, fp16)
    // - Qwen: lowercase with underscores (q4_k_m)
    // - bartowski/TheBloke: uppercase with underscores (Q4_K_M)
    let quant_tag = if preset.starts_with("phi-3") {
        match quant.to_lowercase().as_str() {
            "q4_k_m" | "q4_k" | "q4" => "q4".to_string(),
            "q8_0" | "q8" => "q8_0".to_string(),
            "fp16" | "f16" => "fp16".to_string(),
            other => other.to_string(),
        }
    } else if preset.starts_with("qwen") {
        // Qwen repos use lowercase: qwen2.5-0.5b-instruct-q4_k_m.gguf
        quant.to_lowercase().replace('-', "_")
    } else {
        // bartowski / TheBloke: uppercase, underscores (Q4_K_M)
        quant.to_uppercase().replace('-', "_")
    };

    let file = format!("{}{}{}.gguf", stem, sep, quant_tag);
    let url = format!("https://huggingface.co/{repo}/resolve/main/{file}");
    let out_name = format!("{}-{}.gguf", preset, quant);
    Some((url, out_name))
}

/// Download a URL to a path (async). Returns bytes written.
pub async fn download_url_to_path(url: &str, dest: &Path) -> Result<u64, String> {
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

    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    let n = bytes.len() as u64;

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    std::fs::write(dest, &bytes).map_err(|e| e.to_string())?;
    Ok(n)
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
pub fn dest_for_custom_url(models_dir: &Path, url: &str, filename_override: Option<&str>) -> PathBuf {
    let name = filename_override
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| filename_from_hf_url(url));
    models_dir.join(name)
}
