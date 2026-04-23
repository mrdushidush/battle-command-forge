use battlecommand_forge::MissionRunner;
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "battlecommand-forge",
    version,
    about = "Quality-First AI Coding Army"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Run a mission with the 9-stage quality pipeline
    Mission {
        /// Mission prompt (required unless --test is used)
        prompt: Option<String>,
        #[arg(long, default_value = "premium")]
        preset: String,
        /// Enable voice announcements
        #[arg(long)]
        voice: bool,
        /// Override architect model
        #[arg(long)]
        architect_model: Option<String>,
        /// Override tester model
        #[arg(long)]
        tester_model: Option<String>,
        /// Override coder model
        #[arg(long)]
        coder_model: Option<String>,
        /// Override reviewer model (security + critique + CTO)
        #[arg(long)]
        reviewer_model: Option<String>,
        /// Auto mode: skip human approval, auto-continue fix rounds
        #[arg(long)]
        auto: bool,
        /// Custom output directory (default: auto-generated from prompt)
        #[arg(long, short = 'o')]
        output: Option<String>,
        /// Clone a GitHub repo as context for the mission
        #[arg(long)]
        repo: Option<String>,
        /// Use a local directory as context for the mission
        #[arg(long)]
        path: Option<String>,
        /// Run built-in test mission (C++ CRC-32 calculator) to verify the pipeline
        #[arg(long)]
        test: bool,
    },
    /// Edit an existing codebase with AI assistance
    Edit {
        #[arg(long, default_value = ".")]
        path: String,
        prompt: String,
        #[arg(long, default_value = "premium")]
        preset: String,
    },
    /// Run the stress test suite (C4-C9 graded tasks)
    Stress {
        /// Number of tasks to run (max 21)
        #[arg(long, default_value = "21")]
        tasks: usize,
        #[arg(long, default_value = "premium")]
        preset: String,
    },
    /// Launch the interactive TUI
    Tui,
    /// List and benchmark available models
    Models {
        #[command(subcommand)]
        action: Option<ModelsAction>,
    },
    /// GitHub operations (push, create-pr)
    Github {
        #[command(subcommand)]
        action: GithubAction,
    },
    /// View pipeline run reports
    Report {
        #[command(subcommand)]
        action: ReportAction,
    },
    /// Show system status and mission history
    Status,
    /// Show audit log
    Audit {
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Show or generate model configuration
    Settings {
        #[command(subcommand)]
        action: SettingsAction,
    },
    /// Show hardware metrics
    Hw,
    /// Run verifier only on existing output (fast debug)
    Verify {
        /// Path to output directory
        #[arg(long)]
        path: String,
        /// Language override
        #[arg(long, default_value = "python")]
        lang: String,
    },
    /// Chat with the CTO agent to plan missions (CLI REPL)
    Chat {
        #[arg(long, default_value = "premium")]
        preset: String,
    },
    /// SWE-bench evaluation framework — test against real GitHub issues
    Swebench {
        #[command(subcommand)]
        action: SwebenchAction,
    },
    /// Run multi-model benchmark (5 graded missions)
    Benchmark {
        /// Phase: full (premium preset) or quick (fast preset)
        #[arg(long, default_value = "full")]
        phase: String,
        /// Number of benchmark tasks (max 5)
        #[arg(long, default_value = "5")]
        tasks: usize,
    },
    /// Swarm mode: iterate planner→coder→QA, pick best version
    Swarm {
        prompt: String,
        /// Number of iterations (default 3)
        #[arg(long, default_value = "3")]
        iterations: u32,
        #[arg(long, default_value = "premium")]
        preset: String,
        /// Output directory
        #[arg(long, short = 'o')]
        output: Option<String>,
        /// Language override (python, rust, cpp, go, javascript)
        #[arg(long, default_value = "python")]
        lang: String,
    },
    /// Initialize project with .battlecommand/ directory
    Init,
    /// Show quick-start guide with examples and common workflows
    Guide,
}

#[derive(clap::Subcommand)]
enum ModelsAction {
    List,
    Benchmark {
        model: String,
    },
    Presets,
    /// Profile hardware capabilities and estimate model VRAM requirements
    Profile,
}

#[derive(clap::Subcommand)]
enum ReportAction {
    /// List all pipeline reports
    List,
    /// Show a report (latest by default)
    Show {
        /// Path to a specific report JSON
        path: Option<String>,
    },
}

#[derive(clap::Subcommand)]
enum SettingsAction {
    /// Show resolved model config for a preset
    Show {
        #[arg(long, default_value = "premium")]
        preset: String,
    },
    /// Generate default .battlecommand/models.toml
    Init,
}

#[derive(clap::Subcommand)]
enum SwebenchAction {
    /// Run SWE-bench evaluation
    Run {
        /// Dataset variant: lite, verified, or full
        #[arg(long, default_value = "lite")]
        variant: String,
        /// Path to dataset JSON file
        #[arg(long)]
        dataset: Option<String>,
        /// Run only this instance ID
        #[arg(long)]
        instance: Option<String>,
        /// Max instances to run
        #[arg(long)]
        limit: Option<u32>,
        /// Skip first N instances
        #[arg(long, default_value = "0")]
        offset: u32,
        /// Model override (default: claude-sonnet-4-6)
        #[arg(long)]
        model: Option<String>,
        /// Max agent turns per instance (default 25)
        #[arg(long, default_value = "25")]
        max_turns: u32,
        /// Timeout per instance in seconds (default 1800)
        #[arg(long, default_value = "1800")]
        timeout: u64,
        /// Resume from previous run (skip completed instances)
        #[arg(long)]
        resume: bool,
    },
    /// Generate report from results
    Report,
    /// List instances in dataset
    List {
        /// Filter by repository name
        #[arg(long)]
        repo: Option<String>,
        /// Dataset variant
        #[arg(long, default_value = "lite")]
        variant: String,
    },
}

#[derive(clap::Subcommand)]
enum GithubAction {
    /// Push workspace to GitHub
    Push {
        #[arg(long, default_value = ".")]
        workspace: String,
        #[arg(long, default_value = "main")]
        branch: String,
    },
    /// Create a pull request
    CreatePr {
        #[arg(long, default_value = ".")]
        workspace: String,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        body: String,
        #[arg(long, default_value = "main")]
        base: String,
    },
    /// Check if gh CLI is available
    Check,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file (silently ignore if missing)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    match cli.cmd {
        Command::Mission {
            prompt,
            preset,
            voice,
            architect_model,
            tester_model,
            coder_model,
            reviewer_model,
            auto,
            output,
            repo,
            path,
            test,
        } => {
            let prompt = if test {
                "Build a C++ CRC-32 calculator. Single file `tasks/crc32.cpp` with: CRC-32 using polynomial 0xEDB88320, functions for crc32_byte() and crc32_string(), main() that tests 3 known CRC-32 values and prints PASS if all correct. Compile: c++ -std=c++17 -Wall -o /tmp/bc_crc32 tasks/crc32.cpp && /tmp/bc_crc32".to_string()
            } else if let Some(p) = prompt {
                if p.trim().is_empty() {
                    eprintln!("Error: mission prompt cannot be empty. Use --test for a built-in test mission.");
                    std::process::exit(1);
                }
                p
            } else {
                eprintln!("Error: mission prompt is required. Usage: battlecommand-forge mission \"<prompt>\"");
                eprintln!("       Or use --test for a built-in test mission.");
                std::process::exit(1);
            };
            init_tracing();
            if voice {
                // SAFETY: called before tokio spawns threads (single-threaded at this point)
                unsafe {
                    std::env::set_var("BATTLECOMMAND_VOICE", "1");
                }
            }
            battlecommand_forge::enterprise::audit_log("mission:create", &prompt, &preset)?;
            battlecommand_forge::voice::mission_start(&prompt);

            let preset_enum = preset
                .parse::<battlecommand_forge::model_config::Preset>()
                .unwrap_or(battlecommand_forge::model_config::Preset::Premium);
            let config = battlecommand_forge::model_config::ModelConfig::resolve(
                preset_enum,
                ".",
                architect_model.as_deref(),
                tester_model.as_deref(),
                coder_model.as_deref(),
                reviewer_model.as_deref(),
            );
            let mut runner = MissionRunner::new(config);
            runner.auto_mode = auto;
            runner.output_override = output.map(std::path::PathBuf::from);
            runner.repo_url = repo;
            runner.local_path = path.map(std::path::PathBuf::from);
            runner.run(&prompt).await?;
        }
        Command::Edit {
            path,
            prompt,
            preset,
        } => {
            init_tracing();
            let dir = std::path::Path::new(&path);
            println!("Analyzing project at {}...", dir.display());
            let tree = battlecommand_forge::editor::build_file_tree(dir)?;
            println!("File tree:\n{}", tree);

            let preset_enum = preset
                .parse::<battlecommand_forge::model_config::Preset>()
                .unwrap_or(battlecommand_forge::model_config::Preset::Premium);
            let config = battlecommand_forge::model_config::ModelConfig::resolve(
                preset_enum,
                ".",
                None,
                None,
                None,
                None,
            );
            let llm = battlecommand_forge::llm::LlmClient::with_limits(
                &config.coder.model,
                config.coder.context_size(),
                config.coder.max_predict(),
            );
            let qb =
                std::fs::read_to_string(".battlecommand/quality_policies.md").unwrap_or_default();

            println!("Planning edits (model: {})...", config.coder.model);
            let plan = battlecommand_forge::editor::plan_edit(&llm, dir, &prompt, &qb).await?;
            println!("Plan: {}", plan.summary);
            println!(
                "  Modify: {} | Create: {} | Delete: {}",
                plan.files_to_modify.len(),
                plan.files_to_create.len(),
                plan.files_to_delete.len()
            );

            println!("Applying edits...");
            let modified =
                battlecommand_forge::editor::apply_edits(&llm, dir, &plan, &prompt, &qb).await?;
            println!("Done. {} files changed.", modified.len());
        }
        Command::Stress { tasks, preset } => {
            init_tracing();
            let preset_enum = preset
                .parse::<battlecommand_forge::model_config::Preset>()
                .unwrap_or(battlecommand_forge::model_config::Preset::Premium);
            let config = battlecommand_forge::model_config::ModelConfig::resolve(
                preset_enum,
                ".",
                None,
                None,
                None,
                None,
            );
            let llm = battlecommand_forge::llm::LlmClient::with_limits(
                &config.coder.model,
                config.coder.context_size(),
                config.coder.max_predict(),
            );
            battlecommand_forge::stress::run_stress(&llm, tasks).await?;
        }
        Command::Tui => {
            battlecommand_forge::tui::run_tui().await?;
        }
        Command::Chat { preset } => {
            let preset_enum = preset
                .parse::<battlecommand_forge::model_config::Preset>()
                .unwrap_or(battlecommand_forge::model_config::Preset::Premium);
            let config = battlecommand_forge::model_config::ModelConfig::resolve(
                preset_enum,
                ".",
                None,
                None,
                None,
                None,
            );
            run_chat(config).await?;
        }
        Command::Models { action } => {
            match action.unwrap_or(ModelsAction::List) {
                ModelsAction::List => {
                    let models = battlecommand_forge::models::list_ollama_models().await?;
                    println!("{:<45} {:>10} MODIFIED", "MODEL", "SIZE");
                    println!("{}", "-".repeat(70));
                    for m in &models {
                        println!("{:<45} {:>10} {}", m.name, m.size, m.modified);
                    }
                    println!("\n{} models available", models.len());
                }
                ModelsAction::Benchmark { model } => {
                    println!("Benchmarking {}...", model);
                    let result = battlecommand_forge::models::benchmark_model(&model).await?;
                    println!("{}", result);
                    println!(
                        "VRAM estimate: {:.1} GB",
                        battlecommand_forge::models::estimate_vram_gb(&model)
                    );
                }
                ModelsAction::Presets => {
                    for p in &["fast", "balanced", "premium"] {
                        let c = battlecommand_forge::models::get_preset(p);
                        println!("[{}] coder={}", c.name, c.coder);
                    }
                }
                ModelsAction::Profile => {
                    let metrics = battlecommand_forge::hardware::collect_metrics().await;
                    println!("Hardware Profile");
                    println!("================");
                    println!(
                        "CPU:    {} ({} cores, {:.0}% usage)",
                        metrics.cpu_name, metrics.core_count, metrics.cpu_usage_total
                    );
                    println!(
                        "RAM:    {:.1} GB used / {:.1} GB total ({:.1} GB available)",
                        metrics.mem_used_gb, metrics.mem_total_gb, metrics.mem_available_gb
                    );
                    println!(
                        "VRAM:   {:.1} GB (Ollama loaded models)",
                        metrics.ollama_vram_total_gb.abs()
                    );
                    if metrics.ollama_cpu_pct > 0.0 {
                        println!(
                            "Ollama: {:.0}% CPU, {:.1} GB RAM",
                            metrics.ollama_cpu_pct, metrics.ollama_mem_gb
                        );
                    }
                    println!();
                    // Show VRAM estimates for loaded models
                    let models = battlecommand_forge::models::list_ollama_models()
                        .await
                        .unwrap_or_default();
                    if !models.is_empty() {
                        println!("{:<45} {:>10} {:>12}", "MODEL", "SIZE", "EST. VRAM");
                        println!("{}", "-".repeat(70));
                        for m in &models {
                            let vram = battlecommand_forge::models::estimate_vram_gb(&m.name);
                            println!("{:<45} {:>10} {:>10.1} GB", m.name, m.size, vram);
                        }
                        println!("\n{} models available", models.len());
                    }
                }
            }
        }
        Command::Github { action } => match action {
            GithubAction::Check => {
                if battlecommand_forge::github::GitHubOps::is_available() {
                    println!("gh CLI: authenticated and ready");
                } else {
                    println!("gh CLI: not available (install with: brew install gh)");
                }
            }
            GithubAction::Push { workspace, branch } => {
                let ops = battlecommand_forge::github::GitHubOps::new(&workspace);
                ops.push(&branch)?;
                println!("Pushed to origin/{}", branch);
            }
            GithubAction::CreatePr {
                workspace,
                title,
                body,
                base,
            } => {
                let ops = battlecommand_forge::github::GitHubOps::new(&workspace);
                let pr = ops.create_pr(&title, &body, &base)?;
                println!("PR created: {}", pr.url);
            }
        },
        Command::Status => {
            println!("BattleCommand Forge v{}", env!("CARGO_PKG_VERSION"));
            println!("Modules: 30 | Pipeline: 9-stage | Gate: 8.0-9.2/10 (scaled) | Fix rounds: 5");
            println!();

            let client = reqwest::Client::new();
            match client
                .get(format!(
                    "{}/api/tags",
                    battlecommand_forge::llm::ollama_url()
                ))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().await?;
                    let n = body["models"].as_array().map(|m| m.len()).unwrap_or(0);
                    println!("Ollama: connected ({} models)", n);
                }
                _ => println!("Ollama: not running"),
            }
            if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                println!("Claude: configured");
            } else {
                println!("Claude: not configured");
            }
            if battlecommand_forge::github::GitHubOps::is_available() {
                println!("GitHub: gh authenticated");
            } else {
                println!("GitHub: gh not available");
            }

            let ws = battlecommand_forge::workspace::list_workspaces().unwrap_or_default();
            println!("Workspaces: {}", ws.len());
            let stats = battlecommand_forge::db::get_stats()?;
            println!("{}", stats);
            let cost = battlecommand_forge::enterprise::total_cost()?;
            println!("Total cost: ${:.4}", cost.abs());
        }
        Command::Audit { limit } => {
            let entries = battlecommand_forge::enterprise::read_audit_log(limit)?;
            if entries.is_empty() {
                println!("No audit entries.");
            } else {
                for e in &entries {
                    println!(
                        "[{}] {} {} — {}",
                        e.timestamp, e.actor, e.action, e.resource
                    );
                }
            }
        }
        Command::Report { action } => match action {
            ReportAction::List => {
                let reports = battlecommand_forge::report::list_reports()?;
                if reports.is_empty() {
                    println!("No reports yet. Run a mission to generate one.");
                } else {
                    println!("{} reports:", reports.len());
                    for r in &reports {
                        println!("  {}", r.display());
                    }
                }
            }
            ReportAction::Show { path } => {
                let report_path = match path {
                    Some(p) => std::path::PathBuf::from(p),
                    None => std::path::PathBuf::from(".battlecommand/reports/latest.json"),
                };
                if !report_path.exists() {
                    println!("Report not found: {}", report_path.display());
                    println!("Run a mission first, or specify a path.");
                } else {
                    let report = battlecommand_forge::report::load_report(&report_path)?;
                    battlecommand_forge::report::print_report(&report);
                }
            }
        },
        Command::Settings { action } => match action {
            SettingsAction::Show { preset } => {
                let preset_enum = preset
                    .parse::<battlecommand_forge::model_config::Preset>()
                    .unwrap_or(battlecommand_forge::model_config::Preset::Premium);
                let config = battlecommand_forge::model_config::ModelConfig::resolve(
                    preset_enum,
                    ".",
                    None,
                    None,
                    None,
                    None,
                );
                config.print_summary();
            }
            SettingsAction::Init => {
                let path = ".battlecommand/models.toml";
                if std::path::Path::new(path).exists() {
                    println!("{} already exists", path);
                } else {
                    std::fs::create_dir_all(".battlecommand")?;
                    std::fs::write(
                        path,
                        battlecommand_forge::model_config::ModelConfig::generate_default_toml(),
                    )?;
                    println!("Created {}", path);
                    println!("Edit it to customize per-role model assignments.");
                }
            }
        },
        Command::Hw => {
            let metrics = battlecommand_forge::hardware::collect_metrics().await;
            print!("{}", metrics);
        }
        Command::Verify { path, lang } => {
            let dir = std::path::Path::new(&path);
            if !dir.exists() {
                println!("Directory not found: {}", path);
                return Ok(());
            }
            println!("=== VERIFY: {} (lang={}) ===", path, lang);
            let report = battlecommand_forge::verifier::verify_project(dir, &lang)?;
            println!(
                "Avg score: {:.1}/10 | Files: {} | Tests: {} passed, {} failed",
                report.avg_score,
                report.file_reports.len(),
                report.tests_passed,
                report.tests_failed
            );
            if !report.test_errors.is_empty() {
                println!("\nTest errors:");
                for err in &report.test_errors {
                    println!("  {}", err);
                }
            }
            let issues: Vec<_> = report
                .file_reports
                .iter()
                .filter(|(_, r)| !r.lint_issues.is_empty())
                .collect();
            if !issues.is_empty() {
                println!("\nLint issues:");
                for (f, r) in &issues {
                    for issue in &r.lint_issues {
                        println!("  {}: {}", f, issue);
                    }
                }
            }
            let secrets: Vec<_> = report
                .file_reports
                .iter()
                .filter(|(_, r)| r.has_hardcoded_secrets)
                .collect();
            if !secrets.is_empty() {
                println!("\nSecrets found in:");
                for (f, _) in &secrets {
                    println!("  {}", f);
                }
            }
        }
        Command::Swebench { action } => {
            init_tracing();
            match action {
                SwebenchAction::Run {
                    variant,
                    dataset,
                    instance,
                    limit,
                    offset,
                    model,
                    max_turns,
                    timeout,
                    resume,
                } => {
                    let opts = battlecommand_forge::swebench::SwebenchOpts {
                        dataset_path: dataset,
                        variant,
                        instance_filter: instance,
                        limit,
                        offset,
                        output_dir: ".battlecommand/swebench".into(),
                        model_override: model,
                        max_turns,
                        timeout_secs: timeout,
                        resume,
                    };
                    battlecommand_forge::swebench::run_batch(&opts).await?;
                }
                SwebenchAction::Report => {
                    battlecommand_forge::swebench_eval::generate_report(".battlecommand/swebench")?;
                }
                SwebenchAction::List { repo, variant } => {
                    let opts = battlecommand_forge::swebench::SwebenchOpts {
                        variant,
                        ..Default::default()
                    };
                    battlecommand_forge::swebench::list_instances(&opts, repo.as_deref())?;
                }
            }
        }
        Command::Benchmark { phase, tasks } => {
            init_tracing();
            battlecommand_forge::benchmark::run_benchmark(&phase, tasks).await?;
        }
        Command::Swarm {
            prompt,
            iterations,
            preset,
            output,
            lang,
        } => {
            init_tracing();
            let preset_enum = preset
                .parse::<battlecommand_forge::model_config::Preset>()
                .unwrap_or(battlecommand_forge::model_config::Preset::Premium);
            let config = battlecommand_forge::model_config::ModelConfig::resolve(
                preset_enum,
                ".",
                None,
                None,
                None,
                None,
            );
            let opts = battlecommand_forge::swarm::SwarmOpts {
                iterations,
                output_dir: output.unwrap_or_else(|| "output/swarm".into()),
                language: lang,
            };
            battlecommand_forge::swarm::run_swarm(&prompt, &config, &opts).await?;
        }
        Command::Init => {
            let dir = ".battlecommand";
            std::fs::create_dir_all(dir)?;
            let ctx_path = format!("{}/context.md", dir);
            if !std::path::Path::new(&ctx_path).exists() {
                std::fs::write(&ctx_path, "# Project Context\n\nDescribe your project here. This context is injected into mission prompts.\n")?;
                println!("Created {}", ctx_path);
            } else {
                println!("{} already exists", ctx_path);
            }
            // Create commands directory
            let cmds = format!("{}/commands", dir);
            std::fs::create_dir_all(&cmds)?;
            println!("Initialized .battlecommand/ directory");
            println!("  context.md  — Project context (injected into prompts)");
            println!("  commands/   — Custom commands (create <name>.md files)");
            // Create example custom command
            battlecommand_forge::custom_commands::create_example_command(".").await?;
        }
        Command::Guide => {
            print_help();
        }
    }

    Ok(())
}

fn print_help() {
    println!(
        "\nBattleCommand Forge v{} — Quality-First AI Coding Army",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        r#"══════════════════════════════════════════════════════════

QUICK START
  1. Install Ollama: brew install ollama && ollama serve
  2. Pull a model: ollama pull qwen2.5-coder:7b
  3. Run a mission:
     battlecommand-forge mission "Build a Python CSV to JSON CLI" --preset fast --auto
  4. Your project is in output/

COMMANDS
  mission "<prompt>" [--preset fast|balanced|premium] [--auto] [-o dir]
      Run the full 9-stage quality pipeline. Add --auto to skip manual approval.
      --test              Run built-in test mission to verify the pipeline
      --repo <url>        Clone a GitHub repo as context
      --path <dir>        Use a local directory as context

  chat [--preset premium]
      CTO research agent — plan missions, search the web, then launch with /mission.

  tui
      Interactive terminal UI with CTO chat, model picker, and 15 slash commands.

  edit --path <dir> "<what to change>"
      Modify an existing codebase with AI assistance.

  swarm "<prompt>" [--iterations 3] [--preset premium] [--lang python]
      Iterate planner→coder→QA, pick the best version from N attempts.

  swebench run [--variant lite|verified|full] [--model m] [--resume]
      SWE-bench evaluation — test against real GitHub issues.

  benchmark [--phase full|quick] [--tasks 5]
      Run multi-model benchmark with 5 graded missions.

  init
      Initialize project with .battlecommand/ directory and custom commands.

  verify --path <dir> [--lang python]
      Run quality checks (venv + ruff + pytest) on a project.

  status          System health, mission history, total cost
  models list     Show available Ollama models
  models profile  Hardware capabilities + VRAM estimates
  report list     View pipeline reports
  audit           Show audit log
  settings init   Generate .battlecommand/models.toml
  hw              Hardware metrics (CPU, RAM, VRAM)
  guide           This guide

PRESETS
  fast        All 7B models, $0, ~8GB VRAM, fastest
  balanced    32B models, $0, ~20GB VRAM, better quality
  premium     Opus tester + 80B coder + Sonnet reviews, ~$0.30/mission, best quality

EXAMPLES
  # Simple CLI tool (fast, free)
  battlecommand-forge mission "Build a Python CLI that converts CSV to JSON" --preset fast --auto

  # REST API (premium quality)
  battlecommand-forge mission "Build a FastAPI todo app with SQLite CRUD" --preset premium --auto

  # Verify the pipeline works
  battlecommand-forge mission --test --preset fast --auto

  # Edit existing code
  battlecommand-forge edit --path ./my-project "Add pagination to all list endpoints"

  # Swarm mode: 5 iterations, pick best
  battlecommand-forge swarm "Build a URL shortener" --iterations 5 --preset premium

  # SWE-bench evaluation
  battlecommand-forge swebench run --variant lite --model claude-sonnet-4-6

  # Remote GPU
  OLLAMA_HOST=192.168.1.100:11434 battlecommand-forge mission "..." --auto

ENVIRONMENT VARIABLES
  ANTHROPIC_API_KEY   Claude API key (required for premium preset)
  XAI_API_KEY         xAI API key (for Grok architect)
  OLLAMA_HOST         Remote Ollama URL (e.g. 192.168.1.100:11434)
  BRAVE_API_KEY       CTO web search (falls back to DuckDuckGo)
  CODER_MODEL         Override coder model for any preset
  REVIEWER_MODEL      Override security + critique + CTO together

TUI SLASH COMMANDS (in chat tab)
  /mission <prompt>    Launch a mission
  /verify [path]       Run verifier on output
  /report              View pipeline reports
  /audit [n]           Show audit log
  /preset <name>       Switch preset
  /cost                Show API spend
  /settings            Model picker
  /clear               Clear chat + CTO history
  /compress            Compact long CTO history
  /models /hw /status  Switch tabs / info
  /help                All commands
"#
    );
}

async fn run_chat(config: battlecommand_forge::model_config::ModelConfig) -> anyhow::Result<()> {
    use battlecommand_forge::llm::{LlmClient, StreamEvent};
    use std::io::{self, BufRead, Write};
    use tokio::sync::mpsc;

    let cto_model = &config.cto.model;
    let llm = LlmClient::with_limits(
        cto_model,
        config.cto.context_size(),
        config.cto.max_predict(),
    );
    let mut agent = battlecommand_forge::cto::CtoAgent::new(llm);
    agent.set_model_config(config.clone());
    agent.load_history().ok();

    println!("BattleCommand Forge — CTO Chat ({})", cto_model);
    println!("Plan your mission, research architecture, or ask anything.");
    println!("Type /mission <prompt> to launch. /clear to reset. /quit to exit.\n");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("\x1b[1;36m> \x1b[0m");
        stdout.flush()?;

        let mut input = String::new();
        if stdin.lock().read_line(&mut input)? == 0 {
            break; // EOF
        }
        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }

        // ── Slash commands ──
        if input == "/quit" || input == "/exit" || input == "/q" {
            println!("Goodbye.");
            break;
        }
        if input == "/clear" {
            agent.clear_history();
            agent.save_history().ok();
            println!("\x1b[2mChat history cleared.\x1b[0m");
            continue;
        }
        if input == "/compress" {
            agent.compact_history();
            agent.save_history().ok();
            println!(
                "\x1b[2mHistory compacted to {} messages.\x1b[0m",
                agent.history_len()
            );
            continue;
        }
        if input == "/help" {
            println!("  /mission <prompt>  Launch a mission with the full pipeline");
            println!("  /clear             Clear conversation history");
            println!("  /compress          Compact long history");
            println!("  /quit              Exit chat");
            println!("  Or type anything to chat with the CTO agent");
            continue;
        }
        if input.starts_with("/mission ") {
            let prompt = input.strip_prefix("/mission ").unwrap_or("").trim();
            if prompt.is_empty() {
                println!("Usage: /mission <prompt>");
                continue;
            }
            println!("\n\x1b[1;33mLaunching mission: {}\x1b[0m\n", prompt);
            agent.save_history().ok();
            let mut runner = MissionRunner::new(config.clone());
            runner.auto_mode = true;
            runner.run(prompt).await?;
            println!("\n\x1b[1;32mMission complete. Back to chat.\x1b[0m\n");
            continue;
        }

        // ── Chat with CTO (with tool call display) ──
        let (tx, mut rx) = mpsc::channel(512);
        agent.set_event_tx(tx);

        // Run chat in a spawned task so we can display events
        let mut agent_moved = agent;
        let input_clone = input.clone();
        let handle = tokio::spawn(async move {
            let result = agent_moved.chat(&input_clone).await;
            (agent_moved, result)
        });

        // Display tool calls as they happen
        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::ToolCallStart { name, args } => {
                    print!("\x1b[2m  [{}", name);
                    // Show compact args
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&args) {
                        if let Some(q) = v
                            .get("query")
                            .or(v.get("prompt"))
                            .or(v.get("path"))
                            .or(v.get("url"))
                            .or(v.get("directory"))
                        {
                            print!(": {}", q.as_str().unwrap_or(&args));
                        }
                    }
                    println!("]\x1b[0m");
                }
                StreamEvent::ToolCallResult { name, result } => {
                    let preview: String = result.lines().take(3).collect::<Vec<_>>().join(" | ");
                    let preview = if preview.len() > 120 {
                        format!("{}...", &preview[..120])
                    } else {
                        preview
                    };
                    println!("\x1b[2m  [{} → {}]\x1b[0m", name, preview);
                }
                _ => {}
            }
        }

        let (returned_agent, result) = handle.await?;
        agent = returned_agent;

        match result {
            Ok(response) => {
                println!("\n\x1b[1;37m{}\x1b[0m\n", response);
            }
            Err(e) => {
                println!("\n\x1b[1;31mError: {}\x1b[0m\n", e);
            }
        }
    }

    agent.save_history().ok();
    Ok(())
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "battlecommand_forge=info".into()),
        )
        .init();
}
