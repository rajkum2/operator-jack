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

use operator_core::config::OperatorConfig;
use operator_core::types::{Mode, Plan, RunStatus};
use operator_core::validation::validate_plan;
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
    },

    /// View run logs
    Logs {
        /// Run ID to inspect
        run_id: Option<String>,

        /// Show full JSONL log
        #[arg(long)]
        full: bool,
    },

    /// Stop a running execution
    Stop,

    /// UI automation utilities
    Ui {
        #[command(subcommand)]
        action: UiAction,
    },

    /// Initialize config file at ~/.config/operator-jack/config.toml
    Init,
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
        } => cmd_run(&cli, plan_file.clone(), instruction.clone()),
        Commands::Logs { ref run_id, full } => cmd_logs(&cli, run_id.clone(), *full),
        Commands::Stop => cmd_stop(&cli),
        Commands::Ui { ref action } => match action {
            UiAction::Inspect { ref app, depth } => cmd_ui_inspect(&cli, app, *depth),
        },
        Commands::Init => cmd_init(&cli),
    }
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

    // 4. Accessibility permission (requires helper)
    let accessibility_status = if let Some(ref hp) = helper_path {
        let mut client =
            operator_ipc::client::HelperClient::new(Some(hp.to_string_lossy().to_string()));
        match client.connect() {
            Ok(()) => {
                let result = client.send(
                    "ui.check_accessibility_permission",
                    serde_json::json!({"prompt": true}),
                );
                client.disconnect();
                match result {
                    Ok(val) => {
                        if val
                            .get("trusted")
                            .and_then(|v: &serde_json::Value| v.as_bool())
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
                }
            }
            Err(e) => {
                tracing::warn!("helper connection failed: {}", e);
                "CHECK FAILED"
            }
        }
    } else {
        "SKIPPED (helper not found)"
    };

    // Detect first run (no config file exists).
    let config_path = OperatorConfig::default_path();
    let is_first_run = config_path.as_ref().map_or(true, |p| !p.exists());

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
            "db_path": db_path.display().to_string(),
            "log_dir_path": log_dir.display().to_string(),
            "first_run": is_first_run,
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
            let code = exit_code_for_status(&summary.status);
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
            let code = exit_code_for_status(&summary.status);
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
            } else {
                if runs.is_empty() {
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
                            "{:<6} {:<20} {:<14} {:<8} {}",
                            "INDEX", "STEP ID", "STATUS", "ATTEMPT", "ERROR"
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
            let display_val = if v.len() > 40 { &v[..40] } else { v };
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
fn exit_code_for_status(status: &RunStatus) -> i32 {
    match status {
        RunStatus::Succeeded => 0,
        RunStatus::Failed => 1,
        RunStatus::CompletedWithErrors => 1,
        RunStatus::Cancelled => 5,
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

    // 3. System PATH via `which`.
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

    // 4. Fallback: next to current executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let fallback = dir.join("operator-macos-helper");
            if fallback.exists() {
                return Some(fallback);
            }
        }
    }

    // 5. Dev fallback: Swift build output relative to the executable.
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
