//! Model configuration, listing, and benchmarking.
//!
//! Reads presets from .battlecommand/models.toml.
//! Queries Ollama API for available models.
//! Benchmarks models with a standard prompt.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub size: String,
    pub modified: String,
}

#[derive(Debug, Clone)]
pub struct PresetConfig {
    pub name: String,
    pub architect: String,
    pub tester: String,
    pub coder: String,
    pub reviewer: String,
    pub security: String,
}

impl PresetConfig {
    pub fn default_premium() -> Self {
        Self {
            name: "premium".into(),
            architect: "qwen3-coder-next:q8_0".into(),
            tester: "qwen3-coder-next:q8_0".into(),
            coder: "qwen3-coder-next:q8_0".into(),
            reviewer: "qwen3-coder-next:q8_0".into(),
            security: "qwen3-coder-next:q8_0".into(),
        }
    }

    pub fn default_balanced() -> Self {
        Self {
            name: "balanced".into(),
            architect: "qwen2.5-coder:32b".into(),
            tester: "qwen2.5-coder:32b".into(),
            coder: "qwen2.5-coder:32b".into(),
            reviewer: "qwen2.5-coder:32b".into(),
            security: "qwen2.5-coder:32b".into(),
        }
    }

    pub fn default_fast() -> Self {
        Self {
            name: "fast".into(),
            architect: "qwen2.5-coder:7b".into(),
            tester: "qwen2.5-coder:7b".into(),
            coder: "qwen2.5-coder:7b".into(),
            reviewer: "qwen2.5-coder:7b".into(),
            security: "qwen2.5-coder:7b".into(),
        }
    }
}

/// List all available Ollama models.
pub async fn list_ollama_models() -> Result<Vec<ModelInfo>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/tags", crate::llm::ollama_url()))
        .send()
        .await?;

    let body: serde_json::Value = resp.json().await?;
    let models = body["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| ModelInfo {
                    name: m["name"].as_str().unwrap_or("").to_string(),
                    size: format_bytes(m["size"].as_u64().unwrap_or(0)),
                    modified: m["modified_at"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(10)
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// Get the preset configuration for a given preset name.
pub fn get_preset(name: &str) -> PresetConfig {
    match name {
        "fast" => PresetConfig::default_fast(),
        "balanced" => PresetConfig::default_balanced(),
        _ => PresetConfig::default_premium(),
    }
}

/// Benchmark a model by sending a standard prompt and measuring tokens/sec.
pub async fn benchmark_model(model: &str) -> Result<BenchmarkResult> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let prompt = "Write a Python function that checks if a number is prime. Include docstring and type hints.";

    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "options": { "temperature": 0.0, "num_ctx": 4096 }
    });

    let start = Instant::now();
    let resp = client
        .post(format!("{}/api/generate", crate::llm::ollama_url()))
        .json(&body)
        .send()
        .await?;

    let elapsed = start.elapsed();
    let json: serde_json::Value = resp.json().await?;

    let response = json["response"].as_str().unwrap_or("").to_string();
    let eval_count = json["eval_count"].as_u64().unwrap_or(0);
    let eval_duration_ns = json["eval_duration"].as_u64().unwrap_or(1);
    let tokens_per_sec = if eval_duration_ns > 0 {
        (eval_count as f64) / (eval_duration_ns as f64 / 1_000_000_000.0)
    } else {
        0.0
    };

    Ok(BenchmarkResult {
        model: model.to_string(),
        tokens_generated: eval_count as u32,
        total_time_secs: elapsed.as_secs_f64(),
        tokens_per_sec,
        response_lines: response.lines().count() as u32,
    })
}

#[derive(Debug)]
pub struct BenchmarkResult {
    pub model: String,
    pub tokens_generated: u32,
    pub total_time_secs: f64,
    pub tokens_per_sec: f64,
    pub response_lines: u32,
}

impl std::fmt::Display for BenchmarkResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} tokens in {:.1}s ({:.1} tok/s), {} lines",
            self.model,
            self.tokens_generated,
            self.total_time_secs,
            self.tokens_per_sec,
            self.response_lines
        )
    }
}

/// Estimate VRAM usage for a model based on parameter count and quantization.
pub fn estimate_vram_gb(model_name: &str) -> f64 {
    let lower = model_name.to_lowercase();

    // Extract parameter count
    let params_b: f64 = if lower.contains("80b") || lower.contains("70b") {
        75.0
    } else if lower.contains("35b") || lower.contains("32b") || lower.contains("30b") {
        32.0
    } else if lower.contains("27b") || lower.contains("24b") {
        25.0
    } else if lower.contains("14b") || lower.contains("16b") {
        15.0
    } else if lower.contains("7b") || lower.contains("9b") || lower.contains("8b") {
        8.0
    } else if lower.contains("4b") || lower.contains("3b") {
        4.0
    } else {
        7.0
    };

    // Quantization multiplier (bytes per parameter)
    let bytes_per_param: f64 = if lower.contains("bf16") || lower.contains("fp16") {
        2.0
    } else if lower.contains("q8") {
        1.0
    } else if lower.contains("q4") {
        0.5
    } else {
        0.6 // default ~Q5
    };

    // VRAM = params * bytes_per_param + overhead (~2GB for KV cache)
    (params_b * bytes_per_param) + 2.0
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_preset() {
        let p = get_preset("fast");
        assert_eq!(p.coder, "qwen2.5-coder:7b");

        let p = get_preset("premium");
        assert_eq!(p.coder, "qwen3-coder-next:q8_0");
    }

    #[test]
    fn test_estimate_vram() {
        let vram = estimate_vram_gb("qwen2.5-coder:7b");
        assert!(vram > 4.0 && vram < 15.0);

        // "qwen3-coder-next:q8_0" doesn't have "80b" in name, so defaults to 7B estimate
        // This is a known limitation — model name doesn't always encode param count
        let vram = estimate_vram_gb("model-80b-q8_0");
        assert!(vram > 70.0);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(8_900_000_000), "8.9 GB");
        assert_eq!(format_bytes(500_000_000), "500 MB");
    }
}
