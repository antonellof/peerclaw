//! `peerclawd test` command - Test distributed execution.

use clap::{Args, Subcommand};
use std::sync::Arc;
use std::time::Duration;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::executor::task::{ExecutionTask, InferenceTask, WebFetchTask, TaskData};
use crate::identity::NodeIdentity;
use crate::runtime::Runtime;
use crate::wallet::from_micro;

#[derive(Args)]
pub struct TestArgs {
    #[command(subcommand)]
    pub cmd: TestCommand,
}

#[derive(Subcommand)]
pub enum TestCommand {
    /// Run a local inference test
    Inference {
        /// Model to use
        #[arg(long, default_value = "llama-3.2-3b")]
        model: String,

        /// Prompt to send
        #[arg(long, default_value = "Hello, world! Please respond briefly.")]
        prompt: String,

        /// Maximum tokens
        #[arg(long, default_value = "100")]
        max_tokens: u32,
    },

    /// Run a web fetch test
    Fetch {
        /// URL to fetch
        #[arg(long, default_value = "https://httpbin.org/get")]
        url: String,
    },

    /// Run all tests in sequence
    All,

    /// Show runtime status
    Status,

    /// Run a multi-agent distributed test (spawns multiple nodes)
    Distributed {
        /// Number of agents to spawn
        #[arg(long, default_value = "3")]
        agents: u32,

        /// Duration to run in seconds
        #[arg(long, default_value = "30")]
        duration: u64,
    },
}

pub async fn run(args: TestArgs) -> anyhow::Result<()> {
    match args.cmd {
        TestCommand::Inference { model, prompt, max_tokens } => {
            run_inference_test(&model, &prompt, max_tokens).await
        }
        TestCommand::Fetch { url } => {
            run_fetch_test(&url).await
        }
        TestCommand::All => {
            run_all_tests().await
        }
        TestCommand::Status => {
            show_status().await
        }
        TestCommand::Distributed { agents, duration } => {
            run_distributed_test(agents, duration).await
        }
    }
}

async fn create_runtime() -> anyhow::Result<Runtime> {
    bootstrap::ensure_dirs()?;

    let identity_path = bootstrap::identity_path();
    let identity = if identity_path.exists() {
        Arc::new(NodeIdentity::load(&identity_path)?)
    } else {
        let id = NodeIdentity::generate();
        id.save(&identity_path)?;
        Arc::new(id)
    };

    let config = Config::load()?;
    let db = Database::open(&config.database.path)?;

    Runtime::new(identity, db, config).await
}

async fn run_inference_test(model: &str, prompt: &str, max_tokens: u32) -> anyhow::Result<()> {
    println!("=== Inference Test ===");
    println!("Model: {}", model);
    println!("Prompt: {}", prompt);
    println!("Max tokens: {}", max_tokens);
    println!();

    let runtime = create_runtime().await?;

    let start = std::time::Instant::now();
    let result = runtime.inference(prompt, model, max_tokens).await?;
    let elapsed = start.elapsed();

    println!("Result:");
    match &result.data {
        TaskData::Inference(inference_result) => {
            println!("  Text: {}", inference_result.text);
            println!("  Tokens generated: {}", inference_result.tokens_generated);
            println!("  Tokens/sec: {:.2}", inference_result.tokens_per_second);
        }
        TaskData::Error(e) => {
            println!("  Error: {}", e);
        }
        _ => {
            println!("  Unexpected result type");
        }
    }
    println!("  Location: {:?}", result.location);
    println!("  Time: {:?}", elapsed);
    if let Some(cost) = result.cost {
        println!("  Cost: {:.6} PCLAW", from_micro(cost));
    }

    Ok(())
}

async fn run_fetch_test(url: &str) -> anyhow::Result<()> {
    println!("=== Web Fetch Test ===");
    println!("URL: {}", url);
    println!();

    let runtime = create_runtime().await?;

    let start = std::time::Instant::now();
    let result = runtime.web_fetch(url).await?;
    let elapsed = start.elapsed();

    println!("Result:");
    match &result.data {
        TaskData::WebFetch(fetch_result) => {
            println!("  Status: {}", fetch_result.status);
            println!("  Headers: {} entries", fetch_result.headers.len());
            println!("  Body size: {} bytes", fetch_result.body.len());
            if fetch_result.body.len() < 500 {
                if let Ok(body) = String::from_utf8(fetch_result.body.clone()) {
                    println!("  Body: {}", body);
                }
            }
        }
        TaskData::Error(e) => {
            println!("  Error: {}", e);
        }
        _ => {
            println!("  Unexpected result type");
        }
    }
    println!("  Location: {:?}", result.location);
    println!("  Time: {:?}", elapsed);

    Ok(())
}

async fn run_all_tests() -> anyhow::Result<()> {
    println!("=== Running All Tests ===\n");

    // Inference test
    run_inference_test("llama-3.2-3b", "Hello! Respond with one word.", 50).await?;
    println!();

    // Web fetch test
    run_fetch_test("https://httpbin.org/get").await?;
    println!();

    // Status
    show_status().await?;

    println!("\n=== All Tests Complete ===");
    Ok(())
}

async fn show_status() -> anyhow::Result<()> {
    println!("=== Runtime Status ===");

    let runtime = create_runtime().await?;
    let stats = runtime.stats().await;

    println!("Peer ID: {}", stats.peer_id);
    println!("Connected peers: {}", stats.connected_peers);
    println!("Balance: {:.6} PCLAW", stats.balance);
    println!("Active jobs: {}", stats.active_jobs);
    println!("Completed jobs: {}", stats.completed_jobs);
    println!();
    println!("Resource State:");
    println!("  CPU usage: {:.1}%", stats.resource_state.cpu_usage * 100.0);
    println!("  RAM: {}/{} MB available",
             stats.resource_state.ram_available_mb,
             stats.resource_state.ram_total_mb);
    println!("  Active inference tasks: {}", stats.resource_state.active_inference_tasks);
    println!("  Active web tasks: {}", stats.resource_state.active_web_tasks);
    println!("  Active WASM tasks: {}", stats.resource_state.active_wasm_tasks);
    println!("  Loaded models: {:?}", stats.resource_state.loaded_models);

    Ok(())
}

async fn run_distributed_test(agent_count: u32, duration_secs: u64) -> anyhow::Result<()> {
    println!("=== Distributed Execution Test ===");
    println!("Testing with {} simulated agents for {} seconds", agent_count, duration_secs);
    println!();

    // Create temporary directory for this test run
    let temp_base = std::env::temp_dir().join(format!("peerclawd_test_{}", std::process::id()));
    std::fs::create_dir_all(&temp_base)?;

    // Run agents sequentially to avoid thread-safety issues with libp2p
    // In production, each agent would be a separate process
    let mut results = vec![];

    for i in 0..agent_count {
        let agent_dir = temp_base.join(format!("agent_{}", i));
        std::fs::create_dir_all(&agent_dir)?;

        println!("Running agent {}...", i);

        match run_agent(i, agent_dir, duration_secs / agent_count as u64).await {
            Ok(stats) => {
                results.push((i as usize, stats));
            }
            Err(e) => {
                println!("Agent {} error: {}", i, e);
            }
        }
    }

    // Print summary
    println!("\n=== Results Summary ===");
    for (i, stats) in &results {
        println!("Agent {}:", i);
        println!("  Peer ID: {}...", &stats.peer_id[..16.min(stats.peer_id.len())]);
        println!("  Tasks completed: {}", stats.tasks_completed);
        println!("  Tasks received: {}", stats.tasks_received);
        println!("  Final balance: {:.6} PCLAW", stats.final_balance);
    }

    let total_tasks: usize = results.iter().map(|(_, s)| s.tasks_completed).sum();
    println!("\nTotal tasks completed across all agents: {}", total_tasks);

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_base);

    Ok(())
}

#[derive(Debug)]
struct AgentStats {
    peer_id: String,
    tasks_completed: usize,
    tasks_received: usize,
    final_balance: f64,
}

async fn run_agent(agent_num: u32, base_dir: std::path::PathBuf, duration_secs: u64) -> anyhow::Result<AgentStats> {
    // Create identity for this agent
    let identity = Arc::new(NodeIdentity::generate());
    let peer_id = identity.peer_id().to_string();

    // Create config with unique paths
    let mut config = Config::default();
    config.database.path = base_dir.join("data.redb");
    config.inference.models_dir = base_dir.join("models");
    std::fs::create_dir_all(&config.inference.models_dir)?;

    // Use different ports for each agent
    let port = 9000 + agent_num;
    config.p2p.listen_addresses = vec![format!("/ip4/127.0.0.1/tcp/{}", port)];

    // Connect agents to each other (agent 0 is bootstrap)
    if agent_num > 0 {
        config.p2p.bootstrap_peers = vec![format!("/ip4/127.0.0.1/tcp/{}", 9000)];
    }

    // Create runtime
    let db = Database::open(&config.database.path)?;
    let runtime = Runtime::new(identity, db, config).await?;

    // Subscribe to job topics
    runtime.subscribe_to_job_topics().await?;

    tracing::info!(
        agent = agent_num,
        peer_id = %peer_id,
        "Agent started"
    );

    let mut tasks_completed = 0;
    let mut tasks_received = 0;

    // Run for specified duration, periodically executing tasks
    let start = std::time::Instant::now();
    let duration = Duration::from_secs(duration_secs);

    while start.elapsed() < duration {
        // Each agent periodically executes a task
        if agent_num % 2 == 0 {
            // Even agents do inference tasks
            match runtime.inference("Test prompt", "test-model", 10).await {
                Ok(_) => tasks_completed += 1,
                Err(_) => {}
            }
        } else {
            // Odd agents do web fetch tasks
            match runtime.web_fetch("https://httpbin.org/get").await {
                Ok(_) => tasks_completed += 1,
                Err(_) => {}
            }
        }

        // Check for received tasks (job provider)
        let active = runtime.job_manager.read().await.active_jobs().await;
        tasks_received = active.len();

        // Small delay between tasks
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let final_balance = from_micro(runtime.balance().await);

    Ok(AgentStats {
        peer_id,
        tasks_completed,
        tasks_received,
        final_balance,
    })
}
