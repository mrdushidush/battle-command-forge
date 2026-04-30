//! Enterprise features: audit logging, cost tracking, RBAC.
//!
//! File-based implementation. Can be upgraded to PostgreSQL later.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const AUDIT_LOG: &str = ".battlecommand/audit.jsonl";
const COSTS_LOG: &str = ".battlecommand/costs.jsonl";

// ── Audit Logging ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub actor: String,
    pub action: String,
    pub resource: String,
    pub details: String,
}

/// Log an audit event (append-only, crash-safe).
pub fn audit_log(action: &str, resource: &str, details: &str) -> Result<()> {
    use std::io::Write;

    let entry = AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        actor: whoami(),
        action: action.to_string(),
        resource: resource.to_string(),
        details: details.to_string(),
    };

    let json = serde_json::to_string(&entry)?;

    let path = Path::new(AUDIT_LOG);
    crate::secrets::ensure_secret_file(path)?;
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    writeln!(file, "{}", json)?;
    Ok(())
}

/// Read recent audit entries.
pub fn read_audit_log(limit: usize) -> Result<Vec<AuditEntry>> {
    let content = fs::read_to_string(AUDIT_LOG).unwrap_or_default();
    let entries: Vec<AuditEntry> = content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    Ok(entries.into_iter().rev().take(limit).collect())
}

// ── Cost Tracking ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    pub timestamp: String,
    pub mission_id: String,
    pub model: String,
    pub role: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// Pricing per million tokens (input/output).
fn model_pricing(model: &str) -> (f64, f64) {
    let lower = model.to_lowercase();
    if lower.contains("claude") && lower.contains("opus") {
        (5.0, 25.0) // Opus 4.6: $5/MTok in, $25/MTok out
    } else if lower.contains("claude") && lower.contains("sonnet") {
        (3.0, 15.0) // Sonnet 4.6
    } else if lower.contains("claude") && lower.contains("haiku") {
        (0.25, 1.25) // Haiku 4.5
    } else if lower.contains("grok") {
        (3.0, 15.0) // Grok pricing (xAI)
    } else {
        (0.0, 0.0) // local models are free
    }
}

/// Log a cost entry (append-only, crash-safe).
pub fn log_cost(
    mission_id: &str,
    model: &str,
    role: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Result<()> {
    use std::io::Write;

    let (input_price, output_price) = model_pricing(model);
    let cost =
        (input_tokens as f64 * input_price + output_tokens as f64 * output_price) / 1_000_000.0;

    // Skip logging for free local models
    if cost == 0.0 {
        return Ok(());
    }

    let entry = CostEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        mission_id: mission_id.to_string(),
        model: model.to_string(),
        role: role.to_string(),
        input_tokens,
        output_tokens,
        cost_usd: cost,
    };

    let json = serde_json::to_string(&entry)?;

    let path = Path::new(COSTS_LOG);
    crate::secrets::ensure_secret_file(path)?;
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    writeln!(file, "{}", json)?;
    Ok(())
}

/// Get total cost across all missions.
pub fn total_cost() -> Result<f64> {
    let content = fs::read_to_string(COSTS_LOG).unwrap_or_default();
    let total: f64 = content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<CostEntry>(l).ok())
        .map(|e| e.cost_usd)
        .sum();
    Ok(total)
}

// ── RBAC ──

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    Admin,
    Developer,
    Viewer,
}

impl Role {
    pub fn can_create_mission(&self) -> bool {
        matches!(self, Role::Admin | Role::Developer)
    }

    pub fn can_view_audit(&self) -> bool {
        matches!(self, Role::Admin | Role::Viewer)
    }

    pub fn can_manage_models(&self) -> bool {
        matches!(self, Role::Admin)
    }

    pub fn cost_budget_usd(&self) -> f64 {
        match self {
            Role::Admin => 100.0,
            Role::Developer => 10.0,
            Role::Viewer => 0.0,
        }
    }
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_pricing() {
        let (inp, out) = model_pricing("claude-sonnet-4-20250514");
        assert_eq!(inp, 3.0);
        assert_eq!(out, 15.0);

        let (inp, out) = model_pricing("claude-opus-4-6");
        assert_eq!(inp, 5.0);
        assert_eq!(out, 25.0);

        let (inp, out) = model_pricing("grok-4.20-reasoning");
        assert_eq!(inp, 3.0);
        assert_eq!(out, 15.0);

        let (inp, out) = model_pricing("qwen2.5-coder:32b");
        assert_eq!(inp, 0.0);
        assert_eq!(out, 0.0);
    }

    #[test]
    fn test_rbac() {
        assert!(Role::Admin.can_create_mission());
        assert!(Role::Developer.can_create_mission());
        assert!(!Role::Viewer.can_create_mission());
        assert!(Role::Admin.can_manage_models());
        assert!(!Role::Developer.can_manage_models());
    }

    #[test]
    fn test_total_cost_empty() {
        let cost = total_cost().unwrap();
        assert!(cost >= 0.0);
    }
}
