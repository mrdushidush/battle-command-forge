/// SWE-bench evaluation framework for BattleCommand Forge.
/// Runs against real GitHub issues from the SWE-bench dataset.
/// ReAct agent loop explores repos and generates patches, then validates
/// against the repo's test suite.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use std::time::Instant;

use crate::llm::{ChatMessage, LlmClient, OllamaTool, OllamaToolFunction};
use crate::swebench_tools;

// ─── Per-Repo Configuration ───

struct RepoConfig {
    install_cmd: &'static str,
    test_cmd_template: &'static str,
    env_setup: &'static str,
}

fn get_repo_config(repo: &str) -> RepoConfig {
    match repo {
        "django/django" => RepoConfig {
            install_cmd: "python3 -m pip install -e . -q 2>/dev/null",
            test_cmd_template: "cd tests && python3 runtests.py {test} --verbosity=0 2>&1",
            env_setup: "",
        },
        "sympy/sympy" => RepoConfig {
            install_cmd: "python3 -m pip install -e . -q 2>/dev/null",
            test_cmd_template: "python3 -m pytest -xvs -k {test} 2>&1",
            env_setup: "",
        },
        "matplotlib/matplotlib" => RepoConfig {
            install_cmd: "python3 -m pip install -e '.[dev]' -q 2>/dev/null",
            test_cmd_template: "python3 -m pytest -xvs {test} 2>&1",
            env_setup: "",
        },
        "scikit-learn/scikit-learn" => RepoConfig {
            install_cmd: "python3 -m pip install -e . -q 2>/dev/null",
            test_cmd_template: "python3 -m pytest -xvs {test} 2>&1",
            env_setup: "",
        },
        "pytest-dev/pytest" => RepoConfig {
            install_cmd: "python3 -m pip install -e . -q 2>/dev/null",
            test_cmd_template: "python3 -m pytest -xvs {test} 2>&1",
            env_setup: "",
        },
        "sphinx-doc/sphinx" => RepoConfig {
            install_cmd: "python3 -m pip install -e '.[test]' -q 2>/dev/null",
            test_cmd_template: "python3 -m pytest -xvs {test} 2>&1",
            env_setup: "",
        },
        _ => RepoConfig {
            install_cmd: "python3 -m pip install -e . -q 2>/dev/null",
            test_cmd_template: "python3 -m pytest -xvs {test} 2>&1",
            env_setup: "",
        },
    }
}

fn normalize_django_test(test: &str) -> String {
    if let Some(paren_start) = test.find('(') {
        test[paren_start + 1..]
            .trim_end_matches(')')
            .trim()
            .to_string()
    } else {
        test.to_string()
    }
}

// ─── Types ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwebenchInstance {
    pub instance_id: String,
    pub repo: String,
    pub base_commit: String,
    pub problem_statement: String,
    #[serde(default)]
    pub hints_text: Option<String>,
    pub test_patch: String,
    #[serde(rename = "FAIL_TO_PASS")]
    pub fail_to_pass: serde_json::Value,
    #[serde(rename = "PASS_TO_PASS", default)]
    pub pass_to_pass: serde_json::Value,
    #[serde(default)]
    pub version: Option<String>,
}

impl SwebenchInstance {
    pub fn fail_to_pass_tests(&self) -> Vec<String> {
        parse_test_list(&self.fail_to_pass)
    }
    pub fn pass_to_pass_tests(&self) -> Vec<String> {
        parse_test_list(&self.pass_to_pass)
    }
}

fn parse_test_list(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        serde_json::Value::String(s) => {
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(s) {
                arr
            } else {
                vec![s.clone()]
            }
        }
        _ => vec![],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceResult {
    pub instance_id: String,
    pub repo: String,
    pub model: String,
    pub resolved: bool,
    pub fail_to_pass_count: usize,
    pub fail_to_pass_passed: usize,
    pub turns_used: u32,
    pub tokens_used: u64,
    pub duration_secs: f64,
    pub files_modified: Vec<String>,
    pub patch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
struct Prediction {
    instance_id: String,
    model_name_or_path: String,
    model_patch: String,
}

#[derive(Debug, Clone)]
pub struct SwebenchOpts {
    pub dataset_path: Option<String>,
    pub variant: String,
    pub instance_filter: Option<String>,
    pub limit: Option<u32>,
    pub offset: u32,
    pub output_dir: String,
    pub model_override: Option<String>,
    pub max_turns: u32,
    pub timeout_secs: u64,
    pub resume: bool,
}

impl Default for SwebenchOpts {
    fn default() -> Self {
        Self {
            dataset_path: None,
            variant: "lite".into(),
            instance_filter: None,
            limit: None,
            offset: 0,
            output_dir: ".battlecommand/swebench".into(),
            model_override: None,
            max_turns: 25,
            timeout_secs: 1800,
            resume: false,
        }
    }
}

// ─── Dataset Loading ───

pub fn load_dataset(opts: &SwebenchOpts) -> Result<Vec<SwebenchInstance>> {
    let path = if let Some(ref p) = opts.dataset_path {
        p.clone()
    } else {
        format!("{}/datasets/{}.json", opts.output_dir, opts.variant)
    };

    if !Path::new(&path).exists() {
        return Err(anyhow::anyhow!(
            "Dataset not found at '{}'. Download it first:\n  \
             wget -O {} https://raw.githubusercontent.com/princeton-nlp/SWE-bench/main/swebench/test/{}.json\n  \
             Or use --dataset <path> to specify a local file.",
            path, path, opts.variant
        ));
    }

    let data = std::fs::read_to_string(&path)?;
    let instances: Vec<SwebenchInstance> = if data.trim_start().starts_with('[') {
        serde_json::from_str(&data)?
    } else {
        data.lines()
            .filter(|l| !l.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?
    };

    println!("Loaded {} instances from {}", instances.len(), path);

    let mut filtered = instances;
    if let Some(ref id) = opts.instance_filter {
        filtered.retain(|i| i.instance_id == *id);
    }
    let start = opts.offset as usize;
    if start > 0 && start < filtered.len() {
        filtered = filtered[start..].to_vec();
    }
    if let Some(limit) = opts.limit {
        filtered.truncate(limit as usize);
    }
    Ok(filtered)
}

pub fn list_instances(opts: &SwebenchOpts, repo_filter: Option<&str>) -> Result<()> {
    let instances = load_dataset(opts)?;
    let mut by_repo: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for inst in &instances {
        by_repo
            .entry(inst.repo.clone())
            .or_default()
            .push(inst.instance_id.clone());
    }

    if let Some(repo) = repo_filter {
        if let Some(ids) = by_repo.get(repo) {
            println!("\n{} ({} instances):", repo, ids.len());
            for id in ids {
                println!("  {}", id);
            }
        } else {
            println!("No instances found for repo '{}'", repo);
            println!("Available repos:");
            for r in by_repo.keys() {
                println!("  {}", r);
            }
        }
    } else {
        println!(
            "\n{} total instances across {} repos:\n",
            instances.len(),
            by_repo.len()
        );
        println!("{:<35} Instances", "Repository");
        println!("{}", "-".repeat(50));
        for (repo, ids) in &by_repo {
            println!("{:<35} {}", repo, ids.len());
        }
    }
    Ok(())
}

// ─── Workspace Setup ───

async fn setup_instance_workspace(instance: &SwebenchInstance, output_dir: &str) -> Result<String> {
    let abs_output = std::fs::canonicalize(output_dir)
        .unwrap_or_else(|_| std::path::PathBuf::from(output_dir))
        .to_string_lossy()
        .to_string();
    let workspace = format!("{}/workspaces/{}", abs_output, instance.instance_id);

    if Path::new(&format!("{}/.git", workspace)).exists() {
        println!("  Workspace exists, resetting to base commit...");
        let status = tokio::process::Command::new("git")
            .args(["reset", "--hard", &instance.base_commit])
            .current_dir(&workspace)
            .output()
            .await?;
        if !status.status.success() {
            return Err(anyhow::anyhow!("Failed to reset to base commit"));
        }
        let _ = tokio::process::Command::new("git")
            .args(["clean", "-fd"])
            .current_dir(&workspace)
            .output()
            .await;
        return Ok(workspace);
    }

    let repo_slug = instance.repo.replace('/', "__");
    let cache_dir = format!("{}/repos/{}", abs_output, repo_slug);

    if !Path::new(&format!("{}/.git", cache_dir)).exists() {
        println!(
            "  Cloning {} (first time, will be cached)...",
            instance.repo
        );
        tokio::fs::create_dir_all(&format!("{}/repos", abs_output)).await?;
        let clone_url = format!("https://github.com/{}.git", instance.repo);
        let status = tokio::process::Command::new("git")
            .args(["clone", "--quiet", &clone_url, &cache_dir])
            .output()
            .await?;
        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            return Err(anyhow::anyhow!(
                "Failed to clone {}: {}",
                instance.repo,
                stderr
            ));
        }
    }

    println!("  Creating worktree at {}...", instance.instance_id);
    tokio::fs::create_dir_all(&format!("{}/workspaces", abs_output)).await?;
    let _ = tokio::process::Command::new("git")
        .args(["fetch", "--quiet", "origin", &instance.base_commit])
        .current_dir(&cache_dir)
        .output()
        .await;

    let wt_status = tokio::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "--detach",
            &workspace,
            &instance.base_commit,
        ])
        .current_dir(&cache_dir)
        .output()
        .await?;

    if !wt_status.status.success() {
        println!("  Worktree failed, falling back to direct clone...");
        let _ = tokio::fs::remove_dir_all(&workspace).await;
        tokio::fs::create_dir_all(&workspace).await?;
        let status = tokio::process::Command::new("git")
            .args(["clone", "--quiet", "--shared", &cache_dir, &workspace])
            .output()
            .await?;
        if !status.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to create workspace for {}",
                instance.instance_id
            ));
        }
        let status = tokio::process::Command::new("git")
            .args(["checkout", "--quiet", &instance.base_commit])
            .current_dir(&workspace)
            .output()
            .await?;
        if !status.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to checkout {}",
                instance.base_commit
            ));
        }
    }

    // Apply test patch
    if !instance.test_patch.is_empty() {
        let test_patch_path = format!("{}/test_patch.diff", workspace);
        tokio::fs::write(&test_patch_path, &instance.test_patch).await?;
        let status = tokio::process::Command::new("git")
            .args(["apply", "--allow-empty", "test_patch.diff"])
            .current_dir(&workspace)
            .output()
            .await?;
        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            println!(
                "  Warning: test_patch apply failed (may be ok): {}",
                stderr.trim()
            );
        }
        let _ = tokio::fs::remove_file(&test_patch_path).await;
    }

    // Install dependencies
    let marker = format!("{}/.battlecommand_deps_installed", workspace);
    if !Path::new(&marker).exists() {
        let config = get_repo_config(&instance.repo);
        println!("  Installing dependencies for {}...", instance.repo);
        let install = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(config.install_cmd)
            .current_dir(&workspace)
            .output()
            .await;
        match install {
            Ok(output) if output.status.success() => {
                println!("  Dependencies installed.");
                let _ = tokio::fs::write(&marker, "installed").await;
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!(
                    "  Warning: dependency install may have failed: {}",
                    stderr.lines().last().unwrap_or("unknown")
                );
                let _ = tokio::fs::write(&marker, "attempted").await;
            }
            Err(e) => println!("  Warning: could not install dependencies: {}", e),
        }
    }

    Ok(workspace)
}

// ─── Tool Definitions ───

fn swebench_tools_def() -> Vec<OllamaTool> {
    vec![
        OllamaTool { tool_type: "function".into(), function: OllamaToolFunction {
            name: "read_file".into(),
            description: "Read a file from the repository with line numbers. Max 200 lines per call.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string","description":"File path relative to repo root"},"start_line":{"type":"integer","description":"Starting line (default 1)"},"end_line":{"type":"integer","description":"Ending line"}},"required":["path"]}),
        }},
        OllamaTool { tool_type: "function".into(), function: OllamaToolFunction {
            name: "grep_search".into(),
            description: "Search for text patterns in the repository. Returns matching lines with file paths and line numbers.".into(),
            parameters: json!({"type":"object","properties":{"pattern":{"type":"string","description":"Text or regex pattern to search for"},"path":{"type":"string","description":"Directory to search in (default: root)"}},"required":["pattern"]}),
        }},
        OllamaTool { tool_type: "function".into(), function: OllamaToolFunction {
            name: "list_directory".into(),
            description: "List files and subdirectories. Shows file sizes.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string","description":"Directory path (default: root)"}},"required":[]}),
        }},
        OllamaTool { tool_type: "function".into(), function: OllamaToolFunction {
            name: "run_command".into(),
            description: "Run a shell command in the repository directory. 30-second timeout.".into(),
            parameters: json!({"type":"object","properties":{"command":{"type":"string","description":"Shell command to execute"}},"required":["command"]}),
        }},
        OllamaTool { tool_type: "function".into(), function: OllamaToolFunction {
            name: "write_file".into(),
            description: "Write complete file contents. Creates parent directories if needed.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string","description":"File path relative to repo root"},"content":{"type":"string","description":"Complete file contents"}},"required":["path","content"]}),
        }},
        OllamaTool { tool_type: "function".into(), function: OllamaToolFunction {
            name: "apply_edit".into(),
            description: "Replace specific text in a file. Preferred for small changes (1-10 lines).".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string","description":"File path"},"old_text":{"type":"string","description":"Exact text to find"},"new_text":{"type":"string","description":"Replacement text"}},"required":["path","old_text","new_text"]}),
        }},
        OllamaTool { tool_type: "function".into(), function: OllamaToolFunction {
            name: "submit".into(),
            description: "Signal that you have finished fixing the bug.".into(),
            parameters: json!({"type":"object","properties":{},"required":[]}),
        }},
    ]
}

fn build_system_prompt() -> String {
    r#"You are an expert software engineer tasked with fixing a bug in a Python repository.

You have tools to explore the codebase, understand the issue, and apply a fix.

## WORKFLOW
1. Read the issue carefully — identify what behavior is wrong and what is expected
2. Search the codebase to find relevant files (grep for error messages, class names, function names)
3. Read the relevant source files to understand the code structure
4. Identify the root cause of the bug
5. Make the MINIMAL change needed to fix the bug
6. Run the failing test(s) to verify your fix works
7. Call submit() when done

## RULES
- Make MINIMAL changes — fix the bug, do NOT refactor, add features, or change unrelated code
- Prefer apply_edit for small changes (1-10 lines)
- When using write_file, include the COMPLETE file contents
- Always verify your fix by running tests before calling submit()
- Do NOT modify test files — only fix the source code
- Use `python3` (not `python`) for all commands"#
        .to_string()
}

// ─── ReAct Agent Loop ───

async fn run_agent_loop(
    llm: &LlmClient,
    instance: &SwebenchInstance,
    workspace: &str,
    max_turns: u32,
) -> Result<(u32, u64)> {
    let tools = swebench_tools_def();
    let system_prompt = build_system_prompt();

    let _repo_config = get_repo_config(&instance.repo);
    let test_hint = if instance.repo == "django/django" {
        let test_labels: Vec<String> = instance
            .fail_to_pass_tests()
            .iter()
            .map(|t| normalize_django_test(t))
            .collect();
        format!("\n\n## How to run tests\nDjango project. Run tests with:\n```\ncd tests && python3 runtests.py {}\n```", test_labels.join(" "))
    } else if instance.repo == "sympy/sympy" {
        let tests = instance.fail_to_pass_tests();
        format!("\n\n## How to run tests\nsympy project. Run tests with:\n```\npython3 -m pytest -xvs -k \"{}\"\n```", tests.join(" or "))
    } else {
        let tests = instance.fail_to_pass_tests();
        format!(
            "\n\n## How to run tests\nRun failing tests with:\n```\npython3 -m pytest -xvs {}\n```",
            tests.join(" ")
        )
    };

    let user_content = format!(
        "## Issue to Fix\n\n{}\n\n## Failing Tests\nThe following tests should PASS after your fix:\n{}{}",
        instance.problem_statement, instance.fail_to_pass_tests().join("\n"), test_hint,
    );

    let mut messages: Vec<ChatMessage> = vec![
        ChatMessage {
            role: "system".into(),
            content: system_prompt,
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "user".into(),
            content: user_content,
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let mut turns_used: u32 = 0;
    let mut write_turns: u32 = 0;

    for turn in 0..max_turns {
        turns_used = turn + 1;

        let resp = llm.chat_with_tools(&messages, &tools).await?;

        if !resp.tool_calls.is_empty() {
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: resp.content.clone(),
                tool_calls: Some(resp.tool_calls.clone()),
                tool_call_id: None,
            });

            for tc in &resp.tool_calls {
                let result =
                    swebench_tools::execute(&tc.function.name, &tc.function.arguments, workspace)
                        .await;
                println!(
                    "    [turn {}] {} → {}",
                    turn + 1,
                    tc.function.name,
                    if result.success { "ok" } else { "FAIL" }
                );

                if result.is_submit {
                    messages.push(ChatMessage {
                        role: "tool".into(),
                        content: result.content,
                        tool_calls: None,
                        tool_call_id: Some(tc.function.name.clone()),
                    });
                    return Ok((turns_used, 0));
                }
                if result.is_write {
                    write_turns += 1;
                }

                let content = if result.content.len() > 4096 {
                    format!(
                        "{}...\n[truncated, {} chars total]",
                        &result.content[..result.content.len().min(4096)],
                        result.content.len()
                    )
                } else {
                    result.content
                };
                messages.push(ChatMessage {
                    role: "tool".into(),
                    content,
                    tool_calls: None,
                    tool_call_id: Some(tc.function.name.clone()),
                });
            }

            if turn >= 14 && write_turns == 0 {
                messages.push(ChatMessage {
                    role: "system".into(),
                    content: "You have used 15 turns without making any code changes. Please apply your fix NOW using apply_edit or write_file. Then run tests and submit.".into(),
                    tool_calls: None, tool_call_id: None,
                });
            }

            if turn >= 19 && messages.len() > 30 {
                compact_messages(&mut messages);
            }
            continue;
        }

        messages.push(ChatMessage {
            role: "assistant".into(),
            content: resp.content.clone(),
            tool_calls: None,
            tool_call_id: None,
        });

        let lower = resp.content.to_lowercase();
        if lower.contains("submit")
            || lower.contains("finished")
            || lower.contains("fix has been applied")
        {
            println!("    Agent indicated completion at turn {}", turn + 1);
            return Ok((turns_used, 0));
        }

        messages.push(ChatMessage {
            role: "user".into(),
            content: "Please continue. Use the tools to explore the code and fix the bug. Call submit() when done.".into(),
            tool_calls: None, tool_call_id: None,
        });
    }

    println!("    Agent reached max turns ({})", max_turns);
    Ok((turns_used, 0))
}

fn compact_messages(messages: &mut Vec<ChatMessage>) {
    if messages.len() <= 12 {
        return;
    }
    let keep_start = 2;
    let keep_end = messages.len().saturating_sub(10);
    if keep_end <= keep_start {
        return;
    }

    let mut summary = String::from("## Previous exploration summary\n");
    for msg in &messages[keep_start..keep_end] {
        if msg.role == "tool" {
            let preview = if msg.content.len() > 150 {
                &msg.content[..150]
            } else {
                &msg.content
            };
            summary.push_str(&format!("- tool: {}\n", preview));
        } else if msg.role == "assistant" {
            let preview = if msg.content.len() > 200 {
                &msg.content[..200]
            } else {
                &msg.content
            };
            summary.push_str(&format!("- thought: {}\n", preview));
        }
    }

    let replacement = ChatMessage {
        role: "system".into(),
        content: summary,
        tool_calls: None,
        tool_call_id: None,
    };
    messages.splice(keep_start..keep_end, std::iter::once(replacement));
}

// ─── Patch Generation ───

async fn generate_patch(workspace: &str) -> Result<(String, Vec<String>)> {
    let output = tokio::process::Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(workspace)
        .output()
        .await?;
    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    let files_output = tokio::process::Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(workspace)
        .output()
        .await?;
    let files: Vec<String> = String::from_utf8_lossy(&files_output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    Ok((diff, files))
}

// ─── Checkpoint / Resume ───

fn load_completed_ids(output_dir: &str) -> std::collections::HashSet<String> {
    let path = format!("{}/swebench_results.jsonl", output_dir);
    let mut ids = std::collections::HashSet::new();
    if let Ok(data) = std::fs::read_to_string(&path) {
        for line in data.lines() {
            if let Ok(result) = serde_json::from_str::<InstanceResult>(line) {
                ids.insert(result.instance_id);
            }
        }
    }
    ids
}

fn append_result(output_dir: &str, result: &InstanceResult) -> Result<()> {
    use std::io::Write;
    let path = format!("{}/swebench_results.jsonl", output_dir);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", serde_json::to_string(result)?)?;
    Ok(())
}

fn append_prediction(output_dir: &str, prediction: &Prediction) -> Result<()> {
    use std::io::Write;
    let path = format!("{}/predictions.jsonl", output_dir);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", serde_json::to_string(prediction)?)?;
    Ok(())
}

// ─── Entry Points ───

pub async fn run_single(
    instance: &SwebenchInstance,
    opts: &SwebenchOpts,
) -> Result<InstanceResult> {
    let model = opts
        .model_override
        .as_deref()
        .unwrap_or("claude-sonnet-4-6");
    let llm = LlmClient::with_limits(model, 65536, 8192);

    println!("\n[SWE-bench] Instance: {}", instance.instance_id);
    println!(
        "  Repo: {} @ {}",
        instance.repo,
        &instance.base_commit[..7.min(instance.base_commit.len())]
    );
    println!(
        "  Tests: {} FAIL_TO_PASS",
        instance.fail_to_pass_tests().len()
    );
    println!("  Model: {}", model);

    let workspace = setup_instance_workspace(instance, &opts.output_dir).await?;

    let start = Instant::now();
    let (turns, tokens) = match tokio::time::timeout(
        std::time::Duration::from_secs(opts.timeout_secs),
        run_agent_loop(&llm, instance, &workspace, opts.max_turns),
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            return Ok(InstanceResult {
                instance_id: instance.instance_id.clone(),
                repo: instance.repo.clone(),
                model: model.to_string(),
                resolved: false,
                fail_to_pass_count: instance.fail_to_pass_tests().len(),
                fail_to_pass_passed: 0,
                turns_used: 0,
                tokens_used: 0,
                duration_secs: start.elapsed().as_secs_f64(),
                files_modified: vec![],
                patch: String::new(),
                error: Some(format!("Agent error: {}", e)),
            });
        }
        Err(_) => {
            return Ok(InstanceResult {
                instance_id: instance.instance_id.clone(),
                repo: instance.repo.clone(),
                model: model.to_string(),
                resolved: false,
                fail_to_pass_count: instance.fail_to_pass_tests().len(),
                fail_to_pass_passed: 0,
                turns_used: opts.max_turns,
                tokens_used: 0,
                duration_secs: start.elapsed().as_secs_f64(),
                files_modified: vec![],
                patch: String::new(),
                error: Some(format!("Timeout after {}s", opts.timeout_secs)),
            });
        }
    };
    let duration = start.elapsed().as_secs_f64();

    let (patch, files_modified) = generate_patch(&workspace).await?;

    // Run tests to check if resolved
    let fail_tests = instance.fail_to_pass_tests();
    let repo_config = get_repo_config(&instance.repo);
    let mut passed = 0;
    for test in &fail_tests {
        let normalized = if instance.repo == "django/django" {
            normalize_django_test(test)
        } else {
            test.to_string()
        };
        let test_cmd = format!(
            "{}{}",
            repo_config.env_setup,
            repo_config.test_cmd_template.replace("{test}", &normalized)
        );
        let test_result = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&test_cmd)
            .current_dir(&workspace)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .await;
        if let Ok(output) = test_result {
            if output.status.success() {
                passed += 1;
                println!("  PASS: {}", test);
            } else {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let last = stdout
                    .lines()
                    .rev()
                    .chain(stderr.lines().rev())
                    .find(|l| !l.trim().is_empty() && !l.starts_with("="))
                    .unwrap_or("unknown error");
                println!("  FAIL: {} — {}", test, last);
            }
        } else {
            println!("  ERROR: {} — could not execute test command", test);
        }
    }

    let resolved = passed == fail_tests.len() && !fail_tests.is_empty();
    println!(
        "  Result: {} ({}/{} tests) | {} turns | {:.0}s",
        if resolved { "RESOLVED" } else { "FAILED" },
        passed,
        fail_tests.len(),
        turns,
        duration
    );

    Ok(InstanceResult {
        instance_id: instance.instance_id.clone(),
        repo: instance.repo.clone(),
        model: model.to_string(),
        resolved,
        fail_to_pass_count: fail_tests.len(),
        fail_to_pass_passed: passed,
        turns_used: turns,
        tokens_used: tokens,
        duration_secs: duration,
        files_modified,
        patch,
        error: None,
    })
}

pub async fn run_batch(opts: &SwebenchOpts) -> Result<()> {
    tokio::fs::create_dir_all(&opts.output_dir).await?;
    tokio::fs::create_dir_all(&format!("{}/datasets", opts.output_dir)).await?;

    let instances = load_dataset(opts)?;
    if instances.is_empty() {
        println!("No instances to run.");
        return Ok(());
    }

    let completed = if opts.resume {
        let ids = load_completed_ids(&opts.output_dir);
        if !ids.is_empty() {
            println!("Resuming: {} instances already completed", ids.len());
        }
        ids
    } else {
        std::collections::HashSet::new()
    };

    let total = instances.len();
    let mut resolved_count = 0u32;
    let mut completed_count = 0u32;
    let mut total_duration = 0.0f64;
    let mut total_tokens = 0u64;

    println!("\n========================================");
    println!(" SWE-bench Run: {} instances", total);
    println!("========================================\n");

    for (i, instance) in instances.iter().enumerate() {
        if completed.contains(&instance.instance_id) {
            println!(
                "[{}/{}] SKIP {} (already completed)",
                i + 1,
                total,
                instance.instance_id
            );
            continue;
        }

        println!("\n[{}/{}] {}", i + 1, total, instance.instance_id);

        let result = match run_single(instance, opts).await {
            Ok(r) => r,
            Err(e) => {
                println!("  ERROR: {}", e);
                InstanceResult {
                    instance_id: instance.instance_id.clone(),
                    repo: instance.repo.clone(),
                    model: opts
                        .model_override
                        .clone()
                        .unwrap_or_else(|| "default".into()),
                    resolved: false,
                    fail_to_pass_count: instance.fail_to_pass_tests().len(),
                    fail_to_pass_passed: 0,
                    turns_used: 0,
                    tokens_used: 0,
                    duration_secs: 0.0,
                    files_modified: vec![],
                    patch: String::new(),
                    error: Some(format!("{}", e)),
                }
            }
        };

        if result.resolved {
            resolved_count += 1;
        }
        completed_count += 1;
        total_duration += result.duration_secs;
        total_tokens += result.tokens_used;

        if let Err(e) = append_result(&opts.output_dir, &result) {
            eprintln!("Warning: failed to save result: {}", e);
        }
        if let Err(e) = append_prediction(
            &opts.output_dir,
            &Prediction {
                instance_id: result.instance_id.clone(),
                model_name_or_path: result.model.clone(),
                model_patch: result.patch.clone(),
            },
        ) {
            eprintln!("Warning: failed to save prediction: {}", e);
        }

        let rate = if completed_count > 0 {
            resolved_count as f64 / completed_count as f64 * 100.0
        } else {
            0.0
        };
        println!(
            "\n  Progress: {}/{} completed | {}/{} resolved ({:.1}%) | {:.0}s avg",
            completed_count,
            total,
            resolved_count,
            completed_count,
            rate,
            if completed_count > 0 {
                total_duration / completed_count as f64
            } else {
                0.0
            }
        );
    }

    println!("\n========================================");
    println!(" SWE-bench Run Complete");
    println!("========================================");
    let rate = if completed_count > 0 {
        resolved_count as f64 / completed_count as f64 * 100.0
    } else {
        0.0
    };
    println!(
        "  Resolved: {}/{} ({:.1}%)",
        resolved_count, completed_count, rate
    );
    println!("  Total tokens: {}", total_tokens);
    println!(
        "  Total time: {:.0}s ({:.0}s avg)",
        total_duration,
        if completed_count > 0 {
            total_duration / completed_count as f64
        } else {
            0.0
        }
    );
    println!("  Results: {}/swebench_results.jsonl", opts.output_dir);
    println!("  Predictions: {}/predictions.jsonl", opts.output_dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_test_list_array() {
        let v = json!(["test_foo.py::TestBar", "test_baz.py::TestQux"]);
        let tests = parse_test_list(&v);
        assert_eq!(tests.len(), 2);
    }

    #[test]
    fn test_parse_test_list_string() {
        let v = json!("[\"test_foo.py::TestBar\", \"test_baz.py::TestQux\"]");
        let tests = parse_test_list(&v);
        assert_eq!(tests.len(), 2);
    }

    #[test]
    fn test_parse_test_list_single() {
        let v = json!("test_foo.py::TestBar");
        let tests = parse_test_list(&v);
        assert_eq!(tests.len(), 1);
    }
}
