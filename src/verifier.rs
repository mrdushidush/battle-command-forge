//! Verification engine: static analysis, linting, test execution, security scanning.
//!
//! Runs real tools when available (ruff, mypy, pytest, eslint, cargo clippy),
//! falls back to content-based heuristics when tools are not installed.

use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::sandbox;

/// Quality report for a single file.
#[derive(Debug, serde::Serialize)]
pub struct QualityReport {
    pub lint_passed: bool,
    pub lint_issues: Vec<String>,
    pub syntax_valid: bool,
    pub has_tests: bool,
    pub has_docstring: bool,
    pub has_error_handling: bool,
    pub has_hardcoded_secrets: bool,
    pub score: f32,
}

/// Quality report for an entire project directory.
#[derive(Debug, serde::Serialize)]
pub struct ProjectReport {
    pub file_reports: Vec<(String, QualityReport)>,
    pub tests_passed: u32,
    pub tests_failed: u32,
    pub tests_run: bool,
    pub avg_score: f32,
    /// Import errors or other test-blocking errors captured from pytest
    pub test_errors: Vec<String>,
}

/// Verify a single file with language-specific checks.
pub fn verify_file(path: &Path, language: &str) -> Result<QualityReport> {
    let content = std::fs::read_to_string(path)?;
    let mut report = QualityReport {
        lint_passed: true,
        lint_issues: vec![],
        syntax_valid: true,
        has_tests: false,
        has_docstring: false,
        has_error_handling: false,
        has_hardcoded_secrets: false,
        score: 0.0,
    };

    check_secrets(&content, &mut report);
    check_todos(&content, &mut report);

    match language {
        "python" => verify_python(path, &content, &mut report),
        "javascript" | "typescript" => verify_js_ts(&content, &mut report),
        "rust" => verify_rust(&content, &mut report),
        "go" => verify_go(&content, &mut report),
        _ => verify_generic(&content, &mut report),
    }

    calculate_score(&mut report);
    Ok(report)
}

/// Verify an entire project directory. Runs linters and tests.
pub fn verify_project(dir: &Path, language: &str) -> Result<ProjectReport> {
    let mut file_reports = Vec::new();

    // Verify individual files
    for entry in walkdir_files(dir)? {
        let ext = entry.extension().and_then(|e| e.to_str()).unwrap_or("");
        let file_lang = match ext {
            "py" => "python",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "rs" => "rust",
            "go" => "go",
            "cpp" | "cc" | "cxx" | "hpp" | "h" => "c++",
            _ => continue,
        };

        if let Ok(report) = verify_file(&entry, file_lang) {
            let rel = entry.strip_prefix(dir).unwrap_or(&entry);
            file_reports.push((rel.display().to_string(), report));
        }
    }

    // Run project-level linter
    run_project_linter(dir, language, &mut file_reports);

    // Run tests
    let (tests_passed, tests_failed, tests_run, test_errors) = run_project_tests(dir, language);

    let avg_score = if file_reports.is_empty() {
        5.0
    } else {
        file_reports.iter().map(|(_, r)| r.score).sum::<f32>() / file_reports.len() as f32
    };

    // Scale test bonus/penalty by pass rate (not binary)
    let adjusted_avg = if tests_run && (tests_passed > 0 || tests_failed > 0) {
        let total = tests_passed + tests_failed;
        let pass_rate = tests_passed as f32 / total as f32;
        // 100% pass → +2.0, 90% → +1.0, 50% → -1.0, 0% → -2.0
        let test_adjustment = (pass_rate * 4.0) - 2.0; // range: -2.0 to +2.0
        (avg_score + test_adjustment).clamp(0.0, 10.0)
    } else {
        avg_score
    };

    Ok(ProjectReport {
        file_reports,
        tests_passed,
        tests_failed,
        tests_run,
        avg_score: adjusted_avg,
        test_errors,
    })
}

/// Run project-level linter (ruff for Python, eslint for JS/TS).
fn run_project_linter(dir: &Path, language: &str, reports: &mut Vec<(String, QualityReport)>) {
    if language == "python" {
        let result = sandbox::run_tool("ruff", &["check", "."], dir);
        if !result.success && !result.stderr.contains("not found") {
            let issues = sandbox::parse_ruff_output(&result.stdout);
            if !issues.is_empty() {
                println!("   ruff: {} issues found", issues.len());
                for issue in &issues {
                    // Find matching file report and add the issue
                    if let Some((_, report)) = reports.iter_mut().find(|(f, _)| *f == issue.file) {
                        report.lint_issues.push(issue.to_string());
                        report.lint_passed = false;
                        calculate_score(report);
                    }
                }
            }
        }
    }
}

/// Run project tests (pytest for Python).
/// Returns (passed, failed, tests_run, error_details).
fn run_project_tests(dir: &Path, language: &str) -> (u32, u32, bool, Vec<String>) {
    match language {
        "python" => {
            if !sandbox::tool_exists("python3") {
                println!("   pytest: python3 not found, skipping tests");
                return (0, 0, false, vec![]);
            }

            let venv_dir = dir.join(".venv");
            // Cross-platform: Unix = .venv/bin/python3, Windows = .venv/Scripts/python.exe
            let venv_python = if cfg!(windows) {
                venv_dir.join("Scripts").join("python.exe")
            } else {
                venv_dir.join("bin").join("python3")
            };
            let system_python = if cfg!(windows) { "python" } else { "python3" };

            let python = if venv_python.exists() {
                // Use absolute path but do NOT resolve symlinks — canonicalize
                // follows the symlink → system python, breaking venv isolation
                std::path::absolute(&venv_python)
                    .unwrap_or_else(|_| venv_python.clone())
                    .to_string_lossy()
                    .to_string()
            } else {
                // Create venv for clean dependency isolation
                println!("   Creating venv...");
                let venv_result = sandbox::run_tool_with_timeout(
                    system_python,
                    &["-m", "venv", ".venv"],
                    dir,
                    30,
                );
                if venv_result.success {
                    std::path::absolute(&venv_python)
                        .unwrap_or_else(|_| venv_python.clone())
                        .to_string_lossy()
                        .to_string()
                } else {
                    println!("   venv creation failed, using system python");
                    system_python.to_string()
                }
            };

            // Step 1: Install dependencies
            println!("   Installing dependencies in venv...");
            let pyproject_path = dir.join("pyproject.toml");
            let req_path = dir.join("requirements.txt");

            // Try requirements.txt first (most reliable with pip)
            if req_path.exists() {
                let _ = sandbox::run_tool_with_timeout(
                    &python,
                    &[
                        "-m",
                        "pip",
                        "install",
                        "-q",
                        "--disable-pip-version-check",
                        "-r",
                        "requirements.txt",
                    ],
                    dir,
                    90,
                );
            }

            // Parse deps from pyproject.toml (handles both Poetry and PEP 621)
            if pyproject_path.exists() {
                let deps = extract_deps_from_pyproject(&pyproject_path);
                if !deps.is_empty() {
                    println!("   Installing {} deps from pyproject.toml...", deps.len());
                    let dep_refs: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
                    let mut args =
                        vec!["-m", "pip", "install", "-q", "--disable-pip-version-check"];
                    args.extend(dep_refs.iter());
                    let _ = sandbox::run_tool_with_timeout(&python, &args, dir, 120);
                }
            }

            // Always install test essentials (ensures venv has everything)
            let _ = sandbox::run_tool_with_timeout(
                &python,
                &[
                    "-m",
                    "pip",
                    "install",
                    "-q",
                    "--disable-pip-version-check",
                    "pytest",
                    "pytest-asyncio",
                    "pytest-cov",
                    "httpx",
                    "pydantic-settings",
                    "pydantic[email]",
                    "passlib[bcrypt]",
                    "python-jose[cryptography]",
                    "slowapi",
                    "aiosqlite",
                    "sqlalchemy[asyncio]",
                    "fastapi",
                    "uvicorn",
                    "python-multipart",
                    "email-validator",
                    "alembic",
                    "hypothesis",
                    "aiosqlite",
                    "bcrypt<5",
                ],
                dir,
                90,
            );

            // Step 2: Run pytest with retries for missing deps (up to 3 rounds)
            let mut attempt_result = sandbox::ToolResult {
                success: false,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
            };

            for attempt in 0..3 {
                attempt_result = sandbox::run_tool_sandboxed(
                    &python,
                    &[
                        "-m",
                        "pytest",
                        "--tb=short",
                        "-q",
                        "--no-header",
                        "--import-mode=importlib",
                    ],
                    dir,
                    120,
                    true,
                );

                if attempt_result.timed_out {
                    println!("   pytest: timed out");
                    return (0, 0, true, vec!["pytest timed out after 120s".to_string()]);
                }

                let combined = format!("{}\n{}", attempt_result.stdout, attempt_result.stderr);

                // If pyproject.toml has TOML parse errors, temporarily rename it and retry
                if combined.contains("Cannot overwrite a value")
                    || combined.contains("TOMLDecodeError")
                    || combined.contains("Invalid statement")
                {
                    println!("   pytest: pyproject.toml has TOML errors, retrying without it...");
                    let pyp = dir.join("pyproject.toml");
                    let pyp_bak = dir.join("pyproject.toml.bak");
                    let renamed = std::fs::rename(&pyp, &pyp_bak).is_ok();
                    // Write a minimal pytest.ini so asyncio_mode=auto is still active
                    let ini_path = dir.join("pytest.ini");
                    let _ = std::fs::write(&ini_path, "[pytest]\nasyncio_mode = auto\n");
                    attempt_result = sandbox::run_tool_sandboxed(
                        &python,
                        &[
                            "-m",
                            "pytest",
                            "--tb=short",
                            "-q",
                            "--no-header",
                            "--import-mode=importlib",
                            "tests/",
                        ],
                        dir,
                        120,
                        true,
                    );
                    // Restore pyproject.toml
                    if renamed {
                        let _ = std::fs::rename(&pyp_bak, &pyp);
                    }
                    let _ = std::fs::remove_file(&ini_path);
                    break;
                }

                // Only treat as import-time error if pytest couldn't collect/run tests
                // If we see "FAILED" or "passed", tests ran — errors are test failures, not import errors
                let tests_actually_ran =
                    combined.contains("FAILED ") || combined.contains(" passed");
                if !tests_actually_ran
                    && (combined.contains("ModuleNotFoundError")
                        || combined.contains("ImportError")
                        || combined.contains("NameError")
                        || combined.contains("AttributeError")
                        || combined.contains("TypeError")
                        || combined.contains("SyntaxError")
                        || combined.contains("ValueError"))
                {
                    if let Some(module) = extract_missing_module(&combined) {
                        println!(
                            "   Installing missing module: {} (attempt {})",
                            module,
                            attempt + 1
                        );
                        let _ = sandbox::run_tool_with_timeout(
                            &python,
                            &["-m", "pip", "install", "-q", &module],
                            dir,
                            30,
                        );
                        continue; // retry pytest
                    }
                    // Can't extract module name — capture error details for feedback
                    let mut errors = Vec::new();
                    for line in combined.lines() {
                        if line.contains("ModuleNotFoundError")
                            || line.contains("ImportError")
                            || line.contains("cannot import name")
                            || line.contains("circular import")
                            || line.contains("NameError")
                            || line.contains("AttributeError")
                            || line.contains("TypeError")
                            || line.contains("SyntaxError")
                        {
                            let err = line.trim().to_string();
                            println!("   pytest: {}", err);
                            errors.push(err);
                        }
                    }
                    if errors.is_empty() {
                        errors.push("ImportError prevented tests from running".to_string());
                    }
                    println!("   pytest: import errors prevented tests from running");
                    return (0, 1, true, errors);
                }
                break; // no import errors, we're done
            }

            let combined = format!("{}\n{}", attempt_result.stdout, attempt_result.stderr);

            let test_result =
                sandbox::parse_pytest_output(&attempt_result.stdout, &attempt_result.stderr);

            if test_result.passed > 0 || test_result.failed > 0 || test_result.errors > 0 {
                println!(
                    "   pytest: {} passed, {} failed, {} errors",
                    test_result.passed, test_result.failed, test_result.errors
                );
                // Capture failure details for feedback (deduplicated)
                let mut errors = Vec::new();
                if test_result.failed > 0 || test_result.errors > 0 {
                    for line in combined.lines() {
                        let l = line.trim();
                        if (l.starts_with("FAILED")
                            || l.starts_with("ERROR")
                            || l.contains("AssertionError")
                            || l.contains("assert ")
                            || l.contains("NameError")
                            || l.contains("AttributeError"))
                            && !errors.contains(&l.to_string())
                        {
                            errors.push(l.to_string());
                        }
                    }
                }
                (
                    test_result.passed,
                    test_result.failed + test_result.errors,
                    true,
                    errors,
                )
            } else if combined.contains("no tests ran") || combined.contains("collected 0 items") {
                println!("   pytest: no tests found");
                (0, 0, true, vec![])
            } else {
                // Pytest ran but we couldn't parse output — log stderr for debugging
                println!("   pytest: ran but no parseable results");
                let combined_out = format!("{}\n{}", attempt_result.stdout, attempt_result.stderr);
                let first_lines: Vec<&str> = combined_out
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .take(5)
                    .collect();
                if !first_lines.is_empty() {
                    println!("   pytest output (first 5 lines):");
                    for line in &first_lines {
                        println!("     {}", line);
                    }
                }
                (0, 0, true, vec![])
            }
        }
        "rust" => {
            if !sandbox::tool_exists("cargo") {
                println!("   cargo: not found, skipping tests");
                return (0, 0, false, vec![]);
            }
            let result = sandbox::run_tool_sandboxed("cargo", &["test", "--quiet"], dir, 120, true);
            if result.timed_out {
                println!("   cargo test: timed out");
                return (0, 0, true, vec![]);
            }
            let combined = format!("{}\n{}", result.stdout, result.stderr);
            for line in combined.lines() {
                if line.contains("test result:") {
                    let passed = line
                        .split("passed")
                        .next()
                        .and_then(|s| s.split_whitespace().last())
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    let failed = line
                        .split("failed")
                        .next()
                        .and_then(|s| s.split_whitespace().last())
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    println!("   cargo test: {} passed, {} failed", passed, failed);
                    return (passed, failed, true, vec![]);
                }
            }
            if result.success {
                println!("   cargo test: passed");
                (1, 0, true, vec![])
            } else {
                println!("   cargo test: failed");
                (0, 1, true, vec![])
            }
        }
        "go" => {
            if !sandbox::tool_exists("go") {
                println!("   go: not found, skipping tests");
                return (0, 0, false, vec![]);
            }
            let result =
                sandbox::run_tool_sandboxed("go", &["test", "./...", "-count=1"], dir, 120, true);
            if result.timed_out {
                println!("   go test: timed out");
                return (0, 0, true, vec![]);
            }
            let combined = format!("{}\n{}", result.stdout, result.stderr);
            if combined.contains("PASS") {
                let passed = combined.matches("--- PASS").count() as u32;
                let failed = combined.matches("--- FAIL").count() as u32;
                println!("   go test: {} passed, {} failed", passed.max(1), failed);
                (passed.max(1), failed, true, vec![])
            } else {
                println!("   go test: failed");
                (0, 1, true, vec![])
            }
        }
        "typescript" | "javascript" => {
            let pkg_json = dir.join("package.json");
            if !pkg_json.exists() {
                return (0, 0, false, vec![]);
            }
            let _ = sandbox::run_tool_with_timeout("npm", &["install", "--silent"], dir, 90);
            let result = sandbox::run_tool_sandboxed(
                "npm",
                &["test", "--", "--passWithNoTests"],
                dir,
                120,
                true,
            );
            if result.timed_out {
                println!("   npm test: timed out");
                return (0, 0, true, vec![]);
            }
            if result.success {
                println!("   npm test: passed");
                (1, 0, true, vec![])
            } else {
                println!("   npm test: failed");
                (0, 1, true, vec![])
            }
        }
        _ => (0, 0, false, vec![]),
    }
}

/// Extract pip-installable deps from pyproject.toml (supports Poetry and PEP 621 formats).
fn extract_deps_from_pyproject(path: &std::path::Path) -> Vec<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut deps = Vec::new();
    let mut in_deps_section = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect dependency sections
        if trimmed == "[tool.poetry.dependencies]"
            || trimmed == "[tool.poetry.group.dev.dependencies]"
            || trimmed == "[project]"
            || trimmed.starts_with("[project.optional-dependencies")
        {
            in_deps_section = true;
            continue;
        }

        // End of section
        if trimmed.starts_with('[') && in_deps_section {
            in_deps_section = false;
            continue;
        }

        if !in_deps_section {
            continue;
        }

        // Skip python version constraint
        if trimmed.starts_with("python ") || trimmed.starts_with("python=") {
            continue;
        }

        // Parse "package = ..." lines (Poetry format)
        if let Some(eq_pos) = trimmed.find('=') {
            let pkg = trimmed[..eq_pos].trim().trim_matches('"');
            if pkg.is_empty()
                || pkg.contains(' ')
                || pkg == "name"
                || pkg == "version"
                || pkg == "description"
                || pkg == "authors"
                || pkg == "license"
                || pkg == "readme"
                || pkg == "requires-python"
            {
                continue;
            }

            // Handle extras: passlib = { extras = ["bcrypt"], version = "..." }
            let value = trimmed[eq_pos + 1..].trim();
            if value.contains("extras") {
                // Extract extras
                if let Some(extras_start) = value.find('[') {
                    if let Some(extras_end) = value.find(']') {
                        let extras: Vec<&str> = value[extras_start + 1..extras_end]
                            .split(',')
                            .map(|e| e.trim().trim_matches('"').trim_matches('\''))
                            .collect();
                        let extras_str = extras.join(",");
                        deps.push(format!("{}[{}]", pkg, extras_str));
                        continue;
                    }
                }
            }
            deps.push(pkg.to_string());
        }

        // Parse PEP 621 dependencies list: "package>=1.0"
        if trimmed.starts_with('"') || trimmed.starts_with('\'') {
            let dep = trimmed.trim_matches(|c: char| c == '"' || c == '\'' || c == ',');
            if !dep.is_empty() {
                deps.push(dep.to_string());
            }
        }
    }

    deps
}

/// Extract the missing module name from an ImportError/ModuleNotFoundError message.
fn extract_missing_module(output: &str) -> Option<String> {
    for line in output.lines() {
        // "ModuleNotFoundError: No module named 'jose'"
        if line.contains("No module named") {
            let after = line.split("No module named").nth(1)?;
            let module = after
                .trim()
                .trim_matches(|c: char| c == '\'' || c == '"' || c == ' ');
            // Map common module names to pip packages
            let pip_name = match module {
                "jose" => "python-jose[cryptography]",
                "jwt" => "PyJWT",
                "passlib" => "passlib[bcrypt]",
                "sqlalchemy" => "sqlalchemy",
                "pydantic" => "pydantic[email]",
                "fastapi" => "fastapi",
                "httpx" => "httpx",
                "slowapi" => "slowapi",
                "starlette" => "starlette",
                "dotenv" => "python-dotenv",
                _ => module,
            };
            return Some(pip_name.to_string());
        }
    }
    None
}

fn check_secrets(content: &str, report: &mut QualityReport) {
    let secret_patterns = [
        "password = \"",
        "secret_key = \"",
        "api_key = \"",
        "AWS_SECRET",
        "PRIVATE_KEY",
        "sk-",
        "ghp_",
    ];
    for pattern in &secret_patterns {
        if content.to_lowercase().contains(&pattern.to_lowercase()) {
            report.has_hardcoded_secrets = true;
            report
                .lint_issues
                .push(format!("Possible hardcoded secret: {}", pattern));
        }
    }
}

fn check_todos(content: &str, report: &mut QualityReport) {
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        // Only match TODO/FIXME/HACK as comment markers, not substrings in identifiers/strings
        // Match: "# TODO: fix this", "// FIXME", "/* HACK */", "TODO fix"
        // Skip: "todo_app", "todo-crud", '"Build a todo"'
        let is_comment_line = trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*');
        if !is_comment_line {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if lower.contains("todo") || lower.contains("fixme") || lower.contains("hack") {
            report
                .lint_issues
                .push(format!("Line {}: TODO/FIXME found", i + 1));
        }
    }
}

fn verify_python(_path: &Path, content: &str, report: &mut QualityReport) {
    // Syntax check with python3 — pipe content via stdin to avoid shell injection
    use std::process::Stdio;
    let child = Command::new("python3")
        .args(["-c", "import ast, sys; ast.parse(sys.stdin.read())"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    if let Ok(mut child) = child {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = std::io::Write::write_all(&mut stdin, content.as_bytes());
        }
        if let Ok(output) = child.wait_with_output() {
            report.syntax_valid = output.status.success();
            if !output.status.success() {
                report
                    .lint_issues
                    .push("Python syntax error in generated code".to_string());
            }
            return;
        }
    }
    // If python3 not available, skip syntax check
    report.syntax_valid = true;

    report.has_tests = content.contains("def test_") || content.contains("class Test");
    report.has_docstring = content.contains("\"\"\"") || content.contains("'''");
    report.has_error_handling = content.contains("try:") || content.contains("except ");
}

fn verify_js_ts(content: &str, report: &mut QualityReport) {
    report.has_tests =
        content.contains("describe(") || content.contains("test(") || content.contains("it(");
    report.has_docstring = content.contains("/**") || content.contains("///");
    report.has_error_handling =
        content.contains("try {") || content.contains("catch (") || content.contains(".catch(");
    report.syntax_valid = content.contains("function ")
        || content.contains("const ")
        || content.contains("export ")
        || content.contains("class ");
}

fn verify_rust(content: &str, report: &mut QualityReport) {
    report.has_tests = content.contains("#[test]") || content.contains("#[cfg(test)]");
    report.has_docstring = content.contains("///") || content.contains("//!");
    report.has_error_handling = content.contains("Result<")
        || content.contains("anyhow::")
        || content.contains(".unwrap_or");
    report.syntax_valid =
        content.contains("fn ") || content.contains("struct ") || content.contains("impl ");
}

fn verify_go(content: &str, report: &mut QualityReport) {
    report.has_tests = content.contains("func Test");
    report.has_docstring = content.lines().any(|l| l.starts_with("//"));
    report.has_error_handling = content.contains("if err != nil");
    report.syntax_valid = content.contains("package ") && content.contains("func ");
}

fn verify_generic(content: &str, report: &mut QualityReport) {
    report.syntax_valid = content.len() > 50;
    report.has_docstring =
        content.contains("//") || content.contains("#") || content.contains("/*");
}

fn calculate_score(report: &mut QualityReport) {
    let mut score: f32 = 5.0;
    if report.syntax_valid {
        score += 1.5;
    }
    if report.lint_passed {
        score += 1.0;
    }
    if report.has_tests {
        score += 1.0;
    }
    if report.has_docstring {
        score += 0.5;
    }
    if report.has_error_handling {
        score += 1.0;
    }
    if report.has_hardcoded_secrets {
        score -= 2.0;
    }
    if !report.lint_issues.is_empty() {
        score -= (0.2 * report.lint_issues.len() as f32).min(2.0);
    }
    report.score = score.clamp(0.0, 10.0);
}

fn walkdir_files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // Skip venvs, __pycache__, node_modules, .git, build artifacts
                let name = entry.file_name().to_string_lossy().to_string();
                if name == ".venv"
                    || name == "venv"
                    || name == "__pycache__"
                    || name == "node_modules"
                    || name == ".git"
                    || name == "target"
                    || name == ".mypy_cache"
                    || name == ".pytest_cache"
                    || name == ".ruff_cache"
                    || name == "dist"
                    || name == "build"
                    || name == ".egg-info"
                    || name.ends_with(".egg-info")
                {
                    continue;
                }
                files.extend(walkdir_files(&path)?);
            } else {
                files.push(path);
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_report() -> QualityReport {
        QualityReport {
            lint_passed: true,
            lint_issues: vec![],
            syntax_valid: true,
            has_tests: false,
            has_docstring: false,
            has_error_handling: false,
            has_hardcoded_secrets: false,
            score: 0.0,
        }
    }

    #[test]
    fn test_secret_detection() {
        let mut report = empty_report();
        check_secrets("password = \"hunter2\"", &mut report);
        assert!(report.has_hardcoded_secrets);
    }

    #[test]
    fn test_no_false_positive_secrets() {
        let mut report = empty_report();
        check_secrets("let x = 42;", &mut report);
        assert!(!report.has_hardcoded_secrets);
    }

    #[test]
    fn test_todo_detection() {
        let mut report = empty_report();
        check_todos("# TODO: fix this later\ncode here", &mut report);
        assert_eq!(report.lint_issues.len(), 1);
    }

    #[test]
    fn test_score_calculation() {
        let mut report = empty_report();
        report.syntax_valid = true;
        report.lint_passed = true;
        report.has_tests = true;
        report.has_docstring = true;
        report.has_error_handling = true;
        calculate_score(&mut report);
        assert_eq!(report.score, 10.0);
    }

    #[test]
    fn test_score_with_secrets_penalty() {
        let mut report = empty_report();
        report.has_hardcoded_secrets = true;
        report.lint_issues.push("secret found".into());
        calculate_score(&mut report);
        // Secrets penalty (-2.0) should significantly reduce the score
        let mut clean_report = empty_report();
        calculate_score(&mut clean_report);
        assert!(report.score < clean_report.score);
    }
}
