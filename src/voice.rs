//! Voice TTS announcements using macOS `say` command.
//!
//! Non-blocking — runs in background via tokio::spawn.
//! Enable with --voice flag or BATTLECOMMAND_VOICE=1 env.

use std::process::Command;

/// Check if voice is enabled via env or flag.
pub fn is_enabled() -> bool {
    std::env::var("BATTLECOMMAND_VOICE")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
}

/// Speak a message in the background (non-blocking).
/// Uses macOS `say` command. Silently no-ops on other platforms.
pub fn say(message: &str) {
    if !is_enabled() || !cfg!(target_os = "macos") {
        return;
    }
    let msg = message.to_string();
    tokio::spawn(async move {
        let _ = Command::new("say").args(["-v", "Samantha", &msg]).output();
    });
}

/// Announce mission start.
pub fn mission_start(prompt: &str) {
    let short: String = prompt.chars().take(60).collect();
    say(&format!("Mission starting. {}", short));
}

/// Announce quality gate result.
pub fn quality_gate(score: f32, passed: bool) {
    if passed {
        say(&format!(
            "Quality gate passed with score {:.1}. Shipping production grade code.",
            score
        ));
    } else {
        say(&format!(
            "Quality gate failed. Score {:.1}. Starting fix round.",
            score
        ));
    }
}

/// Announce fix round.
pub fn fix_round(round: usize, max: usize) {
    say(&format!("Fix round {} of {}.", round, max));
}

/// Announce mission complete.
pub fn mission_complete(passed: bool, score: f32) {
    if passed {
        say(&format!(
            "Mission complete. Production grade code shipped. Score {:.1}.",
            score
        ));
    } else {
        say(&format!(
            "Mission complete. Human review required. Best score {:.1}.",
            score
        ));
    }
}

/// Announce decomposition.
pub fn decomposed(count: usize) {
    say(&format!("Mission decomposed into {} subtasks.", count));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_enabled_default() {
        // Should be disabled by default
        assert!(!is_enabled());
    }
}
