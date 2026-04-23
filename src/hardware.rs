//! Full hardware monitoring — CPU, RAM, thermal, Ollama VRAM.
//! Ported from battleclaw-v2. Polls every 2 seconds.

use anyhow::Result;

#[derive(Debug, Default, Clone)]
pub struct HardwareMetrics {
    pub cpu_usage_total: f32,
    pub cpu_name: String,
    pub core_count: usize,
    pub mem_total_gb: f64,
    pub mem_used_gb: f64,
    pub mem_available_gb: f64,
    pub temperatures: Vec<TempReading>,
    pub ollama_models: Vec<OllamaModel>,
    pub ollama_vram_total_gb: f64,
    pub ollama_cpu_pct: f32,
    pub ollama_mem_gb: f64,
}

#[derive(Debug, Clone)]
pub struct TempReading {
    pub label: String,
    pub celsius: f64,
    pub critical: f64,
}

#[derive(Debug, Clone)]
pub struct OllamaModel {
    pub name: String,
    pub size_gb: f64,
    pub vram_gb: f64,
    pub context_length: u64,
}

/// Collect all hardware metrics.
pub async fn collect_metrics() -> HardwareMetrics {
    let mut m = HardwareMetrics::default();

    // CPU info (cross-platform)
    m.core_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    m.cpu_name = get_cpu_name();

    // CPU usage + memory — platform-specific
    collect_cpu_memory(&mut m);

    // Ollama running models (cross-platform via HTTP API)
    if let Ok(models) = get_ollama_running_models().await {
        m.ollama_vram_total_gb = models.iter().map(|m| m.vram_gb).sum();
        m.ollama_models = models;
    }

    // Ollama process stats (Unix only — gracefully skipped on Windows)
    collect_ollama_process_stats(&mut m);

    m
}

fn collect_cpu_memory(m: &mut HardwareMetrics) {
    if cfg!(target_os = "macos") {
        // macOS: sysctl + vm_stat
        if let Ok(output) = std::process::Command::new("sysctl")
            .args(["-n", "vm.loadavg"])
            .output()
        {
            if let Ok(s) = String::from_utf8(output.stdout) {
                let nums: Vec<f64> = s
                    .split_whitespace()
                    .filter_map(|w| {
                        w.trim_matches(|c: char| !c.is_numeric() && c != '.')
                            .parse()
                            .ok()
                    })
                    .collect();
                if !nums.is_empty() {
                    m.cpu_usage_total = ((nums[0] / m.core_count as f64) * 100.0).min(100.0) as f32;
                }
            }
        }
        if let Ok(output) = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
        {
            if let Ok(s) = String::from_utf8(output.stdout) {
                if let Ok(bytes) = s.trim().parse::<u64>() {
                    m.mem_total_gb = bytes as f64 / 1_073_741_824.0;
                }
            }
        }
        if let Ok(output) = std::process::Command::new("vm_stat").output() {
            if let Ok(s) = String::from_utf8(output.stdout) {
                let page_size = 16384u64; // Apple Silicon default
                let mut pages_active = 0u64;
                let mut pages_wired = 0u64;
                let mut pages_compressed = 0u64;
                for line in s.lines() {
                    if line.contains("Pages active") {
                        pages_active = extract_vm_stat_value(line);
                    } else if line.contains("Pages wired") {
                        pages_wired = extract_vm_stat_value(line);
                    } else if line.contains("Pages occupied by compressor") {
                        pages_compressed = extract_vm_stat_value(line);
                    }
                }
                m.mem_used_gb = ((pages_active + pages_wired + pages_compressed) * page_size)
                    as f64
                    / 1_073_741_824.0;
                m.mem_available_gb = m.mem_total_gb - m.mem_used_gb;
            }
        }
    } else if cfg!(target_os = "linux") {
        // Linux: /proc/loadavg + /proc/meminfo
        if let Ok(s) = std::fs::read_to_string("/proc/loadavg") {
            if let Some(load1) = s
                .split_whitespace()
                .next()
                .and_then(|w| w.parse::<f64>().ok())
            {
                m.cpu_usage_total = ((load1 / m.core_count as f64) * 100.0).min(100.0) as f32;
            }
        }
        if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
            for line in s.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(kb) = extract_meminfo_kb(line) {
                        m.mem_total_gb = kb as f64 / 1_048_576.0;
                    }
                } else if line.starts_with("MemAvailable:") {
                    if let Some(kb) = extract_meminfo_kb(line) {
                        m.mem_available_gb = kb as f64 / 1_048_576.0;
                    }
                }
            }
            m.mem_used_gb = m.mem_total_gb - m.mem_available_gb;
        }
    }
    // Windows: falls through with defaults (0.0) — acceptable for v1
}

fn collect_ollama_process_stats(m: &mut HardwareMetrics) {
    if cfg!(windows) {
        return;
    }
    if let Ok(output) = std::process::Command::new("ps")
        .args(["-eo", "pid,%cpu,rss,comm"])
        .output()
    {
        if let Ok(s) = String::from_utf8(output.stdout) {
            for line in s.lines() {
                if line.contains("ollama") && !line.contains("grep") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        m.ollama_cpu_pct = parts[1].parse().unwrap_or(0.0);
                        let rss_kb: f64 = parts[2].parse().unwrap_or(0.0);
                        m.ollama_mem_gb = rss_kb / 1_048_576.0;
                    }
                }
            }
        }
    }
}

fn get_cpu_name() -> String {
    if cfg!(target_os = "macos") {
        if let Ok(output) = std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
        {
            if let Ok(s) = String::from_utf8(output.stdout) {
                let name = s.trim().to_string();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    } else if cfg!(target_os = "linux") {
        if let Ok(s) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in s.lines() {
                if line.starts_with("model name") {
                    if let Some(name) = line.split(':').nth(1) {
                        return name.trim().to_string();
                    }
                }
            }
        }
    }
    format!(
        "{}-core CPU",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    )
}

fn extract_meminfo_kb(line: &str) -> Option<u64> {
    line.split_whitespace().nth(1)?.parse().ok()
}

fn extract_vm_stat_value(line: &str) -> u64 {
    line.split(':')
        .nth(1)
        .and_then(|v| v.trim().trim_end_matches('.').parse().ok())
        .unwrap_or(0)
}

async fn get_ollama_running_models() -> Result<Vec<OllamaModel>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;

    let resp = client
        .get(format!("{}/api/ps", crate::llm::ollama_url()))
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;

    let models = body["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| OllamaModel {
                    name: m["name"].as_str().unwrap_or("").to_string(),
                    size_gb: m["size"].as_u64().unwrap_or(0) as f64 / 1_073_741_824.0,
                    vram_gb: m["size_vram"].as_u64().unwrap_or(0) as f64 / 1_073_741_824.0,
                    context_length: m["details"]["parameter_size"]
                        .as_str()
                        .and_then(|s| s.replace("B", "").parse().ok())
                        .unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// Render hardware metrics as colored text for TUI.
pub fn render_for_tui(m: &HardwareMetrics) -> Vec<String> {
    let mut lines = Vec::new();

    // CPU
    let cpu_bar = progress_bar(m.cpu_usage_total as f64, 100.0, 20);
    lines.push(format!(
        "CPU:  {} {:.0}%  {}",
        cpu_bar, m.cpu_usage_total, m.cpu_name
    ));
    lines.push(format!("      {} cores", m.core_count));

    // Memory
    let mem_pct = if m.mem_total_gb > 0.0 {
        (m.mem_used_gb / m.mem_total_gb) * 100.0
    } else {
        0.0
    };
    let mem_bar = progress_bar(m.mem_used_gb, m.mem_total_gb, 20);
    lines.push(format!(
        "RAM:  {} {:.1}/{:.1} GB ({:.0}%)",
        mem_bar, m.mem_used_gb, m.mem_total_gb, mem_pct
    ));
    lines.push(format!("      {:.1} GB available", m.mem_available_gb));
    lines.push(String::new());

    // Ollama
    if m.ollama_models.is_empty() {
        lines.push("Ollama: no models loaded".to_string());
    } else {
        lines.push(format!(
            "Ollama: {} models loaded ({:.1} GB VRAM)",
            m.ollama_models.len(),
            m.ollama_vram_total_gb
        ));
        for model in &m.ollama_models {
            lines.push(format!(
                "  {} — {:.1} GB (VRAM: {:.1} GB)",
                model.name, model.size_gb, model.vram_gb
            ));
        }
    }
    if m.ollama_cpu_pct > 0.0 || m.ollama_mem_gb > 0.0 {
        lines.push(format!(
            "  Process: {:.0}% CPU, {:.1} GB RAM",
            m.ollama_cpu_pct, m.ollama_mem_gb
        ));
    }

    // Thermal
    if !m.temperatures.is_empty() {
        lines.push(String::new());
        lines.push("Thermal:".to_string());
        for t in &m.temperatures {
            lines.push(format!(
                "  {}: {:.0}°C (critical: {:.0}°C)",
                t.label, t.celsius, t.critical
            ));
        }
    }

    lines
}

fn progress_bar(value: f64, max: f64, width: usize) -> String {
    let ratio = (value / max).clamp(0.0, 1.0);
    let filled = (ratio * width as f64) as usize;
    let empty = width - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

impl std::fmt::Display for HardwareMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for line in render_for_tui(self) {
            writeln!(f, "{}", line)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_bar() {
        let bar = progress_bar(50.0, 100.0, 10);
        assert_eq!(bar, "[█████░░░░░]");
    }

    #[test]
    fn test_num_cpus() {
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        assert!(n >= 1);
    }
}
