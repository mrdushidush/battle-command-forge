//! File-based mission database.
//!
//! Stores mission history as JSON files in .battlecommand/missions/.
//! Can be upgraded to PostgreSQL later (Phase 12 evolution).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MISSIONS_DIR: &str = ".battlecommand/missions";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionRecord {
    pub id: String,
    pub prompt: String,
    pub preset: String,
    pub tier: String,
    pub subtasks: u32,
    pub rounds: u32,
    pub final_score: f32,
    pub passed: bool,
    pub model: String,
    pub files_generated: Vec<String>,
    pub duration_secs: f64,
    pub timestamp: String,
}

/// Save a mission record.
pub fn save_mission(record: &MissionRecord) -> Result<()> {
    let dir = Path::new(MISSIONS_DIR);
    fs::create_dir_all(dir)?;

    let path = dir.join(format!("{}.json", record.id));
    let json = serde_json::to_string_pretty(record)?;
    fs::write(path, json)?;
    Ok(())
}

/// Load a mission record by ID.
pub fn load_mission(id: &str) -> Result<MissionRecord> {
    let path = PathBuf::from(MISSIONS_DIR).join(format!("{}.json", id));
    let json = fs::read_to_string(path)?;
    let record: MissionRecord = serde_json::from_str(&json)?;
    Ok(record)
}

/// List all mission records, sorted by timestamp (newest first).
pub fn list_missions() -> Result<Vec<MissionRecord>> {
    let dir = Path::new(MISSIONS_DIR);
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut missions = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .map(|e| e == "json")
            .unwrap_or(false)
        {
            if let Ok(json) = fs::read_to_string(entry.path()) {
                if let Ok(record) = serde_json::from_str::<MissionRecord>(&json) {
                    missions.push(record);
                }
            }
        }
    }

    missions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(missions)
}

/// Get mission statistics.
pub fn get_stats() -> Result<MissionStats> {
    let missions = list_missions()?;
    let total = missions.len() as u32;
    let passed = missions.iter().filter(|m| m.passed).count() as u32;
    let avg_score = if missions.is_empty() {
        0.0
    } else {
        missions.iter().map(|m| m.final_score).sum::<f32>() / missions.len() as f32
    };
    let total_duration: f64 = missions.iter().map(|m| m.duration_secs).sum();

    Ok(MissionStats {
        total_missions: total,
        passed,
        failed: total - passed,
        avg_score,
        total_duration_secs: total_duration,
    })
}

#[derive(Debug)]
pub struct MissionStats {
    pub total_missions: u32,
    pub passed: u32,
    pub failed: u32,
    pub avg_score: f32,
    pub total_duration_secs: f64,
}

impl std::fmt::Display for MissionStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Missions: {} total ({} passed, {} failed) | Avg score: {:.1} | Total time: {:.0}s",
            self.total_missions,
            self.passed,
            self.failed,
            self.avg_score,
            self.total_duration_secs.abs()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_missions_empty() {
        let missions = list_missions().unwrap();
        // May or may not have missions depending on prior runs
        let _ = missions; // just verify no panic
    }

    #[test]
    fn test_get_stats() {
        let stats = get_stats().unwrap();
        assert!(stats.avg_score >= 0.0);
    }
}
