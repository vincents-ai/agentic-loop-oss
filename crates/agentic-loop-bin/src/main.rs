//! # agentic-loop binary (OSS)
//!
//! Wires trait implementations and runs the agent loop.
//! This is the open-source binary — no commercial features.

use agentic_loop_eval::HeuristicEvaluator;
use agentic_loop_llm::{LlmExecutor, RotationStrategy};
use agentic_loop_storage::EntityStore;
use agentic_loop_storage_gitrefs::GitRefsBackend;
use agentic_loop_tools::ToolProvider;
use agentic_loop_tools_core::CoreToolProvider;
use agentic_loop_workflow::{PromptBuilderImpl, WorkflowEngineImpl, WorkflowParserImpl};
use agentic_loop_types::workflow::Scenario;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

mod tracing_init;
mod completions;

// ─── Config ──────────────────────────────────────────────────────────────────

/// Application configuration.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub repo_path: PathBuf,
    pub workflows_path: PathBuf,
    pub default_provider: String,
    pub default_model: String,
    pub project_id: String,
    pub max_steps: usize,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            repo_path: PathBuf::from(std::env::var("AGENTIC_LOOP_REPO_PATH").unwrap_or_else(|_| ".".to_string())),
            workflows_path: PathBuf::from(std::env::var("AGENTIC_LOOP_WORKFLOWS_PATH").unwrap_or_else(|_| "./examples/workflows".to_string())),
            default_provider: std::env::var("AGENTIC_LOOP_PROVIDER").unwrap_or_else(|_| "anthropic".to_string()),
            default_model: std::env::var("AGENTIC_LOOP_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string()),
            project_id: std::env::var("AGENTIC_LOOP_PROJECT_ID").unwrap_or_else(|_| "default".to_string()),
            max_steps: std::env::var("AGENTIC_LOOP_MAX_STEPS").ok().and_then(|s| s.parse().ok()).unwrap_or(50),
        })
    }
}

// ─── App ─────────────────────────────────────────────────────────────────────

/// The main application — wires all components together.
pub struct App {
    config: AppConfig,
    storage: Arc<dyn EntityStore>,
    tool_provider: Arc<CoreToolProvider>,
    evaluator: HeuristicEvaluator,
    prompt_builder: PromptBuilderImpl,
}

impl App {
    pub fn new(config: AppConfig) -> Result<Self> {
        let storage = Arc::new(GitRefsBackend::new(&config.repo_path)?);
        let tool_provider = Arc::new(CoreToolProvider::new());
        let evaluator = HeuristicEvaluator::new();
        let prompt_builder = PromptBuilderImpl::new();

        Ok(Self { config, storage, tool_provider, evaluator, prompt_builder })
    }

    /// Create an LLM executor for a single provider/model.
    pub async fn create_executor(&self, provider_name: &str, model: &str) -> Result<LlmExecutor> {
        let resolved = agentic_loop_llm::resolve_provider(provider_name)?;
        let provider = agentic_loop_llm::create_provider(&resolved).await?;
        Ok(LlmExecutor::new(provider, model.to_string()))
    }

    /// Create a model pool backed by OpenRouter free models.
    pub async fn create_pool(&self, strategy: RotationStrategy, min_context: u32) -> Result<agentic_loop_llm::ModelPool> {
        let resolved = agentic_loop_llm::resolve_provider("openrouter")?;
        let provider = agentic_loop_llm::create_provider(&resolved).await?;
        agentic_loop_llm::create_free_pool(provider, strategy, min_context).await
    }

    pub fn list_workflows(&self) -> Result<Vec<String>> {
        let mut workflows = Vec::new();
        if self.config.workflows_path.exists() {
            for entry in std::fs::read_dir(&self.config.workflows_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map(|e| e == "workflow").unwrap_or(false) {
                    if let Some(name) = path.file_stem() {
                        workflows.push(name.to_string_lossy().to_string());
                    }
                }
            }
        }
        workflows.sort();
        Ok(workflows)
    }

    pub fn parse_workflow(&self, name: &str) -> Result<agentic_loop_types::workflow::Workflow> {
        let path = self.config.workflows_path.join(format!("{}.workflow", name));
        let content = std::fs::read_to_string(&path)?;
        let parser = WorkflowParserImpl::new();
        Ok(parser.parse(&content)?)
    }

    pub async fn list_tools(&self) -> Vec<agentic_loop_types::tool::ToolInfo> {
        self.tool_provider.list_tools().await
    }

    pub fn config(&self) -> &AppConfig { &self.config }
    pub fn storage(&self) -> &Arc<dyn EntityStore> { &self.storage }
    pub fn evaluator(&self) -> &HeuristicEvaluator { &self.evaluator }
    pub fn prompt_builder(&self) -> &PromptBuilderImpl { &self.prompt_builder }
}

// ─── CLI ─────────────────────────────────────────────────────────────────────

fn print_usage() {
    eprintln!("agentic-loop — AI agent loop with engram integration (OSS)");
    eprintln!();
    eprintln!("Usage: agentic-loop <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  list              List available workflows");
    eprintln!("  tools             List available tools");
    eprintln!("  show <workflow>   Show workflow details");
    eprintln!("  run <workflow> [task] [-p provider] [-m model]");
    eprintln!("  models            Discover free tool-calling models from OpenRouter");
    eprintln!("  providers         List LLM providers and auth status");
    eprintln!("  providers show <name>  Show provider details");
    eprintln!("  completions <shell>    Generate shell completions");
    eprintln!("  help              Show this help");
    eprintln!();
    eprintln!("Environment:");
    eprintln!("  AGENTIC_LOOP_REPO_PATH       Git repo for storage (default: .)");
    eprintln!("  AGENTIC_LOOP_WORKFLOWS_PATH  Workflow files (default: ./examples/workflows)");
    eprintln!("  AGENTIC_LOOP_PROVIDER        Default provider (default: anthropic)");
    eprintln!("  AGENTIC_LOOP_MODEL           Default model (default: claude-sonnet-4-20250514)");
    eprintln!("  AGENTIC_LOOP_PROJECT_ID      Project ID (default: default)");
    eprintln!("  AGENTIC_LOOP_MAX_STEPS       Max steps per run (default: 50)");
}

pub async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let config = AppConfig::from_env()?;
    let app = App::new(config)?;

    let command = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match command {
        "list" | "workflows" => {
            let workflows = app.list_workflows()?;
            println!("Available workflows ({}):", workflows.len());
            for name in &workflows {
                println!("  - {}", name);
            }
        }

        "tools" => {
            let tools = app.list_tools().await;
            println!("Available tools ({}):", tools.len());
            for tool in &tools {
                println!("  - {} : {}", tool.name, tool.description);
            }
        }

        "show" => {
            let workflow_name = args.get(2).ok_or_else(|| {
                anyhow::anyhow!("Usage: agentic-loop show <workflow-name>")
            })?;

            let workflow = app.parse_workflow(workflow_name)?;
            let engine = WorkflowEngineImpl::new(workflow);

            println!("Workflow: {}", engine.workflow().name);
            println!("  Description: {}", engine.workflow().description);
            println!("  States ({}):", engine.workflow().states.len());
            for state in &engine.workflow().states {
                let transitions = engine.valid_transitions(&state.name);
                println!("    - {} -> [{}]", state.name, transitions.join(", "));
            }

            let warnings = engine.validate();
            if !warnings.is_empty() {
                println!("  Warnings:");
                for w in &warnings {
                    println!("    ⚠ {}", w);
                }
            }
        }

        "run" => {
            let workflow_name = args.get(2).ok_or_else(|| {
                anyhow::anyhow!("Usage: agentic-loop run <workflow-name> [task] [-p provider] [-m model]")
            })?;

            // Parse flags
            let mut task_parts = Vec::new();
            let mut provider_name = None;
            let mut model = None;
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--provider" | "-p" => { provider_name = args.get(i + 1).cloned(); i += 2; }
                    "--model" | "-m" => { model = args.get(i + 1).cloned(); i += 2; }
                    _ => { task_parts.push(args[i].clone()); i += 1; }
                }
            }
            let task = if task_parts.is_empty() {
                format!("Execute {} workflow", workflow_name)
            } else {
                task_parts.join(" ")
            };

            let provider = provider_name.as_deref().unwrap_or(&app.config().default_provider);
            let model_id = model.as_deref().unwrap_or(&app.config().default_model);

            println!("Running '{}' with {} ({}) : {}", workflow_name, provider, model_id, task);

            let workflow = app.parse_workflow(workflow_name)?;
            let engine = WorkflowEngineImpl::new(workflow);
            let warnings = engine.validate();
            if !warnings.is_empty() {
                for w in &warnings {
                    eprintln!("  ⚠ {}", w);
                }
            }

            let executor = app.create_executor(provider, model_id).await?;

            // Walk through workflow states, executing LLM at each step
            let session_id = uuid::Uuid::new_v4().to_string();
            let states = &engine.workflow().states;
            let mut steps = 0;
            let mut errors: Vec<String> = Vec::new();

            for state in states {
                steps += 1;
                if steps > app.config().max_steps {
                    errors.push("Max steps reached".into());
                    break;
                }

                let default_scenario = Scenario {
                    state: state.name.clone(),
                    name: "default".into(),
                    given: vec![],
                    when: vec![],
                    then: vec![],
                    model_hint: None,
                    tools: vec![],
                };
                let system_prompt = app.prompt_builder().build_system_prompt(
                    &engine.workflow(),
                    &state.name,
                    &default_scenario,
                    None,
                );

                let result = executor.execute_step(
                    &system_prompt,
                    &task,
                    &[],
                    4096,
                ).await?;

                if let Some(ref text) = result.text {
                    println!("[{}] {}", state.name, &text[..text.len().min(200)]);
                }

                if result.finish_reason.as_deref() == Some("stop") {
                    break;
                }
            }

            println!("\nSession: {}", session_id);
            println!("  Steps: {}", steps);
            println!("  Errors: {}", errors.len());
        }

        "models" => {
            println!("Discovering free models from OpenRouter...");
            let resolved = agentic_loop_llm::resolve_provider("openrouter")?;
            let provider = agentic_loop_llm::create_provider(&resolved).await?;
            match agentic_loop_llm::discover_openrouter_models(provider, 32_000).await {
                Ok(entries) => {
                    println!("Found {} free tool-calling models (>= 32K context):\n", entries.len());
                    println!("{:<5} {:<45} {:>10} {:>12}", "#", "Model", "Ctx (K)", "Capabilities");
                    println!("{}", "-".repeat(75));
                    for (i, entry) in entries.iter().enumerate() {
                        let caps: Vec<&str> = entry.capabilities.iter().map(|c| match c {
                            agentic_loop_types::ModelCapability::ToolCalling => "tools",
                            agentic_loop_types::ModelCapability::Coding => "coding",
                            agentic_loop_types::ModelCapability::Reasoning => "reasoning",
                            agentic_loop_types::ModelCapability::Vision => "vision",
                            agentic_loop_types::ModelCapability::Streaming => "stream",
                        }).collect();
                        println!("{:<5} {:<45} {:>10} {:>12}",
                            i + 1, entry.model, entry.context_length / 1024, caps.join(","));
                    }
                }
                Err(e) => eprintln!("Error discovering models: {}", e),
            }
        }

        "providers" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "show" => {
                    let name = args.get(3).ok_or_else(|| {
                        anyhow::anyhow!("Usage: agentic-loop providers show <name>")
                    })?;
                    let detail = agentic_loop_llm::get_provider_detail(name)?;
                    println!("Provider: {}", detail.name);
                    println!("  API: {}", detail.api);
                    println!("  Base URL: {}", detail.base_url);
                    println!("  Auth: {}", if detail.available { "✓ configured" } else { "✗ not configured" });
                    if let Some(ref env) = detail.env_var {
                        println!("  Env Var: {}", env);
                    }
                    println!("  Total Models: {}", detail.total_models);
                    println!();
                    println!("  Recommended Models:");
                    for m in &detail.recommended_models {
                        let reasoning = if m.reasoning { " [reasoning]" } else { "" };
                        println!("    - {} ({}){} context={}", m.id, m.name, reasoning, m.context_window);
                    }
                }
                _ => {
                    let statuses = agentic_loop_llm::list_all_providers()?;
                    println!("LLM Providers ({} total):", statuses.len());
                    println!();
                    println!("  {:<22} {:<10} {:<8} {}", "PROVIDER", "API TYPE", "AUTH", "MODELS");
                    println!("  {}", "-".repeat(70));
                    for s in &statuses {
                        let auth = if s.available { "✓" } else { "✗" };
                        let env = s.env_var.as_deref().unwrap_or("(special)");
                        let models = format!("{}/{}", s.recommended_count, s.total_count);
                        println!("  {:<22} {:<10} {:<8} {}", s.name, s.api, auth, models);
                        if s.available {
                            println!("    {} ({})", s.base_url, env);
                        } else if s.env_var.is_some() {
                            println!("    {} (set {} to enable)", s.base_url, env);
                        }
                    }
                    println!();
                    println!("  Set API keys via environment variables or ~/.config/agentic-loop/keys.toml");
                }
            }
        }

        "completions" => {
            let shell = args.get(2).map(|s| s.as_str()).unwrap_or("bash");
            let script = completions::generate_completions(shell);
            print!("{}", script);
        }

        "help" | "--help" | "-h" | _ => {
            print_usage();
        }
    }

    Ok(())
}

fn main() {
    tracing_init::init();
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let result = rt.block_on(run());
    tracing_init::shutdown();
    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
