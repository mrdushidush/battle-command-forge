use crate::swebench::InstanceResult;
/// SWE-bench evaluation and reporting.
/// Generates markdown reports from run results, aggregates per-repo stats,
/// and compares against published baselines.
use anyhow::Result;
use std::collections::BTreeMap;

pub fn generate_report(output_dir: &str) -> Result<()> {
    let results_path = format!("{}/swebench_results.jsonl", output_dir);
    if !std::path::Path::new(&results_path).exists() {
        return Err(anyhow::anyhow!(
            "No results found at {}. Run `swebench run` first.",
            results_path
        ));
    }

    let data = std::fs::read_to_string(&results_path)?;
    let results: Vec<InstanceResult> = data
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if results.is_empty() {
        return Err(anyhow::anyhow!("Results file is empty"));
    }

    let model = results[0].model.clone();
    let total = results.len();
    let resolved: usize = results.iter().filter(|r| r.resolved).count();
    let errors: usize = results.iter().filter(|r| r.error.is_some()).count();
    let rate = resolved as f64 / total as f64 * 100.0;
    let avg_turns: f64 = results.iter().map(|r| r.turns_used as f64).sum::<f64>() / total as f64;
    let avg_duration: f64 = results.iter().map(|r| r.duration_secs).sum::<f64>() / total as f64;
    let total_tokens: u64 = results.iter().map(|r| r.tokens_used).sum();
    let total_duration: f64 = results.iter().map(|r| r.duration_secs).sum();

    let mut by_repo: BTreeMap<String, Vec<&InstanceResult>> = BTreeMap::new();
    for r in &results {
        by_repo.entry(r.repo.clone()).or_default().push(r);
    }

    let mut report = format!(
        "# SWE-bench Results — {}\n\nGenerated: {}\n\n",
        model,
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    );

    report.push_str("## Summary\n\n| Metric | Value |\n|--------|-------|\n");
    report.push_str(&format!("| Instances | {} |\n", total));
    report.push_str(&format!(
        "| Resolved | {}/{} ({:.1}%) |\n",
        resolved, total, rate
    ));
    report.push_str(&format!("| Errors | {} |\n", errors));
    report.push_str(&format!("| Avg turns | {:.1} |\n", avg_turns));
    report.push_str(&format!("| Avg duration | {:.0}s |\n", avg_duration));
    report.push_str(&format!("| Total tokens | {} |\n", total_tokens));
    report.push_str(&format!(
        "| Total time | {:.0}s ({:.1}h) |\n\n",
        total_duration,
        total_duration / 3600.0
    ));

    report.push_str("## Per-Repository Breakdown\n\n| Repository | Instances | Resolved | Rate | Avg Turns | Avg Duration |\n|------------|-----------|----------|------|-----------|-------------|\n");
    for (repo, repo_results) in &by_repo {
        let rt = repo_results.len();
        let rr = repo_results.iter().filter(|r| r.resolved).count();
        let rrate = rr as f64 / rt as f64 * 100.0;
        let rat = repo_results
            .iter()
            .map(|r| r.turns_used as f64)
            .sum::<f64>()
            / rt as f64;
        let rad = repo_results.iter().map(|r| r.duration_secs).sum::<f64>() / rt as f64;
        report.push_str(&format!(
            "| {} | {} | {} | {:.1}% | {:.1} | {:.0}s |\n",
            repo, rt, rr, rrate, rat, rad
        ));
    }

    report.push_str("\n## Comparison with Published Results\n\n| Agent | Dataset | Resolve Rate | Cost/Instance |\n|-------|---------|:------------:|:-------------:|\n");
    report.push_str(&format!(
        "| **BattleCommand Forge ({})** | **this run** | **{:.1}%** | **~$0.30** |\n",
        model, rate
    ));
    report.push_str("| OpenHands (Claude 3.5) | Verified | 53.0% | ~$0.50 |\n");
    report.push_str("| Moatless Tools (Claude 3.5) | Verified | 38.4% | ~$0.30 |\n");
    report.push_str("| SWE-agent (GPT-4) | Full | 12.5% | ~$0.50 |\n");
    report.push_str("| Devin | Lite | 13.8% | — |\n\n");

    report.push_str("## Resolved Instances\n\n");
    let resolved_results: Vec<&InstanceResult> = results.iter().filter(|r| r.resolved).collect();
    if resolved_results.is_empty() {
        report.push_str("None resolved.\n\n");
    } else {
        report.push_str("| Instance | Turns | Duration | Files Modified |\n|----------|-------|----------|----------------|\n");
        for r in &resolved_results {
            report.push_str(&format!(
                "| {} | {} | {:.0}s | {} |\n",
                r.instance_id,
                r.turns_used,
                r.duration_secs,
                r.files_modified.join(", ")
            ));
        }
    }

    let error_results: Vec<&InstanceResult> =
        results.iter().filter(|r| r.error.is_some()).collect();
    if !error_results.is_empty() {
        report.push_str("\n## Errors\n\n| Instance | Error |\n|----------|-------|\n");
        for r in &error_results {
            let err = r.error.as_deref().unwrap_or("unknown");
            let short = if err.len() > 80 { &err[..80] } else { err };
            report.push_str(&format!("| {} | {} |\n", r.instance_id, short));
        }
    }

    let report_path = format!("{}/report.md", output_dir);
    std::fs::write(&report_path, &report)?;
    println!("Report written to {}", report_path);
    println!("\n{}", report);
    Ok(())
}
