//! Edit existing codebases with AI assistance.
//!
//! Reads an existing project, sends the file tree + relevant code to the LLM,
//! and applies changes through the same quality pipeline.

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::llm::LlmClient;

/// Build a file tree summary of an existing project.
pub fn build_file_tree(dir: &Path) -> Result<String> {
    let mut tree = String::new();
    build_tree_recursive(dir, dir, &mut tree, 0)?;
    Ok(tree)
}

fn build_tree_recursive(root: &Path, dir: &Path, tree: &mut String, depth: usize) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden dirs, node_modules, target, __pycache__, .git
        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == "__pycache__"
            || name == ".git"
            || name == "venv"
        {
            continue;
        }

        let indent = "  ".repeat(depth);
        if path.is_dir() {
            tree.push_str(&format!("{}{}/\n", indent, name));
            build_tree_recursive(root, &path, tree, depth + 1)?;
        } else {
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            tree.push_str(&format!("{}{}  ({} bytes)\n", indent, name, size));
        }
    }
    Ok(())
}

/// Read relevant source files from a project (up to a size limit).
pub fn read_project_context(dir: &Path, max_bytes: usize) -> Result<String> {
    let source_exts = [
        "py", "rs", "ts", "tsx", "js", "jsx", "go", "java", "toml", "json", "yaml", "yml",
    ];
    let mut context = String::new();
    let mut total_bytes = 0;

    let files = collect_source_files(dir, &source_exts)?;

    for file in &files {
        if total_bytes >= max_bytes {
            context.push_str(&format!("\n... ({} more files truncated)\n", files.len()));
            break;
        }

        let relative = file.strip_prefix(dir).unwrap_or(file);
        match fs::read_to_string(file) {
            Ok(content) => {
                let truncated = if content.len() > 2000 {
                    format!(
                        "{}...\n(truncated, {} total lines)",
                        &content[..2000],
                        content.lines().count()
                    )
                } else {
                    content.clone()
                };

                context.push_str(&format!(
                    "\n### {}\n```\n{}\n```\n",
                    relative.display(),
                    truncated
                ));
                total_bytes += content.len();
            }
            Err(_) => continue,
        }
    }

    Ok(context)
}

/// Generate an edit plan for an existing codebase.
pub async fn plan_edit(
    llm: &LlmClient,
    project_dir: &Path,
    edit_prompt: &str,
    quality_bible: &str,
) -> Result<EditPlan> {
    let file_tree = build_file_tree(project_dir)?;
    let project_context = read_project_context(project_dir, 50_000)?;

    let system = format!(
        "{}\n\nYou are a Senior Software Engineer editing an existing codebase.\n\
         Analyze the file tree and existing code, then produce an edit plan.\n\n\
         Output a JSON object with:\n\
         - \"files_to_modify\": [{{\"path\": \"...\", \"description\": \"what to change\"}}]\n\
         - \"files_to_create\": [{{\"path\": \"...\", \"description\": \"what it contains\"}}]\n\
         - \"files_to_delete\": [\"path\"]\n\
         - \"summary\": \"1-2 sentence summary of changes\"\n\n\
         Output ONLY valid JSON.",
        quality_bible
    );

    let user_prompt = format!(
        "Edit request: {}\n\nFile tree:\n{}\n\nExisting code:\n{}",
        edit_prompt, file_tree, project_context
    );

    let response = llm.generate("EDIT-PLAN", &system, &user_prompt).await?;

    // Parse the plan
    let json_str = extract_json_object(&response);
    match serde_json::from_str::<EditPlan>(&json_str) {
        Ok(plan) => Ok(plan),
        Err(_) => {
            // Fallback plan
            Ok(EditPlan {
                files_to_modify: vec![],
                files_to_create: vec![],
                files_to_delete: vec![],
                summary: format!("Edit: {}", edit_prompt),
            })
        }
    }
}

/// Apply edits to files using the LLM.
pub async fn apply_edits(
    llm: &LlmClient,
    project_dir: &Path,
    plan: &EditPlan,
    edit_prompt: &str,
    quality_bible: &str,
) -> Result<Vec<PathBuf>> {
    let mut modified_files = Vec::new();

    let system = format!(
        "{}\n\nYou are editing an existing file. Output the COMPLETE updated file content.\n\
         Do not omit any existing code unless the edit requires removing it.\n\
         Output ONLY the file content in a code fence, no explanations.",
        quality_bible
    );

    // Modify existing files
    for file_spec in &plan.files_to_modify {
        let file_path = match crate::sandbox::validate_path_within(project_dir, &file_spec.path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[SECURITY] Skipping modify: {}", e);
                continue;
            }
        };
        if let Ok(existing_content) = fs::read_to_string(&file_path) {
            let prompt = format!(
                "Edit this file according to: {}\n\nChange needed: {}\n\nCurrent content:\n```\n{}\n```",
                edit_prompt, file_spec.description, existing_content
            );

            if let Ok(response) = llm
                .generate(&format!("EDIT {}", file_spec.path), &system, &prompt)
                .await
            {
                let new_content = crate::llm::extract_code(&response, "");
                if !new_content.is_empty() {
                    fs::write(&file_path, &new_content)?;
                    modified_files.push(file_path);
                }
            }
        }
    }

    // Create new files
    for file_spec in &plan.files_to_create {
        let file_path = match crate::sandbox::validate_path_within(project_dir, &file_spec.path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[SECURITY] Skipping create: {}", e);
                continue;
            }
        };
        let prompt = format!(
            "Create this new file for: {}\n\nFile: {}\nPurpose: {}",
            edit_prompt, file_spec.path, file_spec.description
        );

        if let Ok(response) = llm
            .generate(&format!("CREATE {}", file_spec.path), &system, &prompt)
            .await
        {
            let content = crate::llm::extract_code(&response, "");
            if !content.is_empty() {
                if let Some(parent) = file_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&file_path, &content)?;
                modified_files.push(file_path);
            }
        }
    }

    // Delete files
    for path in &plan.files_to_delete {
        let file_path = match crate::sandbox::validate_path_within(project_dir, path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[SECURITY] Skipping delete: {}", e);
                continue;
            }
        };
        if file_path.exists() {
            fs::remove_file(&file_path)?;
        }
    }

    Ok(modified_files)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EditPlan {
    #[serde(default)]
    pub files_to_modify: Vec<FileSpec>,
    #[serde(default)]
    pub files_to_create: Vec<FileSpec>,
    #[serde(default)]
    pub files_to_delete: Vec<String>,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileSpec {
    pub path: String,
    pub description: String,
}

fn extract_json_object(raw: &str) -> String {
    // Try inside code fences
    if let Some(start) = raw.find("```json") {
        let after = &raw[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    // Try raw JSON object
    if let Some(start) = raw.find('{') {
        if let Some(end) = raw.rfind('}') {
            if end > start {
                return raw[start..=end].to_string();
            }
        }
    }

    raw.trim().to_string()
}

fn collect_source_files(dir: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == "__pycache__"
            || name == "venv"
        {
            continue;
        }

        if path.is_dir() {
            files.extend(collect_source_files(&path, extensions)?);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if extensions.contains(&ext) {
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
    fn test_extract_json_object() {
        let raw = "Here's the plan:\n```json\n{\"summary\": \"test\"}\n```";
        assert_eq!(extract_json_object(raw), "{\"summary\": \"test\"}");
    }

    #[test]
    fn test_extract_json_object_raw() {
        let raw = "blah {\"key\": \"val\"} more";
        assert_eq!(extract_json_object(raw), "{\"key\": \"val\"}");
    }

    #[test]
    fn test_build_file_tree() {
        // Should not panic on current directory
        let tree = build_file_tree(Path::new(".")).unwrap();
        assert!(tree.contains("Cargo.toml"));
    }
}
