use crate::llm::LlmClient;
use crate::model_config::ModelConfig;
/// Swarm mode: planner → coder → QA iteration.
/// Runs multiple iterations of code generation and picks the best version.
/// Each iteration: plan → code → validate → QA review.
/// Best version selected by validation pass + QA score.
use anyhow::Result;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct SwarmVersion {
    pub iteration: u32,
    pub plan: String,
    pub code: String,
    pub qa_feedback: String,
    pub qa_score: u32,
    pub validated: bool,
    pub validation_output: String,
}

pub struct SwarmOpts {
    pub iterations: u32,
    pub output_dir: String,
    pub language: String,
}

impl Default for SwarmOpts {
    fn default() -> Self {
        Self {
            iterations: 3,
            output_dir: "output/swarm".into(),
            language: "python".into(),
        }
    }
}

pub async fn run_swarm(prompt: &str, config: &ModelConfig, opts: &SwarmOpts) -> Result<()> {
    let start = Instant::now();
    let mut versions: Vec<SwarmVersion> = Vec::new();

    println!("Swarm Mode: {} iterations", opts.iterations);
    println!("  Coder: {}", config.coder.model);
    println!(
        "  Prompt: {}\n",
        if prompt.len() > 80 {
            &prompt[..80]
        } else {
            prompt
        }
    );

    let planner = LlmClient::with_limits(
        &config.architect.model,
        config.architect.context_size(),
        2048,
    );
    let coder = LlmClient::with_limits(
        &config.coder.model,
        config.coder.context_size(),
        config.coder.max_predict(),
    );
    let qa = LlmClient::with_limits(&config.critique.model, config.critique.context_size(), 1024);

    // Setup output directory
    tokio::fs::create_dir_all(&opts.output_dir).await?;

    for iter in 1..=opts.iterations {
        let iter_start = Instant::now();
        let prev = versions.last();

        // ── Phase 1: Planner ──
        let plan_system = "You are a senior engineer creating an implementation plan. Be specific about function names, signatures, data structures, and edge cases. Output a numbered list, no code.";
        let plan_user = if let Some(prev) = prev {
            format!(
                "TASK: {}\nLANGUAGE: {}\n\nPREVIOUS ATTEMPT (iteration {}) scored {}/10:\nQA FEEDBACK:\n{}\n\nCreate an improved plan that addresses the feedback.",
                prompt, opts.language, prev.iteration, prev.qa_score, prev.qa_feedback
            )
        } else {
            format!(
                "TASK: {}\nLANGUAGE: {}\n\nCreate a precise implementation plan.",
                prompt, opts.language
            )
        };

        let plan = planner
            .generate("architect", plan_system, &plan_user)
            .await
            .unwrap_or_else(|e| format!("Planning failed: {}", e));

        // ── Phase 2: Coder ──
        let code_system = format!(
            "You are an elite AI coder. Follow the implementation plan exactly.\n\
             RULES:\n\
             1. Write COMPLETE, WORKING code. No placeholders, no TODOs.\n\
             2. Include ALL imports at the top.\n\
             3. Handle all edge cases from the plan.\n\
             4. Output ONLY code in a ```{}``` code block.",
            opts.language
        );
        let code_user = format!(
            "TASK: {}\n\nIMPLEMENTATION PLAN:\n{}\n\nWrite the complete implementation.",
            prompt, plan
        );

        let code_resp = coder
            .generate_live("coder", &code_system, &code_user)
            .await
            .unwrap_or_else(|e| format!("// Code generation failed: {}", e));
        let code = extract_code(&code_resp, &opts.language);

        // Write code to file
        let file_ext = match opts.language.as_str() {
            "python" => "py",
            "javascript" | "js" => "js",
            "typescript" | "ts" => "ts",
            "rust" => "rs",
            "go" => "go",
            "cpp" | "c++" => "cpp",
            _ => "txt",
        };
        let file_name = format!("{}/main.{}", opts.output_dir, file_ext);
        tokio::fs::write(&file_name, &code).await?;

        // ── Phase 3: Validate ──
        let (validated, validation_output) = run_validation(&file_name, &opts.language).await;

        // ── Phase 4: Mini-QA ──
        let qa_system = "Review this code against the task requirements. Score 1-10. Be STRICT. Respond with ONLY JSON: {\"score\": N, \"feedback\": \"...\"}";
        let code_snippet = if code.len() > 8000 {
            &code[..8000]
        } else {
            &code
        };
        let validation_status = if validated {
            "PASSED".to_string()
        } else {
            format!(
                "FAILED: {}",
                &validation_output[..validation_output.len().min(300)]
            )
        };
        let qa_user = format!(
            "TASK: {}\nVALIDATION: {}\n\nCODE:\n```{}\n{}\n```\n\nScore and list specific issues.",
            prompt, validation_status, opts.language, code_snippet
        );
        let qa_resp = qa
            .generate("critique", qa_system, &qa_user)
            .await
            .unwrap_or_else(|_| "{\"score\": 5, \"feedback\": \"QA failed\"}".into());
        let (qa_feedback, qa_score) = parse_qa_response(&qa_resp);

        let iter_time = iter_start.elapsed().as_secs_f64();
        println!(
            "  [iter {}/{}] score {}/10 {} {:.1}s",
            iter,
            opts.iterations,
            qa_score,
            if validated { "PASS" } else { "FAIL" },
            iter_time
        );

        let version = SwarmVersion {
            iteration: iter,
            plan,
            code,
            qa_feedback,
            qa_score,
            validated,
            validation_output,
        };

        // Early exit on high score + validation pass
        if qa_score >= 9 && validated {
            println!("  Early exit: iter {} scored {}/10 PASS", iter, qa_score);
            versions.push(version);
            break;
        }

        versions.push(version);
    }

    // ── Select best version ──
    let best_idx = select_best(&versions);
    let best = &versions[best_idx];

    // Write best version to disk
    let file_ext = match opts.language.as_str() {
        "python" => "py",
        "javascript" | "js" => "js",
        "typescript" | "ts" => "ts",
        "rust" => "rs",
        "go" => "go",
        "cpp" | "c++" => "cpp",
        _ => "txt",
    };
    let file_name = format!("{}/main.{}", opts.output_dir, file_ext);
    tokio::fs::write(&file_name, &best.code).await?;

    let duration = start.elapsed().as_secs_f64();
    println!(
        "\nSwarm complete: best=iter{} (score {}/10 {}) | {:.1}s total",
        best.iteration,
        best.qa_score,
        if best.validated { "PASS" } else { "FAIL" },
        duration
    );
    println!("Output: {}", file_name);

    Ok(())
}

fn select_best(versions: &[SwarmVersion]) -> usize {
    versions
        .iter()
        .enumerate()
        .max_by_key(|(_, v)| (v.validated as u32 * 100 + v.qa_score, v.iteration))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn extract_code(response: &str, _language: &str) -> String {
    let mut in_block = false;
    let mut code = String::new();
    for line in response.lines() {
        if line.trim_start().starts_with("```") {
            if in_block {
                break;
            }
            in_block = true;
            continue;
        }
        if in_block {
            code.push_str(line);
            code.push('\n');
        }
    }
    if code.is_empty() {
        response.to_string()
    } else {
        code
    }
}

async fn run_validation(file_path: &str, language: &str) -> (bool, String) {
    let cmd = match language {
        "python" => format!(
            "python3 -c \"import ast; ast.parse(open('{}').read()); print('SYNTAX OK')\"",
            file_path
        ),
        "rust" => format!("rustc --edition 2021 {} -o /dev/null 2>&1", file_path),
        "cpp" | "c++" => format!("c++ -std=c++17 -fsyntax-only {} 2>&1", file_path),
        "go" => format!("go vet {} 2>&1", file_path),
        _ => format!("test -f {} && echo 'FILE EXISTS'", file_path),
    };

    match tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .await
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            (output.status.success(), format!("{}{}", stdout, stderr))
        }
        Err(e) => (false, format!("Validation error: {}", e)),
    }
}

fn parse_qa_response(text: &str) -> (String, u32) {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text[start..=end]) {
                let score = parsed["score"].as_u64().unwrap_or(5) as u32;
                let feedback = parsed["feedback"]
                    .as_str()
                    .unwrap_or("No feedback")
                    .to_string();
                return (feedback, score.min(10));
            }
        }
    }
    (text.to_string(), 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_qa_valid() {
        let resp = r#"{"score": 7, "feedback": "Missing error handling"}"#;
        let (feedback, score) = parse_qa_response(resp);
        assert_eq!(score, 7);
        assert!(feedback.contains("error handling"));
    }

    #[test]
    fn parse_qa_garbage() {
        let (_, score) = parse_qa_response("not json");
        assert_eq!(score, 5);
    }

    #[test]
    fn select_best_prefers_validated() {
        let versions = vec![
            SwarmVersion {
                iteration: 1,
                plan: String::new(),
                code: "a".into(),
                qa_feedback: String::new(),
                qa_score: 9,
                validated: false,
                validation_output: String::new(),
            },
            SwarmVersion {
                iteration: 2,
                plan: String::new(),
                code: "b".into(),
                qa_feedback: String::new(),
                qa_score: 6,
                validated: true,
                validation_output: String::new(),
            },
        ];
        assert_eq!(select_best(&versions), 1);
    }
}
