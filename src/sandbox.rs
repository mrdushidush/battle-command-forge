//! Sandboxed command execution with timeouts and environment isolation.

use std::io::Read as IoRead;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Patterns for env vars that must be stripped from subprocesses.
const SENSITIVE_ENV_PATTERNS: &[&str] = &[
    "API_KEY",
    "SECRET",
    "TOKEN",
    "PRIVATE_KEY",
    "PASSWORD",
    "CREDENTIAL",
];

/// Result of running an external tool.
#[derive(Debug)]
pub struct ToolResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Run a command in a sandboxed environment.
/// - Working directory set to `cwd`
/// - Timeout enforced (default 30s)
/// - Sensitive env vars stripped (any *API_KEY, *SECRET, *TOKEN, etc.)
pub fn run_tool(program: &str, args: &[&str], cwd: &Path) -> ToolResult {
    run_tool_with_timeout(program, args, cwd, DEFAULT_TIMEOUT_SECS)
}

pub fn run_tool_with_timeout(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout_secs: u64,
) -> ToolResult {
    // Check if tool exists
    if !tool_exists(program) {
        return ToolResult {
            success: false,
            stdout: String::new(),
            stderr: format!("{} not found in PATH", program),
            timed_out: false,
        };
    }

    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Strip all env vars matching sensitive patterns
    for (key, _) in std::env::vars() {
        let upper = key.to_uppercase();
        if SENSITIVE_ENV_PATTERNS.iter().any(|p| upper.contains(p)) {
            cmd.env_remove(&key);
        }
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                stdout: String::new(),
                stderr: format!("Failed to spawn {}: {}", program, e),
                timed_out: false,
            }
        }
    };

    // Poll with timeout enforcement
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_string(&mut stderr);
                }
                return ToolResult {
                    success: status.success(),
                    stdout,
                    stderr,
                    timed_out: false,
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ToolResult {
                        success: false,
                        stdout: String::new(),
                        stderr: format!("Killed: timed out after {}s", timeout_secs),
                        timed_out: true,
                    };
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                return ToolResult {
                    success: false,
                    stdout: String::new(),
                    stderr: format!("Failed to wait for {}: {}", program, e),
                    timed_out: false,
                }
            }
        }
    }
}

/// Run a tool with network access denied (macOS sandbox-exec).
/// Falls back to normal execution on non-macOS or if sandbox-exec unavailable.
pub fn run_tool_sandboxed(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout_secs: u64,
    deny_network: bool,
) -> ToolResult {
    if !deny_network || !cfg!(target_os = "macos") || !tool_exists("sandbox-exec") {
        return run_tool_with_timeout(program, args, cwd, timeout_secs);
    }
    let profile = "(version 1)\n(allow default)\n(deny network*)";
    let mut sbox_args = vec!["-p", profile, program];
    sbox_args.extend_from_slice(args);
    run_tool_with_timeout("sandbox-exec", &sbox_args, cwd, timeout_secs)
}

/// Validate that a relative path stays within root directory.
/// Rejects path traversal (../), absolute paths, and null bytes.
pub fn validate_path_within(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    let relative = relative.trim();
    if relative.contains("..")
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || relative.contains('\0')
    {
        return Err(format!("Unsafe path rejected: {}", relative));
    }
    let joined = root.join(relative);
    let abs_root = std::path::absolute(root).unwrap_or(root.to_path_buf());
    let abs_joined = std::path::absolute(&joined).unwrap_or(joined.clone());
    if !abs_joined.starts_with(&abs_root) {
        return Err(format!("Path escapes root directory: {}", relative));
    }
    Ok(joined)
}

/// Check if a tool is available (in PATH or as absolute path).
/// Cross-platform: uses `which` on Unix, `where` on Windows.
pub fn tool_exists(program: &str) -> bool {
    let path = std::path::Path::new(program);
    // Absolute or relative path with separators — check if file exists
    if path.is_absolute() || program.contains(std::path::MAIN_SEPARATOR) {
        return path.exists();
    }
    // PATH lookup: `which` on Unix, `where` on Windows
    let lookup = if cfg!(windows) { "where" } else { "which" };
    Command::new(lookup)
        .arg(program)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Parsed lint issue from an external tool.
#[derive(Debug, Clone)]
pub struct LintIssue {
    pub file: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub severity: String,
    pub message: String,
    pub rule: String,
}

impl std::fmt::Display for LintIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(line) = self.line {
            write!(
                f,
                "{}:{}: [{}] {}",
                self.file, line, self.rule, self.message
            )
        } else {
            write!(f, "{}: [{}] {}", self.file, self.rule, self.message)
        }
    }
}

/// Parse ruff output into structured lint issues.
pub fn parse_ruff_output(output: &str) -> Vec<LintIssue> {
    let mut issues = Vec::new();
    for line in output.lines() {
        // Format: file.py:10:5: E501 Line too long
        let parts: Vec<&str> = line.splitn(4, ':').collect();
        if parts.len() >= 4 {
            let file = parts[0].trim().to_string();
            let line_num = parts[1].trim().parse().ok();
            let col = parts[2].trim().parse().ok();
            let rest = parts[3].trim();
            let (rule, msg) = rest.split_once(' ').unwrap_or(("", rest));
            issues.push(LintIssue {
                file,
                line: line_num,
                column: col,
                severity: "warning".into(),
                message: msg.to_string(),
                rule: rule.to_string(),
            });
        }
    }
    issues
}

/// Parse pytest output to extract pass/fail counts.
#[derive(Debug)]
pub struct TestResult {
    pub passed: u32,
    pub failed: u32,
    pub errors: u32,
    pub output: String,
}

pub fn parse_pytest_output(stdout: &str, stderr: &str) -> TestResult {
    let combined = format!("{}\n{}", stdout, stderr);
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut errors = 0u32;

    for line in combined.lines() {
        let lower = line.to_lowercase();
        let trimmed_lower = lower.trim();

        // Only parse pytest summary lines — not arbitrary error messages.
        // Pytest summary format: "5 passed, 2 failed, 1 error in 3.2s"
        // or "===== 5 passed =====" or "5 passed in 0.5s"
        // Key: must contain "passed" or end with "failed" context, AND look like a summary.

        // Method 1: "N word" pairs — safe, precise extraction
        // Matches: "5 passed", "2 failed", "1 error" as adjacent words
        let words: Vec<&str> = line.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(n) = pair[0]
                .trim_matches(|c: char| c == ',' || c == '=')
                .parse::<u32>()
            {
                let what = pair[1].to_lowercase();
                // Only match pytest keywords, with reasonable bounds (< 10000 tests)
                if n < 10000 {
                    if what.starts_with("passed") && passed == 0 {
                        passed = n;
                    } else if what.starts_with("failed") && failed == 0 {
                        failed = n;
                    } else if what.starts_with("error") && errors == 0 {
                        errors = n;
                    }
                }
            }
        }

        // Method 2: count individual FAILED/ERROR lines from pytest -q short output
        // Format: "FAILED tests/test_foo.py::test_bar - AssertionError..."
        // Format: "ERROR tests/test_foo.py::test_bar - ..."
        if trimmed_lower.starts_with("failed ") && trimmed_lower.contains("::") {
            failed += 1;
        }
        if trimmed_lower.starts_with("error ") && trimmed_lower.contains("::") {
            errors += 1;
        }

        // Method 3: pytest -q final line: "3 passed" or "3 passed."
        if trimmed_lower.ends_with("passed") || trimmed_lower.ends_with("passed.") {
            if let Some(n_str) = trimmed_lower.split_whitespace().next() {
                if let Ok(n) = n_str.parse::<u32>() {
                    if passed == 0 && n < 10000 {
                        passed = n;
                    }
                }
            }
        }
    }

    TestResult {
        passed,
        failed,
        errors,
        output: combined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ruff_output() {
        let output =
            "app/main.py:10:5: E501 Line too long (120 > 88)\napp/main.py:25:1: F401 Unused import";
        let issues = parse_ruff_output(output);
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].line, Some(10));
        assert_eq!(issues[0].rule, "E501");
        assert_eq!(issues[1].rule, "F401");
    }

    #[test]
    fn test_parse_pytest_summary_line() {
        let stdout = "5 passed, 2 failed, 1 error in 3.2s";
        let result = parse_pytest_output(stdout, "");
        assert_eq!(result.passed, 5);
        assert_eq!(result.failed, 2);
        assert_eq!(result.errors, 1);
    }

    #[test]
    fn test_parse_pytest_equals_format() {
        let stdout = "===== 3 passed in 0.5s =====";
        let result = parse_pytest_output(stdout, "");
        assert_eq!(result.passed, 3);
    }

    #[test]
    fn test_parse_pytest_q_format() {
        let stdout = "3 passed";
        let result = parse_pytest_output(stdout, "");
        assert_eq!(result.passed, 3);
    }

    #[test]
    fn test_tool_exists() {
        // These should exist on any unix system
        assert!(tool_exists("ls"));
        assert!(!tool_exists("nonexistent_tool_xyz_12345"));
    }

    #[test]
    fn test_validate_path_safe() {
        let root = std::path::Path::new("/tmp/test_root");
        assert!(validate_path_within(root, "app/main.py").is_ok());
        assert!(validate_path_within(root, "tests/test_foo.py").is_ok());
        assert!(validate_path_within(root, "README.md").is_ok());
    }

    #[test]
    fn test_validate_path_traversal_blocked() {
        let root = std::path::Path::new("/tmp/test_root");
        assert!(validate_path_within(root, "../etc/passwd").is_err());
        assert!(validate_path_within(root, "app/../../etc/shadow").is_err());
        assert!(validate_path_within(root, "/etc/passwd").is_err());
        assert!(validate_path_within(root, "\\windows\\system32").is_err());
        assert!(validate_path_within(root, "file\0.py").is_err());
    }

    #[test]
    fn test_env_var_stripping() {
        // Set a test API key, run env via sandbox, verify it's stripped
        unsafe {
            std::env::set_var("TEST_API_KEY", "secret123");
        }
        let result = run_tool("env", &[], std::path::Path::new("/tmp"));
        assert!(
            !result.stdout.contains("secret123"),
            "API key leaked to subprocess!"
        );
        unsafe {
            std::env::remove_var("TEST_API_KEY");
        }
    }

    #[test]
    fn test_timeout_kills_process() {
        let result = run_tool_with_timeout("sleep", &["30"], std::path::Path::new("/tmp"), 2);
        assert!(result.timed_out, "Process should have timed out");
        assert!(!result.success);
    }
}
