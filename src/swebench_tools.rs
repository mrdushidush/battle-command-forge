/// SWE-bench ReAct agent tool implementations.
/// 7 tools for exploring repos and fixing bugs: read_file, grep_search,
/// list_directory, run_command, write_file, apply_edit, submit.
use serde_json::Value;
use std::path::Path;

const MAX_READ_LINES: usize = 200;
const MAX_GREP_MATCHES: usize = 30;
const MAX_OUTPUT_CHARS: usize = 4096;
const COMMAND_TIMEOUT_SECS: u64 = 30;

/// Result from executing a tool.
#[derive(Debug)]
pub struct ToolResult {
    pub tool_name: String,
    pub content: String,
    pub success: bool,
    pub is_write: bool,
    pub is_submit: bool,
}

/// Execute a tool by name with given arguments, scoped to workspace.
pub async fn execute(tool_name: &str, args: &Value, workspace: &str) -> ToolResult {
    match tool_name {
        "read_file" => execute_read_file(args, workspace).await,
        "grep_search" => execute_grep_search(args, workspace).await,
        "list_directory" => execute_list_directory(args, workspace).await,
        "run_command" => execute_run_command(args, workspace).await,
        "write_file" => execute_write_file(args, workspace).await,
        "apply_edit" => execute_apply_edit(args, workspace).await,
        "submit" => execute_submit(),
        _ => ToolResult {
            tool_name: tool_name.to_string(),
            content: format!("Unknown tool: {}", tool_name),
            success: false,
            is_write: false,
            is_submit: false,
        },
    }
}

fn resolve_path(workspace: &str, relative: &str) -> Result<String, String> {
    let cleaned = relative.trim_start_matches("./");
    let root = Path::new(workspace);
    let joined = crate::sandbox::validate_path_within(root, cleaned)?;
    Ok(joined.to_string_lossy().into_owned())
}

fn safe_truncate(s: &str, max: usize) -> &str {
    if max >= s.len() {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

async fn execute_read_file(args: &Value, workspace: &str) -> ToolResult {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let start_line = args.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let end_line = args
        .get("end_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    if path.is_empty() {
        return ToolResult {
            tool_name: "read_file".into(),
            content: "Error: path is required".into(),
            success: false,
            is_write: false,
            is_submit: false,
        };
    }

    let full_path = match resolve_path(workspace, path) {
        Ok(p) => p,
        Err(e) => {
            return ToolResult {
                tool_name: "read_file".into(),
                content: format!("Error: {}", e),
                success: false,
                is_write: false,
                is_submit: false,
            }
        }
    };

    match tokio::fs::read_to_string(&full_path).await {
        Ok(contents) => {
            let lines: Vec<&str> = contents.lines().collect();
            let total = lines.len();
            let start = start_line.saturating_sub(1).min(total);
            let end = end_line.unwrap_or(start + MAX_READ_LINES).min(total);

            let mut output = format!(
                "File: {} ({} lines total, showing {}-{})\n\n",
                path,
                total,
                start + 1,
                end
            );
            for (i, line) in lines[start..end].iter().enumerate() {
                output.push_str(&format!("{:>5} | {}\n", start + i + 1, line));
            }
            if end < total {
                output.push_str(&format!(
                    "\n... {} more lines. Use start_line={} to continue.\n",
                    total - end,
                    end + 1
                ));
            }

            ToolResult {
                tool_name: "read_file".into(),
                content: output,
                success: true,
                is_write: false,
                is_submit: false,
            }
        }
        Err(e) => ToolResult {
            tool_name: "read_file".into(),
            content: format!("Error reading {}: {}", path, e),
            success: false,
            is_write: false,
            is_submit: false,
        },
    }
}

async fn execute_grep_search(args: &Value, workspace: &str) -> ToolResult {
    let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    let search_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    if pattern.is_empty() {
        return ToolResult {
            tool_name: "grep_search".into(),
            content: "Error: pattern is required".into(),
            success: false,
            is_write: false,
            is_submit: false,
        };
    }

    let full_path = match resolve_path(workspace, search_path) {
        Ok(p) => p,
        Err(e) => {
            return ToolResult {
                tool_name: "grep_search".into(),
                content: format!("Error: {}", e),
                success: false,
                is_write: false,
                is_submit: false,
            }
        }
    };

    let result = tokio::process::Command::new("grep")
        .args([
            "-rn",
            "--include=*.py",
            "--include=*.pyx",
            "--include=*.pyi",
            "--include=*.cfg",
            "--include=*.toml",
            "--include=*.txt",
            "--include=*.rst",
            "--include=*.md",
            "--include=*.yml",
            "--include=*.yaml",
            "--include=*.json",
            "--exclude-dir=.git",
            "--exclude-dir=__pycache__",
            "--exclude-dir=*.egg-info",
            "--exclude-dir=.tox",
            "--exclude-dir=build",
            pattern,
            &full_path,
        ])
        .output()
        .await;

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = stdout.lines().collect();
            let total_matches = lines.len();
            let prefix = format!("{}/", workspace.trim_end_matches('/'));
            let mut result_text = format!("Found {} matches for '{}'\n\n", total_matches, pattern);
            for line in lines.iter().take(MAX_GREP_MATCHES) {
                let clean = line.strip_prefix(&prefix).unwrap_or(line);
                result_text.push_str(clean);
                result_text.push('\n');
            }
            if total_matches > MAX_GREP_MATCHES {
                result_text.push_str(&format!(
                    "\n... {} more matches (showing first {})\n",
                    total_matches - MAX_GREP_MATCHES,
                    MAX_GREP_MATCHES
                ));
            }
            ToolResult {
                tool_name: "grep_search".into(),
                content: result_text,
                success: true,
                is_write: false,
                is_submit: false,
            }
        }
        Err(e) => ToolResult {
            tool_name: "grep_search".into(),
            content: format!("Error running grep: {}", e),
            success: false,
            is_write: false,
            is_submit: false,
        },
    }
}

async fn execute_list_directory(args: &Value, workspace: &str) -> ToolResult {
    let dir_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let full_path = match resolve_path(workspace, dir_path) {
        Ok(p) => p,
        Err(e) => {
            return ToolResult {
                tool_name: "list_directory".into(),
                content: format!("Error: {}", e),
                success: false,
                is_write: false,
                is_submit: false,
            }
        }
    };

    let path = Path::new(&full_path);
    if !path.is_dir() {
        return ToolResult {
            tool_name: "list_directory".into(),
            content: format!("Not a directory: {}", dir_path),
            success: false,
            is_write: false,
            is_submit: false,
        };
    }

    let mut entries: Vec<String> = Vec::new();
    match tokio::fs::read_dir(&full_path).await {
        Ok(mut dir) => {
            while let Ok(Some(entry)) = dir.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.')
                    || name == "__pycache__"
                    || name == "node_modules"
                    || name.ends_with(".egg-info")
                    || name == ".tox"
                {
                    continue;
                }
                if let Ok(meta) = entry.metadata().await {
                    if meta.is_dir() {
                        entries.push(format!("[dir]  {}/", name));
                    } else {
                        let size = meta.len();
                        let size_str = if size < 1024 {
                            format!("{} B", size)
                        } else if size < 1024 * 1024 {
                            format!("{:.1} KB", size as f64 / 1024.0)
                        } else {
                            format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
                        };
                        entries.push(format!("[file] {} ({})", name, size_str));
                    }
                }
            }
            entries.sort();
            let mut output = format!("Directory: {} ({} entries)\n\n", dir_path, entries.len());
            for e in &entries {
                output.push_str(e);
                output.push('\n');
            }
            ToolResult {
                tool_name: "list_directory".into(),
                content: output,
                success: true,
                is_write: false,
                is_submit: false,
            }
        }
        Err(e) => ToolResult {
            tool_name: "list_directory".into(),
            content: format!("Error reading directory {}: {}", dir_path, e),
            success: false,
            is_write: false,
            is_submit: false,
        },
    }
}

/// Programs the ReAct agent is allowed to invoke. Any new entry is a
/// deliberate security decision — keep this list tight.
const ALLOWED_RUN_COMMAND_HEADS: &[&str] = &[
    "pytest",
    "python",
    "python3",
    "pip",
    "pip3",
    "uv",
    "ruff",
    "black",
    "mypy",
    "tox",
    "pre-commit",
    "ls",
    "cat",
    "head",
    "tail",
    "grep",
    "find",
    "wc",
    "echo",
    "true",
    "false",
    "test",
    "mkdir",
    "rmdir",
    "rm",
    "touch",
    "cp",
    "mv",
    "git",
    "make",
    "cargo",
    "npm",
    "yarn",
    "node",
];

async fn execute_run_command(args: &Value, workspace: &str) -> ToolResult {
    let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if cmd.is_empty() {
        return ToolResult {
            tool_name: "run_command".into(),
            content: "Error: command is required".into(),
            success: false,
            is_write: false,
            is_submit: false,
        };
    }

    // Parse argv with shell quoting rules. We do NOT pass to `sh -c` — running
    // direct via Command::new(argv[0]).args(argv[1..]) means shell substitution
    // (`$(...)`, backticks, `&&`, `|`, redirects) is never interpreted, which
    // closes the substring-blocklist bypass class outright.
    let mut argv: Vec<String> = match shell_words::split(cmd) {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            return ToolResult {
                tool_name: "run_command".into(),
                content: "Error: empty command after parsing".into(),
                success: false,
                is_write: false,
                is_submit: false,
            };
        }
        Err(e) => {
            return ToolResult {
                tool_name: "run_command".into(),
                content: format!(
                    "Error: cannot parse command (use simple argv form, no unbalanced quotes): {}",
                    e
                ),
                success: false,
                is_write: false,
                is_submit: false,
            };
        }
    };

    // Reject shell-composition tokens. They would only have meaning under
    // `sh -c`; here they'd just be passed as literal args and confuse the
    // child. Failing loudly is friendlier than silently misexecuting.
    for token in &argv {
        if matches!(
            token.as_str(),
            "&&" | "||" | ";" | "|" | ">" | ">>" | "<" | "<<" | "&"
        ) {
            return ToolResult {
                tool_name: "run_command".into(),
                content: format!(
                    "Error: shell metacharacter '{}' is not supported. Run as separate commands.",
                    token
                ),
                success: false,
                is_write: false,
                is_submit: false,
            };
        }
        if token.contains('\0') {
            return ToolResult {
                tool_name: "run_command".into(),
                content: "Error: null byte in argument".into(),
                success: false,
                is_write: false,
                is_submit: false,
            };
        }
    }

    // python → python3 only when argv[0] is exactly the bare interpreter.
    // The old substring-replace corrupted strings like `pythonic_test_file`.
    if argv[0] == "python" {
        argv[0] = "python3".to_string();
    }

    let head = argv[0].clone();
    if !ALLOWED_RUN_COMMAND_HEADS.iter().any(|a| *a == head) {
        return ToolResult {
            tool_name: "run_command".into(),
            content: format!(
                "Error: program '{}' is not in the allowlist. Edit ALLOWED_RUN_COMMAND_HEADS in swebench_tools.rs to permit it.",
                head
            ),
            success: false,
            is_write: false,
            is_submit: false,
        };
    }

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(COMMAND_TIMEOUT_SECS),
        tokio::process::Command::new(&head)
            .args(&argv[1..])
            .current_dir(workspace)
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);
            let mut text = format!("Exit code: {}\n", exit_code);
            if !stdout.is_empty() {
                text.push_str(&format!(
                    "\nSTDOUT:\n{}",
                    safe_truncate(&stdout, MAX_OUTPUT_CHARS)
                ));
                if stdout.len() > MAX_OUTPUT_CHARS {
                    text.push_str(&format!("\n... truncated ({} chars total)\n", stdout.len()));
                }
            }
            if !stderr.is_empty() {
                text.push_str(&format!(
                    "\nSTDERR:\n{}",
                    safe_truncate(&stderr, MAX_OUTPUT_CHARS)
                ));
            }
            ToolResult {
                tool_name: "run_command".into(),
                content: text,
                success: exit_code == 0,
                is_write: false,
                is_submit: false,
            }
        }
        Ok(Err(e)) => ToolResult {
            tool_name: "run_command".into(),
            content: format!("Error executing command: {}", e),
            success: false,
            is_write: false,
            is_submit: false,
        },
        Err(_) => ToolResult {
            tool_name: "run_command".into(),
            content: format!("Command timed out after {}s", COMMAND_TIMEOUT_SECS),
            success: false,
            is_write: false,
            is_submit: false,
        },
    }
}

async fn execute_write_file(args: &Value, workspace: &str) -> ToolResult {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return ToolResult {
            tool_name: "write_file".into(),
            content: "Error: path is required".into(),
            success: false,
            is_write: true,
            is_submit: false,
        };
    }
    let full_path = match resolve_path(workspace, path) {
        Ok(p) => p,
        Err(e) => {
            return ToolResult {
                tool_name: "write_file".into(),
                content: format!("Error: {}", e),
                success: false,
                is_write: true,
                is_submit: false,
            }
        }
    };

    if let Some(parent) = Path::new(&full_path).parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return ToolResult {
                tool_name: "write_file".into(),
                content: format!("Error creating directories: {}", e),
                success: false,
                is_write: true,
                is_submit: false,
            };
        }
    }

    match tokio::fs::write(&full_path, content).await {
        Ok(()) => {
            let lines = content.lines().count();
            ToolResult {
                tool_name: "write_file".into(),
                content: format!(
                    "File written: {} ({} lines, {} bytes)",
                    path,
                    lines,
                    content.len()
                ),
                success: true,
                is_write: true,
                is_submit: false,
            }
        }
        Err(e) => ToolResult {
            tool_name: "write_file".into(),
            content: format!("Error writing {}: {}", path, e),
            success: false,
            is_write: true,
            is_submit: false,
        },
    }
}

async fn execute_apply_edit(args: &Value, workspace: &str) -> ToolResult {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let old_text = args.get("old_text").and_then(|v| v.as_str()).unwrap_or("");
    let new_text = args.get("new_text").and_then(|v| v.as_str()).unwrap_or("");

    if path.is_empty() || old_text.is_empty() {
        return ToolResult {
            tool_name: "apply_edit".into(),
            content: "Error: path and old_text are required".into(),
            success: false,
            is_write: true,
            is_submit: false,
        };
    }

    let full_path = match resolve_path(workspace, path) {
        Ok(p) => p,
        Err(e) => {
            return ToolResult {
                tool_name: "apply_edit".into(),
                content: format!("Error: {}", e),
                success: false,
                is_write: true,
                is_submit: false,
            }
        }
    };

    match tokio::fs::read_to_string(&full_path).await {
        Ok(contents) => {
            if let Some(pos) = contents.find(old_text) {
                let new_contents = format!(
                    "{}{}{}",
                    &contents[..pos],
                    new_text,
                    &contents[pos + old_text.len()..]
                );
                let remaining = &new_contents[pos + new_text.len()..];
                let extra = remaining.matches(old_text).count();

                match tokio::fs::write(&full_path, &new_contents).await {
                    Ok(()) => {
                        let mut msg = format!("Edit applied to {}", path);
                        if extra > 0 {
                            msg.push_str(&format!(
                                " (warning: {} more occurrence(s) of old_text remain)",
                                extra
                            ));
                        }
                        ToolResult {
                            tool_name: "apply_edit".into(),
                            content: msg,
                            success: true,
                            is_write: true,
                            is_submit: false,
                        }
                    }
                    Err(e) => ToolResult {
                        tool_name: "apply_edit".into(),
                        content: format!("Error writing {}: {}", path, e),
                        success: false,
                        is_write: true,
                        is_submit: false,
                    },
                }
            } else {
                ToolResult {
                    tool_name: "apply_edit".into(),
                    content: format!("Error: old_text not found in {}.\nold_text (first 200 chars): '{}'\nFile preview (first 500 chars):\n{}", path, safe_truncate(old_text, 200), safe_truncate(&contents, 500)),
                    success: false, is_write: true, is_submit: false,
                }
            }
        }
        Err(e) => ToolResult {
            tool_name: "apply_edit".into(),
            content: format!("Error reading {}: {}", path, e),
            success: false,
            is_write: true,
            is_submit: false,
        },
    }
}

fn execute_submit() -> ToolResult {
    ToolResult {
        tool_name: "submit".into(),
        content: "Submission recorded. Your changes will now be evaluated against the test suite."
            .into(),
        success: true,
        is_write: false,
        is_submit: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(cmd: &str) -> ToolResult {
        let workspace = std::env::temp_dir()
            .join(format!("bcf-sw-{}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&workspace).ok();
        let args = json!({"command": cmd});
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(execute_run_command(&args, &workspace))
    }

    // Regression: the old substring blocklist let `rm  -rf  /` (double
    // space), `$(echo rm) -rf /`, and backticks slip through. The new
    // implementation drops `sh -c` entirely so these literals would just
    // be passed as arguments to programs that aren't allowlisted.
    #[test]
    fn test_run_command_rejects_command_substitution() {
        let r = run("$(echo rm) -rf /");
        assert!(!r.success);
        // `$(echo` is not in the allowlist, so we get an allowlist error,
        // not silent execution.
        assert!(
            r.content.contains("allowlist") || r.content.contains("metacharacter"),
            "unexpected error: {}",
            r.content
        );
    }

    #[test]
    fn test_run_command_rejects_compound_metachars() {
        for cmd in &["echo a && echo b", "echo a ; echo b", "echo a | grep b"] {
            let r = run(cmd);
            assert!(!r.success, "should have rejected: {}", cmd);
            assert!(
                r.content.contains("metacharacter"),
                "expected metachar error for {:?}, got: {}",
                cmd,
                r.content
            );
        }
    }

    #[test]
    fn test_run_command_rejects_unallowlisted_head() {
        let r = run("curl http://example.com");
        assert!(!r.success);
        assert!(r.content.contains("allowlist"), "got: {}", r.content);
    }

    #[test]
    fn test_run_command_python_rewrite_only_for_bare_head() {
        // `python` head → rewritten to python3 (then either runs or
        // fails-to-spawn; we don't care about exit, just that the
        // allowlist accepted it without rejecting the head).
        let r = run("python --version");
        assert!(
            !r.content.contains("not in the allowlist"),
            "python head should pass allowlist after rewrite: {}",
            r.content
        );

        // A token containing the substring `python ` should NOT be
        // rewritten — the old `cmd.replace("python ", ...)` corrupted
        // strings like `pythonic_test_file`. Now: the head is `echo`,
        // and `pythonic_test_file` is just an arg literal.
        let r = run("echo pythonic_test_file");
        assert!(
            !r.content.contains("python3ic"),
            "python rewrite should not corrupt non-head tokens: {}",
            r.content
        );
    }

    #[test]
    fn test_resolve_path_filename_with_dotdot_allowed() {
        // Old resolve_path rejected `file..py` via `contains("..")`.
        // New impl delegates to validate_path_within which uses
        // Component-walk and allows it.
        let r = resolve_path("/tmp/wsroot", "file..py");
        assert!(
            r.is_ok(),
            "filename with double-dots should be allowed, got {:?}",
            r
        );
    }

    #[test]
    fn test_resolve_path_blocks_real_traversal() {
        assert!(resolve_path("/tmp/wsroot", "../etc/passwd").is_err());
        assert!(resolve_path("/tmp/wsroot", "app/../../etc/shadow").is_err());
        assert!(resolve_path("/tmp/wsroot", "/etc/passwd").is_err());
    }
}
