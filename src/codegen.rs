//! Multi-file code extraction from LLM responses.
//!
//! LLMs often produce output containing multiple files, marked with path headers
//! like `# filepath: app/config.py` or `### app/models/user.py` before code fences.
//! This module parses those into individual files for writing to disk.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// A single generated file extracted from LLM output.
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: String,
    pub language: String,
}

/// Extract multiple files from a raw LLM response.
///
/// Recognizes these patterns before/inside code fences:
/// - `# filepath: app/config.py`
/// - `### app/models/user.py`
/// - `**app/config.py**`
/// - `<!-- file: src/index.ts -->`
/// - First line inside fence: `# app/config.py` (comment-style path)
pub fn extract_files(raw: &str, default_language: &str) -> Vec<GeneratedFile> {
    let mut files = Vec::new();
    let mut pending_path: Option<String> = None;
    let mut in_fence = false;
    let mut fence_lang = String::new();
    let mut fence_content = String::new();

    for line in raw.lines() {
        let trimmed = line.trim();

        // Detect code fence start
        if !in_fence && trimmed.starts_with("```") {
            in_fence = true;
            fence_lang = trimmed.trim_start_matches('`').trim().to_string();
            fence_content.clear();
            continue;
        }

        // Detect code fence end
        if in_fence && trimmed == "```" {
            in_fence = false;
            let content = fence_content.trim().to_string();
            if content.is_empty() {
                pending_path = None;
                continue;
            }

            // Determine the file path
            let file_path = if let Some(ref p) = pending_path {
                p.clone()
            } else {
                // Check if the first line of content is a filepath comment
                extract_path_from_first_line(&content).unwrap_or_default()
            };

            if !file_path.is_empty() {
                // Sanitize path: strip leading #, *, spaces, backticks
                let file_path = file_path
                    .trim_start_matches(['#', '*', ' ', '`'])
                    .to_string();

                if !file_path.is_empty() && looks_like_path(&file_path) {
                    let lang = if fence_lang.is_empty() {
                        detect_lang_from_path(&file_path)
                            .unwrap_or_else(|| default_language.to_string())
                    } else {
                        fence_lang.clone()
                    };

                    // Strip the filepath comment from content if it was the first line
                    let clean_content = strip_filepath_comment(&content, &file_path);

                    // Strip markdown code fences from non-code content (e.g., ```toml wrapper in .toml files)
                    let clean_content = strip_inner_code_fences(&clean_content, &file_path);

                    files.push(GeneratedFile {
                        path: PathBuf::from(&file_path),
                        content: clean_content,
                        language: lang,
                    });
                }
            }

            pending_path = None;
            continue;
        }

        // Inside a fence — accumulate content
        if in_fence {
            fence_content.push_str(line);
            fence_content.push('\n');
            continue;
        }

        // Outside fence — look for file path indicators
        if let Some(path) = extract_path_from_header(trimmed) {
            pending_path = Some(path);
        }
    }

    files
}

/// Try to extract a file path from a header/comment line outside a code fence.
fn extract_path_from_header(line: &str) -> Option<String> {
    let trimmed = line.trim();

    // `# filepath: app/config.py` or `// filepath: src/main.rs`
    for prefix in &["# filepath:", "// filepath:", "filepath:"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let path = rest.trim().trim_matches('`');
            if looks_like_path(path) {
                return Some(path.to_string());
            }
        }
    }

    // `<!-- file: src/index.ts -->`
    if trimmed.starts_with("<!--") && trimmed.contains("file:") {
        if let Some(rest) = trimmed.split("file:").nth(1) {
            let path = rest.trim().trim_end_matches("-->").trim().trim_matches('`');
            if looks_like_path(path) {
                return Some(path.to_string());
            }
        }
    }

    // `### app/models/user.py` or `## app/config.py`
    if trimmed.starts_with('#') {
        let after_hashes = trimmed.trim_start_matches('#').trim();
        // Must contain a dot (extension) and slash (directory) to be a path
        if looks_like_path(after_hashes) {
            let path = after_hashes.trim_matches('`').trim_matches('*');
            return Some(path.to_string());
        }
    }

    // `**app/config.py**`
    if trimmed.starts_with("**") && trimmed.ends_with("**") {
        let inner = &trimmed[2..trimmed.len() - 2];
        if looks_like_path(inner) {
            return Some(inner.to_string());
        }
    }

    // `File: app/main.py` or `File: `app/main.py``
    if let Some(rest) = trimmed.strip_prefix("File:") {
        let path = rest.trim().trim_matches('`');
        if looks_like_path(path) {
            return Some(path.to_string());
        }
    }

    None
}

/// Try to extract a file path from the first line inside a code fence.
/// E.g., `# app/config.py` as the first line of a Python code block.
fn extract_path_from_first_line(content: &str) -> Option<String> {
    let first_line = content.lines().next()?.trim();

    // `# app/config.py`
    if let Some(rest) = first_line.strip_prefix('#') {
        let path = rest.trim();
        if looks_like_path(path) {
            return Some(path.to_string());
        }
    }

    // `// src/index.ts`
    if let Some(rest) = first_line.strip_prefix("//") {
        let path = rest.trim();
        if looks_like_path(path) {
            return Some(path.to_string());
        }
    }

    None
}

/// Strip the filepath comment from the first line of content, if present.
fn strip_filepath_comment(content: &str, path: &str) -> String {
    let mut lines = content.lines();
    if let Some(first_line) = lines.next() {
        let trimmed = first_line.trim();
        // Check if the first line is just a comment with the file path
        if trimmed.contains(path) && (trimmed.starts_with('#') || trimmed.starts_with("//")) {
            return lines.collect::<Vec<_>>().join("\n").trim().to_string();
        }
    }
    content.to_string()
}

/// Strip nested code fences from file content.
/// When the LLM wraps a non-code file (e.g., pyproject.toml) in ```toml...```,
/// the outer fence is stripped by extract_files but the content still starts
/// with ```toml and ends with ```. This strips those inner fences.
fn strip_inner_code_fences(content: &str, path: &str) -> String {
    let trimmed = content.trim();
    // Only strip if the content starts with a code fence that matches the file type
    if !trimmed.starts_with("```") {
        return content.to_string();
    }
    // Don't strip from actual code files (Python, Rust, etc.) — only config/data files
    let config_extensions = [
        ".toml", ".yaml", ".yml", ".json", ".ini", ".cfg", ".env", ".md", ".txt", ".html", ".css",
        ".sql", ".sh",
    ];
    if !config_extensions.iter().any(|ext| path.ends_with(ext)) {
        return content.to_string();
    }
    // Strip opening ```lang and closing ```
    let first_newline = trimmed.find('\n').unwrap_or(0);
    let after_fence = &trimmed[first_newline + 1..];
    let stripped = if after_fence.trim_end().ends_with("```") {
        let end = after_fence.rfind("```").unwrap_or(after_fence.len());
        &after_fence[..end]
    } else {
        after_fence
    };
    stripped.trim().to_string()
}

/// Check if a string looks like a file path (has extension and no weird chars).
fn looks_like_path(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > 200 {
        return false;
    }
    // Reject placeholder paths that LLMs copy from instructions
    if s == "path/to/file.py" || s.starts_with("path/to/") || s == "file.py" {
        return false;
    }
    // Must have a dot (file extension)
    if !s.contains('.') {
        return false;
    }
    // Must not contain path traversal
    if s.contains("..") {
        return false;
    }
    // Must not start with /
    if s.starts_with('/') {
        return false;
    }
    // Should contain common file extensions
    let extensions = [
        ".py", ".rs", ".ts", ".tsx", ".js", ".jsx", ".go", ".java", ".toml", ".yaml", ".yml",
        ".json", ".md", ".txt", ".html", ".css", ".sql", ".sh", ".cfg", ".ini", ".env",
    ];
    extensions.iter().any(|ext| s.ends_with(ext))
}

/// Detect language from file extension.
fn detect_lang_from_path(path: &str) -> Option<String> {
    if path.ends_with(".py") {
        Some("python".into())
    } else if path.ends_with(".ts") || path.ends_with(".tsx") {
        Some("typescript".into())
    } else if path.ends_with(".js") || path.ends_with(".jsx") {
        Some("javascript".into())
    } else if path.ends_with(".rs") {
        Some("rust".into())
    } else if path.ends_with(".go") {
        Some("go".into())
    } else {
        None
    }
}

/// Write a list of generated files to an output directory.
/// Creates parent directories as needed. Returns paths of all written files.
pub fn write_files(output_dir: &Path, files: &[GeneratedFile]) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for file in files {
        let path_str = file.path.display().to_string();
        let full_path = match crate::sandbox::validate_path_within(output_dir, &path_str) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[SECURITY] Skipping file write: {}", e);
                continue;
            }
        };
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create dir for {}", file.path.display()))?;
        }
        fs::write(&full_path, &file.content)
            .with_context(|| format!("Failed to write {}", file.path.display()))?;
        written.push(full_path);
    }
    Ok(written)
}

/// Write boilerplate files (README, Dockerfile, requirements.txt, __init__.py, etc.)
pub fn write_boilerplate(output_dir: &Path, language: &str, prompt: &str) -> Result<()> {
    if language == "python" {
        // __init__.py files for any directories that contain .py files
        create_init_files(output_dir)?;

        if !output_dir.join("requirements.txt").exists() {
            fs::write(
                output_dir.join("requirements.txt"),
                "fastapi\nuvicorn[standard]\nPyJWT\ncryptography\npydantic[email]\npython-multipart\nslowapi\npytest\nhttpx\npasslib[bcrypt]\npython-jose[cryptography]\nsqlalchemy\n",
            )?;
        }
        if !output_dir.join("Dockerfile").exists() {
            fs::write(
                output_dir.join("Dockerfile"),
                "FROM python:3.12-slim\nWORKDIR /app\nCOPY requirements.txt .\nRUN pip install --no-cache-dir -r requirements.txt\nCOPY . .\nEXPOSE 8000\nCMD [\"uvicorn\", \"app.main:app\", \"--host\", \"0.0.0.0\"]\n",
            )?;
        }
    }

    // README
    if !output_dir.join("README.md").exists() {
        fs::write(
            output_dir.join("README.md"),
            format!(
                "# Generated by BattleCommand Forge v1.1\n\n**Prompt:** {}\n\n**Quality gate:** >= 9.2/10\n",
                prompt
            ),
        )?;
    }

    Ok(())
}

/// Create __init__.py in every directory that contains .py files.
fn create_init_files(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let has_py = fs::read_dir(dir)?.any(|e| {
        e.ok()
            .map(|e| e.path().extension().map(|ext| ext == "py").unwrap_or(false))
            .unwrap_or(false)
    });
    if has_py {
        let init = dir.join("__init__.py");
        if !init.exists() {
            fs::write(&init, "")?;
        }
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            create_init_files(&entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_files_with_filepath_headers() {
        let raw = "\
Here are the files:

### app/main.py

```python
from fastapi import FastAPI

app = FastAPI()
```

### app/config.py

```python
import os

SECRET = os.getenv(\"SECRET\")
```
";
        let files = extract_files(raw, "python");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, PathBuf::from("app/main.py"));
        assert!(files[0].content.contains("FastAPI"));
        assert_eq!(files[1].path, PathBuf::from("app/config.py"));
        assert!(files[1].content.contains("SECRET"));
    }

    #[test]
    fn test_extract_files_with_inline_path() {
        let raw = "\
```python
# app/models.py
from sqlalchemy import Column

class User:
    pass
```
";
        let files = extract_files(raw, "python");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("app/models.py"));
        // The filepath comment should be stripped
        assert!(!files[0].content.starts_with("# app/models.py"));
    }

    #[test]
    fn test_extract_files_bold_header() {
        let raw = "\
**app/utils.py**

```python
def helper():
    return 42
```
";
        let files = extract_files(raw, "python");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("app/utils.py"));
    }

    #[test]
    fn test_looks_like_path() {
        assert!(looks_like_path("app/main.py"));
        assert!(looks_like_path("src/index.ts"));
        assert!(!looks_like_path("just some text"));
        assert!(!looks_like_path("../etc/passwd"));
        assert!(!looks_like_path("/root/file.py"));
    }

    #[test]
    fn test_fallback_single_fence_no_path() {
        let raw = "```python\nprint('hello')\n```";
        let files = extract_files(raw, "python");
        // No path detected — should produce 0 files (caller uses fallback)
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_file_prefix_header() {
        let raw = "File: `app/routes.py`\n\n```python\n@app.get('/')\ndef root():\n    pass\n```";
        let files = extract_files(raw, "python");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("app/routes.py"));
    }
}
