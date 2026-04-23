//! Pipeline run report — structured JSON capture of every stage.
//!
//! Generated after EVERY pipeline run (pass or fail).
//! Saved to `.battlecommand/reports/{slug}_{timestamp}.json`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const REPORTS_DIR: &str = ".battlecommand/reports";

// ─── Top-level report ───

#[derive(Debug, Serialize, Deserialize)]
pub struct PipelineReport {
    pub version: u32,
    pub generated_at: String,
    pub mission: MissionMeta,
    pub model_config: ModelConfigSnapshot,
    pub timing: TimingSummary,
    pub router: RouterStageReport,
    pub architect: LlmStageReport,
    pub tester: LlmStageReport,
    pub rounds: Vec<RoundReport>,
    pub result: PipelineResult,
    pub code_metrics: CodeMetrics,
}

// ─── Sub-structures ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionMeta {
    pub mission_id: String,
    pub prompt: String,
    pub preset: String,
    pub language: String,
    pub output_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfigSnapshot {
    pub architect: RoleSnapshot,
    pub tester: RoleSnapshot,
    pub coder: RoleSnapshot,
    pub security: RoleSnapshot,
    pub critique: RoleSnapshot,
    pub cto: RoleSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleSnapshot {
    pub model: String,
    pub provider: String,
    pub context_size: u32,
    pub max_predict: u32,
}

impl RoleSnapshot {
    pub fn from_role(role: &crate::model_config::RoleConfig) -> Self {
        Self {
            model: role.model.clone(),
            provider: role.provider.to_string(),
            context_size: role.context_size(),
            max_predict: role.max_predict(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimingSummary {
    pub total_secs: f64,
    pub router_secs: f64,
    pub architect_secs: f64,
    pub tester_secs: f64,
    pub rounds_secs: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterStageReport {
    pub tier: String,
    pub duration_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmStageReport {
    pub model: String,
    pub duration_secs: f64,
    pub token_count: u64,
    pub tok_per_sec: f64,
    pub output_lines: u64,
}

impl Default for LlmStageReport {
    fn default() -> Self {
        Self {
            model: String::new(),
            duration_secs: 0.0,
            token_count: 0,
            tok_per_sec: 0.0,
            output_lines: 0,
        }
    }
}

// ─── Round report (stages 4-8) ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundReport {
    pub round_number: usize,
    pub coder: LlmStageReport,
    pub verifier: VerifierReport,
    pub security: SecurityReport,
    pub critique: CritiqueReport,
    pub cto: CtoReport,
    pub final_score: f32,
    pub critique_avg: f32,
    pub verifier_score: f32,
    pub feedback_to_next_round: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierReport {
    pub duration_secs: f64,
    pub avg_score: f32,
    pub tests_passed: u32,
    pub tests_failed: u32,
    pub tests_run: bool,
    pub total_lint_issues: usize,
    pub secrets_found: bool,
    pub file_reports: Vec<FileVerifierReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileVerifierReport {
    pub path: String,
    pub score: f32,
    pub lint_passed: bool,
    pub lint_issues: Vec<String>,
    pub syntax_valid: bool,
    pub has_tests: bool,
    pub has_docstring: bool,
    pub has_error_handling: bool,
    pub has_hardcoded_secrets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReport {
    pub model: String,
    pub duration_secs: f64,
    pub verdict: String,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CritiqueReport {
    pub model: String,
    pub duration_secs: f64,
    pub scores: CritiqueScores,
    pub avg: f32,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CritiqueScores {
    pub dev: f32,
    pub arch: f32,
    pub test: f32,
    pub sec: f32,
    pub docs: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtoReport {
    pub model: String,
    pub duration_secs: f64,
    pub verdict: String,
    pub approved: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PipelineResult {
    pub quality_gate_passed: bool,
    pub quality_gate_threshold: f32,
    pub best_score: f32,
    pub best_round: usize,
    pub total_rounds: usize,
    pub max_rounds_allowed: usize,
    pub output_dir: String,
    pub files_shipped: Vec<ShippedFile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShippedFile {
    pub path: String,
    pub language: String,
    pub lines: usize,
    pub bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CodeMetrics {
    pub total_files: usize,
    pub total_loc: usize,
    pub test_files: usize,
    pub languages: HashMap<String, LanguageMetrics>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LanguageMetrics {
    pub files: usize,
    pub loc: usize,
}

// ─── Builder ───

pub struct ReportBuilder {
    pub mission: Option<MissionMeta>,
    pub model_config: Option<ModelConfigSnapshot>,
    pub router: Option<RouterStageReport>,
    pub architect: Option<LlmStageReport>,
    pub tester: Option<LlmStageReport>,
    pub rounds: Vec<RoundReport>,
    pub start_time: std::time::Instant,
    stage_times: StageTimes,
}

struct StageTimes {
    router: f64,
    architect: f64,
    tester: f64,
}

impl Default for ReportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportBuilder {
    pub fn new() -> Self {
        Self {
            mission: None,
            model_config: None,
            router: None,
            architect: None,
            tester: None,
            rounds: Vec::new(),
            start_time: std::time::Instant::now(),
            stage_times: StageTimes {
                router: 0.0,
                architect: 0.0,
                tester: 0.0,
            },
        }
    }

    pub fn set_mission(&mut self, meta: MissionMeta) {
        self.mission = Some(meta);
    }

    pub fn set_model_config(&mut self, config: &crate::model_config::ModelConfig) {
        self.model_config = Some(ModelConfigSnapshot {
            architect: RoleSnapshot::from_role(&config.architect),
            tester: RoleSnapshot::from_role(&config.tester),
            coder: RoleSnapshot::from_role(&config.coder),
            security: RoleSnapshot::from_role(&config.security),
            critique: RoleSnapshot::from_role(&config.critique),
            cto: RoleSnapshot::from_role(&config.cto),
        });
    }

    pub fn set_router(&mut self, tier: &str, duration_secs: f64) {
        self.stage_times.router = duration_secs;
        self.router = Some(RouterStageReport {
            tier: tier.to_string(),
            duration_secs,
        });
    }

    pub fn set_architect(&mut self, stats: LlmStageReport) {
        self.stage_times.architect = stats.duration_secs;
        self.architect = Some(stats);
    }

    pub fn set_tester(&mut self, stats: LlmStageReport) {
        self.stage_times.tester = stats.duration_secs;
        self.tester = Some(stats);
    }

    pub fn add_round(&mut self, round: RoundReport) {
        self.rounds.push(round);
    }

    pub fn build(
        &self,
        passed: bool,
        best_score: f32,
        best_round: usize,
        output_dir: &Path,
        files: &[crate::codegen::GeneratedFile],
    ) -> PipelineReport {
        let total_secs = self.start_time.elapsed().as_secs_f64();

        let files_shipped: Vec<ShippedFile> = files
            .iter()
            .map(|f| ShippedFile {
                path: f.path.display().to_string(),
                language: f.language.clone(),
                lines: f.content.lines().count(),
                bytes: f.content.len(),
            })
            .collect();

        let total_loc: usize = files_shipped.iter().map(|f| f.lines).sum();
        let test_files = files_shipped
            .iter()
            .filter(|f| {
                let p = f.path.to_lowercase();
                p.contains("test") || p.contains("spec")
            })
            .count();

        let mut languages: HashMap<String, LanguageMetrics> = HashMap::new();
        for f in &files_shipped {
            let entry = languages
                .entry(f.language.clone())
                .or_insert(LanguageMetrics { files: 0, loc: 0 });
            entry.files += 1;
            entry.loc += f.lines;
        }

        let rounds_secs: Vec<f64> = self
            .rounds
            .iter()
            .map(|r| {
                r.coder.duration_secs
                    + r.verifier.duration_secs
                    + r.security.duration_secs
                    + r.critique.duration_secs
                    + r.cto.duration_secs
            })
            .collect();

        let total_rounds = self.rounds.len();
        let empty_snap = RoleSnapshot {
            model: String::new(),
            provider: String::new(),
            context_size: 0,
            max_predict: 0,
        };

        PipelineReport {
            version: 1,
            generated_at: chrono::Utc::now().to_rfc3339(),
            mission: self.mission.clone().unwrap_or(MissionMeta {
                mission_id: String::new(),
                prompt: String::new(),
                preset: String::new(),
                language: String::new(),
                output_dir: output_dir.display().to_string(),
            }),
            model_config: self.model_config.clone().unwrap_or(ModelConfigSnapshot {
                architect: empty_snap.clone(),
                tester: empty_snap.clone(),
                coder: empty_snap.clone(),
                security: empty_snap.clone(),
                critique: empty_snap.clone(),
                cto: empty_snap,
            }),
            timing: TimingSummary {
                total_secs,
                router_secs: self.stage_times.router,
                architect_secs: self.stage_times.architect,
                tester_secs: self.stage_times.tester,
                rounds_secs,
            },
            router: self.router.clone().unwrap_or(RouterStageReport {
                tier: "unknown".to_string(),
                duration_secs: 0.0,
            }),
            architect: self.architect.clone().unwrap_or_default(),
            tester: self.tester.clone().unwrap_or_default(),
            rounds: self.rounds.clone(),
            result: PipelineResult {
                quality_gate_passed: passed,
                quality_gate_threshold: 9.2,
                best_score,
                best_round,
                total_rounds,
                max_rounds_allowed: 5,
                output_dir: output_dir.display().to_string(),
                files_shipped,
            },
            code_metrics: CodeMetrics {
                total_files: files.len(),
                total_loc,
                test_files,
                languages,
            },
        }
    }
}

// ─── Save / Load / List ───

pub fn save_report(report: &PipelineReport) -> Result<PathBuf> {
    fs::create_dir_all(REPORTS_DIR)?;

    let slug: String = report
        .mission
        .prompt
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();
    let slug = slug.trim_matches('_').to_string();
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");

    let filename = format!("{}_{}.json", slug, ts);
    let path = PathBuf::from(REPORTS_DIR).join(&filename);

    let json = serde_json::to_string_pretty(report)?;
    fs::write(&path, &json)?;

    // Also write latest.json
    let latest = PathBuf::from(REPORTS_DIR).join("latest.json");
    fs::write(&latest, &json)?;

    println!("[REPORT] Saved to {}", path.display());
    Ok(path)
}

pub fn load_report(path: &Path) -> Result<PipelineReport> {
    let content = fs::read_to_string(path)?;
    let report: PipelineReport = serde_json::from_str(&content)?;
    Ok(report)
}

pub fn list_reports() -> Result<Vec<PathBuf>> {
    let dir = Path::new(REPORTS_DIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut reports: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|e| e == "json").unwrap_or(false)
                && p.file_name().map(|n| n != "latest.json").unwrap_or(true)
        })
        .collect();
    reports.sort();
    Ok(reports)
}

/// Pretty-print a report to stdout.
pub fn print_report(report: &PipelineReport) {
    let r = report;
    println!();
    println!("=== Pipeline Report (v{}) ===", r.version);
    println!("Generated: {}", r.generated_at);
    println!();

    // Mission
    println!("MISSION");
    println!(
        "  Prompt:     {}",
        r.mission.prompt.chars().take(80).collect::<String>()
    );
    println!(
        "  Preset:     {} | Language: {} | Complexity: {}",
        r.mission.preset, r.mission.language, r.router.tier
    );
    println!("  Output:     {}", r.mission.output_dir);
    println!();

    // Model config
    println!("MODELS");
    println!(
        "  Architect:  {} ({})",
        r.model_config.architect.model, r.model_config.architect.provider
    );
    println!(
        "  Tester:     {} ({})",
        r.model_config.tester.model, r.model_config.tester.provider
    );
    println!(
        "  Coder:      {} ({})",
        r.model_config.coder.model, r.model_config.coder.provider
    );
    println!(
        "  Security:   {} ({})",
        r.model_config.security.model, r.model_config.security.provider
    );
    println!(
        "  Critique:   {} ({})",
        r.model_config.critique.model, r.model_config.critique.provider
    );
    println!(
        "  CTO:        {} ({})",
        r.model_config.cto.model, r.model_config.cto.provider
    );
    println!();

    // Timing
    println!("TIMING");
    println!("  Total:      {:.1}s", r.timing.total_secs);
    println!(
        "  Router:     {:.1}s | Architect: {:.1}s | Tester: {:.1}s",
        r.timing.router_secs, r.timing.architect_secs, r.timing.tester_secs
    );
    for (i, secs) in r.timing.rounds_secs.iter().enumerate() {
        println!("  Round {}:    {:.1}s", i + 1, secs);
    }
    println!();

    // Architect + Tester stats
    if r.architect.duration_secs > 0.0 {
        println!(
            "ARCHITECT: {} | {:.1}s | {} tokens | {:.0} tok/s | {} lines",
            r.architect.model,
            r.architect.duration_secs,
            r.architect.token_count,
            r.architect.tok_per_sec,
            r.architect.output_lines
        );
    }
    if r.tester.duration_secs > 0.0 {
        println!(
            "TESTER:    {} | {:.1}s | {} tokens | {:.0} tok/s | {} lines",
            r.tester.model,
            r.tester.duration_secs,
            r.tester.token_count,
            r.tester.tok_per_sec,
            r.tester.output_lines
        );
    }
    println!();

    // Rounds
    for round in &r.rounds {
        let passed = if round.final_score >= r.result.quality_gate_threshold {
            "PASS"
        } else {
            "FAIL"
        };
        println!(
            "ROUND {} [{} — {:.1}/10]",
            round.round_number, passed, round.final_score
        );
        println!(
            "  Coder:    {:.1}s | {} tokens | {:.0} tok/s",
            round.coder.duration_secs, round.coder.token_count, round.coder.tok_per_sec
        );
        println!(
            "  Verifier: {:.1}/10 | tests {}/{} | lint {} | secrets {}",
            round.verifier.avg_score,
            round.verifier.tests_passed,
            round.verifier.tests_failed,
            round.verifier.total_lint_issues,
            if round.verifier.secrets_found {
                "FOUND"
            } else {
                "clean"
            }
        );
        println!(
            "  Security: {} | {}",
            if round.security.passed {
                "PASS"
            } else {
                "FAIL"
            },
            round.security.verdict.lines().next().unwrap_or("")
        );
        println!(
            "  Critique: Dev={:.1} Arch={:.1} Test={:.1} Sec={:.1} Docs={:.1} => {:.1}",
            round.critique.scores.dev,
            round.critique.scores.arch,
            round.critique.scores.test,
            round.critique.scores.sec,
            round.critique.scores.docs,
            round.critique.avg
        );
        println!(
            "  CTO:      {} | {}",
            if round.cto.approved {
                "APPROVE"
            } else {
                "REJECT"
            },
            round.cto.verdict.lines().next().unwrap_or("")
        );
        println!(
            "  Score:    critique {:.1} * 0.4 + verifier {:.1} * 0.6 = {:.1}",
            round.critique_avg, round.verifier_score, round.final_score
        );
        if let Some(ref fb) = round.feedback_to_next_round {
            let lines: usize = fb.lines().count();
            println!("  Feedback: {} lines sent to next round", lines);
        }
        println!();
    }

    // Result
    let gate = if r.result.quality_gate_passed {
        "PASSED"
    } else {
        "FAILED"
    };
    println!(
        "RESULT: {} ({:.1}/10, round {}/{})",
        gate, r.result.best_score, r.result.best_round, r.result.total_rounds
    );
    println!();

    // Code metrics
    println!("CODE METRICS");
    println!(
        "  Files: {} | LOC: {} | Test files: {}",
        r.code_metrics.total_files, r.code_metrics.total_loc, r.code_metrics.test_files
    );
    for (lang, m) in &r.code_metrics.languages {
        println!("  {}: {} files, {} LOC", lang, m.files, m.loc);
    }
    println!();

    // File list
    if !r.result.files_shipped.is_empty() {
        println!("FILES");
        for f in &r.result.files_shipped {
            println!("  {} ({} lines, {} bytes)", f.path, f.lines, f.bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_builder_defaults() {
        let builder = ReportBuilder::new();
        let report = builder.build(false, 5.0, 1, Path::new("output/test"), &[]);
        assert_eq!(report.version, 1);
        assert!(!report.result.quality_gate_passed);
        assert_eq!(report.result.best_score, 5.0);
        assert_eq!(report.code_metrics.total_files, 0);
    }

    #[test]
    fn test_list_reports_empty() {
        // Should not crash if dir doesn't exist
        let _ = list_reports();
    }
}
