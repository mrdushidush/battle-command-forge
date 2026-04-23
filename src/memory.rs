//! Self-improving memory system.
//!
//! After successful missions: distill learnings + save few-shot examples.
//! Before missions: load relevant context from learnings + examples.

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::llm::LlmClient;

const LEARNINGS_FILE: &str = ".battlecommand/learnings.md";
const EXAMPLES_DIR: &str = ".battlecommand/examples";
const CONTEXT_FILE: &str = ".battlecommand/context.md";
const FAILURES_FILE: &str = ".battlecommand/failure_patterns.md";
const MAX_EXAMPLES: usize = 100;
const MAX_FAILURE_PATTERNS: usize = 30;

/// Load relevant context for a mission prompt.
/// Combines: context.md + matching learnings + relevant few-shot examples.
pub fn load_context(prompt: &str) -> String {
    let mut context = String::new();

    // Always load context.md
    if let Ok(ctx) = fs::read_to_string(CONTEXT_FILE) {
        context.push_str(&ctx);
        context.push_str("\n\n");
    }

    // Search learnings for relevant entries
    if let Ok(learnings) = fs::read_to_string(LEARNINGS_FILE) {
        let relevant = find_relevant_sections(&learnings, prompt, 3);
        if !relevant.is_empty() {
            context.push_str("## Relevant Learnings from Past Missions\n\n");
            context.push_str(&relevant);
            context.push_str("\n\n");
        }
    }

    // Find relevant few-shot examples
    let examples = find_relevant_examples(prompt, 2);
    if !examples.is_empty() {
        context.push_str("## Reference Examples (follow this style)\n\n");
        for (name, content) in &examples {
            context.push_str(&format!("### Example: {}\n```\n{}\n```\n\n", name, content));
        }
    }

    context
}

/// Save a learning after a successful mission.
pub async fn distill_and_save(
    llm: &LlmClient,
    prompt: &str,
    code_summary: &str,
    score: f32,
) -> Result<()> {
    // Distill the mission into a learning
    let system = "You are a knowledge distiller. Extract the key patterns, decisions, and \
                  pitfalls from this successful mission into 2-3 bullet points. \
                  Focus on reusable patterns, not specifics. Output ONLY bullet points.";
    let user_prompt = format!(
        "Mission: {}\nScore: {:.1}/10\nCode summary:\n{}",
        prompt, score, code_summary
    );

    let learning = llm
        .generate("DISTILL", system, &user_prompt)
        .await
        .unwrap_or_else(|_| format!("- Completed: {}", prompt));

    // Append to learnings file
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M");
    let entry = format!(
        "\n## [{}] {}\nScore: {:.1}/10\n{}\n",
        timestamp,
        prompt.chars().take(80).collect::<String>(),
        score,
        learning.trim()
    );

    let mut existing = fs::read_to_string(LEARNINGS_FILE).unwrap_or_default();
    existing.push_str(&entry);
    fs::write(LEARNINGS_FILE, existing)?;

    Ok(())
}

/// Save successful output files as a few-shot example.
pub fn save_example(prompt: &str, output_dir: &Path, language: &str) -> Result<()> {
    let examples_dir = Path::new(EXAMPLES_DIR);
    fs::create_dir_all(examples_dir)?;

    // Create example directory name
    let slug: String = prompt
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect::<String>()
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join("_");

    let example_dir = examples_dir.join(format!("{}_{}", language, slug));
    fs::create_dir_all(&example_dir)?;

    // Copy key source files (not __init__.py, not boilerplate)
    for entry in walkdir_source_files(output_dir, language)? {
        let relative = entry.strip_prefix(output_dir).unwrap_or(&entry);
        let dest = example_dir.join(relative);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let _ = fs::copy(&entry, &dest);
    }

    // Enforce max examples limit
    enforce_example_limit(examples_dir)?;

    Ok(())
}

/// Find sections in learnings.md that match the prompt keywords.
fn find_relevant_sections(learnings: &str, prompt: &str, max: usize) -> String {
    let lowered = prompt.to_lowercase();
    let keywords: Vec<&str> = lowered.split_whitespace().filter(|w| w.len() > 3).collect();

    let mut sections: Vec<(usize, &str)> = Vec::new();
    let mut current_section = String::new();
    let mut section_start = 0;

    for (i, line) in learnings.lines().enumerate() {
        if line.starts_with("## ") {
            if !current_section.is_empty() {
                let score = keywords
                    .iter()
                    .filter(|k| current_section.to_lowercase().contains(*k))
                    .count();
                if score > 0 {
                    let start = section_start;
                    let end = i;
                    let section_text = learnings
                        .lines()
                        .skip(start)
                        .take(end - start)
                        .collect::<Vec<_>>()
                        .join("\n");
                    sections.push((score, Box::leak(section_text.into_boxed_str())));
                }
            }
            current_section = line.to_string();
            section_start = i;
        } else {
            current_section.push('\n');
            current_section.push_str(line);
        }
    }

    sections.sort_by(|a, b| b.0.cmp(&a.0));
    sections
        .into_iter()
        .take(max)
        .map(|(_, s)| s)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Find relevant few-shot examples by matching directory names to prompt keywords.
fn find_relevant_examples(prompt: &str, max: usize) -> Vec<(String, String)> {
    let examples_dir = Path::new(EXAMPLES_DIR);
    if !examples_dir.exists() {
        return vec![];
    }

    let keywords: Vec<String> = prompt
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .map(String::from)
        .collect();

    let mut matches: Vec<(usize, String, String)> = Vec::new();

    if let Ok(entries) = fs::read_dir(examples_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let score = keywords
                .iter()
                .filter(|k| name.contains(k.as_str()))
                .count();

            if score > 0 {
                // Read the main source file from the example
                let content = read_example_summary(&entry.path());
                if !content.is_empty() {
                    matches.push((score, name, content));
                }
            }
        }
    }

    matches.sort_by(|a, b| b.0.cmp(&a.0));
    matches
        .into_iter()
        .take(max)
        .map(|(_, name, content)| (name, content))
        .collect()
}

/// Read a summary of an example (first source file, truncated).
fn read_example_summary(dir: &Path) -> String {
    let extensions = ["py", "ts", "js", "rs", "go"];
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if extensions.contains(&ext) {
                        if let Ok(content) = fs::read_to_string(&path) {
                            // Truncate to first 50 lines
                            return content.lines().take(50).collect::<Vec<_>>().join("\n");
                        }
                    }
                }
            }
        }
    }
    String::new()
}

/// Walk directory for source files (skip __init__.py, boilerplate).
fn walkdir_source_files(dir: &Path, language: &str) -> Result<Vec<PathBuf>> {
    let source_exts: Vec<&str> = match language {
        "python" => vec!["py"],
        "typescript" => vec!["ts", "tsx"],
        "javascript" => vec!["js", "jsx"],
        "rust" => vec!["rs"],
        "go" => vec!["go"],
        _ => vec!["py"],
    };

    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(walkdir_source_files(&path, language)?);
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if source_exts.contains(&ext) {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if name != "__init__.py" {
                        files.push(path);
                    }
                }
            }
        }
    }
    Ok(files)
}

/// Remove oldest examples if we exceed the limit.
fn enforce_example_limit(examples_dir: &Path) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(examples_dir)?
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();

    if entries.len() <= MAX_EXAMPLES {
        return Ok(());
    }

    // Sort by modification time (oldest first)
    entries.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });

    // Remove oldest
    let to_remove = entries.len() - MAX_EXAMPLES;
    for entry in entries.into_iter().take(to_remove) {
        let _ = fs::remove_dir_all(entry.path());
    }

    Ok(())
}

/// Save failure patterns from a failed mission for future runs.
/// Extracts error categories from verifier output and stores them.
pub fn save_failure_patterns(language: &str, errors: &[String], score: f32) {
    if errors.is_empty() {
        return;
    }

    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M");
    let mut entry = format!("\n## [{}] {} (score: {:.1})\n", timestamp, language, score);
    for err in errors.iter().take(10) {
        // Normalize errors into reusable patterns
        let pattern = normalize_error_pattern(err);
        if !pattern.is_empty() {
            entry.push_str(&format!("- {}\n", pattern));
        }
    }

    let mut existing = fs::read_to_string(FAILURES_FILE).unwrap_or_default();
    existing.push_str(&entry);

    // Trim to max patterns (keep most recent)
    let sections: Vec<&str> = existing.split("\n## ").collect();
    if sections.len() > MAX_FAILURE_PATTERNS {
        let kept: String = sections
            .iter()
            .skip(sections.len() - MAX_FAILURE_PATTERNS)
            .map(|s| format!("\n## {}", s))
            .collect();
        let _ = fs::write(FAILURES_FILE, kept.trim_start());
    } else {
        let _ = fs::write(FAILURES_FILE, &existing);
    }
}

/// Load failure patterns relevant to a language.
pub fn load_failure_patterns(language: &str) -> String {
    let content = match fs::read_to_string(FAILURES_FILE) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let mut patterns: Vec<String> = Vec::new();
    let mut in_matching_section = false;

    for line in content.lines() {
        if line.starts_with("## ") {
            in_matching_section = line.to_lowercase().contains(language);
        } else if in_matching_section && line.starts_with("- ") {
            let pattern = line[2..].trim().to_string();
            if !patterns.contains(&pattern) {
                patterns.push(pattern);
            }
        }
    }

    if patterns.is_empty() {
        return String::new();
    }

    // Deduplicate and limit
    patterns.truncate(15);
    let mut result =
        String::from("## Patterns from previous failed runs (DO NOT repeat these mistakes):\n");
    for p in &patterns {
        result.push_str(&format!("- {}\n", p));
    }
    result
}

/// Normalize an error message into a reusable pattern.
fn normalize_error_pattern(error: &str) -> String {
    let lower = error.to_lowercase();

    // Python-specific normalizations
    if lower.contains("modulenotfounderror") || lower.contains("importerror") {
        if lower.contains("pydantic") && lower.contains("basesettings") {
            return "Use pydantic_settings.BaseSettings, not pydantic.BaseSettings (Pydantic v2)"
                .to_string();
        }
        if lower.contains("no module named") {
            return format!(
                "Missing import: {}",
                error
                    .split("named")
                    .last()
                    .unwrap_or("")
                    .trim()
                    .trim_matches('\'')
            );
        }
    }
    if lower.contains("nameerror") {
        return format!(
            "Undefined name: {}",
            error
                .split("name")
                .last()
                .unwrap_or("")
                .trim()
                .trim_matches('\'')
        );
    }
    if lower.contains("attributeerror") {
        return format!("Wrong attribute/method: {}", error.trim());
    }
    if lower.contains("syntax error") || lower.contains("syntaxerror") {
        return "Python syntax error in generated code".to_string();
    }
    if lower.contains("hardcoded secret") || lower.contains("hardcoded") {
        return "Hardcoded secrets — use environment variables".to_string();
    }

    // Generic: keep short errors as-is
    if error.len() < 100 {
        error.trim().to_string()
    } else {
        error.trim().chars().take(100).collect::<String>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_context_missing_files() {
        // Should not panic even if files don't exist
        let ctx = load_context("build something");
        // May or may not have content depending on if .battlecommand exists
        let _ = ctx; // just verify it doesn't panic
    }

    #[test]
    fn test_find_relevant_examples_empty() {
        let examples = find_relevant_examples("nonexistent prompt xyz", 3);
        // May be empty if no examples dir exists
        assert!(examples.len() <= 3);
    }
}
