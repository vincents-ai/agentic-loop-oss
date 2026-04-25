//! # agentic-loop binary
//!
//! Wires trait implementations and runs the agent loop.

use agentic_core::{AgentLoopRunner, SessionManager};
use agentic_loop_eval::HeuristicEvaluator;
use agentic_loop_llm::ModelSelectorImpl;
use agentic_loop_storage::EntityStore;
use agentic_loop_storage_gitrefs::GitRefsBackend;
use agentic_loop_tools::ToolProvider;
use agentic_loop_tools_core::CoreToolProvider;
use agentic_loop_workflow::{PromptBuilderImpl, WorkflowEngineImpl, WorkflowParserImpl};
#[allow(unused_imports)]
use agentic_loop_types::{schedule::ScheduledTask, storage::QueryFilter};

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

mod tracing_init;
mod completions;

/// Application configuration.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Path to the git repository for storage.
    pub repo_path: PathBuf,
    /// Path to workflow files.
    pub workflows_path: PathBuf,
    /// Default model provider.
    pub default_provider: String,
    /// Default model.
    pub default_model: String,
    /// Project ID for storage sharding.
    pub project_id: String,
    /// Maximum steps per workflow run.
    pub max_steps: usize,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let repo_path = std::env::var("AGENTIC_LOOP_REPO_PATH")
            .unwrap_or_else(|_| ".".to_string());
        let workflows_path = std::env::var("AGENTIC_LOOP_WORKFLOWS_PATH")
            .unwrap_or_else(|_| "./examples/workflows".to_string());
        let default_provider = std::env::var("AGENTIC_LOOP_PROVIDER")
            .unwrap_or_else(|_| "anthropic".to_string());
        let default_model = std::env::var("AGENTIC_LOOP_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());
        let project_id = std::env::var("AGENTIC_LOOP_PROJECT_ID")
            .unwrap_or_else(|_| "default".to_string());
        let max_steps = std::env::var("AGENTIC_LOOP_MAX_STEPS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(50);

        Ok(Self {
            repo_path: PathBuf::from(repo_path),
            workflows_path: PathBuf::from(workflows_path),
            default_provider,
            default_model,
            project_id,
            max_steps,
        })
    }
}

/// The main application, wiring all components together.
pub struct App {
    config: AppConfig,
    storage: Arc<dyn EntityStore>,
    tool_provider: Arc<CoreToolProvider>,
    #[allow(dead_code)]
    model_selector: ModelSelectorImpl,
    #[allow(dead_code)]
    evaluator: HeuristicEvaluator,
    prompt_builder: PromptBuilderImpl,
}

impl App {
    /// Create a new application with the given config.
    pub fn new(config: AppConfig) -> Result<Self> {
        let storage = Arc::new(GitRefsBackend::new(&config.repo_path)?);
        let tool_provider = Arc::new(CoreToolProvider::new());
        let model_selector = ModelSelectorImpl::new();
        let evaluator = HeuristicEvaluator::new();
        let prompt_builder = PromptBuilderImpl::new();

        Ok(Self {
            config,
            storage,
            tool_provider,
            model_selector,
            evaluator,
            prompt_builder,
        })
    }

    /// Create an AgentLoopRunner with a ModelPool for the given model type.
    /// Discovers free tool-calling models from OpenRouter and creates a pool.
    pub async fn create_pool_runner(
        &self,
        model_type: &str,
    ) -> Result<AgentLoopRunner> {
        self.create_pool_runner_with_strategy(model_type, agentic_loop_llm::RotationStrategy::default()).await
    }

    /// Create an AgentLoopRunner with a ModelPool for the given model type + strategy.
    pub async fn create_pool_runner_with_strategy(
        &self,
        model_type: &str,
        strategy: agentic_loop_llm::RotationStrategy,
    ) -> Result<AgentLoopRunner> {
        use agentic_loop_types::ModelType;

        let mt: ModelType = model_type.parse()
            .map_err(|e| anyhow::anyhow!("Invalid model type '{}': {}", model_type, e))?;

        let resolved = agentic_loop_llm::resolve_provider("openrouter")?;
        let wrapper = agentic_loop_llm::create_wrapper(&resolved).await?;

        let min_context: u32 = match mt {
            ModelType::Fast => 32_000,
            _ => 32_000,
        };

        println!("Discovering {} models from OpenRouter...", mt);
        let pool = agentic_loop_llm::create_free_pool(
            wrapper, strategy, min_context,
        ).await?;

        let entries = pool.entries();
        println!("Model pool ({} models):", entries.len());
        for (i, entry) in entries.iter().enumerate() {
            let marker = if i == 0 { " [active]" } else { "" };
            println!("  {}. {} ({}K ctx){}",
                i + 1, entry.model, entry.context_length / 1024, marker);
        }
        println!();

        let runner_config = agentic_core::RunnerConfig {
            workflows_path: self.config.workflows_path.clone(),
            project_id: self.config.project_id.clone(),
            max_steps: self.config.max_steps,
            eval_enabled: true,
            max_turns_per_step: 10,
            context_window: entries.first()
                .map(|e| e.context_length)
                .unwrap_or(128_000),
        };

        Ok(AgentLoopRunner::with_llm(
            runner_config,
            self.storage.clone(),
            self.tool_provider.clone(),
            Arc::new(pool) as Arc<dyn agentic_loop_llm::LlmExecutorTrait>,
        ))
    }

    /// Create an AgentLoopRunner with a primary model and a fallback.
    /// If the primary fails, the fallback is used automatically.
    pub async fn create_fallback_runner(
        &self,
        provider_name: &str,
        primary_model: &str,
        fallback_model: &str,
    ) -> Result<AgentLoopRunner> {
        let resolved = agentic_loop_llm::resolve_provider(provider_name)?;
        let wrapper = agentic_loop_llm::create_wrapper(&resolved).await?;

        let entries = vec![
            agentic_loop_llm::PoolModelEntry {
                provider: provider_name.to_string(),
                model: primary_model.to_string(),
                wrapper: std::sync::Arc::clone(&wrapper),
                context_length: 128_000,
                capabilities: vec![agentic_loop_types::ModelCapability::ToolCalling],
                cost_tier: agentic_loop_types::ModelCostTier::Free,
            },
            agentic_loop_llm::PoolModelEntry {
                provider: provider_name.to_string(),
                model: fallback_model.to_string(),
                wrapper,
                context_length: 128_000,
                capabilities: vec![agentic_loop_types::ModelCapability::ToolCalling],
                cost_tier: agentic_loop_types::ModelCostTier::Free,
            },
        ];

        println!("Fallback pool: primary={}, fallback={}", primary_model, fallback_model);
        println!();

        let pool = agentic_loop_llm::ModelPool::new(entries, agentic_loop_llm::RotationStrategy::Healthiest);

        let runner_config = agentic_core::RunnerConfig {
            workflows_path: self.config.workflows_path.clone(),
            project_id: self.config.project_id.clone(),
            max_steps: self.config.max_steps,
            eval_enabled: true,
            max_turns_per_step: 10,
            context_window: 128_000,
        };

        Ok(AgentLoopRunner::with_llm(
            runner_config,
            self.storage.clone(),
            self.tool_provider.clone(),
            Arc::new(pool) as Arc<dyn agentic_loop_llm::LlmExecutorTrait>,
        ))
    }

    /// Create an AgentLoopRunner from this app's configuration.
    /// If provider_name is given, wires up real LLM execution.
    pub async fn create_runner_with_llm(
        &self,
        provider_name: &str,
        model: &str,
    ) -> Result<AgentLoopRunner> {
        let resolved = agentic_loop_llm::resolve_provider(provider_name)?;
        let wrapper = agentic_loop_llm::create_wrapper(&resolved).await?;
        let executor = agentic_loop_llm::LlmExecutor::new(
            wrapper,
            resolved.name.clone(),
            model.to_string(),
            "agentic-loop".to_string(),
        );

        let runner_config = agentic_core::RunnerConfig {
            workflows_path: self.config.workflows_path.clone(),
            project_id: self.config.project_id.clone(),
            max_steps: self.config.max_steps,
            eval_enabled: true,
            max_turns_per_step: 10,
            context_window: 128000,
        };
        Ok(AgentLoopRunner::with_llm(
            runner_config,
            self.storage.clone(),
            self.tool_provider.clone(),
            Arc::new(executor) as Arc<dyn agentic_loop_llm::LlmExecutorTrait>,
        ))
    }

    /// Create an AgentLoopRunner in simulation mode (no LLM).
    pub fn create_runner(&self) -> AgentLoopRunner {
        let runner_config = agentic_core::RunnerConfig {
            workflows_path: self.config.workflows_path.clone(),
            project_id: self.config.project_id.clone(),
            max_steps: self.config.max_steps,
            eval_enabled: true,
            max_turns_per_step: 10,
            context_window: 128000,
        };
        AgentLoopRunner::new(runner_config, self.storage.clone(), self.tool_provider.clone())
    }

    /// Create a session manager.
    pub fn session_manager(&self) -> SessionManager {
        SessionManager::new(self.storage.clone())
    }

    /// List available workflows.
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

    /// Parse a workflow file.
    pub fn parse_workflow(&self, name: &str) -> Result<agentic_loop_types::workflow::Workflow> {
        let path = self.config.workflows_path.join(format!("{}.workflow", name));
        let content = std::fs::read_to_string(&path)?;
        let parser = WorkflowParserImpl::new();
        Ok(parser.parse(&content)?)
    }

    /// Create a workflow engine for a parsed workflow.
    pub fn create_engine(
        &self,
        workflow: agentic_loop_types::workflow::Workflow,
    ) -> WorkflowEngineImpl {
        WorkflowEngineImpl::new(workflow)
    }

    /// List available tools.
    pub async fn list_tools(&self) -> Vec<agentic_loop_types::tool::ToolInfo> {
        self.tool_provider.list_tools().await
    }

    /// Get a reference to storage.
    pub fn storage(&self) -> &Arc<dyn EntityStore> {
        &self.storage
    }

    /// Get a reference to the prompt builder.
    pub fn prompt_builder(&self) -> &PromptBuilderImpl {
        &self.prompt_builder
    }

    /// Get a reference to the evaluator.
    pub fn evaluator(&self) -> &HeuristicEvaluator {
        &self.evaluator
    }

    /// Get a reference to config.
    pub fn config(&self) -> &AppConfig {
        &self.config
    }
}

/// Parse CLI arguments.
fn parse_args() -> Result<Vec<String>> {
    let args: Vec<String> = std::env::args().collect();
    Ok(args)
}

/// Generate shell completion scripts.
fn print_usage() {
    eprintln!("Usage: agentic-loop [command] [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  list              List available workflows");
    eprintln!("  tools             List available tools");
    eprintln!("  show <workflow>   Show workflow details");
    eprintln!("  run <workflow> [task]   Run a workflow (-p provider -m model -t type -f fallback -s strategy)");
    eprintln!("  resume <id>           Resume a session from last state");
    eprintln!("  sessions          List persisted sessions");
    eprintln!("  providers         List LLM providers and auth status");
    eprintln!("  models            Discover free tool-calling models from OpenRouter");
    eprintln!("  providers show <n> Show provider details and models");
    eprintln!("  providers refresh  Write provider registry to git-refs");
    eprintln!("  daemon           Run as long-running daemon (use --help for options)");
    eprintln!("  repl             Interactive REPL mode");
    eprintln!("  schedule add <cron> <workflow> <task>    Add a scheduled task");
    eprintln!("  version           Show version information");
    eprintln!("  schedule list                          List scheduled tasks");
    eprintln!("  schedule remove <id>                    Remove a scheduled task");
    eprintln!("  sync add <name> <url> [type]    Add a sync remote (personas/skills/workflows/knowledge/mixed)");
    eprintln!("  sync pull [remote]              Pull entities from remote(s)");
    eprintln!("  sync status                     Show sync status");
    eprintln!("  flakiness                        Show test flakiness report");
    eprintln!("  quality-gate                     Run quality gate check");
    eprintln!("  quality-gate hook                Generate pre-commit hook");
    eprintln!("  help              Show this help");
}

/// Run the CLI.
pub async fn run() -> Result<()> {
    let args = parse_args()?;
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
            let engine = app.create_engine(workflow);

            println!("Workflow: {}", engine.workflow().name);
            println!("  Description: {}", engine.workflow().description);
            println!("  States ({}):", engine.workflow().states.len());
            for state in &engine.workflow().states {
                let transitions = engine.valid_transitions(&state.name);
                println!(
                    "    - {} -> [{}]",
                    state.name,
                    transitions.join(", ")
                );
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
                anyhow::anyhow!("Usage: agentic-loop run <workflow-name> [task] [-p provider] [-m model] [-t type] [-f fallback] [-s strategy]")
            })?;

            // Parse optional flags from args
            let mut task_parts = Vec::new();
            let mut provider_name = None;
            let mut model = None;
            let mut model_type = None;
            let mut fallback = None;
            let mut pool_strategy = None;
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--provider" | "-p" => {
                        provider_name = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--model" | "-m" => {
                        model = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--model-type" | "-t" => {
                        model_type = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--fallback" | "-f" => {
                        fallback = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--pool-strategy" | "-s" => {
                        pool_strategy = args.get(i + 1).cloned();
                        i += 2;
                    }
                    _ => {
                        task_parts.push(args[i].clone());
                        i += 1;
                    }
                }
            }
            let task = if task_parts.is_empty() {
                format!("Execute {} workflow", workflow_name)
            } else {
                task_parts.join(" ")
            };

            // Resolve provider and create runner
            let (session_id, result) = if let Some(ref mt) = model_type {
                // Pool mode: discover models and create pool
                let strategy: agentic_loop_llm::RotationStrategy = pool_strategy
                    .as_deref()
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e: anyhow::Error| e)?
                    .unwrap_or_default();
                println!("Running workflow '{}' with model type {} : {}", workflow_name, mt, task);
                println!();
                let runner = app.create_pool_runner_with_strategy(mt, strategy).await?;
                runner
                    .run_workflow(workflow_name, &task, serde_json::json!({}))
                    .await?
            } else if let (Some(ref pname), Some(ref fb_model)) = (&provider_name, &fallback) {
                // Fallback mode: primary + fallback model
                let model_id = model.as_deref().unwrap_or("default");
                println!("Running workflow '{}' with {} ({}) + fallback {} : {}",
                    workflow_name, pname, model_id, fb_model, task);
                println!();
                let runner = app.create_fallback_runner(pname, model_id, fb_model).await?;
                runner
                    .run_workflow(workflow_name, &task, serde_json::json!({}))
                    .await?
            } else if let Some(ref pname) = provider_name {
                let model_id = model.unwrap_or_else(|| {
                    // Default model from env or provider registry
                    std::env::var("AGENTIC_LOOP_MODEL").unwrap_or_else(|_| "default".to_string())
                });
                println!("Running workflow '{}' with {} ({}) : {}", workflow_name, pname, model_id, task);
                println!();

                let runner = app.create_runner_with_llm(pname, &model_id).await?;
                runner
                    .run_workflow(workflow_name, &task, serde_json::json!({}))
                    .await?
            } else {
                println!("Running workflow '{}' (simulation) : {}", workflow_name, task);
                println!();

                let runner = app.create_runner();
                runner
                    .run_workflow(workflow_name, &task, serde_json::json!({}))
                    .await?
            };

            println!("Session: {}", session_id);
            println!("  Status: {:?}", result.status);
            println!("  Steps completed: {}", result.steps_completed);
            println!("  Avg eval score: {:.2}", result.total_eval_score);
            println!("  Final state: {}", result.final_state);

            if !result.errors.is_empty() {
                println!("  Errors:");
                for e in &result.errors {
                    println!("    ✗ {}", e);
                }
            }
        }

        "sessions" => {
            let sm = app.session_manager();
            let sessions = sm.list_sessions(&app.config().project_id).await?;
            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!("Sessions ({}):", sessions.len());
                for id in &sessions {
                    if let Some(record) = sm.load(&app.config().project_id, id).await? {
                        println!(
                            "  - {} [{}] {} ({} steps)",
                            record.id,
                            format!("{:?}", record.status).to_lowercase(),
                            record.workflow_name,
                            record.steps.len()
                        );
                    }
                }
            }
        }

        "resume" => {
            let session_id = args.get(2).ok_or_else(|| {
                anyhow::anyhow!("Usage: agentic-loop resume <session-id>")
            })?;

            println!("Resuming session '{}' ...", session_id);
            let runner = app.create_runner();
            let result = runner.resume_workflow(session_id).await?;

            println!("  Status: {:?}", result.status);
            println!("  Steps completed: {}", result.steps_completed);
            println!("  Avg eval score: {:.2}", result.total_eval_score);
            println!("  Final state: {}", result.final_state);

            if !result.errors.is_empty() {
                println!("  Errors:");
                for e in &result.errors {
                    println!("    ✗ {}", e);
                }
            }
        }

        "models" => {
            let model_type = args.get(2).map(|s| s.as_str()).unwrap_or("free");
            println!("Discovering {} models from OpenRouter...", model_type);
            let resolved = agentic_loop_llm::resolve_provider("openrouter")?;
            let wrapper = agentic_loop_llm::create_wrapper(&resolved).await?;
            match agentic_loop_llm::discover_openrouter_models(wrapper, 32_000).await {
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
                    println!("  Auth: {}", if detail.available { "\u{2713} configured" } else { "\u{2717} not configured" });
                    if let Some(ref env) = detail.env_var {
                        println!("  Env Var: {}", env);
                    }
                    println!("  Total Models: {}", detail.total_models);
                    println!("  Registry Version: {} ({})", detail.version, detail.source);
                    println!();
                    println!("  Recommended Models:");
                    for m in &detail.recommended_models {
                        let reasoning = if m.reasoning { " [reasoning]" } else { "" };
                        println!("    - {} ({}){} context={}", m.id, m.name, reasoning, m.context_window);
                    }
                }
                "refresh" => {
                    let registry = agentic_loop_llm::load_embedded_registry()?;
                    let json = serde_json::to_vec(&registry)?;
                    app.storage().store(
                        &app.config().project_id,
                        "providers",
                        "_registry",
                        &json,
                    ).await?;
                    // Also write each provider individually
                    for (name, entry) in &registry.providers {
                        let data = serde_json::to_vec(entry)?;
                        app.storage().store(
                            &app.config().project_id,
                            "providers",
                            name,
                            &data,
                        ).await?;
                    }
                    println!("Refreshed {} providers to git-refs (project: {})",
                        registry.providers.len(),
                        app.config().project_id
                    );
                }
                _ => {
                    // Default: list all providers
                    use agentic_loop_llm::list_all_providers;
                    let statuses = list_all_providers()?;
                    println!("LLM Providers ({} total):", statuses.len());
                    println!();
                    println!("  {:<22} {:<10} {:<8} {}", "PROVIDER", "API TYPE", "AUTH", "MODELS");
                    println!("  {}", "-".repeat(70));
                    for s in &statuses {
                        let auth = if s.available { "\u{2713}" } else { "\u{2717}" };
                        let env = s.env_var.as_deref().unwrap_or("(special)");
                        let models = format!("{}/{}", s.recommended_count, s.total_count);
                        println!("  {:<22} {:<10} {:<8} {}", s.name, s.api, auth, models);
                        if s.available {
                            println!("    {} ({})", s.base_url, env);
                        } else if s.env_var.is_some() {
                            println!("    {} (set {} to enable)", s.base_url, env);
                        } else {
                            println!("    {} (requires OAuth/special auth)", s.base_url);
                        }
                    }
                    println!();
                    println!("  Usage: providers show <name> | providers refresh");
                    println!("  Set API keys via environment variables or ~/.config/agentic-loop/keys.toml");
                }
            }
        }

        "daemon" => {
            // Daemon mode: run with DaemonScheduler for priority + failover
            use agentic_core::daemon_scheduler::{DaemonScheduler, SchedulerConfig, QueuedTask, TaskPriority};

            let poll_interval = std::env::var("AGENTIC_LOOP_POLL_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(300);

            let task_type = std::env::var("AGENTIC_LOOP_TASK_TYPE")
                .unwrap_or_else(|_| "task".to_string());
            let task_statuses: Vec<String> = std::env::var("AGENTIC_LOOP_TASK_STATUS")
                .map(|s| s.split(',').map(String::from).collect())
                .unwrap_or_else(|_| vec![String::from("todo"), String::from("in_progress")]);
            let default_workflow = std::env::var("AGENTIC_LOOP_DAEMON_WORKFLOW")
                .unwrap_or_else(|_| "quick".to_string());
            let git_pull = std::env::var("AGENTIC_LOOP_GIT_PULL")
                .map(|s| s == "true")
                .unwrap_or(true);
            let max_concurrency = std::env::var("AGENTIC_LOOP_MAX_CONCURRENCY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3);

            let storage = app.storage();
            let project_id = app.config().project_id.clone();
            let provider = app.config().default_provider.clone();
            let model = app.config().default_model.clone();
            let repo_path = app.config().repo_path.clone();

            // Initialize scheduler with provider failover
            let mut scheduler = DaemonScheduler::new(SchedulerConfig {
                max_concurrency,
                ..Default::default()
            });
            scheduler.register_provider(&provider);
            // Register fallback providers from env
            if let Ok(fallbacks) = std::env::var("AGENTIC_LOOP_FALLBACK_PROVIDERS") {
                let providers: Vec<String> = fallbacks.split(',').map(String::from).collect();
                for p in &providers { scheduler.register_provider(p); }
                scheduler.set_fallback_order(providers);
            }

            println!("agentic-loop daemon started");
            println!("  Poll interval: {}s", poll_interval);
            println!("  Task filter: type={} statuses={:?}", task_type, task_statuses);
            println!("  Default workflow: {}", default_workflow);
            println!("  Max concurrency: {}", max_concurrency);
            println!("  Git pull: {}", git_pull);
            println!();
            println!("Press Ctrl+C to stop.");

            let mut interval = tokio::time::interval(std::time::Duration::from_secs(poll_interval));
            interval.tick().await;

            loop {
                tracing::info!("Daemon: poll cycle starting");

                if git_pull {
                    if let Err(e) = daemon_git_pull(&repo_path).await {
                        tracing::warn!("Git pull failed: {}", e);
                    }
                }

                // 1. Poll for tasks and enqueue into scheduler
                match poll_for_tasks(&storage, &project_id, &task_type, &task_statuses).await {
                    Ok(tasks) if !tasks.is_empty() => {
                        tracing::info!("Daemon: found {} tasks", tasks.len());
                        for (i, task_desc) in tasks.into_iter().enumerate() {
                            let priority = TaskPriority::Medium; // Default priority
                            scheduler.enqueue(QueuedTask {
                                id: format!("task-{}-{}", chrono::Utc::now().timestamp(), i),
                                description: task_desc.clone(),
                                workflow: default_workflow.clone(),
                                priority,
                                complexity: 5, // Default
                                depends_on: vec![],
                                provider: provider.clone(),
                                model: model.clone(),
                                tags: vec![],
                                is_self_scheduled: false,
                                max_retries: 3,
                                retry_count: 0,
                            });
                        }
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!("Daemon: poll tasks error: {}", e),
                }

                // 2. Execute tasks from scheduler with provider failover
                while let Some(task) = scheduler.next_task() {
                    // Resolve provider with failover
                    let resolved_provider = scheduler.get_provider(&task.provider)
                        .unwrap_or_else(|| provider.clone());

                    tracing::info!("Daemon: executing '{}' (priority={:?}, provider={})",
                        task.description, task.priority, resolved_provider);

                    let result = run_daemon_task(
                        &project_id, &task.workflow, &task.description,
                        &resolved_provider, &task.model
                    ).await;

                    let success = result.is_ok();
                    let error = result.err().map(|e| e.to_string());

                    scheduler.record_result(agentic_core::daemon_scheduler::TaskResult {
                        task_id: task.id.clone(),
                        success,
                        error,
                        duration_ms: 0,
                        follow_up_tasks: vec![],
                    });

                    if success {
                        tracing::info!("Daemon: task completed");
                    } else {
                        tracing::error!("Daemon: task failed (will retry if retries left)");
                    }
                }

                // 3. Check and execute due scheduled tasks
                match load_scheduled_tasks(&storage, &project_id).await {
                    Ok(schedules) if !schedules.is_empty() => {
                        let now = chrono::Utc::now();
                        for schedule in schedules {
                            if !schedule.enabled { continue; }
                            if let Some(next) = schedule.next_run {
                                if next <= now {
                                    tracing::info!("Daemon: schedule {} is due", schedule.id);
                                    let sched_provider = schedule.provider.clone()
                                        .unwrap_or_else(|| provider.clone());
                                    let sched_model = schedule.model.clone()
                                        .unwrap_or_else(|| model.clone());

                                    match run_daemon_task(&project_id, &schedule.workflow, &schedule.task, &sched_provider, &sched_model).await {
                                        Ok(_) => {
                                            let mut schedule = schedule;
                                            schedule.last_run = Some(now);
                                            schedule.next_run = schedule.compute_next_run();
                                            if let Err(e) = save_scheduled_task(&storage, &project_id, &schedule).await {
                                                tracing::warn!("Failed to update schedule: {}", e);
                                            }
                                            tracing::info!("Daemon: schedule completed");
                                        }
                                        Err(e) => tracing::error!("Daemon: schedule failed: {}", e),
                                    }
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!("Daemon: load schedules error: {}", e),
                }

                tracing::info!("Daemon: poll cycle done");
                interval.tick().await;
            }
        }

        "schedule" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let storage = app.storage();
            let project_id = app.config().project_id.clone();

            match sub {
                "add" => {
                    let cron = args.get(3).ok_or_else(|| anyhow::anyhow!("Usage: schedule add <cron> <workflow> <task>"))?;
                    let workflow = args.get(4).ok_or_else(|| anyhow::anyhow!("Usage: schedule add <cron> <workflow> <task>"))?;
                    let task = args.get(5).map(|s| s.as_str()).unwrap_or("");

                    let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
                    let mut schedule = ScheduledTask {
                        id: id.clone(),
                        cron: cron.to_string(),
                        workflow: workflow.to_string(),
                        task: task.to_string(),
                        persona: None,
                        provider: None,
                        model: None,
                        variables: serde_json::json!({}),
                        last_run: None,
                        next_run: None,
                        enabled: true,
                        max_concurrent: 0,
                    };
                    schedule.next_run = schedule.compute_next_run();

                    save_scheduled_task(&storage, &project_id, &schedule).await?;

                    println!("Scheduled task '{}' added:", id);
                    println!("  Cron: {}", cron);
                    println!("  Workflow: {}", workflow);
                    println!("  Task: {}", task);
                    if let Some(next) = schedule.next_run {
                        println!("  Next run: {}", next);
                    }
                }
                "list" => {
                    let schedules = load_scheduled_tasks(&storage, &project_id).await?;
                    if schedules.is_empty() {
                        println!("No scheduled tasks.");
                    } else {
                        println!("Scheduled tasks ({}):", schedules.len());
                        for s in &schedules {
                            let status = if s.enabled { "enabled" } else { "disabled" };
                            let next = s.next_run.map(|t| t.to_rfc3339()).unwrap_or_else(|| "unknown".to_string());
                            println!("  - {} [{}] {} next={}", s.id, status, s.workflow, next);
                        }
                    }
                }
                "remove" => {
                    let id = args.get(3).ok_or_else(|| anyhow::anyhow!("Usage: schedule remove <id>"))?;
                    let schedules = load_scheduled_tasks(&storage, &project_id).await?;
                    let mut found = false;
                    for mut s in schedules {
                        if *s.id == *id {
                            s.enabled = false;
                            save_scheduled_task(&storage, &project_id, &s).await?;
                            println!("Scheduled task '{}' removed", id);
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        anyhow::bail!("Scheduled task '{}' not found", id);
                    }
                }
                _ => {
                    eprintln!("Usage: schedule add <cron> <workflow> <task> | schedule list | schedule remove <id>");
                }
            }
        }

        "sync" => {
            let cache_dir = std::env::var("AGENTIC_LOOP_SYNC_CACHE")
                .unwrap_or_else(|_| format!("{}/.cache/agentic-loop-sync", std::env::var("HOME").unwrap_or_else(|_| ".".to_string())));
            let mut sync_mgr = agentic_core::sync::SyncManager::new(&cache_dir);

            match args.get(2).map(|s| s.as_str()).unwrap_or("help") {
                "add" => {
                    let name = args.get(3).ok_or_else(|| anyhow::anyhow!("Usage: sync add <name> <url> [personas|skills|workflows|knowledge|mixed]"))?;
                    let url = args.get(4).ok_or_else(|| anyhow::anyhow!("Usage: sync add <name> <url> [entity-type]"))?;
                    let entity_type_str = args.get(5).map(|s| s.as_str()).unwrap_or("mixed");
                    let entity_type = match entity_type_str {
                        "personas" => agentic_core::sync::RemoteEntityType::Personas,
                        "skills" => agentic_core::sync::RemoteEntityType::Skills,
                        "workflows" => agentic_core::sync::RemoteEntityType::Workflows,
                        "knowledge" => agentic_core::sync::RemoteEntityType::Knowledge,
                        _ => agentic_core::sync::RemoteEntityType::Mixed,
                    };
                    println!("Remote '{}' added ({}, type={:?})", name, url, entity_type);
                    sync_mgr.add_remote(agentic_core::sync::RemoteConfig {
                        name: name.to_string(),
                        url: url.to_string(),
                        branch: "main".to_string(),
                        subpath: None,
                        entity_type,
                    });
                }
                "pull" => {
                    let remote = args.get(3);
                    if let Some(r) = remote {
                        println!("Pulling from '{}'...", r);
                        let result = sync_mgr.pull(r).await;
                        println!("  Pulled: {} | Updated: {} | Skipped: {} | Conflicts: {}",
                            result.pulled, result.updated, result.skipped, result.conflicts);
                        if !result.errors.is_empty() {
                            for e in &result.errors {
                                eprintln!("  Error: {}", e);
                            }
                        }
                    } else {
                        println!("Pulling from all remotes...");
                        let results = sync_mgr.pull_all().await;
                        for r in &results {
                            println!("  {}: pulled={} updated={} errors={}",
                                r.remote_name, r.pulled, r.updated, r.errors.len());
                        }
                    }
                }
                "status" => {
                    let statuses = sync_mgr.status().await;
                    if statuses.is_empty() {
                        println!("No remotes configured. Use 'sync add <name> <url>'.");
                    } else {
                        println!("Sync remotes ({}):", statuses.len());
                        for s in &statuses {
                            let last = s.last_sync.map(|t| t.to_rfc3339()).unwrap_or_else(|| "never".to_string());
                            let reachable = if s.reachable { "✓" } else { "✗" };
                            println!("  {} [{}] {} ({:?}) local={} last_sync={}",
                                reachable, s.config.name, s.config.url, s.config.entity_type, s.local_entity_count, last);
                        }
                    }
                }
                _ => {
                    eprintln!("Usage: sync add <name> <url> [type] | sync pull [remote] | sync status");
                }
            }
        }

        "completions" => {
            let shell = args.get(2).map(|s| s.as_str()).unwrap_or("bash");
            let script = completions::generate_completions(shell);
            print!("{}", script);
        }

        "flakiness" => {
            // Track test flakiness across runs
            let mut tracker = agentic_core::flakiness::FlakinessTracker::new();
            // Load existing data if available
            let flakiness_path = app.config.workflows_path.join(".flakiness.json");
            if flakiness_path.exists() {
                if let Ok(data) = std::fs::read_to_string(&flakiness_path) {
                    if let Ok(loaded) = serde_json::from_str::<agentic_core::flakiness::FlakinessTracker>(&data) {
                        tracker = loaded;
                    }
                }
            }
            let summaries = tracker.all_summaries();
            if summaries.is_empty() {
                println!("No flaky test data recorded yet.");
                println!("Record test runs via the API or use the daemon to auto-track.");
            } else {
                println!("Tracked tests ({}):", summaries.len());
                for s in &summaries {
                    let status_icon = match s.status {
                        agentic_core::flakiness::TestStatus::Stable => "✓",
                        agentic_core::flakiness::TestStatus::Unstable => "⚠",
                        agentic_core::flakiness::TestStatus::Flaky => "✗",
                        agentic_core::flakiness::TestStatus::Recovered => "↻",
                    };
                    println!(
                        "  {} {} — {} runs, {:.0}% fail rate",
                        status_icon, s.name, s.run_count, s.failure_rate * 100.0
                    );
                }
                let blacklisted: Vec<_> = tracker.blacklisted_tests();
                if !blacklisted.is_empty() {
                    println!("\nBlacklisted ({}):", blacklisted.len());
                    for name in &blacklisted {
                        println!("  ✗ {}", name);
                    }
                }
            }
        }

        "quality-gate" => {
            // Run quality gate check
            let gate = agentic_core::quality_gate::QualityGate::new();
            let score = gate.score(0.85, 1.0, 0.80);
            println!("Quality Gate Report");
            println!("  Composite:  {:.1}%", score.composite * 100.0);
            println!("  Eval:       {:.1}%", score.eval_score * 100.0);
            println!("  Tests:      {:.1}%", score.test_pass_rate * 100.0);
            println!("  Coverage:   {:.1}%", score.coverage * 100.0);
            println!("  Threshold:  {:.1}%", score.threshold * 100.0);
            println!("  Result:     {}", if score.passed { "PASS ✓" } else { "FAIL ✗" });

            // Generate pre-commit hook if requested
            if args.get(2).map(|s| s.as_str()) == Some("hook") {
                let hook = agentic_core::quality_gate::generate_precommit_hook();
                println!("\nPre-commit hook:\n{}", hook);
            }
        }

        "repl" => {
            agentic_core::repl::print_welcome();
            loop {
                print!("> ");
                use std::io::Write;
                std::io::stdout().flush().ok();
                let mut input = String::new();
                match std::io::stdin().read_line(&mut input) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let line = input.trim().to_string();
                        if line.is_empty() { continue; }
                        match agentic_core::repl::parse_command(&line) {
                            agentic_core::repl::ReplCommand::Run { workflow, args } => {
                                let task = args.join(" ");
                                let task = if task.is_empty() { format!("Execute {} workflow", workflow) } else { task };
                                println!("Running '{}' : {}", workflow, task);
                                let runner = app.create_runner();
                                match runner.run_workflow(&workflow, &task, serde_json::json!({})).await {
                                    Ok((id, result)) => {
                                        println!("Session: {}  Steps: {}  Eval: {:.2}  State: {}\n",
                                            id, result.steps_completed, result.total_eval_score, result.final_state);
                                    }
                                    Err(e) => eprintln!("Error: {}\n", e),
                                }
                            }
                            agentic_core::repl::ReplCommand::Tools => {
                                let tools = app.tool_provider.list_tools().await;
                                println!("Available tools ({}):", tools.len());
                                for t in &tools { println!("  {} - {}", t.name, t.description); }
                                println!();
                            }
                            agentic_core::repl::ReplCommand::Providers => {
                                match agentic_loop_llm::list_all_providers() {
                                    Ok(providers) => {
                                        for p in &providers {
                                            let avail = if p.available { "✓" } else { "✗" };
                                            println!("  {} {} ({})", avail, p.name, p.api);
                                        }
                                    }
                                    Err(e) => eprintln!("Error: {}\n", e),
                                }
                                println!();
                            }
                            agentic_core::repl::ReplCommand::State => {
                                println!("State: No active session\n");
                            }
                            agentic_core::repl::ReplCommand::Help => {
                                agentic_core::repl::print_help();
                            }
                            agentic_core::repl::ReplCommand::Quit => break,
                            agentic_core::repl::ReplCommand::Unknown(text) => {
                                // Unknown = run quick workflow
                                println!("Running quick: {}", text);
                                let runner = app.create_runner();
                                match runner.run_workflow("quick", &text, serde_json::json!({})).await {
                                    Ok((id, result)) => {
                                        println!("Session: {} | Steps: {} | Eval: {:.2} | State: {}\n",
                                            id, result.steps_completed, result.total_eval_score, result.final_state);
                                    }
                                    Err(e) => eprintln!("Error: {}\n", e),
                                }
                            }
                            _ => { /* Empty, etc */ }
                        }
                    }
                    Err(e) => { eprintln!("Error: {}", e); break; }
                }
            }
        }

        "help" | "--help" | "-h" | _ => {
            print_usage();
        }
    }

    Ok(())
}

/// Run a task in daemon context.
async fn run_daemon_task(
    project_id: &str,
    workflow: &str,
    task: &str,
    provider: &str,
    model: &str,
) -> Result<()> {
    // Create config (creates its own storage)
    let config = AppConfig {
        repo_path: ".".into(),
        workflows_path: "./examples/workflows".into(),
        default_provider: provider.to_string(),
        default_model: model.to_string(),
        project_id: project_id.to_string(),
        max_steps: 50,
    };
    let app = App::new(config)?;
    let runner = app.create_runner_with_llm(provider, model).await?;
    let (_session_id, result) = runner.run_workflow(workflow, task, serde_json::json!({})).await?;
    if !result.errors.is_empty() {
        anyhow::bail!("Task errors: {:?}", result.errors);
    }
    Ok(())
}

/// Poll storage for tasks matching type and status (multiple statuses).
async fn poll_for_tasks(
    storage: &Arc<dyn EntityStore>,
    project_id: &str,
    task_type: &str,
    task_statuses: &[String],
) -> Result<Vec<String>> {
    let filter = QueryFilter {
        entity_type: Some(task_type.to_string()),
        ..Default::default()
    };
    let result = storage.query(project_id, &filter).await?;

    let tasks: Vec<String> = result
        .entities
        .iter()
        .filter_map(|entity| {
            if let Some(status) = entity.get("status").and_then(|s| s.as_str()) {
                if task_statuses.iter().any(|s| s.as_str() == status) {
                    let desc = entity.get("description").and_then(|s| s.as_str()).unwrap_or("unknown");
                    return Some(desc.to_string());
                }
            }
            None
        })
        .collect();

    Ok(tasks)
}

/// Load scheduled tasks from storage.
async fn load_scheduled_tasks(
    storage: &Arc<dyn EntityStore>,
    project_id: &str,
) -> Result<Vec<ScheduledTask>> {
    let filter = QueryFilter {
        entity_type: Some("scheduled_task".to_string()),
        ..Default::default()
    };
    let result = storage.query(project_id, &filter).await?;

    let mut schedules = Vec::new();
    for entity in result.entities {
        if let Ok(schedule) = serde_json::from_value::<ScheduledTask>(entity) {
            schedules.push(schedule);
        }
    }
    Ok(schedules)
}

/// Save a scheduled task to storage.
async fn save_scheduled_task(
    storage: &Arc<dyn EntityStore>,
    project_id: &str,
    schedule: &ScheduledTask,
) -> Result<()> {
    let data = serde_json::to_vec(schedule)?;
    storage.store(project_id, "scheduled_task", &schedule.id, &data).await?;
    Ok(())
}

/// Git pull using git CLI.
async fn daemon_git_pull(repo_path: &std::path::Path) -> Result<()> {
    use tokio::process::Command;

    let output = Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git fetch failed: {}", stderr);
    }

    // Check if we can fast-forward
    let output = Command::new("git")
        .args(["merge-base", "--is-ancestor", "FETCH_HEAD", "HEAD"])
        .current_dir(repo_path)
        .output()
        .await?;

    if output.status.success() {
        tracing::info!("Git: already up to date");
        return Ok(());
    }

    // Fast-forward
    let output = Command::new("git")
        .args(["merge", "--ff-only", "FETCH_HEAD"])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git merge failed: {}", stderr);
    }

    tracing::info!("Git: fast-forwarded");
    Ok(())
}

fn main() {
    tracing_init::init();
    // hello
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let result = rt.block_on(run());
    tracing_init::shutdown();
    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
