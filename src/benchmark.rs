use crate::mission::MissionRunner;
use crate::model_config::{ModelConfig, Preset};
/// Multi-model benchmark framework.
/// Runs N missions across different model configurations to compare
/// quality, speed, and cost.
use anyhow::Result;
use std::time::Instant;

const BENCHMARK_MISSIONS: &[(&str, u32)] = &[
    ("Build a Python CLI that converts CSV files to JSON with column filtering and pretty-print options. Single file `tasks/csv_to_json.py`. Include argparse, read/write, error handling for missing files. Validate: python3 tasks/csv_to_json.py --help", 4),
    ("Build a Python sensor data fusion system. Single file `tasks/sensor_fusion.py` with: a 1D KalmanFilter class (predict/update), a TrackManager class that maintains multiple tracks, correlates detections (nearest-neighbor within gate distance), creates new tracks for uncorrelated detections, prunes stale tracks. Test with 2 simulated targets over 20 time steps.", 7),
    ("Build a Python URL shortener with FastAPI. Files: `tasks/main.py`, `tasks/models.py`, `tasks/database.py`. SQLite backend, base62 encoding, redirect endpoint, stats endpoint showing click count. Include proper error handling for duplicate URLs and not-found cases.", 5),
    ("Build a Python config parser library. Single file `tasks/config_parser.py`. Support TOML, JSON, and YAML formats. Merge multiple config files with priority ordering. Environment variable interpolation (${VAR} syntax). Type coercion for boolean, int, float values. Validate: python3 -c \"from tasks.config_parser import ConfigParser; c=ConfigParser(); print('PASS')\"", 6),
    ("Build a C++ coordinate transform library. Single file `tasks/coord_transforms.cpp` with: WGS84 constants, geodetic to ECEF conversion, ECEF to ENU given reference point, haversine great-circle distance. All angles in radians. Test: London to Paris must be 340-345 km. Compile: c++ -std=c++17 -Wall -o /tmp/bc_coords tasks/coord_transforms.cpp && /tmp/bc_coords", 8),
];

struct BenchmarkResult {
    mission: String,
    model: String,
    time_seconds: f64,
    score: Option<f64>,
    error: Option<String>,
}

pub async fn run_benchmark(phase: &str, tasks: usize) -> Result<()> {
    println!("BattleCommand Forge Benchmark");
    println!("=============================\n");

    let missions = &BENCHMARK_MISSIONS[..tasks.min(BENCHMARK_MISSIONS.len())];

    match phase {
        "full" => run_full_benchmark(missions).await?,
        "quick" => run_quick_benchmark(missions).await?,
        _ => {
            return Err(anyhow::anyhow!(
                "Unknown phase '{}'. Use: full, quick",
                phase
            ))
        }
    }

    Ok(())
}

async fn run_full_benchmark(missions: &[(&str, u32)]) -> Result<()> {
    println!("Phase: Full Pipeline ({} missions)\n", missions.len());

    let mut results: Vec<BenchmarkResult> = Vec::new();

    for (i, (mission, _complexity)) in missions.iter().enumerate() {
        let start = Instant::now();
        println!("[{}/{}] {}", i + 1, missions.len(), truncate(mission, 60));

        let config = ModelConfig::resolve(Preset::Premium, ".", None, None, None, None);
        let mut runner = MissionRunner::new(config);
        runner.auto_mode = true;

        let result = match runner.run(mission).await {
            Ok(()) => {
                let score = runner.best_score();
                BenchmarkResult {
                    mission: truncate(mission, 50),
                    model: "premium".to_string(),
                    time_seconds: start.elapsed().as_secs_f64(),
                    score: Some(score),
                    error: None,
                }
            }
            Err(e) => BenchmarkResult {
                mission: truncate(mission, 50),
                model: "premium".to_string(),
                time_seconds: start.elapsed().as_secs_f64(),
                score: None,
                error: Some(format!("{}", e)),
            },
        };

        if let Some(ref err) = result.error {
            println!("  [FAIL] {:.1}s | {}", result.time_seconds, err);
        } else {
            println!(
                "  [OK]   {:.1}s | score: {:.1}/10",
                result.time_seconds,
                result.score.unwrap_or(0.0)
            );
        }

        results.push(result);
    }

    print_summary("Full Pipeline", &results);
    save_results("full_benchmark.md", "Full Pipeline", &results).await?;
    Ok(())
}

async fn run_quick_benchmark(missions: &[(&str, u32)]) -> Result<()> {
    println!("Phase: Quick (fast preset, {} missions)\n", missions.len());

    let mut results: Vec<BenchmarkResult> = Vec::new();

    for (i, (mission, _complexity)) in missions.iter().enumerate() {
        let start = Instant::now();
        println!("[{}/{}] {}", i + 1, missions.len(), truncate(mission, 60));

        let config = ModelConfig::resolve(Preset::Fast, ".", None, None, None, None);
        let mut runner = MissionRunner::new(config);
        runner.auto_mode = true;

        let result = match runner.run(mission).await {
            Ok(()) => BenchmarkResult {
                mission: truncate(mission, 50),
                model: "fast".to_string(),
                time_seconds: start.elapsed().as_secs_f64(),
                score: Some(runner.best_score()),
                error: None,
            },
            Err(e) => BenchmarkResult {
                mission: truncate(mission, 50),
                model: "fast".to_string(),
                time_seconds: start.elapsed().as_secs_f64(),
                score: None,
                error: Some(format!("{}", e)),
            },
        };

        if let Some(ref err) = result.error {
            println!("  [FAIL] {:.1}s | {}", result.time_seconds, err);
        } else {
            println!(
                "  [OK]   {:.1}s | score: {:.1}/10",
                result.time_seconds,
                result.score.unwrap_or(0.0)
            );
        }

        results.push(result);
    }

    print_summary("Quick", &results);
    save_results("quick_benchmark.md", "Quick", &results).await?;
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

fn print_summary(phase: &str, results: &[BenchmarkResult]) {
    println!("\n{} Benchmark Summary", phase);
    println!("{}", "=".repeat(90));
    println!(
        "{:<55} {:>8} {:>8} {:>6}",
        "Mission", "Time", "Score", "Status"
    );
    println!("{}", "-".repeat(90));

    for r in results {
        let status = if r.error.is_some() { "FAIL" } else { "OK" };
        let score = r
            .score
            .map(|s| format!("{:.1}", s))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<55} {:>7.1}s {:>8} {:>6}",
            r.mission, r.time_seconds, score, status
        );
    }
    println!("{}", "=".repeat(90));

    let total = results.len();
    let passed = results.iter().filter(|r| r.error.is_none()).count();
    let avg_score: f64 = results.iter().filter_map(|r| r.score).sum::<f64>() / passed.max(1) as f64;
    let avg_time: f64 = results.iter().map(|r| r.time_seconds).sum::<f64>() / total as f64;
    println!(
        "\n  Pass: {}/{} | Avg score: {:.1} | Avg time: {:.0}s",
        passed, total, avg_score, avg_time
    );
}

async fn save_results(filename: &str, phase: &str, results: &[BenchmarkResult]) -> Result<()> {
    let dir = ".battlecommand/benchmarks";
    tokio::fs::create_dir_all(dir).await?;

    let mut md = format!(
        "# {} Benchmark Results\n\nGenerated: {}\n\n",
        phase,
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    );
    md.push_str("| Mission | Model | Time | Score | Status |\n|---------|-------|------|-------|--------|\n");
    for r in results {
        let status = match &r.error {
            Some(e) => format!("ERR: {}", truncate(e, 30)),
            None => "OK".into(),
        };
        let score = r
            .score
            .map(|s| format!("{:.1}", s))
            .unwrap_or_else(|| "-".into());
        md.push_str(&format!(
            "| {} | {} | {:.1}s | {} | {} |\n",
            r.mission, r.model, r.time_seconds, score, status
        ));
    }

    let path = format!("{}/{}", dir, filename);
    tokio::fs::write(&path, &md).await?;
    println!("\nResults saved to {}", path);
    Ok(())
}
