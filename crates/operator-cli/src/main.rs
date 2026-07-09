//! Operator Jack — macOS-first CLI for deterministic computer automation.
//!
//! This is the binary entry point for the `operator-jack` command. It uses `clap`
//! derive for argument parsing and dispatches to command functions that
//! coordinate between `operator-core`, `operator-store`, and
//! `operator-runtime`.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use operator_core::config::{OperatorConfig, ProviderSettings};
use operator_core::types::{Mode, Plan, RunStatus};
use operator_core::validation::validate_plan;
use operator_planner::{
    Planner, PlannerConfig as PlannerPlannerConfig, ProviderConfig, ProviderType,
};
use operator_runtime::engine::{Engine, EngineConfig, RunSummary};
use operator_store::Store;

// ---------------------------------------------------------------------------
// CLI argument structures
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "operator-jack",
    version,
    about = "macOS-first CLI for deterministic computer automation"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Execution mode
    #[arg(long, global = true, default_value = "safe")]
    mode: String,

    /// Enable interactive prompts
    #[arg(long, global = true)]
    interactive: bool,

    /// Disable interactive prompts
    #[arg(long, global = true)]
    no_interactive: bool,

    /// Auto-approve all policy gates
    #[arg(long, global = true)]
    yes: bool,

    /// Simulate execution without side effects
    #[arg(long, global = true)]
    dry_run: bool,

    /// Path to macOS helper binary
    #[arg(long, global = true)]
    helper_path: Option<String>,

    /// Restrict to these apps (comma-separated)
    #[arg(long, global = true)]
    allow_apps: Option<String>,

    /// Restrict to these domains (comma-separated)
    #[arg(long, global = true)]
    allow_domains: Option<String>,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Quiet output (errors only)
    #[arg(short, long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Check environment, permissions, and dependencies
    Doctor,

    /// Plan management
    Plan {
        #[command(subcommand)]
        action: PlanAction,
    },

    /// Execute a saved plan by ID
    Exec {
        /// Plan ID (ULID)
        plan_id: String,
    },

    /// Validate, save, and execute a plan
    Run {
        /// Path to plan JSON file
        #[arg(long)]
        plan_file: Option<std::path::PathBuf>,

        /// Inline instruction or json:{...}
        instruction: Option<String>,

        /// Resume a paused plan run by its run-id (skips completed steps)
        #[arg(long)]
        resume: Option<String>,
    },

    /// Execute a natural language instruction
    Do {
        /// Natural language instruction (e.g., "open Notes and type hello")
        instruction: Vec<String>,

        /// LLM provider to use (kimi, openai, anthropic, ollama)
        #[arg(long)]
        provider: Option<String>,

        /// Show the generated plan without executing
        #[arg(long)]
        show_plan: bool,

        /// Save the generated plan to a file
        #[arg(long)]
        save_plan: Option<std::path::PathBuf>,
    },

    /// View run logs
    Logs {
        /// Run ID to inspect
        run_id: Option<String>,

        /// Show full JSONL log
        #[arg(long)]
        full: bool,

        /// Follow the run's event log live (tail -f), like `logs --follow <run-id>`
        #[arg(long)]
        follow: bool,

        /// Prune old runs from the store instead of viewing (retention)
        #[arg(long)]
        prune: bool,

        /// With --prune: delete runs older than this many days (default 30)
        #[arg(long, default_value = "30")]
        older_than_days: i64,

        /// With --prune: always keep at least this many most-recent runs (default 20)
        #[arg(long, default_value = "20")]
        keep: usize,
    },

    /// Stop a running execution
    Stop,

    /// Manage pending approvals for paused runs (M11)
    Approvals {
        #[command(subcommand)]
        action: ApprovalsAction,
    },

    /// Inject a steering message into a running agent (M11)
    Steer {
        /// The run-id of the agent to steer
        run_id: String,
        /// The guidance to inject at the agent's next turn
        message: String,
    },

    /// UI automation utilities
    Ui {
        #[command(subcommand)]
        action: UiAction,
    },

    /// Initialize config file at ~/.config/operator-jack/config.toml
    Init,

    /// Skill management (M6)
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },

    /// Browser automation (M5)
    Browser {
        #[command(subcommand)]
        action: BrowserAction,
    },

    /// Manage keychain secrets referenced in plans as `{{secret:name}}` (M8)
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },

    /// MCP server mode (M9) — expose gated executors as MCP tools over stdio
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },

    /// Autonomous agent (M10) — pursue a goal by observe→act→re-observe with an LLM
    Agent {
        /// The goal, e.g. "open Calculator and compute 2 + 3"
        #[arg(default_value = "")]
        instruction: String,
        /// Maximum observe→act iterations
        #[arg(long, default_value = "15")]
        max_iters: usize,
        /// LLM provider (kimi | openai | anthropic | ollama)
        #[arg(long)]
        provider: Option<String>,
        /// Path to a JSON grants file bounding the session
        #[arg(long)]
        grants: Option<std::path::PathBuf>,
        /// Resume a paused agent run by its run-id (after approving its request)
        #[arg(long)]
        resume: Option<String>,
    },
}

#[derive(Subcommand)]
enum ApprovalsAction {
    /// List approvals (pending by default; --all for every status)
    List {
        /// Show approvals of every status, not just pending
        #[arg(long)]
        all: bool,
    },
    /// Approve a pending request (allows the escalating action once on resume)
    Approve {
        /// Approval id (from `approvals list`)
        id: String,
    },
    /// Deny a pending request
    Deny {
        /// Approval id (from `approvals list`)
        id: String,
    },
}

#[derive(Subcommand)]
enum McpAction {
    /// Serve the MCP protocol on stdio (connect an MCP client to this process).
    Serve {
        /// Path to a JSON file with the session's grants envelope
        /// (e.g. {"fs_write":["~/scraped/**"],"exec":["pip install *"]}).
        #[arg(long)]
        grants: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum SecretAction {
    /// Store (or update) a secret. The value is read from stdin (or --value).
    Set {
        /// Secret name (referenced in plans as `{{secret:name}}`).
        name: String,
        /// Value (omit to read from stdin, which avoids shell history).
        #[arg(long)]
        value: Option<String>,
    },
    /// List stored secret names (never their values).
    List,
    /// Remove a secret.
    Rm {
        /// Secret name.
        name: String,
    },
}

#[derive(Subcommand)]
enum UiAction {
    /// Inspect the accessibility tree of an application
    Inspect {
        /// Application name
        #[arg(long)]
        app: String,

        /// Maximum tree depth (default 5)
        #[arg(long, default_value = "5")]
        depth: u32,
    },
}

#[derive(Subcommand)]
enum PlanAction {
    /// Validate a plan file
    Validate {
        #[arg(long)]
        plan_file: std::path::PathBuf,
    },

    /// Save a plan to the store
    Save {
        #[arg(long)]
        plan_file: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
enum SkillAction {
    /// List available skills
    List,

    /// Validate a skill manifest file
    Validate {
        /// Path to skill manifest file
        skill_file: std::path::PathBuf,
    },

    /// Run a skill with parameters
    Run {
        /// Skill name
        skill_name: String,

        /// Parameters as key=value pairs
        #[arg(long = "param", value_parser = parse_key_value)]
        params: Vec<(String, String)>,
    },
}

#[derive(Subcommand)]
enum BrowserAction {
    /// Check if Chrome is available
    Doctor,

    /// Launch Chrome with remote debugging
    Launch {
        /// Port for remote debugging (default: 9222)
        #[arg(long, default_value = "9222")]
        port: u16,
    },

    /// Navigate to a URL
    Navigate {
        /// URL to navigate to
        url: String,

        /// Chrome debugging port
        #[arg(long, default_value = "9222")]
        port: u16,
    },

    /// Take a screenshot
    Screenshot {
        /// Output file path
        #[arg(long)]
        output: std::path::PathBuf,

        /// Capture full page
        #[arg(long)]
        full_page: bool,

        /// Chrome debugging port
        #[arg(long, default_value = "9222")]
        port: u16,
    },
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing based on --verbose / --quiet flags.
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else if cli.quiet {
        EnvFilter::new("error")
    } else {
        EnvFilter::new("info")
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    // Resolve standard paths.
    let data_dir = resolve_data_dir();
    let log_dir = resolve_log_dir();

    // Ensure data and log directories exist.
    fs::create_dir_all(&data_dir)
        .with_context(|| format!("failed to create data dir: {}", data_dir.display()))?;
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create log dir: {}", log_dir.display()))?;

    // Dispatch to command handlers.
    match &cli.command {
        Commands::Doctor => cmd_doctor(&cli),
        Commands::Plan { ref action } => match action {
            PlanAction::Validate { ref plan_file } => cmd_plan_validate(&cli, plan_file),
            PlanAction::Save { ref plan_file } => cmd_plan_save(&cli, plan_file),
        },
        Commands::Exec { ref plan_id } => cmd_exec(&cli, plan_id),
        Commands::Run {
            ref plan_file,
            ref instruction,
            ref resume,
        } => {
            if let Some(run_id) = resume {
                cmd_run_resume(&cli, run_id)
            } else {
                cmd_run(&cli, plan_file.clone(), instruction.clone())
            }
        }
        Commands::Do {
            ref instruction,
            ref provider,
            show_plan,
            ref save_plan,
        } => cmd_do(
            &cli,
            instruction.join(" "),
            provider.clone(),
            *show_plan,
            save_plan.clone(),
        ),
        Commands::Logs {
            ref run_id,
            full,
            follow,
            prune,
            older_than_days,
            keep,
        } => {
            // Accept `logs prune` / `logs follow` as verbs (first positional), in
            // addition to the `--prune` / `--follow` flags.
            let verb = run_id.as_deref();
            if *prune || verb == Some("prune") {
                cmd_logs_prune(&cli, *older_than_days, *keep)
            } else if verb == Some("follow") {
                cmd_logs_follow(&cli, None)
            } else if *follow {
                cmd_logs_follow(&cli, run_id.clone())
            } else {
                cmd_logs(&cli, run_id.clone(), *full)
            }
        }
        Commands::Stop => cmd_stop(&cli),
        Commands::Approvals { ref action } => match action {
            ApprovalsAction::List { all } => cmd_approvals_list(&cli, *all),
            ApprovalsAction::Approve { id } => cmd_approvals_decide(&cli, id, true),
            ApprovalsAction::Deny { id } => cmd_approvals_decide(&cli, id, false),
        },
        Commands::Steer {
            ref run_id,
            ref message,
        } => cmd_steer(&cli, run_id, message),
        Commands::Ui { ref action } => match action {
            UiAction::Inspect { ref app, depth } => cmd_ui_inspect(&cli, app, *depth),
        },
        Commands::Init => cmd_init(&cli),
        Commands::Skill { ref action } => match action {
            SkillAction::List => cmd_skill_list(&cli),
            SkillAction::Validate { ref skill_file } => cmd_skill_validate(&cli, skill_file),
            SkillAction::Run {
                ref skill_name,
                ref params,
            } => cmd_skill_run(&cli, skill_name, params),
        },
        Commands::Browser { ref action } => match action {
            BrowserAction::Doctor => cmd_browser_doctor(&cli),
            BrowserAction::Launch { port } => cmd_browser_launch(&cli, *port),
            BrowserAction::Navigate { url, port } => cmd_browser_navigate(&cli, url, *port),
            BrowserAction::Screenshot {
                output,
                full_page,
                port,
            } => cmd_browser_screenshot(&cli, output, *full_page, *port),
        },
        Commands::Secret { ref action } => match action {
            SecretAction::Set { name, value } => cmd_secret_set(&cli, name, value.as_deref()),
            SecretAction::List => cmd_secret_list(&cli),
            SecretAction::Rm { name } => cmd_secret_rm(&cli, name),
        },
        Commands::Mcp { ref action } => match action {
            McpAction::Serve { grants } => cmd_mcp_serve(&cli, grants.as_deref()),
        },
        Commands::Agent {
            ref instruction,
            max_iters,
            ref provider,
            ref grants,
            ref resume,
        } => cmd_agent(
            &cli,
            instruction,
            *max_iters,
            provider.clone(),
            grants.as_deref(),
            resume.as_deref(),
        ),
    }
}

// ---------------------------------------------------------------------------
// cmd_agent — autonomous agent loop (M10)
// ---------------------------------------------------------------------------

/// Runs the native agent loop: an LLM proposes one action at a time, each
/// executed through a gated `RunSession`, observing results until done or the
/// iteration budget is hit. Like MCP mode, it auto-approves confirmations and
/// relies on the grant envelope + escalation classifier for safety.
#[allow(clippy::too_many_arguments)]
fn cmd_agent(
    cli: &Cli,
    instruction: &str,
    max_iters: usize,
    provider_override: Option<String>,
    grants_file: Option<&Path>,
    resume: Option<&str>,
) -> Result<()> {
    if let Some(run_id) = resume {
        return cmd_agent_resume(cli, run_id, max_iters, provider_override);
    }

    use operator_core::grants::Grants;
    use operator_planner::{Planner, ProviderType};

    if instruction.trim().is_empty() {
        eprintln!(
            "Error: provide a goal, e.g. operator-jack agent \"open Calculator and compute 2 + 3\""
        );
        std::process::exit(6);
    }

    let cfg = OperatorConfig::load().unwrap_or_default();
    let planner = Planner::new(build_planner_config(&cfg));

    // Resolve provider: explicit override, else first available, else Ollama.
    let provider_type = match provider_override {
        Some(p) => p
            .parse::<ProviderType>()
            .map_err(|e| anyhow::anyhow!("{e}"))?,
        None => planner
            .first_available_provider()
            .unwrap_or(ProviderType::Ollama),
    };
    let provider = planner
        .create_provider(provider_type)
        .map_err(|e| anyhow::anyhow!("failed to initialize provider: {e}"))?;

    // Autonomous, non-interactive posture (same as MCP): safety via grants +
    // escalation classifier, not per-step confirmation.
    let mut config = build_engine_config(cli)?;
    config.yes_to_all = true;
    config.interactive = false;

    let session_grants: Option<Grants> = if let Some(path) = grants_file {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read grants file {}", path.display()))?;
        Some(serde_json::from_str(&text).with_context(|| "failed to parse grants file")?)
    } else {
        config.grants.take()
    };
    config.grants = None;

    eprintln!("operator-jack agent — goal: {instruction}");
    eprintln!("Provider: {}", provider_type.display_name());
    print_grant_preview(session_grants.as_ref());

    let store = open_store()?;
    let session = operator_runtime::engine::RunSession::open(store, config, session_grants, None)
        .map_err(|e| anyhow::anyhow!("failed to open agent session: {e}"))?;

    drive_agent(
        cli,
        provider.as_ref(),
        session,
        instruction,
        max_iters,
        Vec::new(),
    )
}

/// Resumes a paused agent run: verifies its approval was granted, re-opens a
/// session under the same grant envelope (with a one-shot escalation allowance
/// if the pause was an escalation), re-seeds the model with the saved
/// transcript, and continues the loop.
fn cmd_agent_resume(
    cli: &Cli,
    run_id: &str,
    max_iters: usize,
    provider_override: Option<String>,
) -> Result<()> {
    use operator_core::types::{ApprovalKind, ApprovalStatus, CheckpointKind};
    use operator_planner::{Planner, ProviderType};

    let store = open_store()?;
    let cp = store
        .load_checkpoint(run_id)
        .context("failed to read checkpoint")?
        .ok_or_else(|| {
            anyhow::anyhow!("no paused run with id {run_id} (no checkpoint on record)")
        })?;
    if cp.kind != CheckpointKind::Agent {
        bail!("run {run_id} is a plan run — resume it with `operator-jack run --resume {run_id}`");
    }

    // Gate on the approval decision.
    let approval = store
        .latest_approval_for_run(run_id)
        .context("failed to read approvals")?
        .ok_or_else(|| anyhow::anyhow!("no approval on record for run {run_id}"))?;
    match approval.status {
        ApprovalStatus::Pending => bail!(
            "run {run_id} is still awaiting approval — run `operator-jack approvals approve {}` first",
            approval.id
        ),
        ApprovalStatus::Denied => bail!("the approval for run {run_id} was denied; not resuming"),
        ApprovalStatus::Approved => {}
    }

    // Rebuild the session envelope from the paused run's plan row.
    let old_run = store.get_run(run_id).context("failed to load paused run")?;
    let (_pid, old_plan) = store
        .get_plan(&old_run.plan_id)
        .context("failed to load paused run's plan")?;

    let cfg = OperatorConfig::load().unwrap_or_default();
    let planner = Planner::new(build_planner_config(&cfg));
    let provider_type = match provider_override {
        Some(p) => p
            .parse::<ProviderType>()
            .map_err(|e| anyhow::anyhow!("{e}"))?,
        None => planner
            .first_available_provider()
            .unwrap_or(ProviderType::Ollama),
    };
    let provider = planner
        .create_provider(provider_type)
        .map_err(|e| anyhow::anyhow!("failed to initialize provider: {e}"))?;

    let mut config = build_engine_config(cli)?;
    config.yes_to_all = true;
    config.interactive = false;
    if let Some(mode) = old_plan.mode.clone() {
        config.mode = mode;
    }
    config.grants = None;

    let goal = cp.goal.clone().unwrap_or_default();
    let transcript = cp.transcript.clone().unwrap_or_default();

    eprintln!("operator-jack agent — resuming run {run_id}");
    eprintln!("Provider: {}", provider_type.display_name());
    print_grant_preview(old_plan.grants.as_ref());

    let store2 = open_store()?;
    let session =
        operator_runtime::engine::RunSession::open(store2, config, old_plan.grants.clone(), None)
            .map_err(|e| anyhow::anyhow!("failed to open resume session: {e}"))?;

    // A one-shot escalation allowance so the approved action goes through once.
    if approval.kind == ApprovalKind::Escalation {
        session.grant_one_escalation();
    }

    // Consume the checkpoint so the same pause can't be resumed twice.
    let _ = store.delete_checkpoint(run_id);

    drive_agent(
        cli,
        provider.as_ref(),
        session,
        &goal,
        max_iters,
        transcript,
    )
}

/// Shared agent driver used by both fresh runs and resumes: writes the PID file,
/// runs the supervised loop, and either abandons (if paused) or finishes the
/// session, then prints the outcome.
fn drive_agent(
    cli: &Cli,
    provider: &dyn operator_planner::provider::LlmProvider,
    mut session: operator_runtime::engine::RunSession,
    goal: &str,
    max_iters: usize,
    initial_transcript: Vec<String>,
) -> Result<()> {
    use operator_agent::AgentStatus;

    let run_id = session.run_id().to_string();
    let data_dir = resolve_data_dir();
    let _ = write_pid_file(&data_dir, &run_id);

    let mut supervisor = CliSupervisor::new(resolve_log_dir());
    let outcome = operator_agent::run_agent_supervised(
        goal,
        provider,
        &mut session,
        max_iters,
        &mut supervisor,
        initial_transcript,
    );

    // On pause the supervisor has already set status=Paused and checkpointed;
    // don't overwrite it with a terminal status.
    let paused = outcome.status == AgentStatus::Paused;
    let steps_total = if paused {
        session.abandon();
        0
    } else {
        session.finish().steps_total
    };
    let _ = remove_pid_file(&data_dir);

    if cli.json {
        let obj = serde_json::json!({
            "run_id": run_id,
            "status": format!("{:?}", outcome.status),
            "iterations": outcome.iterations,
            "summary": outcome.summary,
            "steps": steps_total,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!();
        println!("Agent finished: {:?}", outcome.status);
        println!("  Iterations: {}", outcome.iterations);
        println!("  Summary:    {}", outcome.summary);
        println!("  Run:        {run_id}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CliSupervisor — store-backed AgentSupervisor (M11)
// ---------------------------------------------------------------------------

/// The CLI's implementation of the agent supervisor: persists pause checkpoints
/// and approvals to the store, drains steer messages from a per-run file, and
/// raises a macOS notification on pause.
struct CliSupervisor {
    store: Store,
    log_dir: PathBuf,
}

impl CliSupervisor {
    fn new(log_dir: PathBuf) -> Self {
        // A dedicated store connection (the session owns its own); both point at
        // the same WAL database.
        let store = open_store().expect("failed to open supervisor store");
        Self { store, log_dir }
    }

    fn steer_path(&self, run_id: &str) -> PathBuf {
        self.log_dir.join(format!("{run_id}.steer"))
    }
}

impl operator_agent::AgentSupervisor for CliSupervisor {
    fn pause(
        &mut self,
        run_id: &str,
        kind: operator_agent::PauseKind,
        cp: &operator_agent::AgentCheckpoint,
    ) -> String {
        use operator_core::types::{ApprovalKind, Checkpoint, CheckpointKind};

        let plan_id = self
            .store
            .get_run(run_id)
            .map(|r| r.plan_id)
            .unwrap_or_default();
        let checkpoint = Checkpoint {
            run_id: run_id.to_string(),
            plan_id,
            kind: CheckpointKind::Agent,
            cursor: serde_json::json!({ "iteration": cp.iteration }),
            variables: None,
            transcript: Some(cp.transcript.to_vec()),
            goal: Some(cp.goal.to_string()),
            reason: cp.reason.to_string(),
        };
        let _ = self.store.save_checkpoint(&checkpoint);
        let akind = match kind {
            operator_agent::PauseKind::Escalation => ApprovalKind::Escalation,
            operator_agent::PauseKind::Takeover => ApprovalKind::Takeover,
        };
        let id = self
            .store
            .create_approval(run_id, akind, cp.reason)
            .unwrap_or_default();
        let _ = self
            .store
            .update_run_status(run_id, &RunStatus::Paused, None);
        id
    }

    fn poll_steer(&mut self, run_id: &str) -> Vec<String> {
        let path = self.steer_path(run_id);
        let Ok(contents) = fs::read_to_string(&path) else {
            return Vec::new();
        };
        if contents.trim().is_empty() {
            return Vec::new();
        }
        // Drain: truncate the file so each message is injected exactly once.
        let _ = fs::write(&path, "");
        contents
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn notify(&self, title: &str, body: &str) {
        notify_user(title, body);
    }
}

// ---------------------------------------------------------------------------
// Approvals + steering + retention (M11)
// ---------------------------------------------------------------------------

/// Lists approvals (pending by default; `--all` for every status).
fn cmd_approvals_list(cli: &Cli, all: bool) -> Result<()> {
    use operator_core::types::ApprovalStatus;

    let store = open_store()?;
    let filter = if all {
        None
    } else {
        Some(ApprovalStatus::Pending)
    };
    let approvals = store
        .list_approvals(filter)
        .context("failed to list approvals")?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&approvals)?);
        return Ok(());
    }

    if approvals.is_empty() {
        println!("No {} approvals.", if all { "recorded" } else { "pending" });
        return Ok(());
    }

    println!(
        "{:<28}  {:<10}  {:<10}  REASON",
        "APPROVAL ID", "KIND", "STATUS"
    );
    for a in &approvals {
        println!(
            "{:<28}  {:<10}  {:<10}  {}",
            a.id,
            format!("{:?}", a.kind).to_lowercase(),
            format!("{:?}", a.status).to_lowercase(),
            truncate_str(&a.summary, 60),
        );
    }
    println!();
    println!("Approve with:  operator-jack approvals approve <id>");
    println!("Then resume:   operator-jack agent --resume <run-id>   (run-id shown in the pause message)");
    Ok(())
}

/// Approves or denies a pending approval.
fn cmd_approvals_decide(cli: &Cli, id: &str, approve: bool) -> Result<()> {
    use operator_core::types::ApprovalStatus;

    let store = open_store()?;
    let existing = store
        .get_approval(id)
        .context("failed to read approval")?
        .ok_or_else(|| anyhow::anyhow!("no approval with id {id}"))?;
    if existing.status != ApprovalStatus::Pending {
        bail!(
            "approval {id} is already {:?} — nothing to do",
            existing.status
        );
    }

    let new_status = if approve {
        ApprovalStatus::Approved
    } else {
        ApprovalStatus::Denied
    };
    store
        .decide_approval(id, new_status)
        .context("failed to update approval")?;

    if cli.json {
        let obj = serde_json::json!({
            "id": id,
            "run_id": existing.run_id,
            "status": if approve { "approved" } else { "denied" },
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else if approve {
        println!("Approved {id}.");
        println!(
            "Resume the run with:  operator-jack agent --resume {}",
            existing.run_id
        );
    } else {
        println!("Denied {id}. The paused run will not resume.");
    }
    Ok(())
}

/// Injects a steering message into a running agent: appends to a per-run file
/// the agent drains at the start of its next turn.
fn cmd_steer(cli: &Cli, run_id: &str, message: &str) -> Result<()> {
    use std::io::Write as _;

    let path = resolve_log_dir().join(format!("{run_id}.steer"));
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open steer file {}", path.display()))?;
    writeln!(f, "{}", message.replace('\n', " "))
        .with_context(|| "failed to write steer message")?;

    if cli.json {
        println!(
            "{}",
            serde_json::json!({ "run_id": run_id, "queued": message })
        );
    } else {
        println!("Steering message queued for run {run_id}; it will be injected at the next turn.");
    }
    Ok(())
}

/// Prunes old runs from the store (retention). Deletes runs older than
/// `older_than_days`, always keeping at least `keep` of the most recent.
fn cmd_logs_prune(cli: &Cli, older_than_days: i64, keep: usize) -> Result<()> {
    let store = open_store()?;
    let cutoff = chrono::Utc::now() - chrono::Duration::days(older_than_days.max(0));
    let pruned = store
        .prune_runs(cutoff, keep)
        .context("failed to prune runs")?;

    if cli.json {
        println!(
            "{}",
            serde_json::json!({
                "pruned": pruned,
                "older_than_days": older_than_days,
                "kept_min": keep,
            })
        );
    } else {
        println!(
            "Pruned {pruned} run(s) older than {older_than_days} day(s) (kept the {keep} most recent)."
        );
    }
    Ok(())
}

/// Follows a run's event log live (tail -f). Resolves the newest run if no id is
/// given, prints existing lines, then polls for new ones until interrupted or
/// the run reaches a terminal status.
fn cmd_logs_follow(_cli: &Cli, run_id: Option<String>) -> Result<()> {
    let log_dir = resolve_log_dir();
    let store = open_store()?;

    let rid = match run_id {
        Some(r) => r,
        None => store
            .list_runs(1)
            .ok()
            .and_then(|runs| runs.into_iter().next())
            .map(|r| r.id)
            .ok_or_else(|| anyhow::anyhow!("no runs found to follow"))?,
    };
    let log_path = log_dir.join(format!("{rid}.jsonl"));

    // Trap Ctrl-C so following stops cleanly.
    let stop = Arc::new(AtomicBool::new(false));
    let _ = set_ctrlc_handler(Arc::clone(&stop));

    eprintln!("Following run {rid} (Ctrl-C to stop)…");
    let mut pos: u64 = 0;
    let mut missing_ticks = 0;
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match fs::File::open(&log_path) {
            Ok(mut f) => {
                use std::io::{Read, Seek, SeekFrom};
                let _ = f.seek(SeekFrom::Start(pos));
                let mut buf = String::new();
                if f.read_to_string(&mut buf).is_ok() && !buf.is_empty() {
                    pos += buf.len() as u64;
                    for line in buf.lines() {
                        print_log_event_line(line);
                    }
                }
            }
            Err(_) => {
                missing_ticks += 1;
                if missing_ticks > 20 {
                    eprintln!("(no log file for run {rid} yet: {})", log_path.display());
                    missing_ticks = 0;
                }
            }
        }
        // Stop once the run is terminal and we've drained the file.
        if let Ok(run) = store.get_run(&rid) {
            if is_terminal_status(&run.status) {
                // One last drain already happened above.
                break;
            }
        }
        thread::sleep(Duration::from_millis(400));
    }
    Ok(())
}

/// True if a run status is terminal (won't produce more events).
fn is_terminal_status(status: &RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Succeeded
            | RunStatus::CompletedWithErrors
            | RunStatus::Failed
            | RunStatus::Cancelled
            | RunStatus::Partial
    )
}

/// Pretty-prints one JSONL event line for `logs --follow`.
fn print_log_event_line(line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(v) => {
            let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");
            let etype = v
                .get("event_type")
                .and_then(|t| t.as_str())
                .unwrap_or("event");
            let payload = v
                .get("payload_json")
                .or_else(|| v.get("payload"))
                .map(|p| p.to_string())
                .unwrap_or_default();
            println!("{ts}  {etype}  {}", truncate_str(&payload, 120));
        }
        Err(_) => println!("{line}"),
    }
}

/// Raises a macOS notification via `osascript` (best-effort; ignored on failure
/// or non-macOS).
fn notify_user(title: &str, body: &str) {
    // Escape double quotes for the AppleScript string literals.
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        esc(body),
        esc(title)
    );
    let _ = Command::new("osascript").arg("-e").arg(script).output();
}

// ---------------------------------------------------------------------------
// cmd_mcp_serve — MCP server over stdio (M9)
// ---------------------------------------------------------------------------

/// Serves the MCP protocol on stdio. An MCP client drives Operator Jack's
/// executors as tools; every tool call runs through a `RunSession` (the same
/// grants / escalation / redaction / audit as plan execution).
///
/// Because stdio is the protocol channel, there is no interactive terminal for
/// confirmation prompts — so the session auto-approves confirmations and relies
/// on **grants + the escalation classifier + secure-field refusal** for safety.
/// All diagnostics and the grant preview go to stderr; stdout is protocol-only.
fn cmd_mcp_serve(cli: &Cli, grants_file: Option<&Path>) -> Result<()> {
    use operator_core::grants::Grants;

    let mut config = build_engine_config(cli)?;
    // No interactive confirmations are possible over stdio; safety comes from
    // the grant envelope + escalation classifier, not per-step prompts.
    config.yes_to_all = true;
    config.interactive = false;

    // Resolve the session grant envelope: --grants file overrides config grants.
    let session_grants: Option<Grants> = if let Some(path) = grants_file {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read grants file {}", path.display()))?;
        let g: Grants = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse grants file {}", path.display()))?;
        Some(g)
    } else {
        config.grants.take()
    };
    // Avoid double-applying: the session grants are the single envelope layer.
    config.grants = None;

    // Grant preview (to stderr — stdout is the protocol channel).
    eprintln!("operator-jack MCP server (protocol on stdio)");
    print_grant_preview(session_grants.as_ref());
    eprintln!("  Escalation rules and secure-field refusal always apply.");

    let store = open_store()?;
    let session = operator_runtime::engine::RunSession::open(store, config, session_grants, None)
        .map_err(|e| anyhow::anyhow!("failed to open MCP session: {e}"))?;
    operator_mcp::serve(session).context("MCP server error")?;
    Ok(())
}

/// Prints the effective grant envelope to stderr so the operator sees what the
/// session may do before tools unlock.
fn print_grant_preview(grants: Option<&operator_core::grants::Grants>) {
    match grants {
        Some(g) if !g.is_unrestricted() => {
            eprintln!("This session may:");
            let show = |label: &str, pats: &Option<Vec<String>>| {
                if let Some(p) = pats {
                    eprintln!("  {label}: {}", p.join(", "));
                }
            };
            show("fs_read ", &g.fs_read);
            show("fs_write", &g.fs_write);
            show("exec    ", &g.exec);
            show("domains ", &g.domains);
            eprintln!("  (a capability with no grant listed above is unrestricted)");
        }
        _ => {
            eprintln!(
                "⚠  UNRESTRICTED session: no grants set — file reads/writes, commands, and any \
                 domain are allowed."
            );
            eprintln!("   Pass --grants <file> to bound this session.");
        }
    }
}

// ---------------------------------------------------------------------------
// cmd_secret_* — keychain secret management (M8)
// ---------------------------------------------------------------------------

/// Stores a secret. Reads the value from `--value` or stdin (so it stays out of
/// shell history). Never echoes the value.
fn cmd_secret_set(cli: &Cli, name: &str, value: Option<&str>) -> Result<()> {
    use std::io::{Read, Write};
    let value = match value {
        Some(v) => v.to_string(),
        None => {
            if atty_is_interactive() {
                eprint!("Enter value for secret '{name}' (input hidden not guaranteed): ");
                let _ = std::io::stderr().flush();
            }
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read secret value from stdin")?;
            buf.trim_end_matches(['\n', '\r']).to_string()
        }
    };
    if value.is_empty() {
        anyhow::bail!("refusing to store an empty secret value");
    }
    operator_core::secrets::set_secret(name, &value)
        .map_err(|e| anyhow::anyhow!("failed to store secret: {e}"))?;
    if !cli.quiet {
        eprintln!("Stored secret '{name}' in the keychain. Reference it in plans as {{{{secret:{name}}}}}.");
    }
    Ok(())
}

fn cmd_secret_list(cli: &Cli) -> Result<()> {
    let names = operator_core::secrets::list_secrets();
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "secrets": names }))?
        );
    } else if names.is_empty() {
        println!("No secrets stored. Add one with: operator-jack secret set <name>");
    } else {
        println!("Stored secrets ({}):", names.len());
        for n in &names {
            println!("  - {n}");
        }
    }
    Ok(())
}

fn cmd_secret_rm(cli: &Cli, name: &str) -> Result<()> {
    operator_core::secrets::delete_secret(name)
        .map_err(|e| anyhow::anyhow!("failed to remove secret: {e}"))?;
    if !cli.quiet {
        eprintln!("Removed secret '{name}'.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// cmd_doctor — environment health check
// ---------------------------------------------------------------------------

/// Checks that the environment is correctly configured:
/// - SQLite store can be opened
/// - Log directory exists and is writable
/// - Helper binary is reachable
/// - Accessibility permission is granted (if helper is available)
fn cmd_doctor(cli: &Cli) -> Result<()> {
    let db_path = resolve_db_path();
    let log_dir = resolve_log_dir();

    // 1. SQLite store
    let sqlite_ok = Store::open(&db_path).is_ok();
    let sqlite_status = if sqlite_ok { "OK" } else { "FAIL" };

    // 2. Log directory
    let log_ok = log_dir.is_dir() && is_dir_writable(&log_dir);
    let log_status = if log_ok { "OK" } else { "FAIL" };

    // 3. Helper binary
    let helper_path = resolve_helper_path(cli);
    let helper_found = helper_path.is_some();
    let helper_status = if helper_found { "OK" } else { "NOT FOUND" };

    // 4. Accessibility + Screen Recording permissions (require helper)
    let (accessibility_status, screen_recording_status) = if let Some(ref hp) = helper_path {
        let mut client =
            operator_ipc::client::HelperClient::new(Some(hp.to_string_lossy().to_string()));
        match client.connect() {
            Ok(()) => {
                let ax = client.send(
                    "ui.check_accessibility_permission",
                    serde_json::json!({"prompt": true}),
                );
                let sr = client.send(
                    "ui.check_screen_recording_permission",
                    serde_json::json!({}),
                );
                client.disconnect();
                let ax_status = match ax {
                    Ok(val) => {
                        if val
                            .get("trusted")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            "GRANTED"
                        } else {
                            "NOT GRANTED"
                        }
                    }
                    Err(e) => {
                        tracing::warn!("accessibility check failed: {}", e);
                        "CHECK FAILED"
                    }
                };
                let sr_status = match sr {
                    Ok(val) => {
                        if val
                            .get("granted")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            "GRANTED"
                        } else {
                            "NOT GRANTED"
                        }
                    }
                    Err(e) => {
                        tracing::warn!("screen recording check failed: {}", e);
                        "CHECK FAILED"
                    }
                };
                (ax_status, sr_status)
            }
            Err(e) => {
                tracing::warn!("helper connection failed: {}", e);
                ("CHECK FAILED", "CHECK FAILED")
            }
        }
    } else {
        ("SKIPPED (helper not found)", "SKIPPED (helper not found)")
    };

    // 5. Planner providers (readiness of the `do` natural-language command).
    let planner = operator_planner::Planner::new(operator_planner::PlannerConfig::default());
    let providers = planner.available_providers();
    let do_ready = providers.iter().any(|(_, ready)| *ready);

    // Detect first run (no config file exists).
    let config_path = OperatorConfig::default_path();
    let is_first_run = config_path.as_ref().is_none_or(|p| !p.exists());

    // Detect terminal app for targeted accessibility instructions.
    let terminal_app = std::env::var("TERM_PROGRAM").ok();
    let terminal_display = match terminal_app.as_deref() {
        Some("iTerm.app") => "iTerm2",
        Some("Apple_Terminal") => "Terminal.app",
        Some("vscode") => "Visual Studio Code",
        Some("WarpTerminal") => "Warp",
        Some(other) => other,
        None => "your terminal app",
    };

    if cli.json {
        let obj = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "sqlite": sqlite_status,
            "log_dir": log_status,
            "helper": helper_status,
            "accessibility": match accessibility_status {
                "GRANTED" => "granted",
                "NOT GRANTED" => "not_granted",
                _ => "skipped",
            },
            "screen_recording": match screen_recording_status {
                "GRANTED" => "granted",
                "NOT GRANTED" => "not_granted",
                _ => "skipped",
            },
            "db_path": db_path.display().to_string(),
            "log_dir_path": log_dir.display().to_string(),
            "first_run": is_first_run,
            "planner": {
                "do_ready": do_ready,
                "providers": providers
                    .iter()
                    .map(|(pt, ready)| (pt.display_name().to_string(), serde_json::json!(ready)))
                    .collect::<serde_json::Map<_, _>>(),
            },
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        if is_first_run {
            println!("Welcome to Operator Jack!");
            println!("========================");
            println!();
        } else {
            println!("Operator Jack Doctor");
            println!("====================");
        }

        println!("Version:       {}", env!("CARGO_PKG_VERSION"));
        println!("SQLite:        {}", sqlite_status);
        println!("Log dir:       {}", log_status);
        println!("Helper:        {}", helper_status);
        println!("Accessibility: {}", accessibility_status);
        println!("Screen Rec.:   {}", screen_recording_status);
        println!(
            "Planner (do):  {}",
            if do_ready { "READY" } else { "NOT CONFIGURED" }
        );
        for (pt, ready) in &providers {
            let mark = if *ready { "✓" } else { "✗" };
            let hint = if *ready {
                String::new()
            } else if pt.requires_api_key() {
                format!(" — set {}", pt.api_key_env_var())
            } else {
                " — start Ollama and pull a model".to_string()
            };
            println!("               {} {}{}", mark, pt.display_name(), hint);
        }

        if !do_ready {
            println!();
            println!("  `operator-jack do` needs an LLM provider. Either:");
            println!("    - export ANTHROPIC_API_KEY / OPENAI_API_KEY / KIMI_API_KEY, or");
            println!("    - run Ollama locally and `ollama pull llama3.2`");
        }

        if accessibility_status == "NOT GRANTED" {
            println!();
            println!("  To grant accessibility access:");
            println!("  1. Open System Settings > Privacy & Security > Accessibility");
            println!("  2. Click '+' and add: {}", terminal_display);
            println!(
                "  3. Restart {} and run: operator-jack doctor",
                terminal_display
            );
        }

        if !helper_found {
            println!();
            println!("  Helper binary not found. Install it:");
            println!("    brew install rajkum2/tap/operator-jack");
            println!("  Or build from source:");
            println!("    cd macos-helper && swift build -c release");
            println!("    cp .build/release/operator-macos-helper /usr/local/bin/");
        }

        if is_first_run {
            println!();
            println!("  Quick start:");
            println!("    operator-jack init                  # Create config file");
            println!("    operator-jack run --plan-file docs/examples/open-app.json --yes");
        }

        println!();
        println!("DB path:  {}", db_path.display());
        println!("Log dir:  {}", log_dir.display());
        if let Some(ref cp) = config_path {
            println!(
                "Config:   {}{}",
                cp.display(),
                if is_first_run {
                    " (not yet created)"
                } else {
                    ""
                }
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// cmd_plan_validate — validate a plan file
// ---------------------------------------------------------------------------

/// Reads a plan JSON file from disk, parses it, and runs the validation
/// suite. Prints validation errors and exits with code 2 on failure.
fn cmd_plan_validate(cli: &Cli, plan_file: &Path) -> Result<()> {
    let plan = read_plan_file(plan_file)?;

    match validate_plan(&plan) {
        Ok(()) => {
            let step_count = plan.steps.len();
            if cli.json {
                let obj = serde_json::json!({
                    "valid": true,
                    "name": plan.name,
                    "step_count": step_count,
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                println!("Plan is valid: {} ({} steps)", plan.name, step_count);
            }
            Ok(())
        }
        Err(errors) => {
            if cli.json {
                let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
                let obj = serde_json::json!({
                    "valid": false,
                    "errors": msgs,
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                eprintln!("Validation failed with {} error(s):", errors.len());
                for err in &errors {
                    eprintln!("  - {}", err);
                }
            }
            std::process::exit(2);
        }
    }
}

// ---------------------------------------------------------------------------
// cmd_plan_save — validate and persist a plan
// ---------------------------------------------------------------------------

/// Reads a plan from disk, validates it, opens the store, and saves the plan.
/// Prints the generated plan ID on success.
fn cmd_plan_save(cli: &Cli, plan_file: &Path) -> Result<()> {
    let plan = read_plan_file(plan_file)?;

    // Validate before saving.
    if let Err(errors) = validate_plan(&plan) {
        if cli.json {
            let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            let obj = serde_json::json!({ "valid": false, "errors": msgs });
            println!("{}", serde_json::to_string_pretty(&obj)?);
        } else {
            eprintln!("Validation failed with {} error(s):", errors.len());
            for err in &errors {
                eprintln!("  - {}", err);
            }
        }
        std::process::exit(2);
    }

    let store = open_store()?;
    let plan_id = store
        .save_plan(&plan)
        .context("failed to save plan to store")?;

    if cli.json {
        let obj = serde_json::json!({ "plan_id": plan_id });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!("Plan saved: {}", plan_id);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// cmd_exec — execute a previously-saved plan
// ---------------------------------------------------------------------------

/// Opens the store, builds an engine, and executes the plan identified by
/// `plan_id`. Writes a PID file during execution and removes it when done.
/// Exits with an appropriate code reflecting the run outcome.
fn cmd_exec(cli: &Cli, plan_id: &str) -> Result<()> {
    let store = open_store()?;
    let config = build_engine_config(cli)?;
    let data_dir = resolve_data_dir();
    let mut engine = Engine::new(store, config);

    // Set up cancellation handlers for SIGINT and SIGTERM.
    let cancel_flag: Arc<AtomicBool> = engine.cancel_flag();
    let flag_clone = Arc::clone(&cancel_flag);
    let _ = set_ctrlc_handler(flag_clone);

    // P2 fix: Write PID file with the actual run_id (not plan_id).
    // Use the on_run_created callback to get the run_id early.
    let data_dir_clone = data_dir.clone();
    engine.set_on_run_created(move |run_id| {
        let _ = write_pid_file(&data_dir_clone, run_id);
    });

    let result = engine.execute_plan(plan_id);

    // Always clean up PID file.
    let _ = remove_pid_file(&data_dir);

    match result {
        Ok(summary) => {
            print_run_summary(cli, &summary);
            print_pause_hint_if_paused(&open_store()?, &summary);
            let code = exit_code_for_summary(&summary);
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Err(e) => {
            if cli.json {
                let obj = serde_json::json!({ "error": e.to_string() });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                eprintln!("Execution failed: {}", e);
            }
            std::process::exit(1);
        }
    }
}

/// Resumes a paused PLAN run (M11): verifies its approval, then re-executes the
/// plan skipping already-completed steps (with a one-shot escalation allowance
/// if the pause was an escalation).
fn cmd_run_resume(cli: &Cli, run_id: &str) -> Result<()> {
    use operator_core::types::{ApprovalKind, ApprovalStatus, CheckpointKind};
    use operator_runtime::engine::ResumeState;

    let store = open_store()?;
    let cp = store
        .load_checkpoint(run_id)
        .context("failed to read checkpoint")?
        .ok_or_else(|| {
            anyhow::anyhow!("no paused run with id {run_id} (no checkpoint on record)")
        })?;
    if cp.kind != CheckpointKind::Plan {
        bail!(
            "run {run_id} is an agent run — resume it with `operator-jack agent --resume {run_id}`"
        );
    }

    let approval = store
        .latest_approval_for_run(run_id)
        .context("failed to read approvals")?
        .ok_or_else(|| anyhow::anyhow!("no approval on record for run {run_id}"))?;
    match approval.status {
        ApprovalStatus::Pending => bail!(
            "run {run_id} is still awaiting approval — run `operator-jack approvals approve {}` first",
            approval.id
        ),
        ApprovalStatus::Denied => bail!("the approval for run {run_id} was denied; not resuming"),
        ApprovalStatus::Approved => {}
    }

    let old_run = store.get_run(run_id).context("failed to load paused run")?;
    let cursor = cp
        .cursor
        .get("step_index")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let variables: std::collections::HashMap<String, serde_json::Value> = cp
        .variables
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let config = build_engine_config(cli)?;
    let data_dir = resolve_data_dir();
    let store2 = open_store()?;
    let mut engine = Engine::new(store2, config);
    engine.resume_next(ResumeState {
        cursor,
        variables,
        grant_escalation: approval.kind == ApprovalKind::Escalation,
    });

    let cancel_flag: Arc<AtomicBool> = engine.cancel_flag();
    let _ = set_ctrlc_handler(Arc::clone(&cancel_flag));
    let data_dir_clone = data_dir.clone();
    engine.set_on_run_created(move |rid| {
        let _ = write_pid_file(&data_dir_clone, rid);
    });

    eprintln!("Resuming plan run {run_id} from step {cursor}…");
    let result = engine.execute_plan(&old_run.plan_id);
    let _ = remove_pid_file(&data_dir);

    // Consume the old checkpoint so the same pause can't be resumed twice.
    let _ = store.delete_checkpoint(run_id);

    match result {
        Ok(summary) => {
            print_run_summary(cli, &summary);
            print_pause_hint_if_paused(&store, &summary);
            let code = exit_code_for_summary(&summary);
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Err(e) => {
            if cli.json {
                println!("{}", serde_json::json!({ "error": e.to_string() }));
            } else {
                eprintln!("Resume failed: {}", e);
            }
            std::process::exit(1);
        }
    }
}

/// If a plan run paused, prints the pending approval id + resume command and
/// raises a notification (M11 supervision DX).
fn print_pause_hint_if_paused(store: &Store, summary: &RunSummary) {
    if summary.status != RunStatus::Paused {
        return;
    }
    let approval = store
        .latest_approval_for_run(&summary.run_id)
        .ok()
        .flatten();
    let reason = approval
        .as_ref()
        .map(|a| a.summary.clone())
        .unwrap_or_else(|| "human input required".to_string());
    notify_user("Operator Jack: run paused", &reason);
    if let Some(a) = approval {
        eprintln!();
        eprintln!("⏸  Run paused: {reason}");
        eprintln!("   Approve:  operator-jack approvals approve {}", a.id);
        eprintln!("   Resume:   operator-jack run --resume {}", summary.run_id);
    }
}

// ---------------------------------------------------------------------------
// cmd_run — validate, save, and execute a plan in one shot
// ---------------------------------------------------------------------------

/// Accepts either a `--plan-file` path or an inline `json:{...}` instruction.
/// Validates the plan, saves it to the store, and then executes it.
fn cmd_run(cli: &Cli, plan_file: Option<PathBuf>, instruction: Option<String>) -> Result<()> {
    // Resolve the plan from the provided source.
    let plan = if let Some(ref path) = plan_file {
        read_plan_file(path)?
    } else if let Some(ref instr) = instruction {
        if let Some(json_str) = instr.strip_prefix("json:") {
            serde_json::from_str::<Plan>(json_str).context("failed to parse inline plan JSON")?
        } else {
            if cli.json {
                let obj = serde_json::json!({
                    "error": "Provide --plan-file or inline json:{...}"
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                eprintln!("Error: Provide --plan-file or inline json:{{...}}");
            }
            std::process::exit(6);
        }
    } else {
        if cli.json {
            let obj = serde_json::json!({
                "error": "Provide --plan-file or inline json:{...}"
            });
            println!("{}", serde_json::to_string_pretty(&obj)?);
        } else {
            eprintln!("Error: Provide --plan-file or inline json:{{...}}");
        }
        std::process::exit(6);
    };

    // Validate.
    if let Err(errors) = validate_plan(&plan) {
        if cli.json {
            let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            let obj = serde_json::json!({ "valid": false, "errors": msgs });
            println!("{}", serde_json::to_string_pretty(&obj)?);
        } else {
            eprintln!("Validation failed with {} error(s):", errors.len());
            for err in &errors {
                eprintln!("  - {}", err);
            }
        }
        std::process::exit(2);
    }

    // Save plan to store.
    let store = open_store()?;
    let plan_id = store
        .save_plan(&plan)
        .context("failed to save plan to store")?;

    if !cli.quiet {
        if cli.json {
            // Plan ID will be included in the final summary.
        } else {
            println!("Plan saved: {}", plan_id);
        }
    }

    // Execute (same logic as cmd_exec).
    let config = build_engine_config(cli)?;
    let data_dir = resolve_data_dir();

    // Re-open store because Engine takes ownership.
    let store2 = open_store()?;
    let mut engine = Engine::new(store2, config);

    let cancel_flag: Arc<AtomicBool> = engine.cancel_flag();
    let flag_clone = Arc::clone(&cancel_flag);
    let _ = set_ctrlc_handler(flag_clone);

    // P2 fix: Write PID file with the actual run_id (not plan_id).
    let data_dir_clone = data_dir.clone();
    engine.set_on_run_created(move |run_id| {
        let _ = write_pid_file(&data_dir_clone, run_id);
    });

    let result = engine.execute_plan(&plan_id);

    let _ = remove_pid_file(&data_dir);

    match result {
        Ok(summary) => {
            print_run_summary(cli, &summary);
            print_pause_hint_if_paused(&open_store()?, &summary);
            let code = exit_code_for_summary(&summary);
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Err(e) => {
            if cli.json {
                let obj = serde_json::json!({ "error": e.to_string() });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                eprintln!("Execution failed: {}", e);
            }
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// cmd_do — natural language to plan generation and execution
// ---------------------------------------------------------------------------

/// Converts a natural language instruction to a plan using an LLM provider,
/// then optionally executes it.
fn cmd_do(
    cli: &Cli,
    instruction: String,
    provider_override: Option<String>,
    show_plan_only: bool,
    save_plan_path: Option<PathBuf>,
) -> Result<()> {
    if instruction.trim().is_empty() {
        if cli.json {
            let obj = serde_json::json!({ "error": "Empty instruction provided" });
            println!("{}", serde_json::to_string_pretty(&obj)?);
        } else {
            eprintln!("Error: Please provide a natural language instruction.");
            eprintln!("Example: operator-jack do \"open Notes and type hello\"");
        }
        std::process::exit(6);
    }

    // Load config
    let cfg = OperatorConfig::load().unwrap_or_else(|e| {
        tracing::warn!("failed to load config: {e}, using defaults");
        OperatorConfig::default()
    });

    // Build planner config from operator-core config
    let planner_config = build_planner_config(&cfg);

    // Create planner
    let planner = Planner::new(planner_config);

    // Determine provider
    let provider_type = if let Some(p) = provider_override {
        match p.parse::<ProviderType>() {
            Ok(pt) => pt,
            Err(e) => {
                if cli.json {
                    let obj = serde_json::json!({ "error": e });
                    println!("{}", serde_json::to_string_pretty(&obj)?);
                } else {
                    eprintln!("Error: {}", e);
                    eprintln!("Valid providers: kimi, openai, anthropic, ollama");
                }
                std::process::exit(6);
            }
        }
    } else if let Ok(env_provider) = std::env::var("OPERATOR_DEFAULT_PROVIDER") {
        env_provider.parse::<ProviderType>().unwrap_or_else(|_| {
            planner
                .first_available_provider()
                .unwrap_or(ProviderType::Ollama)
        })
    } else if let Some(first_available) = planner.first_available_provider() {
        first_available
    } else {
        // Interactive selection if TTY
        if atty_is_interactive() {
            match operator_planner::select_provider_interactive(&planner) {
                Some(pt) => pt,
                None => {
                    if cli.json {
                        let obj = serde_json::json!({ "error": "No provider selected" });
                        println!("{}", serde_json::to_string_pretty(&obj)?);
                    } else {
                        eprintln!("No provider selected. Exiting.");
                    }
                    std::process::exit(6);
                }
            }
        } else {
            if cli.json {
                let obj = serde_json::json!({
                    "error": "No LLM provider available. Set an API key or start Ollama."
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                eprintln!("No LLM provider available.");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  1. Set an API key environment variable:");
                eprintln!("     - KIMI_API_KEY for Kimi");
                eprintln!("     - OPENAI_API_KEY for OpenAI");
                eprintln!("     - ANTHROPIC_API_KEY for Anthropic");
                eprintln!("  2. Start Ollama locally:");
                eprintln!("     - ollama pull llama3.2");
                eprintln!("     - ollama serve");
                eprintln!("  3. Use --provider to specify which provider to use");
            }
            std::process::exit(6);
        }
    };

    if !cli.quiet && !cli.json {
        println!("Generating plan using {}...", provider_type.display_name());
    }

    // Generate plan
    let plan = match planner.plan_with_provider(&instruction, provider_type) {
        Ok(p) => p,
        Err(e) => {
            if cli.json {
                let obj = serde_json::json!({ "error": e.to_string() });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                eprintln!("Failed to generate plan: {}", e);
                if e.is_retryable() {
                    eprintln!("This error may be temporary. Please try again.");
                }
            }
            std::process::exit(1);
        }
    };

    // Save plan if requested
    if let Some(path) = save_plan_path {
        let plan_json = serde_json::to_string_pretty(&plan)?;
        fs::write(&path, plan_json)
            .with_context(|| format!("failed to write plan to: {}", path.display()))?;
        if !cli.quiet && !cli.json {
            println!("Plan saved to: {}", path.display());
        }
    }

    // Show plan and exit if --show-plan
    if show_plan_only {
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            println!("\nGenerated Plan: {}", plan.name);
            println!(
                "Description: {}",
                plan.description.as_deref().unwrap_or("N/A")
            );
            println!("Steps ({}):", plan.steps.len());
            for (i, step) in plan.steps.iter().enumerate() {
                println!("  {}. {} - {:?}", i + 1, step.id, step.step_type);
            }
            println!();
            println!("Full JSON:");
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        return Ok(());
    }

    // Validate the generated plan
    if let Err(errors) = validate_plan(&plan) {
        if cli.json {
            let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            let obj = serde_json::json!({
                "error": "Generated plan failed validation",
                "validation_errors": msgs
            });
            println!("{}", serde_json::to_string_pretty(&obj)?);
        } else {
            eprintln!("Generated plan failed validation:");
            for err in &errors {
                eprintln!("  - {}", err);
            }
            eprintln!();
            eprintln!(
                "You can view the raw plan with: operator-jack do \"{}\" --show-plan",
                instruction
            );
        }
        std::process::exit(2);
    }

    // Save and execute the plan (similar to cmd_run)
    let store = open_store()?;
    let plan_id = store
        .save_plan(&plan)
        .context("failed to save plan to store")?;

    if !cli.quiet && !cli.json {
        println!("Plan saved: {}", plan_id);
    }

    // Execute
    let config = build_engine_config(cli)?;
    let data_dir = resolve_data_dir();

    let store2 = open_store()?;
    let mut engine = Engine::new(store2, config);

    let cancel_flag: Arc<AtomicBool> = engine.cancel_flag();
    let flag_clone = Arc::clone(&cancel_flag);
    let _ = set_ctrlc_handler(flag_clone);

    let data_dir_clone = data_dir.clone();
    engine.set_on_run_created(move |run_id| {
        let _ = write_pid_file(&data_dir_clone, run_id);
    });

    let result = engine.execute_plan(&plan_id);

    let _ = remove_pid_file(&data_dir);

    match result {
        Ok(summary) => {
            print_run_summary(cli, &summary);
            print_pause_hint_if_paused(&open_store()?, &summary);
            let code = exit_code_for_summary(&summary);
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Err(e) => {
            if cli.json {
                let obj = serde_json::json!({ "error": e.to_string() });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                eprintln!("Execution failed: {}", e);
            }
            std::process::exit(1);
        }
    }
}

/// Builds planner configuration from operator-core config.
fn build_planner_config(cfg: &OperatorConfig) -> PlannerPlannerConfig {
    PlannerPlannerConfig {
        provider: cfg
            .default_provider
            .parse::<ProviderType>()
            .unwrap_or(ProviderType::Ollama),
        kimi: convert_provider_settings(&cfg.planner.kimi, "KIMI_API_KEY"),
        openai: convert_provider_settings(&cfg.planner.openai, "OPENAI_API_KEY"),
        anthropic: convert_provider_settings(&cfg.planner.anthropic, "ANTHROPIC_API_KEY"),
        ollama: convert_provider_settings(&cfg.planner.ollama, ""),
    }
}

/// Converts operator-core ProviderSettings to operator-planner ProviderConfig.
fn convert_provider_settings(settings: &ProviderSettings, env_var: &str) -> ProviderConfig {
    ProviderConfig {
        api_key: if env_var.is_empty() {
            None // Ollama doesn't need API key
        } else {
            std::env::var(env_var).ok()
        },
        base_url: settings.base_url.clone(),
        model: settings.model.clone(),
        max_tokens: settings.max_tokens,
        temperature: settings.temperature,
        timeout_seconds: settings.timeout_seconds,
    }
}

// ---------------------------------------------------------------------------
// cmd_logs — view run logs
// ---------------------------------------------------------------------------

/// Lists recent runs when no run_id is given. When a run_id is provided,
/// shows run details and step results. With `--full`, prints the raw JSONL
/// log file.
fn cmd_logs(cli: &Cli, run_id: Option<String>, full: bool) -> Result<()> {
    let store = open_store()?;

    match run_id {
        None => {
            // List recent runs.
            let runs = store.list_runs(20_u32).context("failed to list runs")?;

            if cli.json {
                let obj = serde_json::to_value(&runs)?;
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else if runs.is_empty() {
                println!("No runs found.");
            } else {
                println!(
                    "{:<28} {:<28} {:<22} {:<12}",
                    "RUN ID", "PLAN ID", "STATUS", "STARTED"
                );
                println!("{}", "-".repeat(90));
                for run in &runs {
                    let status_str = serde_json::to_value(&run.status)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_else(|| format!("{:?}", run.status));
                    let started = run.started_at.format("%Y-%m-%d %H:%M:%S").to_string();
                    println!(
                        "{:<28} {:<28} {:<22} {:<12}",
                        run.id, run.plan_id, status_str, started
                    );
                }
            }
        }
        Some(ref rid) => {
            if full {
                // Print raw JSONL log file.
                let log_dir = resolve_log_dir();
                let log_path = log_dir.join(format!("{}.jsonl", rid));

                if !log_path.exists() {
                    if cli.json {
                        let obj = serde_json::json!({ "error": "Log file not found" });
                        println!("{}", serde_json::to_string_pretty(&obj)?);
                    } else {
                        eprintln!("Log file not found: {}", log_path.display());
                    }
                    return Ok(());
                }

                let file = fs::File::open(&log_path)
                    .with_context(|| format!("failed to open log: {}", log_path.display()))?;
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    let line = line?;
                    if cli.json {
                        // Already JSON, print directly.
                        println!("{}", line);
                    } else {
                        // Pretty-print each JSON line.
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                            println!("{}", serde_json::to_string_pretty(&val)?);
                        } else {
                            println!("{}", line);
                        }
                    }
                }
            } else {
                // Show run detail + step results.
                let run = store.get_run(rid).context("failed to get run")?;
                let step_results = store
                    .get_step_results(rid)
                    .context("failed to get step results")?;

                if cli.json {
                    let obj = serde_json::json!({
                        "run": serde_json::to_value(&run)?,
                        "step_results": serde_json::to_value(&step_results)?,
                    });
                    println!("{}", serde_json::to_string_pretty(&obj)?);
                } else {
                    let status_str = serde_json::to_value(&run.status)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_else(|| format!("{:?}", run.status));

                    println!("Run:     {}", run.id);
                    println!("Plan:    {}", run.plan_id);
                    println!("Status:  {}", status_str);
                    println!(
                        "Started: {}",
                        run.started_at.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                    if let Some(ended) = run.ended_at {
                        println!("Ended:   {}", ended.format("%Y-%m-%d %H:%M:%S UTC"));
                    }
                    if let Some(ref err) = run.error {
                        println!("Error:   {}", err);
                    }
                    println!();

                    if step_results.is_empty() {
                        println!("No step results recorded.");
                    } else {
                        println!(
                            "{:<6} {:<20} {:<14} {:<8} ERROR",
                            "INDEX", "STEP ID", "STATUS", "ATTEMPT"
                        );
                        println!("{}", "-".repeat(70));
                        for sr in &step_results {
                            let sr_status = serde_json::to_value(&sr.status)
                                .ok()
                                .and_then(|v| v.as_str().map(String::from))
                                .unwrap_or_else(|| format!("{:?}", sr.status));
                            let error_msg = sr
                                .error_json
                                .as_ref()
                                .and_then(|e| e.get("message"))
                                .and_then(|m| m.as_str())
                                .unwrap_or("-");
                            println!(
                                "{:<6} {:<20} {:<14} {:<8} {}",
                                sr.step_index, sr.step_id, sr_status, sr.attempt, error_msg
                            );

                            // In verbose mode, show params and output.
                            if cli.verbose {
                                println!("        input:  {}", sr.input_json);
                                if let Some(ref out) = sr.output_json {
                                    println!("        output: {}", out);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// cmd_stop — stop a running execution
// ---------------------------------------------------------------------------

/// Reads the PID file from the data directory. If a running process is found,
/// sends SIGTERM and waits up to 5 seconds. If still alive, sends SIGKILL.
fn cmd_stop(cli: &Cli) -> Result<()> {
    let data_dir = resolve_data_dir();

    match read_pid_file(&data_dir)? {
        None => {
            if cli.json {
                let obj = serde_json::json!({ "status": "no_active_run" });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                println!("No active run found.");
            }
            Ok(())
        }
        Some((pid, run_id)) => {
            // Send SIGTERM via the kill command.
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .output();

            // Wait up to 5 seconds for the process to exit.
            let mut alive = true;
            for _ in 0..50 {
                thread::sleep(Duration::from_millis(100));
                // Check if process is still alive: kill -0 returns success if alive.
                let status = Command::new("kill").args(["-0", &pid.to_string()]).output();
                match status {
                    Ok(output) if output.status.success() => {
                        // Still alive, keep waiting.
                    }
                    _ => {
                        alive = false;
                        break;
                    }
                }
            }

            if alive {
                // Force kill.
                let _ = Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .output();
            }

            // Clean up PID file.
            let _ = remove_pid_file(&data_dir);

            if cli.json {
                let obj = serde_json::json!({
                    "status": "stopped",
                    "run_id": run_id,
                    "pid": pid,
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                println!("Stopped run {} (pid {})", run_id, pid);
            }

            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// cmd_ui_inspect — inspect accessibility tree
// ---------------------------------------------------------------------------

/// Connects to the macOS helper and dumps the accessibility tree for an app.
fn cmd_ui_inspect(cli: &Cli, app: &str, depth: u32) -> Result<()> {
    let helper_path = resolve_helper_path(cli).ok_or_else(|| {
        anyhow::anyhow!("Helper binary not found. Run 'operator doctor' to check.")
    })?;

    let mut client =
        operator_ipc::client::HelperClient::new(Some(helper_path.to_string_lossy().to_string()));
    client.connect().context("failed to connect to helper")?;

    let result = client.send(
        "ui.inspect",
        serde_json::json!({
            "app": app,
            "depth": depth,
        }),
    );
    client.disconnect();

    match result {
        Ok(val) => {
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&val)?);
            } else {
                // Pretty-print the tree with indentation
                let node_count = val.get("node_count").and_then(|v| v.as_u64()).unwrap_or(0);
                println!(
                    "Accessibility tree for \"{}\" ({} nodes, depth {}):",
                    app, node_count, depth
                );
                println!();
                if let Some(tree) = val.get("tree") {
                    print_ax_tree(tree, 0);
                }
            }
            Ok(())
        }
        Err(e) => {
            bail!("inspect failed: {}", e);
        }
    }
}

/// Recursively prints an AX tree node with indentation.
fn print_ax_tree(node: &serde_json::Value, indent: usize) {
    let prefix = "  ".repeat(indent);
    let role = node.get("role").and_then(|v| v.as_str()).unwrap_or("?");
    let name = node.get("name").and_then(|v| v.as_str());
    let value = node.get("value").and_then(|v| v.as_str());
    let identifier = node.get("identifier").and_then(|v| v.as_str());

    let mut line = format!("{}{}", prefix, role);

    if let Some(n) = name {
        if !n.is_empty() {
            line.push_str(&format!(" name=\"{}\"", n));
        }
    }
    if let Some(v) = value {
        if !v.is_empty() {
            let display_val = truncate_str(v, 40);
            line.push_str(&format!(" value=\"{}\"", display_val));
        }
    }
    if let Some(id) = identifier {
        if !id.is_empty() {
            line.push_str(&format!(" id=\"{}\"", id));
        }
    }

    println!("{}", line);

    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
        for child in children {
            print_ax_tree(child, indent + 1);
        }
    }
}

// ===========================================================================
// Helper functions
// ===========================================================================

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

/// Returns the data directory: `$DATA_DIR/operator-jack` (e.g.
/// `~/Library/Application Support/operator-jack` on macOS).
fn resolve_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("operator-jack")
}

/// Returns the log directory: `<data_dir>/logs`.
fn resolve_log_dir() -> PathBuf {
    resolve_data_dir().join("logs")
}

/// Returns the database path: `<data_dir>/operator-jack.db`.
fn resolve_db_path() -> PathBuf {
    resolve_data_dir().join("operator-jack.db")
}

// ---------------------------------------------------------------------------
// Store helpers
// ---------------------------------------------------------------------------

/// Opens the SQLite store at the resolved database path.
fn open_store() -> Result<Store> {
    let db_path = resolve_db_path();
    Store::open(&db_path).context("failed to open store")
}

// ---------------------------------------------------------------------------
// Plan file I/O
// ---------------------------------------------------------------------------

/// Reads a plan JSON file from disk and parses it into a `Plan`.
fn read_plan_file(path: &Path) -> Result<Plan> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read plan file: {}", path.display()))?;
    let plan: Plan = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse plan JSON from: {}", path.display()))?;
    Ok(plan)
}

// ---------------------------------------------------------------------------
// Engine configuration
// ---------------------------------------------------------------------------

/// Builds an `EngineConfig` from config file + env vars + CLI arguments.
/// Precedence: config file < env vars < CLI flags.
fn build_engine_config(cli: &Cli) -> Result<EngineConfig> {
    // Load config file (defaults if missing, errors only on parse failures).
    let cfg = OperatorConfig::load().unwrap_or_else(|e| {
        tracing::warn!("failed to load config: {e}, using defaults");
        OperatorConfig::default()
    });

    // CLI mode overrides config default_mode.
    let mode = parse_mode(&cli.mode)?;

    // Determine interactive mode: --interactive overrides, --no-interactive
    // overrides, config value, or detect TTY.
    let interactive = if cli.no_interactive {
        false
    } else if cli.interactive {
        true
    } else {
        atty_is_interactive()
    };

    // CLI allow_apps/allow_domains override config values.
    let allow_apps = if cli.allow_apps.is_some() {
        parse_comma_list(&cli.allow_apps)
    } else {
        cfg.allow_apps.clone()
    };
    let allow_domains = if cli.allow_domains.is_some() {
        parse_comma_list(&cli.allow_domains)
    } else {
        cfg.allow_domains.clone()
    };

    let log_dir = if let Some(ref d) = cfg.log_dir {
        PathBuf::from(d)
    } else {
        resolve_log_dir()
    };

    // Resolve the helper path through all discovery mechanisms (CLI flag,
    // env var, PATH, sibling, dev fallback) instead of passing just the
    // raw CLI flag.
    let helper_path = resolve_helper_path(cli).map(|p| p.to_string_lossy().to_string());

    Ok(EngineConfig {
        mode,
        yes_to_all: cli.yes,
        interactive,
        dry_run: cli.dry_run,
        allow_apps,
        allow_domains,
        log_dir,
        helper_path,
        default_timeout_ms: cfg.default_step_timeout_ms,
        default_retries: cfg.default_retries,
        default_backoff_ms: cfg.default_retry_backoff_ms,
        grants: cfg.grants.clone(),
        budget: cfg.budget.clone(),
    })
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parses a mode string ("safe" or "unsafe") into a `Mode` enum value.
fn parse_mode(s: &str) -> Result<Mode> {
    match s.to_ascii_lowercase().as_str() {
        "safe" => Ok(Mode::Safe),
        "unsafe" => Ok(Mode::Unsafe),
        other => bail!("invalid mode '{}': expected 'safe' or 'unsafe'", other),
    }
}

/// Splits a comma-separated string into a list of trimmed, non-empty strings.
fn parse_comma_list(s: &Option<String>) -> Vec<String> {
    match s {
        Some(val) => val
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// PID file management
// ---------------------------------------------------------------------------

/// Writes a PID file containing the current process ID and the associated
/// run identifier. Format: `<pid>\n<run_id>\n`.
fn write_pid_file(data_dir: &Path, run_id: &str) -> Result<()> {
    let pid_path = data_dir.join("operator-jack.pid");
    let contents = format!("{}\n{}\n", std::process::id(), run_id);
    fs::write(&pid_path, contents)
        .with_context(|| format!("failed to write PID file: {}", pid_path.display()))?;
    Ok(())
}

/// Removes the PID file if it exists.
fn remove_pid_file(data_dir: &Path) -> Result<()> {
    let pid_path = data_dir.join("operator-jack.pid");
    if pid_path.exists() {
        fs::remove_file(&pid_path)
            .with_context(|| format!("failed to remove PID file: {}", pid_path.display()))?;
    }
    Ok(())
}

/// Reads the PID file and returns `(pid, run_id)` if the file exists and
/// is well-formed.
fn read_pid_file(data_dir: &Path) -> Result<Option<(u32, String)>> {
    let pid_path = data_dir.join("operator-jack.pid");
    if !pid_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&pid_path)
        .with_context(|| format!("failed to read PID file: {}", pid_path.display()))?;
    let mut lines = contents.lines();

    let pid_str = match lines.next() {
        Some(s) => s.trim(),
        None => return Ok(None),
    };
    let run_id = match lines.next() {
        Some(s) => s.trim().to_string(),
        None => return Ok(None),
    };

    let pid: u32 = pid_str
        .parse()
        .with_context(|| format!("invalid PID in PID file: '{}'", pid_str))?;

    Ok(Some((pid, run_id)))
}

// ---------------------------------------------------------------------------
// Exit code mapping
// ---------------------------------------------------------------------------

/// Maps a `RunStatus` to the process exit code per the operator spec:
/// - 0: succeeded
/// - 1: failed or completed_with_errors
/// - 2: validation failure (handled elsewhere)
/// - 3: policy denied (handled elsewhere)
/// - 4: reserved
/// - 5: cancelled
fn exit_code_for_summary(summary: &RunSummary) -> i32 {
    // A policy/grant/escalation denial takes the dedicated exit code 3 over a
    // generic failure code.
    if summary.policy_denied
        && matches!(
            summary.status,
            RunStatus::Failed | RunStatus::CompletedWithErrors
        )
    {
        return 3;
    }
    match summary.status {
        RunStatus::Succeeded => 0,
        RunStatus::Failed => 1,
        RunStatus::CompletedWithErrors => 1,
        RunStatus::Cancelled => 5,
        // Stopped early for a non-error reason (budget): distinct from success
        // and from a step error.
        RunStatus::Partial => 7,
        // Paused awaiting approval/takeover — a clean, resumable stop (exit 0).
        RunStatus::Paused => 0,
        RunStatus::Queued | RunStatus::Running => 0,
    }
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

/// Prints a `RunSummary` to stdout, respecting --json and --quiet flags.
fn print_run_summary(cli: &Cli, summary: &RunSummary) {
    if cli.json {
        let status_str = serde_json::to_value(&summary.status)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", summary.status));
        let obj = serde_json::json!({
            "run_id": summary.run_id,
            "plan_id": summary.plan_id,
            "status": status_str,
            "steps_total": summary.steps_total,
            "steps_succeeded": summary.steps_succeeded,
            "steps_failed": summary.steps_failed,
            "steps_skipped": summary.steps_skipped,
            "duration_ms": summary.duration_ms,
        });
        // Safe to unwrap: the object is always serializable.
        println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
        return;
    }

    if cli.quiet {
        let status_str = serde_json::to_value(&summary.status)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", summary.status));
        println!("{}", status_str);
        return;
    }

    let status_str = serde_json::to_value(&summary.status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| format!("{:?}", summary.status));

    println!();
    println!("Run complete");
    println!("  Run ID:    {}", summary.run_id);
    println!("  Plan ID:   {}", summary.plan_id);
    println!("  Status:    {}", status_str);
    println!(
        "  Steps:     {} total, {} succeeded, {} failed, {} skipped",
        summary.steps_total, summary.steps_succeeded, summary.steps_failed, summary.steps_skipped
    );
    println!("  Duration:  {} ms", summary.duration_ms);
}

// ---------------------------------------------------------------------------
// Helper binary resolution
// ---------------------------------------------------------------------------

/// Attempts to locate the macOS helper binary. Checks (in order):
/// 1. `--helper-path` CLI flag
/// 2. `OPERATOR_HELPER_PATH` environment variable
/// 3. `operator-macos-helper` on the system PATH
/// 4. A fallback path next to the current executable
///
/// Returns `Some(path)` if the binary is found, `None` otherwise.
/// Truncates a string to at most `max` characters, cutting at a UTF-8 char
/// boundary. Byte-slicing at a fixed index (`&s[..n]`) panics when `n` lands
/// inside a multi-byte character.
fn truncate_str(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

fn resolve_helper_path(cli: &Cli) -> Option<PathBuf> {
    // 1. CLI flag.
    if let Some(ref p) = cli.helper_path {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    // 2. Environment variable.
    if let Ok(p) = std::env::var("OPERATOR_HELPER_PATH") {
        let path = PathBuf::from(&p);
        if path.exists() {
            return Some(path);
        }
    }

    // 3. Config file (`helper_path` in ~/.config/operator-jack/config.toml).
    if let Ok(cfg) = OperatorConfig::load() {
        if let Some(ref p) = cfg.helper_path {
            let path = PathBuf::from(p);
            if path.exists() {
                return Some(path);
            }
        }
    }

    // 4. System PATH via `which`.
    if let Ok(output) = Command::new("which").arg("operator-macos-helper").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                let path = PathBuf::from(&path_str);
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }

    // 5. Fallback: next to current executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let fallback = dir.join("operator-macos-helper");
            if fallback.exists() {
                return Some(fallback);
            }
        }
    }

    // 6. Dev fallback: Swift build output relative to the executable.
    //    When running from `target/debug/operator-jack`, the helper lives at
    //    `../../macos-helper/.build/{release,debug}/operator-macos-helper`.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for profile in &["release", "debug"] {
                let dev_path = dir
                    .join("../../macos-helper/.build")
                    .join(profile)
                    .join("operator-macos-helper");
                if let Ok(canonical) = dev_path.canonicalize() {
                    if canonical.exists() {
                        return Some(canonical);
                    }
                }
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Misc helpers
// ---------------------------------------------------------------------------

/// Checks whether a directory is writable by attempting to create and
/// immediately remove a temporary file.
fn is_dir_writable(dir: &Path) -> bool {
    let test_path = dir.join(".operator_write_test");
    if fs::write(&test_path, b"test").is_ok() {
        let _ = fs::remove_file(&test_path);
        true
    } else {
        false
    }
}

/// Returns true if stderr is connected to a terminal. Uses the POSIX
/// `isatty()` function via libc FFI to check file descriptor 2 (stderr).
fn atty_is_interactive() -> bool {
    #[cfg(unix)]
    {
        extern "C" {
            fn isatty(fd: i32) -> i32;
        }
        // Check stderr (fd 2) — prompts are written to stderr.
        unsafe { isatty(2) != 0 }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Sets up SIGINT and SIGTERM handlers that set the given `AtomicBool` to
/// `true`. This is a best-effort setup; failures are silently ignored.
fn set_ctrlc_handler(flag: Arc<AtomicBool>) -> Result<()> {
    // Safety: AtomicBool operations are async-signal-safe. We only set a
    // flag and do not allocate or call non-signal-safe functions. The Arc
    // is intentionally leaked (via `into_raw`) so the pointed-to
    // AtomicBool lives for the duration of the process.
    unsafe {
        CANCEL_FLAG_PTR.store(Arc::into_raw(flag) as usize, Ordering::SeqCst);

        // Register handlers for both SIGINT (Ctrl-C) and SIGTERM.
        libc_signal(SIGINT, signal_handler as SignalHandler);
        libc_signal(SIGTERM, signal_handler as SignalHandler);
    }
    Ok(())
}

/// SIGINT signal number on Unix / macOS.
const SIGINT: i32 = 2;
/// SIGTERM signal number on Unix / macOS.
const SIGTERM: i32 = 15;

/// Type alias for a POSIX signal handler function pointer.
type SignalHandler = extern "C" fn(i32);

/// Global storage for the cancel flag pointer, accessed from the signal
/// handler. Stored as a `usize` because `AtomicPtr` is not available in
/// `const` context and `usize` is the same size as a pointer.
static CANCEL_FLAG_PTR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Minimal signal handler that sets the cancel flag. Handles both SIGINT
/// and SIGTERM.
///
/// # Safety
/// Only performs an atomic store, which is async-signal-safe.
extern "C" fn signal_handler(_sig: i32) {
    let ptr = CANCEL_FLAG_PTR.load(Ordering::SeqCst);
    if ptr != 0 {
        let flag = unsafe { &*(ptr as *const AtomicBool) };
        flag.store(true, Ordering::SeqCst);
    }
}

/// Registers a signal handler using the POSIX `signal` function.
///
/// # Safety
/// Caller must ensure `handler` is a valid signal handler function.
#[cfg(unix)]
unsafe fn libc_signal(sig: i32, handler: SignalHandler) {
    extern "C" {
        fn signal(sig: i32, handler: SignalHandler) -> usize;
    }
    unsafe {
        let _ = signal(sig, handler);
    }
}

#[cfg(not(unix))]
unsafe fn libc_signal(_sig: i32, _handler: SignalHandler) {
    // Signal handling is only supported on Unix/macOS.
}

// ---------------------------------------------------------------------------
// cmd_init — create default config file
// ---------------------------------------------------------------------------

fn cmd_init(_cli: &Cli) -> Result<()> {
    let config_path =
        OperatorConfig::default_path().context("could not determine config directory")?;

    if config_path.exists() {
        eprintln!("Config file already exists: {}", config_path.display());
        eprintln!("Edit it directly or delete it to regenerate.");
        return Ok(());
    }

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir: {}", parent.display()))?;
    }

    fs::write(&config_path, OperatorConfig::default_toml())
        .with_context(|| format!("failed to write config file: {}", config_path.display()))?;

    eprintln!("Created config file: {}", config_path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Skill command handlers (M6)
// ---------------------------------------------------------------------------

/// Parse a key=value string into a tuple.
fn parse_key_value(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .ok_or_else(|| format!("Expected key=value format, got: {}", s))
}

/// List all available skills.
fn cmd_skill_list(cli: &Cli) -> Result<()> {
    use operator_skills::SkillRegistry;

    let mut registry = SkillRegistry::new();
    registry
        .discover()
        .map_err(|e| anyhow::anyhow!("Failed to discover skills: {}", e))?;

    let skills = registry.list();

    if cli.json {
        let obj = serde_json::json!({
            "skills": skills.iter().map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "description": s.description,
                    "version": s.version,
                    "author": s.author,
                    "parameters": s.parameters.len(),
                    "steps": s.steps.len(),
                })
            }).collect::<Vec<_>>(),
            "count": skills.len(),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else if skills.is_empty() {
        println!("No skills found.");
        println!();
        println!("Skills are loaded from:");
        for path in registry.search_paths() {
            println!("  - {}", path.display());
        }
        println!();
        println!("Create a skill file or run 'operator-jack skill validate <file>' to check a skill manifest.");
    } else {
        println!("Available skills ({}):", skills.len());
        println!();
        println!("{:<20} {:<30} PARAMETERS", "NAME", "DESCRIPTION");
        println!("{}", "-".repeat(80));
        for skill in skills {
            let desc = skill.description.as_deref().unwrap_or("-");
            let desc_short = truncate_str(desc, 28);
            let params: Vec<_> = skill.parameters.iter().map(|p| p.name.clone()).collect();
            let params_str = if params.is_empty() {
                "none".to_string()
            } else {
                params.join(", ")
            };
            println!("{:<20} {:<30} {}", skill.name, desc_short, params_str);
        }
        println!();
        println!("Run a skill with: operator-jack skill run <name> --param key=value");
    }

    Ok(())
}

/// Validate a skill manifest file.
fn cmd_skill_validate(cli: &Cli, skill_file: &Path) -> Result<()> {
    use operator_skills::SkillRegistry;

    let registry = SkillRegistry::new();
    match registry.load_from_path(skill_file) {
        Ok(manifest) => {
            if cli.json {
                let obj = serde_json::json!({
                    "valid": true,
                    "name": manifest.name,
                    "description": manifest.description,
                    "version": manifest.version,
                    "author": manifest.author,
                    "parameters": manifest.parameters.len(),
                    "steps": manifest.steps.len(),
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                println!("✓ Skill manifest is valid: {}", manifest.name);
                if let Some(ref desc) = manifest.description {
                    println!("  Description: {}", desc);
                }
                println!("  Parameters: {}", manifest.parameters.len());
                for param in &manifest.parameters {
                    let req = if param.required {
                        "required"
                    } else {
                        "optional"
                    };
                    println!("    - {} ({:?}, {})", param.name, param.param_type, req);
                }
                println!("  Steps: {}", manifest.steps.len());
            }
            Ok(())
        }
        Err(e) => {
            if cli.json {
                let obj = serde_json::json!({
                    "valid": false,
                    "error": e.to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                eprintln!("✗ Skill validation failed: {}", e);
            }
            std::process::exit(2);
        }
    }
}

/// Run a skill with parameters.
fn cmd_skill_run(cli: &Cli, skill_name: &str, params: &[(String, String)]) -> Result<()> {
    use operator_skills::{registry::ensure_user_skills_dir, SkillRegistry};
    use std::collections::HashMap;

    // Ensure user skills dir exists (creates it if not present)
    let _ = ensure_user_skills_dir();

    let mut registry = SkillRegistry::new();
    registry
        .discover()
        .map_err(|e| anyhow::anyhow!("Failed to discover skills: {}", e))?;

    let manifest = registry.get(skill_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Skill '{}' not found. Run 'operator-jack skill list' to see available skills.",
            skill_name
        )
    })?;

    // Convert params to HashMap
    let params_map: HashMap<String, String> = params.iter().cloned().collect();

    // Resolve parameters
    let resolved = manifest
        .resolve(params_map)
        .map_err(|e| anyhow::anyhow!("Failed to resolve skill parameters: {}", e))?;

    // Expand to plan
    let plan = resolved
        .to_plan()
        .map_err(|e| anyhow::anyhow!("Failed to expand skill to plan: {}", e))?;

    // Validate the expanded plan BEFORE executing, so a skill with bad step
    // params fails loudly here rather than mid-run inside the Swift helper.
    if let Err(errors) = validate_plan(&plan) {
        if cli.json {
            let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            let obj = serde_json::json!({ "valid": false, "errors": msgs });
            println!("{}", serde_json::to_string_pretty(&obj)?);
        } else {
            eprintln!(
                "Skill '{skill_name}' expanded to an invalid plan ({} error(s)):",
                errors.len()
            );
            for err in &errors {
                eprintln!("  - {err}");
            }
        }
        std::process::exit(2);
    }

    // Execute the plan (same logic as cmd_run)
    let store = open_store()?;
    let plan_id = store
        .save_plan(&plan)
        .context("failed to save plan to store")?;

    if !cli.quiet && !cli.json {
        println!("Skill '{}' expanded to plan: {}", skill_name, plan_id);
    }

    let config = build_engine_config(cli)?;
    let data_dir = resolve_data_dir();

    let store2 = open_store()?;
    let mut engine = Engine::new(store2, config);

    let cancel_flag: Arc<AtomicBool> = engine.cancel_flag();
    let flag_clone = Arc::clone(&cancel_flag);
    let _ = set_ctrlc_handler(flag_clone);

    let data_dir_clone = data_dir.clone();
    engine.set_on_run_created(move |run_id| {
        let _ = write_pid_file(&data_dir_clone, run_id);
    });

    let result = engine.execute_plan(&plan_id);

    let _ = remove_pid_file(&data_dir);

    match result {
        Ok(summary) => {
            print_run_summary(cli, &summary);
            print_pause_hint_if_paused(&open_store()?, &summary);
            let code = exit_code_for_summary(&summary);
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Err(e) => {
            if cli.json {
                let obj = serde_json::json!({ "error": e.to_string() });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                eprintln!("Execution failed: {}", e);
            }
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Browser command handlers (M5)
// ---------------------------------------------------------------------------

/// Check browser availability.
fn cmd_browser_doctor(cli: &Cli) -> Result<()> {
    let available = operator_exec_browser::is_chrome_available();

    if cli.json {
        let obj = serde_json::json!({
            "chrome_available": available,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else if available {
        println!("✓ Chrome is available");
    } else {
        println!("✗ Chrome not found");
        println!("Install Chrome or Chromium to use browser automation.");
    }
    Ok(())
}

/// Launch Chrome with remote debugging.
async fn cmd_browser_launch_async(_cli: &Cli, port: u16) -> Result<()> {
    let mut child = operator_exec_browser::launch_chrome(port).await?;

    println!("Chrome launched with remote debugging on port {}", port);
    println!("Press Ctrl+C to stop");

    // Wait for the process
    tokio::signal::ctrl_c().await?;

    child.kill().await?;
    println!("Chrome stopped");

    Ok(())
}

fn cmd_browser_launch(cli: &Cli, port: u16) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(cmd_browser_launch_async(cli, port))
}

/// Navigate to a URL.
async fn cmd_browser_navigate_async(_cli: &Cli, url: &str, port: u16) -> Result<()> {
    use operator_exec_browser::{discover_chrome_ws_url, BrowserClient};

    let ws_url = discover_chrome_ws_url(port).await?;
    let client = BrowserClient::connect(ws_url).await?;

    let frame_id = client.navigate(url).await?;
    println!("Navigated to {} (frame: {})", url, frame_id);

    client.close().await;
    Ok(())
}

fn cmd_browser_navigate(cli: &Cli, url: &str, port: u16) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(cmd_browser_navigate_async(cli, url, port))
}

/// Take a screenshot.
async fn cmd_browser_screenshot_async(
    _cli: &Cli,
    output: &std::path::Path,
    full_page: bool,
    port: u16,
) -> Result<()> {
    use operator_exec_browser::{discover_chrome_ws_url, BrowserClient};

    let ws_url = discover_chrome_ws_url(port).await?;
    let client = BrowserClient::connect(ws_url).await?;

    let image_data = client.screenshot(full_page).await?;
    std::fs::write(output, image_data)?;

    println!("Screenshot saved to {}", output.display());

    client.close().await;
    Ok(())
}

fn cmd_browser_screenshot(
    cli: &Cli,
    output: &std::path::Path,
    full_page: bool,
    port: u16,
) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(cmd_browser_screenshot_async(cli, output, full_page, port))
}
