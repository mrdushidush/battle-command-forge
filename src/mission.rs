use crate::codegen::{self, GeneratedFile};
use crate::db;
use crate::llm::{self, LlmCallStats, LlmClient};
use crate::memory;
use crate::model_config::ModelConfig;
use crate::report::{
    self, CritiqueReport, CritiqueScores, CtoReport, FileVerifierReport, LlmStageReport,
    MissionMeta, ReportBuilder, RoundReport, SecurityReport, VerifierReport,
};
use crate::router;
use crate::verifier::{self, QualityReport};
use crate::voice;
use crate::workspace::{self, Workspace};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

/// Events emitted by MissionRunner for TUI consumption.
#[derive(Debug, Clone)]
pub enum TuiEvent {
    /// Structured log message with level
    Log { level: String, message: String },
    /// Pipeline stage started
    StageStarted {
        stage: String,
        step: String,
        model: String,
    },
    /// Pipeline stage completed
    StageCompleted { stage: String, status: String },
    /// Code chunk from coder (for Code tab)
    CodeChunk {
        content: String,
        model: String,
        done: bool,
    },
    /// Mission completed with final score
    MissionCompleted { score: f64, output_dir: String },
    /// Mission failed
    MissionFailed { error: String },
    /// Cost update from API call
    CostUpdate { total_usd: f64 },
    /// LLM thinking/reasoning chunk for live visualization
    ThinkingChunk {
        model: String,
        content: String,
        done: bool,
    },
}

const MAX_FIX_ROUNDS: usize = 5;

/// Quality gate threshold scaled by complexity.
/// C1-C6: 9.2 (achievable with good models)
/// C7-C8: 8.5 (complex multi-file projects)
/// C9-C10: 8.0 (mega-projects)
fn quality_gate(complexity: u32) -> f32 {
    match complexity {
        0..=6 => 9.2,
        7..=8 => 8.5,
        _ => 8.0,
    }
}

/// Result of a single attempt through stages 4-8.
struct AttemptResult {
    files: Vec<GeneratedFile>,
    reports: Vec<QualityReport>,
    critique_scores: Vec<f32>,
    critique_details: Vec<String>,
    security_verdict: String,
    cto_verdict: String,
    verifier_score: f32,
    critique_avg: f32,
    final_score: f32,
    /// Import/test errors captured from pytest for surgical fix targeting
    test_errors: Vec<String>,
    tests_passed: u32,
    tests_failed: u32,
}

pub struct MissionRunner {
    config: ModelConfig,
    llm_architect: LlmClient,
    llm_tester: LlmClient,
    llm_coder: LlmClient,
    llm_fix_coder: LlmClient,
    llm_security: LlmClient,
    llm_critique: LlmClient,
    llm_cto: LlmClient,
    quality_bible: String,
    pub auto_mode: bool,
    /// Custom output directory override
    pub output_override: Option<PathBuf>,
    /// GitHub repo URL to clone as context
    pub repo_url: Option<String>,
    /// Local directory to use as context
    pub local_path: Option<PathBuf>,
    /// Loaded repo/project context (file tree + source) for injection into prompts
    repo_context: Option<String>,
    /// Complexity score from router (C1-C10). Determines single-shot vs staged generation.
    complexity: u32,
    /// Best score achieved across all fix rounds (for benchmark reporting).
    pub last_best_score: f64,
    /// Optional TUI event sender for live updates.
    pub event_tx: Option<mpsc::UnboundedSender<TuiEvent>>,
}

impl MissionRunner {
    pub fn new(config: ModelConfig) -> Self {
        config.print_summary();

        let quality_bible = fs::read_to_string(".battlecommand/quality_policies.md")
            .unwrap_or_else(|_| {
                "TDD first, 90% coverage, OWASP, zero TODOs, full error handling.".to_string()
            });

        Self {
            llm_architect: LlmClient::with_limits(
                &config.architect.model,
                config.architect.context_size(),
                config.architect.max_predict(),
            ),
            llm_tester: LlmClient::with_limits(
                &config.tester.model,
                config.tester.context_size(),
                config.tester.max_predict(),
            ),
            llm_coder: LlmClient::with_limits(
                &config.coder.model,
                config.coder.context_size(),
                config.coder.max_predict(),
            ),
            llm_fix_coder: LlmClient::with_limits(
                &config.fix_coder.model,
                config.fix_coder.context_size(),
                config.fix_coder.max_predict(),
            ),
            llm_security: LlmClient::with_limits(
                &config.security.model,
                config.security.context_size(),
                config.security.max_predict(),
            ),
            llm_critique: LlmClient::with_limits(
                &config.critique.model,
                config.critique.context_size(),
                config.critique.max_predict(),
            ),
            llm_cto: LlmClient::with_limits(
                &config.cto.model,
                config.cto.context_size(),
                config.cto.max_predict(),
            ),
            config,
            quality_bible,
            auto_mode: false,
            output_override: None,
            repo_url: None,
            local_path: None,
            repo_context: None,
            complexity: 5,
            last_best_score: 0.0,
            event_tx: None,
        }
    }

    /// Returns the best score achieved in the most recent run.
    pub fn best_score(&self) -> f64 {
        self.last_best_score
    }

    /// Emit a TUI event (no-op if no listener).
    fn emit(&self, event: TuiEvent) {
        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(event);
        }
    }

    fn emit_log(&self, level: &str, message: &str) {
        println!("{}", message);
        self.emit(TuiEvent::Log {
            level: level.into(),
            message: message.into(),
        });
    }

    fn emit_stage(&self, stage: &str, step: &str, model: &str) {
        self.emit(TuiEvent::StageStarted {
            stage: stage.into(),
            step: step.into(),
            model: model.into(),
        });
    }

    fn emit_stage_done(&self, stage: &str, status: &str) {
        self.emit(TuiEvent::StageCompleted {
            stage: stage.into(),
            status: status.into(),
        });
    }

    pub async fn run(&mut self, prompt: &str) -> Result<()> {
        println!();
        println!("=== BattleCommand Forge v{} ===", env!("CARGO_PKG_VERSION"));
        println!("Preset: {}", self.config.preset);
        println!();

        // Check for API keys if any role uses cloud models
        let roles = [
            (&self.config.architect, "architect"),
            (&self.config.tester, "tester"),
            (&self.config.coder, "coder"),
            (&self.config.fix_coder, "fix_coder"),
            (&self.config.security, "security"),
            (&self.config.critique, "critique"),
            (&self.config.cto, "cto"),
        ];
        let claude_roles: Vec<&str> = roles
            .iter()
            .filter(|(r, _)| r.model.starts_with("claude-"))
            .map(|(_, name)| *name)
            .collect();
        if !claude_roles.is_empty() && std::env::var("ANTHROPIC_API_KEY").is_err() {
            anyhow::bail!(
                "ANTHROPIC_API_KEY not set but {} use Claude models ({}). Set it with: export ANTHROPIC_API_KEY=sk-...",
                claude_roles.len(), claude_roles.join(", ")
            );
        }
        let grok_roles: Vec<&str> = roles
            .iter()
            .filter(|(r, _)| r.model.starts_with("grok-"))
            .map(|(_, name)| *name)
            .collect();
        if !grok_roles.is_empty() && std::env::var("XAI_API_KEY").is_err() {
            anyhow::bail!(
                "XAI_API_KEY not set but {} use Grok models ({}). Set it with: export XAI_API_KEY=xai-...",
                grok_roles.len(), grok_roles.join(", ")
            );
        }

        let pipeline_start = std::time::Instant::now();

        // Initialize report builder
        let mut rb = ReportBuilder::new();
        rb.set_model_config(&self.config);

        // Load memory context
        let memory_context = memory::load_context(prompt);
        if !memory_context.is_empty() {
            self.emit_log(
                "info",
                &format!("[MEMORY] Loaded context ({} chars)", memory_context.len()),
            );
        }

        // Stage 1: Complexity Assessment (dual: rules + AI)
        let router_start = std::time::Instant::now();
        self.emit_stage("1/9", "ROUTER", &self.config.complexity.model.clone());
        let llm_complexity = LlmClient::with_limits(
            &self.config.complexity.model,
            self.config.complexity.context_size(),
            self.config.complexity.max_predict(),
        );
        let routing = router::assess_complexity_dual(prompt, &llm_complexity).await;
        let tier = routing.tier;
        self.complexity = routing.complexity;
        self.emit_log(
            "info",
            &format!("[1/9] ROUTER: {} ({})", tier.label(), routing.reasoning),
        );
        self.emit_stage_done("1/9", &format!("C{} {}", routing.complexity, tier.label()));
        rb.set_router(
            &format!(
                "C{} {} ({})",
                routing.complexity,
                tier.label(),
                routing.source
            ),
            router_start.elapsed().as_secs_f64(),
        );

        // C1-C6: if architect is a cloud model (Grok/Claude), downgrade to the configured
        // coder's local model for detailed specs (local models produce better specs for simple tasks)
        if self.complexity < 7
            && self.config.architect.provider == crate::model_config::ModelProvider::Cloud
        {
            // Use the coder model as fallback — it's always configured and typically local
            if self.config.coder.provider == crate::model_config::ModelProvider::Local {
                let local_arch = self.config.coder.model.clone();
                println!("[DOWNGRADE] C{} detected — switching architect from {} to {} for detailed specs",
                    self.complexity, self.config.architect.model, local_arch);
                self.config.architect.model = local_arch.clone();
                self.config.architect.provider = crate::model_config::ModelProvider::Local;
                self.llm_architect = LlmClient::with_limits(
                    &local_arch,
                    self.config.architect.context_size(),
                    self.config.architect.max_predict(),
                );
            }
        }

        // C7+ complexity: upgrade coder to the fix_coder model for precision on complex projects
        // Auth/E-commerce/WebSocket consistently land at C7-C8
        if self.complexity >= 7
            && !self.config.coder.model.starts_with("claude-")
            && !self.config.coder.model.starts_with("grok-")
        {
            // Use the fix_coder model — it's always a capable cloud model in premium preset
            if self.config.fix_coder.provider == crate::model_config::ModelProvider::Cloud {
                let upgrade = self.config.fix_coder.model.clone();
                println!(
                    "[UPGRADE] C{} detected — switching coder from {} to {} for precision",
                    self.complexity, self.config.coder.model, upgrade
                );
                self.config.coder.model = upgrade.clone();
                self.config.coder.provider = crate::model_config::ModelProvider::Cloud;
                self.llm_coder = LlmClient::with_limits(
                    &upgrade,
                    self.config.coder.context_size(),
                    self.config.coder.max_predict(),
                );
            }
        }

        // Create workspace + output dir
        let mission_id = workspace::mission_id_from_prompt(prompt);
        let ws = Workspace::create(&mission_id).ok();
        if ws.is_some() {
            println!("[WORKSPACE] Created: {}", mission_id);
        }
        let output_dir = match &self.output_override {
            Some(dir) => {
                fs::create_dir_all(dir).context("Failed to create output directory")?;
                dir.clone()
            }
            None => create_output_dir(prompt)?,
        };

        let language = detect_language(prompt);
        rb.set_mission(MissionMeta {
            mission_id: mission_id.clone(),
            prompt: prompt.to_string(),
            preset: self.config.preset.to_string(),
            language: language.clone(),
            output_dir: output_dir.display().to_string(),
        });

        // Load repo/project context if --repo or --path specified
        if let Some(ref url) = self.repo_url {
            let clone_dir = output_dir.join(".repo_clone");
            crate::workspace::clone_repo(url, &clone_dir)?;
            self.repo_context = Some(crate::editor::read_project_context(&clone_dir, 50_000)?);
        } else if let Some(ref path) = self.local_path {
            self.repo_context = Some(crate::editor::read_project_context(path, 50_000)?);
        }

        // Run as single task — multi-file extraction handles project structure.
        // Decomposition caused duplicate projects; single-task + good prompts is better.
        self.run_single_task(prompt, prompt, &output_dir, &mut rb)
            .await?;

        // Save mission to history database
        let duration_secs = pipeline_start.elapsed().as_secs_f64();
        let latest_report =
            report::load_report(Path::new(".battlecommand/reports/latest.json")).ok();
        let (final_score, passed) = latest_report
            .as_ref()
            .map(|r| (r.result.best_score, r.result.quality_gate_passed))
            .unwrap_or((0.0, false));
        let files: Vec<String> = std::fs::read_dir(&output_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        let _ = db::save_mission(&db::MissionRecord {
            id: mission_id,
            prompt: prompt.to_string(),
            preset: self.config.preset.to_string(),
            tier: format!("C{}", self.complexity),
            subtasks: 0,
            rounds: latest_report
                .as_ref()
                .map(|r| r.result.total_rounds as u32)
                .unwrap_or(1),
            final_score,
            passed,
            model: self.config.coder.model.clone(),
            files_generated: files,
            duration_secs,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });

        // Clean up build artifacts from output
        cleanup_artifacts(&output_dir);

        self.print_results(&output_dir)?;
        Ok(())
    }

    /// Run the full pipeline (stages 2-9) for a single task/subtask.
    async fn run_single_task(
        &mut self,
        _original_prompt: &str,
        task_prompt: &str,
        output_dir: &Path,
        rb: &mut ReportBuilder,
    ) -> Result<()> {
        // Stage 2: Architect
        self.emit_stage("2/9", "ARCHITECT", &self.config.architect.model.clone());
        self.emit_log(
            "info",
            "[2/9] ARCHITECT: Designing spec + file manifest + TDD plan...",
        );
        let (spec, arch_stats) = self.run_architect_with_stats(task_prompt).await?;
        self.emit_stage_done(
            "2/9",
            &format!(
                "{} lines, {:.0}s",
                arch_stats.output_lines, arch_stats.duration_secs
            ),
        );
        rb.set_architect(LlmStageReport {
            model: arch_stats.model,
            duration_secs: arch_stats.duration_secs,
            token_count: arch_stats.token_count,
            tok_per_sec: arch_stats.tok_per_sec,
            output_lines: arch_stats.output_lines,
        });
        // Offload architect before tester (if different models)
        if self.config.architect.model != self.config.tester.model {
            offload_model(&self.config.architect.model).await;
        }

        // Stage 3: Tester-first
        self.emit_stage("3/9", "TESTER", &self.config.tester.model.clone());
        self.emit_log("info", "[3/9] TESTER: Writing tests first (TDD)...");
        let (tests_raw, test_stats) = self.run_tester_with_stats(task_prompt, &spec).await?;
        self.emit_stage_done(
            "3/9",
            &format!(
                "{} lines, {:.0}s",
                test_stats.output_lines, test_stats.duration_secs
            ),
        );
        rb.set_tester(LlmStageReport {
            model: test_stats.model,
            duration_secs: test_stats.duration_secs,
            token_count: test_stats.token_count,
            tok_per_sec: test_stats.tok_per_sec,
            output_lines: test_stats.output_lines,
        });

        // Offload tester before coder (if different models)
        if self.config.tester.model != self.config.coder.model {
            offload_model(&self.config.tester.model).await;
        }

        let language = detect_language(task_prompt);

        // === FIX-PASS RETRY LOOP (Stages 4-8) ===
        let mut feedback = String::new();
        let mut previous_code = String::new();
        let mut persistent_issues: Vec<String> = Vec::new();
        let mut best_result: Option<AttemptResult> = None;
        let mut best_round: usize = 1;

        for round in 0..MAX_FIX_ROUNDS {
            if round > 0 {
                self.emit_log(
                    "warn",
                    &format!("=== FIX ROUND {}/{} ===", round + 1, MAX_FIX_ROUNDS),
                );
            }

            let (result, round_report) = self
                .attempt_round_with_report(
                    task_prompt,
                    &spec,
                    &tests_raw,
                    &language,
                    output_dir,
                    round,
                    &feedback,
                    &previous_code,
                )
                .await?;

            // ─── Show full report for this round ───
            let min_score = quality_gate(self.complexity);
            let passed_gate = result.final_score >= min_score;

            // Build a snapshot report up to this round for display
            rb.add_round(round_report.clone());
            let snap_report = rb.build(
                passed_gate,
                result.final_score,
                round + 1,
                output_dir,
                &result.files,
            );
            report::print_report(&snap_report);
            // Pop the round back — we'll re-add it (possibly with feedback) below
            rb.rounds.pop();

            if passed_gate {
                voice::quality_gate(result.final_score, true);
            }

            // ─── Human-in-the-loop approval (or auto-mode) ───
            let remaining = MAX_FIX_ROUNDS - (round + 1);

            let (accept, abort) = if self.auto_mode {
                // Auto mode: accept if passed, continue fixing if not, accept on last round
                if passed_gate {
                    println!(
                        "[AUTO] Quality gate PASSED ({:.1}/10) — accepting",
                        result.final_score
                    );
                    (true, false)
                } else if remaining > 0 {
                    println!("[AUTO] Quality gate FAILED ({:.1} < {:.1}) — continuing to fix round ({} remaining)",
                        result.final_score, min_score, remaining);
                    (false, false)
                } else {
                    println!(
                        "[AUTO] Final round complete ({:.1}/10) — accepting",
                        result.final_score
                    );
                    (true, false)
                }
            } else {
                if passed_gate {
                    println!();
                    println!("Quality gate PASSED. What would you like to do?");
                    println!("  [a] Accept and ship (default)");
                    println!(
                        "  [f] Run another fix round anyway ({} remaining)",
                        remaining
                    );
                    println!("  [q] Abort mission");
                } else if remaining > 0 {
                    println!();
                    println!(
                        "Quality gate FAILED ({:.1} < {:.1}). What would you like to do?",
                        result.final_score, min_score
                    );
                    println!(
                        "  [f] Continue to next fix round (default, {} remaining)",
                        remaining
                    );
                    println!("  [a] Accept current output as-is");
                    println!("  [q] Abort mission");
                } else {
                    println!();
                    println!(
                        "Final round complete ({:.1}/10). What would you like to do?",
                        result.final_score
                    );
                    println!("  [a] Accept current output (default)");
                    println!("  [q] Abort mission");
                }

                print!("> ");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                let mut input = String::new();
                let _ = std::io::stdin().read_line(&mut input);
                let choice = input.trim().to_lowercase();

                let accept = if passed_gate {
                    choice != "f"
                } else if remaining > 0 {
                    choice == "a"
                } else {
                    choice != "q"
                };
                let abort = choice == "q";
                (accept, abort)
            };

            if abort {
                println!();
                println!("Mission aborted by user.");

                rb.add_round(round_report);
                let score = best_result
                    .as_ref()
                    .map(|b| b.final_score)
                    .unwrap_or(result.final_score);
                let report = rb.build(false, score, best_round, output_dir, &result.files);
                let _ = report::save_report(&report);

                return Ok(());
            }

            if accept {
                let accepted_passed = result.final_score >= min_score;
                rb.add_round(round_report);

                let report = rb.build(
                    accepted_passed,
                    result.final_score,
                    round + 1,
                    output_dir,
                    &result.files,
                );
                let _ = report::save_report(&report);

                if accepted_passed {
                    let code_summary: String = result
                        .files
                        .iter()
                        .map(|f| {
                            format!("{}: {} lines", f.path.display(), f.content.lines().count())
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let _ = memory::distill_and_save(
                        &self.llm_coder,
                        task_prompt,
                        &code_summary,
                        result.final_score,
                    )
                    .await;
                    let _ = memory::save_example(task_prompt, output_dir, &language);
                    println!("[MEMORY] Learnings saved");
                }

                self.last_best_score = result.final_score as f64;
                self.emit_log(
                    "info",
                    &format!(
                        "Output accepted ({:.1}/10). Files at: {}",
                        result.final_score,
                        output_dir.display()
                    ),
                );
                self.emit(TuiEvent::MissionCompleted {
                    score: result.final_score as f64,
                    output_dir: output_dir.display().to_string(),
                });
                voice::mission_complete(accepted_passed, result.final_score);
                return Ok(());
            }

            // Save failure patterns for future missions
            let error_keys: Vec<String> = result
                .reports
                .iter()
                .flat_map(|r| r.lint_issues.clone())
                .collect();
            memory::save_failure_patterns(&language, &error_keys, result.final_score);

            // Continue to next fix round — build feedback with v2-style truncation
            let new_issues = self.extract_issue_keys(&result);
            for issue in &new_issues {
                if !persistent_issues.contains(issue) {
                    persistent_issues.push(issue.clone());
                }
            }
            feedback = self.build_feedback_v2(&result, round, &persistent_issues);

            // Save previous code (truncated per v2 strategy — 2K chars max per file, top 5 files)
            previous_code = result
                .files
                .iter()
                .take(5)
                .map(|f| {
                    let snippet = if f.content.len() > 2000 {
                        format!(
                            "{}...(truncated)",
                            &f.content[..f.content.floor_char_boundary(2000)]
                        )
                    } else {
                        f.content.clone()
                    };
                    format!("### {}\n```\n{}\n```", f.path.display(), snippet)
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            let mut rr = round_report;
            rr.feedback_to_next_round = Some(feedback.clone());
            rb.add_round(rr);

            let improved = best_result
                .as_ref()
                .map(|b| result.final_score > b.final_score)
                .unwrap_or(true);
            if improved {
                best_round = round + 1;
                best_result = Some(result);
            } else if round >= 2 {
                // Fix 3: Stop early if score hasn't improved for 2 consecutive rounds
                let prev_best = best_result.as_ref().map(|b| b.final_score).unwrap_or(0.0);
                if result.final_score < prev_best - 0.1 {
                    println!("[AUTO] Score declining ({:.1} < best {:.1}) — stopping early, restoring best round {}",
                        result.final_score, prev_best, best_round);
                    break;
                }
            }

            voice::fix_round(round + 2, MAX_FIX_ROUNDS);
        }

        // Should only reach here if all rounds exhausted without accept/abort
        let best = best_result.expect("BUG: no rounds completed — MAX_FIX_ROUNDS must be >= 1");
        println!();
        println!(
            "All {} fix rounds exhausted (best: {:.1}/10, round {})",
            MAX_FIX_ROUNDS, best.final_score, best_round
        );

        // Restore best round's files to disk (fix rounds may have degraded)
        if self.repo_context.is_none() {
            let _ = fs::remove_dir_all(output_dir);
            fs::create_dir_all(output_dir)?;
        }
        codegen::write_files(output_dir, &best.files)?;

        let report = rb.build(false, best.final_score, best_round, output_dir, &best.files);
        let _ = report::save_report(&report);

        self.last_best_score = best.final_score as f64;
        voice::mission_complete(false, best.final_score);
        Ok(())
    }

    /// Run architect with stats capture.
    async fn run_architect_with_stats(&self, prompt: &str) -> Result<(String, LlmCallStats)> {
        let mem_context = memory::load_context(prompt);
        let repo_section = if let Some(ref ctx) = self.repo_context {
            format!(
                "\n\nExisting codebase:\n{}\n\n\
                     You are EXTENDING this codebase, not creating from scratch.\n\
                     Follow existing conventions, naming patterns, and directory structure.\n\
                     Only list files that need to be CREATED or MODIFIED.",
                ctx
            )
        } else {
            String::new()
        };
        let system = format!(
            "{}\n\n{}\n\nYou are a Principal Software Architect.\n\
             Produce a clear, actionable specification with:\n\
             1. Architecture Decision Record (1 paragraph)\n\
             2. COMPLETE file manifest with exact relative paths and purpose of each file\n\
             3. TDD test plan (list every test case with expected behavior)\n\
             4. Security considerations\n\n\
             IMPORTANT: List EVERY file that needs to be created. The coder will generate\n\
             all of these files. Include config files, __init__.py, models, routes, etc.\n\
             Output structured text, NOT code.{}",
            self.quality_bible, mem_context, repo_section
        );
        self.llm_architect
            .generate_live_with_stats("ARCHITECT", &system, prompt)
            .await
    }

    /// Run tester with stats capture.
    async fn run_tester_with_stats(
        &self,
        prompt: &str,
        spec: &str,
    ) -> Result<(String, LlmCallStats)> {
        let system = format!(
            "{}\n\nYou are a Senior Test Engineer. Write COMPLETE test files.\n\
             Rules:\n\
             - Write tests FIRST, before any implementation exists\n\
             - Cover: happy path, edge cases, error cases, security\n\
             - Use the standard test framework for the language\n\
             - Aim for >= 90% coverage of the spec\n\
             - Output MULTIPLE test files if the spec has multiple modules\n\
             - For each file, start with a header: ### tests/test_<module>.py\n\
             - Then a fenced code block with the test code\n\
             - Output ALL test files, no explanations between them",
            self.quality_bible
        );
        let user_prompt = format!(
            "Based on this spec, write the complete test suite:\n\n{}\n\nOriginal request: {}",
            spec, prompt
        );
        self.llm_tester
            .generate_live_with_stats("TESTER", &system, &user_prompt)
            .await
    }

    /// Run one attempt through stages 4-8 with report capture.
    async fn attempt_round_with_report(
        &self,
        prompt: &str,
        spec: &str,
        tests_raw: &str,
        language: &str,
        output_dir: &Path,
        round: usize,
        feedback: &str,
        previous_code: &str,
    ) -> Result<(AttemptResult, RoundReport)> {
        let result = self
            .attempt_round(
                prompt,
                spec,
                tests_raw,
                language,
                output_dir,
                round,
                feedback,
                previous_code,
            )
            .await?;

        // Build round report from the result
        let file_reports: Vec<FileVerifierReport> = result
            .reports
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let path = result
                    .files
                    .get(i)
                    .map(|f| f.path.display().to_string())
                    .unwrap_or_else(|| format!("file_{}", i));
                FileVerifierReport {
                    path,
                    score: r.score,
                    lint_passed: r.lint_passed,
                    lint_issues: r.lint_issues.clone(),
                    syntax_valid: r.syntax_valid,
                    has_tests: r.has_tests,
                    has_docstring: r.has_docstring,
                    has_error_handling: r.has_error_handling,
                    has_hardcoded_secrets: r.has_hardcoded_secrets,
                }
            })
            .collect();

        let total_lint_issues: usize = result.reports.iter().map(|r| r.lint_issues.len()).sum();
        let secrets_found = result.reports.iter().any(|r| r.has_hardcoded_secrets);

        let critique_scores = if result.critique_scores.len() >= 5 {
            CritiqueScores {
                dev: result.critique_scores[0],
                arch: result.critique_scores[1],
                test: result.critique_scores[2],
                sec: result.critique_scores[3],
                docs: result.critique_scores[4],
            }
        } else {
            CritiqueScores {
                dev: 7.0,
                arch: 7.0,
                test: 7.0,
                sec: 7.0,
                docs: 7.0,
            }
        };

        let cto_approved = result.cto_verdict.to_uppercase().contains("APPROVE");
        let sec_passed = !result.security_verdict.to_uppercase().contains("FAIL");

        let rr = RoundReport {
            round_number: round + 1,
            coder: LlmStageReport {
                model: self.config.coder.model.clone(),
                duration_secs: 0.0, // timing captured at LLM level
                token_count: 0,
                tok_per_sec: 0.0,
                output_lines: result
                    .files
                    .iter()
                    .map(|f| f.content.lines().count() as u64)
                    .sum(),
            },
            verifier: VerifierReport {
                duration_secs: 0.0,
                avg_score: result.verifier_score,
                tests_passed: result.tests_passed,
                tests_failed: result.tests_failed,
                tests_run: result.tests_passed > 0 || result.tests_failed > 0,
                total_lint_issues,
                secrets_found,
                file_reports,
            },
            security: SecurityReport {
                model: self.config.security.model.clone(),
                duration_secs: 0.0,
                verdict: result
                    .security_verdict
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string(),
                passed: sec_passed,
            },
            critique: CritiqueReport {
                model: self.config.critique.model.clone(),
                duration_secs: 0.0,
                scores: critique_scores,
                avg: result.critique_avg,
                details: result.critique_details.clone(),
            },
            cto: CtoReport {
                model: self.config.cto.model.clone(),
                duration_secs: 0.0,
                verdict: result.cto_verdict.lines().next().unwrap_or("").to_string(),
                approved: cto_approved,
            },
            final_score: result.final_score,
            critique_avg: result.critique_avg,
            verifier_score: result.verifier_score,
            feedback_to_next_round: None,
        };

        Ok((result, rr))
    }

    /// Run one attempt through stages 4-8.
    async fn attempt_round(
        &self,
        prompt: &str,
        spec: &str,
        tests_raw: &str,
        language: &str,
        output_dir: &Path,
        round: usize,
        feedback: &str,
        previous_code: &str,
    ) -> Result<AttemptResult> {
        let files = if round == 0 {
            // === ROUND 1: Full generation ===
            self.single_shot_generate(prompt, spec, tests_raw, language, output_dir)
                .await?
        } else {
            // === FIX ROUNDS: Surgical or full regen based on issue ratio ===
            self.surgical_or_regen(
                prompt,
                spec,
                tests_raw,
                language,
                output_dir,
                round,
                feedback,
                previous_code,
            )
            .await?
        };

        // If fix round produced 0 files, reuse previous round's files from disk
        let files = if files.is_empty() && round > 0 {
            let disk_files = load_files_from_dir(output_dir).unwrap_or_default();
            if disk_files.is_empty() {
                println!("[FIX] No files on disk — returning previous round score");
                return Ok(AttemptResult {
                    files: vec![],
                    reports: vec![],
                    critique_scores: vec![],
                    critique_details: vec![],
                    security_verdict: String::new(),
                    cto_verdict: String::new(),
                    verifier_score: 0.0,
                    critique_avg: 0.0,
                    final_score: 0.0,
                    test_errors: vec![],
                    tests_passed: 0,
                    tests_failed: 0,
                });
            }
            disk_files
        } else {
            files
        };

        // Unload coder/fix_coder model to free VRAM for reviewer
        offload_model(&self.config.coder.model).await;
        if self.config.fix_coder.model != self.config.coder.model {
            offload_model(&self.config.fix_coder.model).await;
        }

        // Stage 5: Verifier — verify entire project (linters + tests)
        let stage = if round == 0 { "[5/9]" } else { "[FIX]" };
        self.emit_stage("5/9", "VERIFIER", "ruff+pytest");
        self.emit_log(
            "info",
            &format!(
                "{} VERIFIER: Checking {} files + running tests...",
                stage,
                files.len()
            ),
        );

        let project_report = verifier::verify_project(output_dir, language)?;

        let verifier_score = project_report.avg_score;
        let verifier_tests_passed = project_report.tests_passed;
        let verifier_tests_failed = project_report.tests_failed;
        let reports = project_report
            .file_reports
            .iter()
            .map(|(_, r)| r)
            .collect::<Vec<_>>();
        let any_secrets = reports.iter().any(|r| r.has_hardcoded_secrets);
        let total_issues: usize = reports.iter().map(|r| r.lint_issues.len()).sum();

        let verifier_summary = format!(
            "   Verifier avg: {:.1}/10 | Files: {} | Issues: {} | Secrets: {} | Tests: {}",
            verifier_score,
            reports.len(),
            total_issues,
            if any_secrets { "FOUND" } else { "clean" },
            if project_report.tests_run {
                format!(
                    "{} passed, {} failed",
                    project_report.tests_passed, project_report.tests_failed
                )
            } else {
                "not run".to_string()
            }
        );
        self.emit_log("info", &verifier_summary);
        self.emit_stage_done("5/9", &format!("{:.1}/10", verifier_score));

        // Capture test errors for surgical fix targeting
        let test_errors = project_report.test_errors.clone();

        // Convert to owned reports for AttemptResult
        let reports: Vec<verifier::QualityReport> = project_report
            .file_reports
            .into_iter()
            .map(|(_, r)| r)
            .collect();

        // Combine all code for review stages
        let all_code = files
            .iter()
            .map(|f| format!("### {}\n```\n{}\n```", f.path.display(), f.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        // Stage 6: Security Auditor
        let stage = if round == 0 { "[6/9]" } else { "[FIX]" };
        self.emit_stage("6/9", "SECURITY", &self.config.security.model.clone());
        self.emit_log(
            "info",
            &format!("{} SECURITY: Checking for vulnerabilities...", stage),
        );
        let security_system = "/no_think\nYou are a Security Auditor. Review this code for:\n\
             1. OWASP Top 10 vulnerabilities\n\
             2. Hardcoded secrets or credentials\n\
             3. SQL injection, XSS, CSRF risks\n\
             4. Missing input validation\n\
             5. Missing rate limiting\n\
             Output a brief verdict: PASS or FAIL with specific issues.";
        let security_verdict = self
            .llm_security
            .generate("SECURITY", security_system, &all_code)
            .await
            .unwrap_or_else(|_| "REVIEW SKIPPED".to_string());

        // Stage 7: Critique Panel (5 reviewers)
        let stage = if round == 0 { "[7/9]" } else { "[FIX]" };
        self.emit_stage("7/9", "CRITIQUE", &self.config.critique.model.clone());
        self.emit_log(
            "info",
            &format!("{} CRITIQUE PANEL: 5 specialist reviews...", stage),
        );
        self.emit_stage_done("6/9", "done");
        let (critique_scores, critique_details) = self.run_critique_panel(&all_code, spec).await?;
        let critique_avg = if critique_scores.is_empty() {
            5.0
        } else {
            critique_scores.iter().sum::<f32>() / critique_scores.len() as f32
        };
        if critique_scores.len() >= 5 {
            println!(
                "   Dev={:.1} Arch={:.1} Test={:.1} Sec={:.1} Docs={:.1} => Avg={:.1}",
                critique_scores[0],
                critique_scores[1],
                critique_scores[2],
                critique_scores[3],
                critique_scores[4],
                critique_avg
            );
        } else {
            println!(
                "   Critique avg={:.1} ({} scores)",
                critique_avg,
                critique_scores.len()
            );
        }

        // Offload critique model before loading CTO (if different)
        if self.config.critique.model != self.config.cto.model {
            offload_model(&self.config.critique.model).await;
        }

        // Stage 8: CTO Final Review
        let stage = if round == 0 { "[8/9]" } else { "[FIX]" };
        self.emit_stage_done("7/9", &format!("avg={:.1}", critique_avg));
        self.emit_stage("8/9", "CTO REVIEW", &self.config.cto.model.clone());
        self.emit_log(
            "info",
            &format!("{} CTO REVIEW: Mission-level coherence...", stage),
        );
        let cto_system = "/no_think\nYou are a CTO doing a final review. Check:\n\
             1. Does the code match the original request?\n\
             2. Is it production-ready? Are all modules present?\n\
             3. Would you deploy this to customers?\n\
             Output: APPROVE or REJECT with reason (1-2 sentences).";
        let cto_prompt = format!(
            "Original request: {}\n\nGenerated code ({} files):\n{}\n\nSecurity review: {}",
            prompt,
            files.len(),
            all_code,
            security_verdict
        );
        let cto_verdict = self
            .llm_cto
            .generate("CTO", cto_system, &cto_prompt)
            .await
            .unwrap_or_else(|_| "REVIEW SKIPPED".to_string());

        // Calculate final score: critique 40% + verifier 60%
        // Verifier (tests + linting) is the real quality signal — weight it higher
        let final_score = critique_avg * 0.4 + verifier_score * 0.6;
        self.emit_stage_done("8/9", "done");
        self.emit_log("info", &format!("[9/9] GATE: Score {:.1}/10", final_score));

        Ok(AttemptResult {
            files,
            reports,
            critique_scores,
            critique_details,
            security_verdict,
            cto_verdict,
            verifier_score,
            critique_avg,
            final_score,
            test_errors,
            tests_passed: verifier_tests_passed,
            tests_failed: verifier_tests_failed,
        })
    }

    /// Full project generation via 5 architectural stages.
    /// Each stage sees the output of all previous stages for cross-file coherence.
    /// Single-shot generation for simple tasks (C1-C7).
    /// One LLM call produces all files — better coherence, fewer import seam failures.
    async fn single_shot_generate(
        &self,
        prompt: &str,
        spec: &str,
        tests_raw: &str,
        language: &str,
        output_dir: &Path,
    ) -> Result<Vec<GeneratedFile>> {
        self.emit_stage("4/9", "CODER", &self.config.coder.model.clone());
        self.emit_log(
            "info",
            &format!(
                "[4/9] CODER: Implementing (single-shot, C{})...",
                self.complexity
            ),
        );

        let warnings = known_bad_patterns(language);
        let coder_system = format!(
            "{}\n\nYou are a Senior Software Engineer (10+ years).\n\
             Rules:\n\
             - Output EVERY file as a separate fenced code block\n\
             - Before each code block, write the ACTUAL file path as a markdown header, e.g.: ### app/main.py\n\
             - Follow SOLID principles, clean architecture\n\
             - Full error handling (no bare except, no unwrap)\n\
             - No hardcoded secrets — use environment variables\n\
             - No TODO/FIXME comments\n\
             - Include docstrings and type hints\n\
             - Do NOT reference modules you haven't created\n\
             - IMPORTANT: Use 'app/' as the Python package root (NOT 'src/'). All imports must use 'from app.xxx import yyy'.\n\
             - IMPORTANT: All __init__.py files must be EMPTY (just a docstring or blank). Do NOT put imports or re-exports in __init__.py.\n\
             - IMPORTANT: Every name used in type hints MUST be imported in that file.\n\n\
             {}\n\n{}",
            self.quality_bible, warnings, memory::load_failure_patterns(language)
        );

        let tests_compact = truncate_str(tests_raw, 4000);
        let repo_section = if let Some(ref ctx) = self.repo_context {
            format!("\n\nExisting codebase (read-only context — build on this, follow its conventions):\n{}\n", ctx)
        } else {
            String::new()
        };
        let coder_prompt = format!(
            "Implement the COMPLETE project based on this spec.\n\n\
             Spec:\n{}\n\n\
             Test plan (your code must pass these tests):\n{}\n\n\
             Original request: {}{}\n\n\
             Generate ALL files: production code, tests (conftest.py + test files), and pyproject.toml.\n\
             Output every file with its real path as a ### header before its code block (e.g., ### app/config.py, ### tests/conftest.py).",
            spec, tests_compact, prompt, repo_section
        );

        let code_raw = self
            .llm_coder
            .generate_live("CODER", &coder_system, &coder_prompt)
            .await?;
        let mut all_files = codegen::extract_files(&code_raw, language);

        // Merge tester-generated test files (don't overwrite coder's)
        let test_files = codegen::extract_files(tests_raw, language);
        for tf in &test_files {
            if !all_files.iter().any(|f| f.path == tf.path) {
                all_files.push(tf.clone());
            }
        }

        // Fallback: single-file extraction
        if all_files.is_empty() {
            let code = llm::extract_code(&code_raw, language);
            let tests = llm::extract_code(tests_raw, language);
            all_files.push(GeneratedFile {
                path: PathBuf::from(default_code_path(language)),
                content: code,
                language: language.to_string(),
            });
            all_files.push(GeneratedFile {
                path: PathBuf::from(default_test_path(language)),
                content: tests,
                language: language.to_string(),
            });
        }

        // Sanitize Python code: fix common import mistakes + strip __init__.py re-exports
        if language == "python" {
            sanitize_python_imports(&mut all_files);
            sanitize_init_files(&mut all_files);
        }

        self.emit_log(
            "info",
            &format!(
                "[4/9] CODER: {} files generated (single-shot)",
                all_files.len()
            ),
        );
        self.emit_stage_done("4/9", &format!("{} files", all_files.len()));
        // Send generated code to Code tab
        let code_preview: String = all_files
            .iter()
            .map(|f| format!("### {}\n{}\n", f.path.display(), f.content))
            .collect();
        self.emit(TuiEvent::CodeChunk {
            content: code_preview,
            model: self.config.coder.model.clone(),
            done: true,
        });

        // Write all files (repo mode: overwrite only, don't nuke existing dir)
        if self.repo_context.is_none() {
            let _ = fs::remove_dir_all(output_dir);
            fs::create_dir_all(output_dir)?;
        }
        codegen::write_files(output_dir, &all_files)?;
        codegen::write_boilerplate(output_dir, language, prompt)?;

        Ok(all_files)
    }

    /// Surgical fix or full regen based on issue ratio.
    /// If <= 50% of files have issues → fix only broken files (v2 general_direct_fix).
    /// If > 50% → full regen with feedback (too many issues for surgical approach).
    async fn surgical_or_regen(
        &self,
        prompt: &str,
        _spec: &str,
        _tests_raw: &str,
        language: &str,
        output_dir: &Path,
        _round: usize,
        feedback: &str,
        _previous_code: &str,
    ) -> Result<Vec<GeneratedFile>> {
        // Load previous files from output dir
        let prev_files = load_files_from_dir(output_dir)?;
        if prev_files.is_empty() {
            // No previous files — nothing to fix, skip round
            println!("[FIX] No previous files found — skipping round");
            return Ok(vec![]);
        }

        // Identify which files have issues from the feedback
        let broken_files = identify_broken_files(feedback, &prev_files);

        if broken_files.is_empty() {
            // Fix 2: Never fall back to full regen — it always degrades quality.
            // Skip this round and keep previous files intact.
            println!(
                "[FIX] Cannot identify specific broken files — skipping round (keeping previous code)"
            );
            Ok(prev_files)
        } else {
            // === SURGICAL FIX MODE (v2 general_direct_fix) ===
            println!(
                "[FIX] SURGICAL: {} of {} files need fixing — patching only broken files",
                broken_files.len(),
                prev_files.len()
            );

            let mut files = prev_files;

            for (file_idx, file_issues) in &broken_files {
                let file = &files[*file_idx];

                // v2 size gate: skip surgical fix for large files needing structural rewrites
                if file.content.lines().count() > 500
                    && file_issues.iter().any(|i| {
                        i.contains("restructure") || i.contains("redesign") || i.contains("rewrite")
                    })
                {
                    println!(
                        "   [skip] {} — too large for surgical fix ({} lines)",
                        file.path.display(),
                        file.content.lines().count()
                    );
                    continue;
                }

                println!(
                    "   [fix] {} ({} issue(s))",
                    file.path.display(),
                    file_issues.len()
                );

                // Build findings text (v2 format)
                let findings_text: String = file_issues
                    .iter()
                    .map(|issue| format!("  - {}", issue))
                    .collect::<Vec<_>>()
                    .join("\n");

                let fixed_content = self
                    .surgical_fix_file(
                        &file.path.display().to_string(),
                        &file.content,
                        &findings_text,
                        language,
                        prompt,
                    )
                    .await?;

                // Replace the file content
                let idx = *file_idx;
                files[idx] = GeneratedFile {
                    path: file.path.clone(),
                    content: fixed_content,
                    language: language.to_string(),
                };
            }

            // Write updated files
            if language == "python" {
                sanitize_python_imports(&mut files);
                sanitize_init_files(&mut files);
            }

            if self.repo_context.is_none() {
                let _ = fs::remove_dir_all(output_dir);
                fs::create_dir_all(output_dir)?;
            }
            codegen::write_files(output_dir, &files)?;
            codegen::write_boilerplate(output_dir, language, prompt)?;

            Ok(files)
        }
    }

    /// Fix a single file surgically (v2 general_direct_fix prompt).
    /// Uses generate_live for streaming output so you can see tokens flowing.
    async fn surgical_fix_file(
        &self,
        file_name: &str,
        file_content: &str,
        findings: &str,
        language: &str,
        _mission: &str,
    ) -> Result<String> {
        let system = format!(
            "{}\n\nYou are a Senior Software Engineer fixing bugs in existing code.\n\
             Fix the issues listed below. Preserve all working code. Only change what is broken.",
            self.quality_bible
        );

        // Truncate file content for context (v2: 2K cap per file for retry, but surgical gets full file up to 4K)
        let code_for_prompt = truncate_str(file_content, 4000);

        let user_prompt = format!(
            "Fix the following issues in this file.\n\n\
             ### {file_name}\n\
             ```{language}\n\
             {code_for_prompt}\n\
             ```\n\n\
             Issues to fix:\n\
             {findings}\n\n\
             Output the COMPLETE fixed file as a single fenced code block.\n\
             Fix ONLY the issues listed. Do not refactor working code.\n\
             If a fix requires adding imports, add them.\n\n\
             ### {file_name}\n\
             ```{language}"
        );

        // Use generate_live so tokens stream to stdout
        let raw = self
            .llm_fix_coder
            .generate_live("  FIX", &system, &user_prompt)
            .await?;
        let mut fixed = llm::extract_code(&raw, language);

        // Strip inner code fences from config files (LLM wraps pyproject.toml in ```toml)
        let config_exts = [
            ".toml", ".yaml", ".yml", ".json", ".ini", ".cfg", ".env", ".txt",
        ];
        if config_exts.iter().any(|ext| file_name.ends_with(ext)) && fixed.trim().starts_with("```")
        {
            if let Some(nl) = fixed.find('\n') {
                let after = &fixed[nl + 1..];
                fixed = if after.trim_end().ends_with("```") {
                    let end = after.rfind("```").unwrap_or(after.len());
                    after[..end].trim().to_string()
                } else {
                    after.trim().to_string()
                };
            }
        }

        // Content validation
        // 1. Too short — LLM returned stub or empty
        if fixed.len() < file_content.len() / 3 {
            println!(
                "   [warn] Fix for {} too short ({} vs {} chars), keeping original",
                file_name,
                fixed.len(),
                file_content.len()
            );
            return Ok(file_content.to_string());
        }

        // 2. Reasoning leak — LLM embedded its thinking instead of code
        let reasoning_markers = [
            "looking at the error",
            "the issue is",
            "let me",
            "I need to",
            "the fix is",
            "we need to",
            "the problem is",
            "actually",
            "re-reading the instruction",
            "Wait -",
            "Hmm",
            "However, the instruction says",
            "false positive",
            "no actual",
            "nothing to fix",
            "no issues to fix",
            "examining the file",
        ];
        let reasoning_count = reasoning_markers
            .iter()
            .filter(|m| fixed.to_lowercase().contains(&m.to_lowercase()))
            .count();
        // Config files: stricter threshold (1 marker = leak). Code files: 3 markers.
        let config_exts = [".toml", ".yaml", ".yml", ".json", ".ini", ".cfg"];
        let threshold = if config_exts.iter().any(|ext| file_name.ends_with(ext)) {
            1
        } else {
            3
        };
        if reasoning_count >= threshold {
            println!(
                "   [warn] Fix for {} contains LLM reasoning (not code), keeping original",
                file_name
            );
            return Ok(file_content.to_string());
        }

        Ok(fixed)
    }

    /// Build compact feedback (v2 strategy: truncate everything, focus on actionable issues).
    fn build_feedback_v2(
        &self,
        result: &AttemptResult,
        round: usize,
        persistent_issues: &[String],
    ) -> String {
        let mut feedback = String::new();

        // Scale truncation limits by complexity (C8+ gets 2x budget)
        let verifier_limit = if self.complexity >= 8 { 1500 } else { 800 };
        let critique_limit = if self.complexity >= 8 { 600 } else { 400 };
        let verdict_limit = if self.complexity >= 8 { 400 } else { 200 };

        // Verifier issues
        let mut verifier_section = String::new();
        for (i, report) in result.reports.iter().enumerate() {
            if !report.lint_issues.is_empty() {
                let file_name = result
                    .files
                    .get(i)
                    .map(|f| f.path.display().to_string())
                    .unwrap_or_else(|| format!("file_{}", i));
                verifier_section.push_str(&format!("{}: ", file_name));
                verifier_section.push_str(&report.lint_issues.join("; "));
                verifier_section.push('\n');
            }
            if report.has_hardcoded_secrets {
                verifier_section.push_str("CRITICAL: Hardcoded secrets — use os.getenv()\n");
            }
        }
        if !verifier_section.is_empty() {
            feedback.push_str("## Verifier\n");
            feedback.push_str(&truncate_str(&verifier_section, verifier_limit));
            feedback.push('\n');
        }

        // Test errors — critical for surgical fix targeting
        if !result.test_errors.is_empty() {
            feedback.push_str("## Test errors\n");
            let max_errors = if self.complexity >= 8 { 20 } else { 10 };
            for err in result.test_errors.iter().take(max_errors) {
                feedback.push_str(&format!("{}\n", err));
            }
            feedback.push('\n');
        }

        // Critique defects — one line each, truncated
        let critique_text: String = result
            .critique_details
            .iter()
            .filter(|d| !d.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        if !critique_text.is_empty() {
            feedback.push_str("## Critique defects\n");
            feedback.push_str(&truncate_str(&critique_text, critique_limit));
            feedback.push('\n');
        }

        // CTO verdict — first line only
        let cto_first_line = result.cto_verdict.lines().next().unwrap_or("");
        feedback.push_str(&format!(
            "## CTO: {}\n",
            truncate_str(cto_first_line, verdict_limit)
        ));

        // Security — first line only
        let sec_first_line = result.security_verdict.lines().next().unwrap_or("");
        feedback.push_str(&format!(
            "## Security: {}\n",
            truncate_str(sec_first_line, verdict_limit)
        ));

        // Persistent issues — flag things that haven't been fixed across rounds
        if round >= 2 && !persistent_issues.is_empty() {
            feedback.push_str(&format!(
                "\n## PERSISTENT ISSUES (unfixed for {} rounds)\n",
                round
            ));
            for issue in persistent_issues.iter().take(5) {
                feedback.push_str(&format!("- {}\n", issue));
            }
        }

        feedback.push_str(&format!(
            "\nScore: {:.1}/10 (need >= {:.1}).\n\
             IMPORTANT: Fix ONLY bugs (import errors, missing imports, test failures, syntax errors).\n\
             Do NOT add new features, middleware, auth, or rate limiting unless the original prompt asked for them.\n\
             Do NOT restructure working code. Preserve what works.\n",
            result.final_score, quality_gate(self.complexity)
        ));

        feedback
    }

    /// Extract key issue identifiers for persistent issue tracking.
    fn extract_issue_keys(&self, result: &AttemptResult) -> Vec<String> {
        let mut keys = Vec::new();
        for report in &result.reports {
            for issue in &report.lint_issues {
                // Normalize to short key
                let key = if issue.contains("hardcoded secret") || issue.contains("Hardcoded") {
                    "hardcoded secrets".to_string()
                } else if issue.contains("syntax error") {
                    "syntax errors".to_string()
                } else if issue.contains("import") {
                    "import errors".to_string()
                } else {
                    truncate_str(issue, 60).to_string()
                };
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            if report.has_hardcoded_secrets && !keys.contains(&"hardcoded secrets".to_string()) {
                keys.push("hardcoded secrets".to_string());
            }
        }
        keys
    }

    /// Run critique panel as a SINGLE LLM call (5 scores in one response).
    async fn run_critique_panel(&self, code: &str, spec: &str) -> Result<(Vec<f32>, Vec<String>)> {
        let system = "/no_think\nYou are 5 expert reviewers in one. Score this code 0-10 on each dimension.\n\
            Output EXACTLY this format (one line per role, nothing else):\n\
            DEV: X.X | defects: ...\n\
            ARCH: X.X | defects: ...\n\
            TEST: X.X | defects: ...\n\
            SEC: X.X | defects: ...\n\
            DOCS: X.X | defects: ...\n\n\
            DEV = correctness, robustness\n\
            ARCH = architecture, SOLID, maintainability\n\
            TEST = test quality, coverage\n\
            SEC = security, OWASP, secrets\n\
            DOCS = documentation, readability";

        let prompt = format!("Code:\n{}\n\nSpec:\n{}", code, spec);

        let response = self
            .llm_critique
            .generate("  CRITIQUE", system, &prompt)
            .await
            .unwrap_or_else(|e| {
                eprintln!("   CRITIQUE FAILED: {}", e);
                "DEV: 5.0\nARCH: 5.0\nTEST: 5.0\nSEC: 5.0\nDOCS: 5.0".to_string()
            });

        if response.trim().is_empty() {
            eprintln!("   CRITIQUE returned empty response — using default 5.0 scores");
            return Ok((vec![5.0f32; 5], vec![String::new(); 5]));
        }

        // Parse 5 scores from the single response
        let mut scores = vec![5.0f32; 5];
        let mut details = vec![String::new(); 5];
        let prefixes = ["DEV", "ARCH", "TEST", "SEC", "DOCS"];

        for line in response.lines() {
            // Strip markdown formatting (**, *, #, etc.) before matching
            let stripped: String = line.chars().filter(|c| *c != '*' && *c != '#').collect();
            let upper = stripped.to_uppercase();
            for (i, prefix) in prefixes.iter().enumerate() {
                // Match "DEV:" or "DEV :" or "DEV=" etc.
                if let Some(pos) = upper.find(prefix) {
                    let after = &upper[pos + prefix.len()..];
                    if after.starts_with(':') || after.starts_with(' ') || after.starts_with('=') {
                        // Extract score — first number 0-10 on the line
                        for word in stripped.split_whitespace() {
                            let cleaned = word.trim_matches(|c: char| !c.is_numeric() && c != '.');
                            if let Ok(n) = cleaned.parse::<f32>() {
                                if (0.0..=10.0).contains(&n) {
                                    scores[i] = n;
                                    break;
                                }
                            }
                        }
                        // Extract defects after |
                        if let Some(defect_part) = line.split('|').nth(1) {
                            details[i] = defect_part.trim().to_string();
                        }
                    }
                }
            }
        }

        // Warn if all scores stayed at default (parser couldn't extract)
        if scores.iter().all(|&s| s == 5.0) {
            eprintln!("   WARNING: Critique parser extracted no scores from {} lines — model may have used unexpected format", response.lines().count());
            // Print first 5 lines for debugging
            for (i, line) in response.lines().take(5).enumerate() {
                eprintln!("     line {}: {}", i + 1, line);
            }
        }

        Ok((scores, details))
    }

    fn print_results(&self, output_dir: &Path) -> Result<()> {
        println!();
        println!("=== Mission Complete ===");
        println!("Output: {}", output_dir.display());
        println!();
        list_files(output_dir)?;
        Ok(())
    }
}

// ── Helper functions ──

/// Remove __pycache__, .pytest_cache, and other build artifacts from output.
fn cleanup_artifacts(dir: &Path) {
    let artifacts = [
        "__pycache__",
        ".pytest_cache",
        "__pypackages__",
        ".mypy_cache",
        "node_modules",
        ".venv",
    ];
    if let Ok(entries) = walkdir(dir) {
        for entry in entries {
            for artifact in &artifacts {
                if entry.to_string_lossy().contains(artifact) {
                    if entry.is_dir() {
                        let _ = fs::remove_dir_all(&entry);
                    } else {
                        let _ = fs::remove_file(&entry);
                    }
                }
            }
        }
    }
    // Also clean up directories
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if artifacts.contains(&name.as_str()) {
                let _ = fs::remove_dir_all(entry.path());
            }
            if entry.path().is_dir() {
                cleanup_artifacts(&entry.path());
            }
        }
    }
}

/// Offload a model from Ollama VRAM to free memory for the next stage.
async fn offload_model(model: &str) {
    let client = reqwest::Client::new();
    let body = serde_json::json!({"model": model, "keep_alive": 0});
    match client
        .post(format!("{}/api/generate", crate::llm::ollama_url()))
        .json(&body)
        .send()
        .await
    {
        Ok(_) => println!("   [VRAM] Offloaded {}", model),
        Err(_) => {} // silent fail — model may not be loaded
    }
}

// Score extraction and defect parsing are now handled inline in run_critique_panel.

fn default_code_path(language: &str) -> &str {
    match language {
        "python" => "app/main.py",
        "typescript" => "src/index.ts",
        "javascript" => "src/index.js",
        "rust" => "src/main.rs",
        "go" => "main.go",
        _ => "main.py",
    }
}

fn default_test_path(language: &str) -> &str {
    match language {
        "python" => "tests/test_main.py",
        "typescript" => "__tests__/index.test.ts",
        "javascript" => "__tests__/index.test.js",
        "rust" => "src/tests.rs",
        "go" => "main_test.go",
        _ => "test_main.py",
    }
}

/// Language-specific known-bad patterns learned from previous runs.
/// These prevent the most common LLM mistakes for each language.
fn known_bad_patterns(language: &str) -> String {
    let common = "\
COMMON MISTAKES TO AVOID:\n\
- Service/repository methods must return ORM/database models, NOT request schemas\n\
- register()/create() MUST call repository.create() and return the saved model\n\
- Every class/function you reference MUST be imported — verify your imports\n\
- Route handlers that need injected services MUST use the DI pattern for your framework\n\
- Do NOT return password hashes in response schemas\n\
- No hardcoded secrets — use environment variables\n";

    let lang_specific = match language {
        "python" => "\
Python-specific:\n\
- Pydantic v2: use pydantic_settings.BaseSettings (NOT pydantic.BaseSettings)\n\
- Pydantic v2: use model_config = ConfigDict(from_attributes=True) (NOT class Config: orm_mode = True)\n\
- Pydantic v2: use @field_validator (NOT @validator)\n\
- Pydantic v2: use .model_validate() (NOT .from_orm())\n\
- Pydantic v2: models are frozen by default — use model_copy(update={...}) to modify\n\
- python-jose: use 'from jose import jwt' (NOT 'import jwt' — that's PyJWT, different API)\n\
- SQLAlchemy: cast PostgresDsn to str() before passing to create_async_engine()\n\
- SQLAlchemy 2.0: 'from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column' (NOT from sqlalchemy.ext.declarative — that module does NOT have DeclarativeBase)\n\
- FastAPI DI: route params need Depends() — e.g. service: MyService = Depends(get_service)\n\
- FastAPI routes: always import Depends from fastapi in router files\n\
- pytest-asyncio: add @pytest.mark.asyncio to async test functions\n\
- conftest.py: set env vars BEFORE importing Settings (module-level singletons)\n\
- CIRCULAR IMPORTS: dependencies.py must NEVER import from routes. Routes import from dependencies, not the other way around.\n\
- Dependencies (get_db, get_current_user) must be in a separate file that does NOT import route modules.\n\
- Surgical fix must NOT create empty __init__.py files — only fix the specific file content.\n\
- httpx 0.28+: use AsyncClient(transport=ASGITransport(app=app), base_url='http://test') NOT AsyncClient(app=app). Import ASGITransport from httpx.\n\
- pydantic-settings: do NOT use Field(...) (required) for settings fields — always provide a default value. Field(...) crashes if env var is not set.\n\
- @property: if a Settings field is a @property, access it as attribute (settings.foo), NOT as method call (settings.foo()).\n\
- FastAPI routes: if router prefix is '/users', the route decorator should be @router.post('') not @router.post('/') to avoid trailing-slash 307 redirects. Or use '/users/' consistently in tests.\n\
- Every name used in type hints (return types, params) MUST be imported. 'def foo() -> User' requires 'from app.models.user import User'.\n\
- SQLAlchemy: define Base in ONE place only (models.py or db/base.py). Do NOT create a second Base in database.py with declarative_base(). Import Base from that one place everywhere.\n\
- SQLAlchemy: use ONLY DeclarativeBase (new style), NEVER declarative_base() (old style). Never mix both in the same project.\n\
- Mocking: patch targets must match import style. If code does 'import smtplib' then patch 'module.smtplib.SMTP', NOT 'module.SMTP'.\n\
- Pydantic v2 validation error types: 'missing' (not 'field_required'), 'value_error' (not 'value_error.email'), 'int_parsing' (not 'value_error.integer').\n\
- Tests: use tempfile.TemporaryDirectory() for test dirs, never hardcoded relative paths.\n\
- Tests: if production code uses direct instantiation (obj = Foo()), do NOT mock with context manager (__enter__/__exit__).\n\
- NAMING: Pydantic response schemas MUST use different names than ORM models. Use UserResponse or UserRead (NOT User). Importing both 'from models import User' and 'from schemas import User' in the same file shadows the ORM model and crashes.\n\
- NAMING: If you have an ORM model User and a schema User, rename the schema to UserResponse, UserRead, or UserOut.\n\
- Pydantic v2: do NOT use 'from pydantic.networks import Url' — Url does not exist in pydantic v2. Use HttpUrl directly from pydantic: 'from pydantic import HttpUrl'.\n\
- conftest.py: import async_sessionmaker from sqlalchemy.ext.asyncio (NOT from sqlalchemy.orm import sessionmaker for async).\n\
- SQLite: do NOT use Mapped[uuid.UUID] — SQLite has no native UUID type. Use Mapped[str] with default=lambda: str(uuid.uuid4()).\n\
- SQLAlchemy: do NOT set Base = None and assign later. Define Base as 'class Base(DeclarativeBase): pass' in database.py and import it everywhere.\n\
- conftest.py: MUST override app dependencies with test session. Use app.dependency_overrides[get_db] = get_test_db.\n\
- pytest-asyncio: in pyproject.toml use asyncio_mode (underscore), NOT asyncio-mode (dash).\n\
- Pydantic response schemas: datetime fields must be typed as datetime, NOT str. SQLAlchemy returns datetime objects.\n\
- SQLAlchemy: IntegrityError is in sqlalchemy.exc, NOT sqlalchemy. Use 'from sqlalchemy.exc import IntegrityError'.\n\
- Every test file MUST start with 'import pytest'. Never use pytest.fixture or pytest.mark without importing pytest first.\n\
- DEPENDENCIES: Put ALL dependencies (including pytest, pytest-asyncio, httpx, hypothesis) in [project.dependencies], NOT in [project.optional-dependencies]. The verifier installs from [project.dependencies] only.\n\
- DEPENDENCIES: Always include a requirements.txt with ALL deps (including test deps) as a fallback.\n\
- PYTEST CONFIG: pyproject.toml MUST include [tool.pytest.ini_options] with asyncio_mode = 'auto' and testpaths = ['tests']. This eliminates the need for @pytest.mark.asyncio on every test.\n\
- ASYNC/SYNC CONSISTENCY: If production uses create_async_engine + AsyncSession, tests MUST use httpx.AsyncClient(transport=ASGITransport(app=app)) with async fixtures. Do NOT use TestClient with async production code — TestClient triggers the lifespan which calls the async engine.\n\
- LIFESPAN: FastAPI lifespan handlers that create DB tables MUST wrap engine.begin() in try/except so tests with overridden databases don't crash. Or use 'if os.getenv(\"TESTING\") != \"1\"' guard.\n\
- TYPING: Always import Optional, List, Dict from typing for Python <3.10 compat. Or use 'from __future__ import annotations' at the top of every file.\n\
- TOML: pyproject.toml must NOT have duplicate keys (e.g. two 'warn_return_any = true' in [tool.mypy]). Duplicate keys cause TOML parse errors that prevent pytest from running.\n",
        "rust" => "\
Rust-specific:\n\
- Handle all Result/Option types — no unwrap() in production code\n\
- Use thiserror for custom error types\n\
- Ensure all public types derive necessary traits (Debug, Clone, Serialize, Deserialize)\n\
- Use ? operator for error propagation, not unwrap()\n",
        "go" => "\
Go-specific:\n\
- Always check error returns — no _ = err\n\
- Use context.Context as first parameter in functions that do I/O\n\
- Close resources with defer\n\
- Use interfaces for dependency injection\n",
        "typescript" => "\
TypeScript-specific:\n\
- Use strict mode in tsconfig.json\n\
- Avoid any type — use proper generics\n\
- Use async/await consistently, not mixed callbacks\n",
        _ => "",
    };

    format!("{}{}", common, lang_specific)
}

/// Merge new files into accumulator, skipping duplicates (first version wins).
#[allow(dead_code)]
fn merge_files(all: &mut Vec<GeneratedFile>, new: Vec<GeneratedFile>) {
    for f in new {
        if !all.iter().any(|existing| existing.path == f.path) {
            all.push(f);
        }
    }
}

/// Auto-fix common Python import mistakes that LLMs consistently make.
/// These are deterministic regex fixes — cheaper and more reliable than re-prompting.
fn sanitize_python_imports(files: &mut Vec<GeneratedFile>) {
    let fixes: &[(&str, &str)] = &[
        // ConfigDict must come from pydantic, not pydantic_settings
        (
            "from pydantic_settings import ConfigDict",
            "from pydantic import ConfigDict",
        ),
        (
            "from pydantic_settings import BaseSettings, ConfigDict",
            "from pydantic_settings import BaseSettings\nfrom pydantic import ConfigDict",
        ),
        // BaseSettings must come from pydantic_settings, not pydantic
        (
            "from pydantic import BaseSettings",
            "from pydantic_settings import BaseSettings",
        ),
        // DeclarativeBase must come from sqlalchemy.orm, not sqlalchemy.ext.declarative
        (
            "from sqlalchemy.ext.declarative import DeclarativeBase",
            "from sqlalchemy.orm import DeclarativeBase",
        ),
        // validator is v1, field_validator is v2
        (
            "from pydantic import validator",
            "from pydantic import field_validator",
        ),
    ];

    for file in files.iter_mut() {
        if !file.path.to_string_lossy().ends_with(".py") {
            continue;
        }
        let mut changed = false;
        let mut content = file.content.clone();
        for (bad, good) in fixes {
            if content.contains(bad) {
                content = content.replace(bad, good);
                changed = true;
            }
        }
        if changed {
            file.content = content;
        }
    }
}

/// Sanitize __init__.py files: strip re-exports that cause circular imports.
/// LLMs consistently ignore "empty __init__.py" instructions, so we enforce it.
fn sanitize_init_files(files: &mut Vec<GeneratedFile>) {
    for file in files.iter_mut() {
        let path_str = file.path.display().to_string();
        if !path_str.ends_with("__init__.py") {
            continue;
        }
        // Check if file has actual imports (not just docstrings/comments/__all__)
        let has_imports = file.content.lines().any(|line| {
            let trimmed = line.trim();
            (trimmed.starts_with("from ") || trimmed.starts_with("import "))
                && !trimmed.starts_with("from __future__")
        });
        if has_imports {
            // Replace with empty file (just a docstring)
            let docstring = file
                .content
                .lines()
                .take_while(|l| {
                    l.starts_with('#')
                        || l.starts_with("\"\"\"")
                        || l.starts_with("'''")
                        || l.trim().is_empty()
                })
                .collect::<Vec<_>>()
                .join("\n");
            let cleaned = if docstring.trim().is_empty() {
                format!(
                    "\"\"\"{}\"\"\"",
                    path_str
                        .replace("__init__.py", "")
                        .replace('/', ".")
                        .trim_matches('.')
                )
            } else {
                docstring
            };
            file.content = cleaned;
        }
    }
}

/// Format files as code listing for LLM context.
/// Truncate a string to max_chars, appending "..." if truncated.
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let end = s.floor_char_boundary(max_chars);
        format!("{}...", &s[..end])
    }
}

/// Load existing generated files from the output directory.
fn load_files_from_dir(dir: &Path) -> Result<Vec<GeneratedFile>> {
    let mut files = Vec::new();
    let skip = [
        "__pycache__",
        ".pytest_cache",
        ".pyc",
        ".mypy_cache",
        ".venv",
        "node_modules",
    ];

    for path in walkdir(dir)? {
        let path_str = path.to_string_lossy();
        if skip.iter().any(|s| path_str.contains(s)) {
            continue;
        }
        // Only load source files
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ![
            "py", "ts", "js", "tsx", "jsx", "rs", "go", "toml", "json", "yml", "yaml", "md", "txt",
            "cfg", "ini", "html", "css",
        ]
        .contains(&ext)
        {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            let relative = path.strip_prefix(dir).unwrap_or(&path).to_path_buf();
            let lang = match ext {
                "py" => "python",
                "ts" | "tsx" => "typescript",
                "js" | "jsx" => "javascript",
                "rs" => "rust",
                "go" => "go",
                _ => "text",
            };
            files.push(GeneratedFile {
                path: relative,
                content,
                language: lang.to_string(),
            });
        }
    }
    Ok(files)
}

/// Identify which files have issues based on feedback text.
/// Returns Vec of (file_index, Vec<issue_string>).
fn identify_broken_files(feedback: &str, files: &[GeneratedFile]) -> Vec<(usize, Vec<String>)> {
    let mut broken: Vec<(usize, Vec<String>)> = Vec::new();
    let feedback_lower = feedback.to_lowercase();

    // Step 1: Find files explicitly mentioned by path in feedback
    for (idx, file) in files.iter().enumerate() {
        let file_name = file.path.display().to_string();
        let mut issues = Vec::new();

        for line in feedback.lines() {
            if line.contains(&file_name) {
                issues.push(line.trim().to_string());
            }
        }

        if !issues.is_empty() {
            broken.push((idx, issues));
        }
    }

    // Step 2: Import error chain tracing — find the SOURCE of import failures
    for line in feedback.lines() {
        let lower_line = line.to_lowercase();
        if !(lower_line.contains("importerror")
            || lower_line.contains("modulenotfounderror")
            || lower_line.contains("cannot import name")
            || lower_line.contains("circular import"))
        {
            continue;
        }

        // Extract the module path (e.g., "src.database", "app.api.v1.routes.users")
        if let Some(module) = extract_failed_module(&lower_line) {
            // Convert module path to file path: "src.database" → "src/database/__init__.py" or "src/database.py"
            let module_as_path = module.replace('.', "/");

            for (idx, file) in files.iter().enumerate() {
                if broken.iter().any(|(i, _)| *i == idx) {
                    continue;
                }

                let file_path = file.path.display().to_string();

                // Match: file IS the broken module (e.g., src/database/__init__.py or src/database/session.py)
                let file_module = file_path
                    .replace('/', ".")
                    .trim_end_matches(".py")
                    .replace(".__init__", "")
                    .to_string();
                if file_module == module
                    || file_module.starts_with(&format!("{}.", module))
                    || module.starts_with(&format!("{}.", file_module))
                {
                    broken.push((
                        idx,
                        vec![format!(
                            "Import chain error — this module is part of the broken import: {}",
                            line.trim()
                        )],
                    ));
                }

                // Match: file path contains the module path (e.g., "src/database/session.py" contains "src/database")
                if file_path.starts_with(&module_as_path)
                    && !file_path.contains("__pycache__")
                    && !broken.iter().any(|(i, _)| *i == idx)
                {
                    broken.push((
                        idx,
                        vec![format!("Part of broken module {}: {}", module, line.trim())],
                    ));
                }
            }
        }
    }

    // Step 2b: NameError tracing — find the specific file where the error occurred
    // e.g., "NameError: name 'Bookmark' is not defined" in conftest.py
    // Only flag the file mentioned in the error context, not every file using the name
    for line in feedback.lines() {
        if !line.contains("NameError") {
            continue;
        }
        if let Some(start) = line.find("name '") {
            if let Some(end) = line[start + 6..].find('\'') {
                let undefined_name = &line[start + 6..start + 6 + end];
                let mut found_specific = false;

                // First: try to find the specific file mentioned in surrounding error context
                // pytest errors mention file paths like "tests/conftest.py:42"
                for ctx_line in feedback.lines() {
                    for (idx, file) in files.iter().enumerate() {
                        if broken.iter().any(|(i, _)| *i == idx) {
                            continue;
                        }
                        let file_name = file.path.display().to_string();
                        if ctx_line.contains(&file_name)
                            && (ctx_line.contains("Error")
                                || ctx_line.contains("conftest")
                                || ctx_line.contains("FAILED"))
                            && file.content.contains(undefined_name)
                        {
                            broken.push((idx, vec![format!(
                                    "NameError: '{}' used but not imported. Add the missing import.", undefined_name
                                )]));
                            found_specific = true;
                        }
                    }
                }

                // Fallback: if no specific file found, flag ONLY test/conftest files missing the import
                // (NameErrors almost always originate in test code, not production code)
                if !found_specific {
                    for (idx, file) in files.iter().enumerate() {
                        if broken.iter().any(|(i, _)| *i == idx) {
                            continue;
                        }
                        let file_name = file.path.display().to_string();
                        if !file_name.ends_with(".py") || file_name.ends_with("__init__.py") {
                            continue;
                        }
                        // Only check test files and conftest as fallback
                        if !(file_name.contains("test") || file_name.contains("conftest")) {
                            continue;
                        }
                        if file.content.contains(undefined_name) {
                            let has_import = file.content.lines().any(|l| {
                                let t = l.trim();
                                if !(t.starts_with("from ") || t.starts_with("import ")) {
                                    return false;
                                }
                                t.split(|c: char| !c.is_alphanumeric() && c != '_')
                                    .any(|word| word == undefined_name)
                            });
                            if !has_import {
                                broken.push((idx, vec![format!(
                                    "NameError: '{}' used but not imported. Add the missing import.", undefined_name
                                )]));
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 2c: AttributeError tracing — find class definitions missing attributes
    // e.g., "'Settings' object has no attribute 'ALLOWED_ORIGINS'" → find file defining Settings class
    for line in feedback.lines() {
        if !line.contains("AttributeError") || !line.contains("has no attribute") {
            continue;
        }
        // Extract class name and attribute from "'ClassName' object has no attribute 'attr_name'"
        if let Some(cls_start) = line.find('\'') {
            if let Some(cls_end) = line[cls_start + 1..].find('\'') {
                let class_name = &line[cls_start + 1..cls_start + 1 + cls_end];
                if let Some(attr_start) = line.rfind("'") {
                    let before_last = &line[..attr_start];
                    if let Some(attr_start2) = before_last.rfind("'") {
                        let attr_name = &line[attr_start2 + 1..attr_start];
                        // Find files that define this class
                        for (idx, file) in files.iter().enumerate() {
                            if broken.iter().any(|(i, _)| *i == idx) {
                                continue;
                            }
                            let class_def = format!("class {}", class_name);
                            if file.content.contains(&class_def) {
                                broken.push((idx, vec![format!(
                                    "AttributeError: class '{}' missing attribute '{}'. Add it.", class_name, attr_name
                                )]));
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 2d: "cannot import name 'X' from 'external.package'" — find PROJECT files that have
    // the bad import line. The module is external (pydantic, sqlalchemy, etc.) so import chain
    // tracing won't match any project file. Instead, grep for the import in project code.
    for line in feedback.lines() {
        let lower_line = line.to_lowercase();
        if !lower_line.contains("cannot import name") {
            continue;
        }
        // Extract name and source: "cannot import name 'Url' from 'pydantic.networks'"
        if let Some(name_start) = lower_line.find("cannot import name '") {
            let after_name = &lower_line[name_start + 20..];
            if let Some(name_end) = after_name.find('\'') {
                let import_name = &line[name_start + 20..name_start + 20 + name_end];
                if let Some(from_start) = lower_line.find("from '") {
                    let after_from = &line[from_start + 6..];
                    if let Some(from_end) = after_from.find('\'') {
                        let from_module = &after_from[..from_end];
                        // Build the import pattern to search for in project files
                        let import_pattern = format!("from {} import", from_module);
                        for (idx, file) in files.iter().enumerate() {
                            if broken.iter().any(|(i, _)| *i == idx) {
                                continue;
                            }
                            if file.content.contains(&import_pattern)
                                && file.content.contains(import_name)
                            {
                                broken.push((idx, vec![format!(
                                    "Bad import: 'from {} import {}' — '{}' does not exist in this package. Remove or replace it.",
                                    from_module, import_name, import_name
                                )]));
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 3: __init__.py re-export detection — flag __init__.py files that import from submodules
    // (common cause of circular imports)
    if feedback_lower.contains("importerror")
        || feedback_lower.contains("circular")
        || feedback_lower.contains("nameerror")
    {
        for (idx, file) in files.iter().enumerate() {
            if broken.iter().any(|(i, _)| *i == idx) {
                continue;
            }
            let file_name = file.path.display().to_string();
            let lower_content = file.content.to_lowercase();

            // Flag __init__.py files that re-export (these commonly cause circular imports)
            if file_name.ends_with("__init__.py")
                && !file.content.trim().is_empty()
                && lower_content.contains("from ")
                && lower_content.contains(" import ")
            {
                broken.push((idx, vec!["__init__.py re-exports cause circular imports — should be empty or contain only __all__".to_string()]));
            }
        }
    }

    // Step 4: Content-based issue detection
    for (idx, file) in files.iter().enumerate() {
        if broken.iter().any(|(i, _)| *i == idx) {
            continue;
        }

        let file_name = file.path.display().to_string();
        let lower_content = file.content.to_lowercase();
        let mut issues = Vec::new();

        // Hardcoded secrets: only flag config/docker/env files
        if feedback_lower.contains("hardcoded secret") || feedback_lower.contains("secrets found") {
            let is_config_file = file_name.contains("config")
                || file_name.contains("docker")
                || file_name.contains("settings")
                || file_name.contains(".env")
                || file_name.contains(".yml")
                || file_name.contains(".yaml");
            if is_config_file
                && (lower_content.contains("password = \"")
                    || lower_content.contains("secret_key = \""))
            {
                issues.push("Hardcoded secrets — use environment variable references".to_string());
            }
        }

        // Pydantic v1/v2: only flag files that actually use wrong imports
        if lower_content.contains("from pydantic import basesettings") {
            issues.push("Pydantic v1/v2 mismatch: use pydantic_settings.BaseSettings".to_string());
        }

        if !issues.is_empty() {
            broken.push((idx, issues));
        }
    }

    broken
}

/// Extract the failed module path from an import error message.
fn extract_failed_module(error_line: &str) -> Option<String> {
    // "No module named 'app.api.v1.endpoints'"
    if let Some(pos) = error_line.find("no module named") {
        let after = &error_line[pos + 16..];
        let module = after
            .trim()
            .trim_matches(|c: char| c == '\'' || c == '"' || c == ' ' || c == '(');
        let module = module.trim_end_matches(['\'', '"', ')']);
        if !module.is_empty() {
            return Some(module.to_string());
        }
    }
    // "cannot import name 'X' from 'app.module'"
    if let Some(pos) = error_line.find("from '") {
        let after = &error_line[pos + 6..];
        if let Some(end) = after.find('\'') {
            return Some(after[..end].to_string());
        }
    }
    None
}

fn detect_language(prompt: &str) -> String {
    let lower = prompt.to_lowercase();
    if lower.contains("python")
        || lower.contains("fastapi")
        || lower.contains("django")
        || lower.contains("flask")
    {
        "python".to_string()
    } else if lower.contains("typescript") || lower.contains("next.js") || lower.contains("react") {
        "typescript".to_string()
    } else if lower.contains("javascript") || lower.contains("node") || lower.contains("express") {
        "javascript".to_string()
    } else if lower.contains("rust") || lower.contains("cargo") {
        "rust".to_string()
    } else if lower.contains("go ") || lower.contains("golang") {
        "go".to_string()
    } else if lower.contains("c++") || lower.contains("cpp") || lower.contains("cmake") {
        "c++".to_string()
    } else {
        "python".to_string()
    }
}

fn create_output_dir(prompt: &str) -> Result<PathBuf> {
    let safe_name: String = prompt
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .chars()
        .take(40)
        .collect();

    let mut cleaned = String::new();
    let mut last_was_underscore = false;
    for c in safe_name.chars() {
        if c == '_' {
            if !last_was_underscore {
                cleaned.push(c);
            }
            last_was_underscore = true;
        } else {
            cleaned.push(c);
            last_was_underscore = false;
        }
    }
    let cleaned = cleaned.trim_matches('_');

    let dir = PathBuf::from(format!("output/{}", cleaned));
    fs::create_dir_all(&dir).context("Failed to create output directory")?;
    Ok(dir)
}

fn list_files(dir: &Path) -> Result<()> {
    println!("Files:");
    let skip = ["__pycache__", ".pytest_cache", ".pyc", ".mypy_cache"];
    for entry in walkdir(dir)? {
        let path_str = entry.to_string_lossy();
        if skip.iter().any(|s| path_str.contains(s)) {
            continue;
        }
        let relative = entry.strip_prefix(dir).unwrap_or(&entry);
        let size = fs::metadata(&entry).map(|m| m.len()).unwrap_or(0);
        println!("  {} ({} bytes)", relative.display(), size);
    }
    Ok(())
}

fn walkdir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(walkdir(&path)?);
            } else {
                files.push(path);
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language_python() {
        assert_eq!(detect_language("Build a FastAPI endpoint"), "python");
    }

    #[test]
    fn test_detect_language_typescript() {
        assert_eq!(detect_language("Create a Next.js dashboard"), "typescript");
    }

    #[test]
    fn test_detect_language_default() {
        assert_eq!(detect_language("build something cool"), "python");
    }

    #[test]
    fn test_create_output_dir() {
        let dir = create_output_dir("Build a REST API!").unwrap();
        assert!(dir.to_str().unwrap().contains("build_a_rest_api"));
        let _ = std::fs::remove_dir_all("output");
    }

    #[test]
    fn test_default_paths() {
        assert_eq!(default_code_path("python"), "app/main.py");
        assert_eq!(default_test_path("python"), "tests/test_main.py");
        assert_eq!(default_code_path("rust"), "src/main.rs");
    }
}
