//! `peerclaw models` command - Manage AI models.

use clap::{Args, Subcommand};
use std::io::{self, Write};

use crate::bootstrap;

#[derive(Args)]
pub struct ModelsArgs {
    #[command(subcommand)]
    pub cmd: Option<ModelsCommand>,
}

#[derive(Subcommand)]
pub enum ModelsCommand {
    /// List available models
    List,

    /// Download a model from Hugging Face
    Download {
        /// Model name (e.g., llama-3.2-1b, llama-3.2-3b, phi-3-mini)
        model: String,

        /// Quantization level (q4_k_m, q5_k_m, q6_k, q8_0)
        #[arg(long, default_value = "q4_k_m")]
        quant: String,
    },

    /// Remove a downloaded model
    Remove {
        /// Model name to remove
        model: String,
    },

    /// Show model information
    Info {
        /// Model name
        model: String,
    },
}

pub async fn run(args: ModelsArgs) -> anyhow::Result<()> {
    match args.cmd {
        None | Some(ModelsCommand::List) => list_models().await,
        Some(ModelsCommand::Download { model, quant }) => download_model(&model, &quant).await,
        Some(ModelsCommand::Remove { model }) => remove_model(&model).await,
        Some(ModelsCommand::Info { model }) => show_info(&model).await,
    }
}

async fn list_models() -> anyhow::Result<()> {
    let models_dir = bootstrap::base_dir().join("models");
    std::fs::create_dir_all(&models_dir)?;

    println!();
    println!("\x1b[1m═══ Downloaded Models ═══\x1b[0m");
    println!("  Directory: \x1b[90m{}\x1b[0m", models_dir.display());
    println!();

    let mut found = false;
    if let Ok(entries) = std::fs::read_dir(&models_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gguf") {
                found = true;
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                let size = std::fs::metadata(&path)
                    .map(|m| {
                        let bytes = m.len();
                        if bytes >= 1_073_741_824 {
                            format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
                        } else {
                            format!("{:.0} MB", bytes as f64 / 1_048_576.0)
                        }
                    })
                    .unwrap_or_else(|_| "? bytes".to_string());
                println!(
                    "  \x1b[32m✓\x1b[0m \x1b[36m{}\x1b[0m \x1b[90m({})\x1b[0m",
                    name, size
                );
            }
        }
    }

    if !found {
        println!("  \x1b[33mNo models downloaded yet.\x1b[0m");
    }

    println!();
    println!("\x1b[1m═══ Available for Download ═══\x1b[0m");
    println!();

    for (name, _repo, _filename) in crate::models_hf::KNOWN_GGUF_PRESETS {
        println!("  • \x1b[36m{}\x1b[0m", name);
    }

    println!();
    println!("  To download: \x1b[36mpeerclaw models download <name>\x1b[0m");
    println!("  Example:     \x1b[36mpeerclaw models download llama-3.2-1b\x1b[0m");
    println!();

    Ok(())
}

async fn download_model(model: &str, quant: &str) -> anyhow::Result<()> {
    let models_dir = bootstrap::base_dir().join("models");
    std::fs::create_dir_all(&models_dir)?;

    let Some((url, out_name)) = crate::models_hf::preset_to_hf_url(model, quant) else {
        println!("\x1b[33mModel '{}' not in known list.\x1b[0m", model);
        println!();
        println!("Available models:");
        for (name, _, _) in crate::models_hf::KNOWN_GGUF_PRESETS {
            println!("  • {}", name);
        }
        return Ok(());
    };

    let output_path = models_dir.join(out_name);

    if output_path.exists() {
        println!(
            "\x1b[33mModel already exists:\x1b[0m {}",
            output_path.display()
        );
        return Ok(());
    }

    println!();
    println!("\x1b[1m═══ Downloading Model ═══\x1b[0m");
    println!("  Model:  \x1b[36m{}\x1b[0m", model);
    println!("  Quant:  \x1b[36m{}\x1b[0m", quant);
    println!("  From:   \x1b[90m{}\x1b[0m", url);
    println!("  To:     \x1b[90m{}\x1b[0m", output_path.display());
    println!();

    println!("\x1b[33mDownloading...\x1b[0m (this may take a while)");
    println!();

    let downloaded = crate::models_hf::download_url_to_path(&url, &output_path)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    let downloaded_mb = downloaded as f64 / 1_048_576.0;
    println!("  Downloaded: {:.0} MB", downloaded_mb);

    println!();
    println!();
    println!("\x1b[32m✓ Download complete!\x1b[0m");
    println!("  Model saved to: \x1b[36m{}\x1b[0m", output_path.display());
    println!();
    println!(
        "  To use in chat: \x1b[36mpeerclaw chat --model {}-{}\x1b[0m",
        model, quant
    );
    println!();

    Ok(())
}

async fn remove_model(model: &str) -> anyhow::Result<()> {
    let models_dir = bootstrap::base_dir().join("models");

    // Find matching model file
    let mut found = None;
    if let Ok(entries) = std::fs::read_dir(&models_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gguf") {
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if name.to_lowercase().contains(&model.to_lowercase()) {
                    found = Some(path);
                    break;
                }
            }
        }
    }

    match found {
        Some(path) => {
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
            print!("Remove model '{}'? [y/N] ", name);
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if input.trim().eq_ignore_ascii_case("y") {
                std::fs::remove_file(&path)?;
                println!("\x1b[32m✓\x1b[0m Model removed.");
            } else {
                println!("Cancelled.");
            }
        }
        None => {
            println!("\x1b[33mModel '{}' not found.\x1b[0m", model);
            println!("Run \x1b[36mpeerclaw models list\x1b[0m to see downloaded models.");
        }
    }

    Ok(())
}

async fn show_info(model: &str) -> anyhow::Result<()> {
    let models_dir = bootstrap::base_dir().join("models");

    // Find matching model file
    let mut found = None;
    if let Ok(entries) = std::fs::read_dir(&models_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gguf") {
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if name.to_lowercase().contains(&model.to_lowercase()) {
                    found = Some(path);
                    break;
                }
            }
        }
    }

    match found {
        Some(path) => {
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
            let meta = std::fs::metadata(&path)?;
            let size_gb = meta.len() as f64 / 1_073_741_824.0;

            println!();
            println!("\x1b[1m═══ Model Info ═══\x1b[0m");
            println!("  Name:     \x1b[36m{}\x1b[0m", name);
            println!("  Path:     \x1b[90m{}\x1b[0m", path.display());
            println!("  Size:     {:.2} GB", size_gb);
            println!("  Format:   GGUF");
            println!();
        }
        None => {
            println!("\x1b[33mModel '{}' not found.\x1b[0m", model);
        }
    }

    Ok(())
}
