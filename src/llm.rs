use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;
use std::time::Instant;
use tokio::sync::mpsc;

/// Get the Ollama base URL from OLLAMA_HOST env var, or default to localhost.
/// Supports: "host:port", "http://host:port", or just "host".
pub fn ollama_url() -> String {
    match std::env::var("OLLAMA_HOST") {
        Ok(host) if !host.is_empty() => {
            let host = host.trim_end_matches('/');
            if host.starts_with("http://") || host.starts_with("https://") {
                host.to_string()
            } else {
                format!("http://{}", host)
            }
        }
        _ => "http://localhost:11434".to_string(),
    }
}

/// Events emitted during streaming generation.
#[derive(Debug)]
pub enum StreamEvent {
    /// A chunk of generated text.
    Token(String),
    /// Generation complete, full text included.
    Done(String),
    /// An error occurred.
    Error(String),
    /// CTO tool call started.
    ToolCallStart { name: String, args: String },
    /// CTO tool call result.
    ToolCallResult { name: String, result: String },
    /// CTO agent returned after async task.
    AgentReturn(Box<crate::cto::CtoAgent>),
}

// ── Ollama /api/chat tool calling types ──

/// Chat message for /api/chat (multi-turn with tool support).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OllamaToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Ollama tool definition (JSON Schema).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OllamaTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OllamaToolFunction,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OllamaToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Tool call returned by the model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OllamaToolCall {
    pub function: OllamaToolCallFunction,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OllamaToolCallFunction {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Result of chat_with_tools call.
#[derive(Debug, Clone)]
pub struct ChatToolResponse {
    pub content: String,
    pub tool_calls: Vec<OllamaToolCall>,
}

/// Stats captured from an LLM call for pipeline reports.
#[derive(Debug, Clone)]
pub struct LlmCallStats {
    pub model: String,
    pub duration_secs: f64,
    pub token_count: u64,
    pub tok_per_sec: f64,
    pub output_lines: u64,
}

/// Cloud provider for routing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CloudProvider {
    None,   // Local Ollama
    Claude, // Anthropic API
    Grok,   // xAI API (OpenAI-compatible)
}

/// Unified LLM client — routes to Claude API, Grok API, or Ollama based on model name.
/// Supports both blocking and streaming generation.
#[derive(Clone)]
pub struct LlmClient {
    http: Client,
    claude_key: Option<String>,
    grok_key: Option<String>,
    model: String,
    provider: CloudProvider,
    context_size: u32,
    max_predict: u32,
}

impl LlmClient {
    pub fn new(model: &str) -> Self {
        Self::with_limits(model, 32768, 8192)
    }

    pub fn with_limits(model: &str, context_size: u32, max_predict: u32) -> Self {
        let provider = if model.starts_with("claude-") {
            CloudProvider::Claude
        } else if model.starts_with("grok-") {
            CloudProvider::Grok
        } else {
            CloudProvider::None
        };
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(1800))
                .build()
                .expect("http client"),
            claude_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            grok_key: std::env::var("XAI_API_KEY").ok(),
            model: model.to_string(),
            provider,
            context_size,
            max_predict,
        }
    }

    /// Route to the appropriate cloud/local provider.
    async fn route_generate(&self, role: &str, system: &str, user_prompt: &str) -> Result<String> {
        match self.provider {
            CloudProvider::Claude => {
                if let Some(ref key) = self.claude_key {
                    self.call_claude(key, role, system, user_prompt).await
                } else {
                    eprintln!("   {} Claude model selected but ANTHROPIC_API_KEY not set, falling back to Ollama", role);
                    self.call_ollama(role, system, user_prompt).await
                }
            }
            CloudProvider::Grok => {
                if let Some(ref key) = self.grok_key {
                    self.call_grok(key, role, system, user_prompt).await
                } else {
                    eprintln!(
                        "   {} Grok model selected but XAI_API_KEY not set, falling back to Ollama",
                        role
                    );
                    self.call_ollama(role, system, user_prompt).await
                }
            }
            CloudProvider::None => {
                let ollama_result = self.call_ollama(role, system, user_prompt).await;
                if ollama_result.is_err() {
                    if let Some(ref key) = self.claude_key {
                        eprintln!(
                            "   {} Ollama unavailable, falling back to Claude Opus",
                            role
                        );
                        self.call_claude_fallback(key, role, system, user_prompt)
                            .await
                    } else {
                        ollama_result
                    }
                } else {
                    ollama_result
                }
            }
        }
    }

    /// Generate text (blocking — waits for full response).
    pub async fn generate(&self, role: &str, system: &str, user_prompt: &str) -> Result<String> {
        let start = Instant::now();
        let result = self.route_generate(role, system, user_prompt).await;

        match &result {
            Ok(text) => {
                let dur = start.elapsed();
                let lines = text.lines().count();
                println!("   {} [{} lines, {:.1}s]", role, lines, dur.as_secs_f64());
            }
            Err(e) => {
                eprintln!("   {} FAILED: {}", role, e);
            }
        }
        result
    }

    /// Generate text and return stats for reports.
    pub async fn generate_with_stats(
        &self,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<(String, LlmCallStats)> {
        let start = Instant::now();
        let result = self.route_generate(role, system, user_prompt).await;

        match result {
            Ok(text) => {
                let dur = start.elapsed();
                let lines = text.lines().count() as u64;
                println!("   {} [{} lines, {:.1}s]", role, lines, dur.as_secs_f64());
                let stats = LlmCallStats {
                    model: self.model.clone(),
                    duration_secs: dur.as_secs_f64(),
                    token_count: 0, // non-streaming doesn't count tokens
                    tok_per_sec: 0.0,
                    output_lines: lines,
                };
                Ok((text, stats))
            }
            Err(e) => {
                eprintln!("   {} FAILED: {}", role, e);
                Err(e)
            }
        }
    }

    /// Generate with live streaming and return stats for reports.
    pub async fn generate_live_with_stats(
        &self,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<(String, LlmCallStats)> {
        if self.provider != CloudProvider::None {
            // Cloud models: use generate_live (which now streams), then wrap in stats
            let start = Instant::now();
            let text = self.generate_live(role, system, user_prompt).await?;
            let dur = start.elapsed();
            let lines = text.lines().count() as u64;
            return Ok((
                text,
                LlmCallStats {
                    model: self.model.clone(),
                    duration_secs: dur.as_secs_f64(),
                    token_count: 0,
                    tok_per_sec: 0.0,
                    output_lines: lines,
                },
            ));
        }

        let live_result = self.call_ollama_live(role, system, user_prompt).await;
        if live_result.is_err() {
            if let Some(ref key) = self.claude_key {
                eprintln!(
                    "   {} Ollama unavailable, falling back to Claude Opus",
                    role
                );
                return self
                    .generate_with_stats_claude_fallback(key, role, system, user_prompt)
                    .await;
            }
        }
        let (text, token_count, line_count, dur) = live_result?;
        let tok_per_sec = if dur > 0.0 {
            token_count as f64 / dur
        } else {
            0.0
        };

        let stats = LlmCallStats {
            model: self.model.clone(),
            duration_secs: dur,
            token_count,
            tok_per_sec,
            output_lines: line_count,
        };
        Ok((text, stats))
    }

    /// Internal: Ollama live streaming, returns (text, token_count, line_count, duration_secs).
    async fn call_ollama_live(
        &self,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<(String, u64, u64, f64)> {
        use futures_util::StreamExt;
        use std::io::Write;

        let start = Instant::now();
        println!("   {} -> Ollama live ({})", role, self.model);
        print!("   \x1b[90m");

        let body = serde_json::json!({
            "model": &self.model,
            "system": system,
            "prompt": user_prompt,
            "stream": true,
            "options": { "temperature": 0.0, "num_ctx": self.context_size, "num_predict": self.max_predict }
        });

        let resp = self
            .http
            .post(format!("{}/api/generate", ollama_url()))
            .json(&body)
            .send()
            .await
            .context("Ollama request failed — is `ollama serve` running?")?;

        // Check HTTP status before streaming (model not found returns 404)
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let err_msg = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|j| j["error"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("HTTP {}", status));
            print!("\x1b[0m");
            anyhow::bail!("Ollama error for model '{}': {}", self.model, err_msg);
        }

        let mut full_text = String::new();
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut token_count = 0u64;
        let mut line_count = 0u64;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Stream chunk error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(nl) = buffer.find('\n') {
                let line = buffer[..nl].to_string();
                buffer = buffer[nl + 1..].to_string();

                if line.trim().is_empty() {
                    continue;
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(token) = json["response"].as_str() {
                        if !token.is_empty() {
                            full_text.push_str(token);
                            token_count += 1;
                            print!("{}", token);
                            let _ = std::io::stdout().flush();
                            line_count += token.matches('\n').count() as u64;
                        }
                    }
                    if json["done"].as_bool().unwrap_or(false) {
                        break;
                    }
                }
            }
        }

        let dur = start.elapsed();
        let tok_per_sec = if dur.as_secs_f64() > 0.0 {
            token_count as f64 / dur.as_secs_f64()
        } else {
            0.0
        };

        println!("\x1b[0m");
        println!(
            "   {} [{} lines, {} tokens, {:.1}s, {:.0} tok/s]",
            role,
            line_count,
            token_count,
            dur.as_secs_f64(),
            tok_per_sec
        );

        Ok((full_text, token_count, line_count, dur.as_secs_f64()))
    }

    /// Generate with live token-by-token output to stdout.
    /// Shows the model's thinking process in real-time.
    pub async fn generate_live(
        &self,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<String> {
        use std::io::Write;

        match self.provider {
            CloudProvider::Claude => {
                if let Some(ref key) = self.claude_key {
                    return self.call_claude_live(key, role, system, user_prompt).await;
                }
                return self.generate(role, system, user_prompt).await;
            }
            CloudProvider::Grok => {
                if let Some(ref key) = self.grok_key {
                    return self.call_grok_live(key, role, system, user_prompt).await;
                }
                return self.generate(role, system, user_prompt).await;
            }
            CloudProvider::None => {} // fall through to Ollama live below
        }

        // Test Ollama connectivity first with a quick check
        let ollama_check = self
            .http
            .get(format!("{}/api/tags", ollama_url()))
            .send()
            .await;
        if ollama_check.is_err() {
            if let Some(ref key) = self.claude_key {
                eprintln!(
                    "   {} Ollama unavailable, falling back to Claude Opus",
                    role
                );
                return self
                    .call_claude_fallback(key, role, system, user_prompt)
                    .await;
            }
        }

        let start = Instant::now();
        println!("   {} -> Ollama live ({})", role, self.model);
        print!("   \x1b[90m"); // dim gray for streaming output

        let body = serde_json::json!({
            "model": &self.model,
            "system": system,
            "prompt": user_prompt,
            "stream": true,
            "options": { "temperature": 0.0, "num_ctx": self.context_size, "num_predict": self.max_predict }
        });

        let resp = self
            .http
            .post(format!("{}/api/generate", ollama_url()))
            .json(&body)
            .send()
            .await
            .context("Ollama request failed — is `ollama serve` running?")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let err_msg = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|j| j["error"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("HTTP {}", status));
            print!("\x1b[0m");
            anyhow::bail!("Ollama error for model '{}': {}", self.model, err_msg);
        }

        let mut full_text = String::new();
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut token_count = 0u64;
        let mut line_count = 0u64;

        use futures_util::StreamExt;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Stream chunk error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(nl) = buffer.find('\n') {
                let line = buffer[..nl].to_string();
                buffer = buffer[nl + 1..].to_string();

                if line.trim().is_empty() {
                    continue;
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(token) = json["response"].as_str() {
                        if !token.is_empty() {
                            full_text.push_str(token);
                            token_count += 1;

                            // Print token to stdout in real-time
                            print!("{}", token);
                            let _ = std::io::stdout().flush();

                            // Track newlines for line count
                            line_count += token.matches('\n').count() as u64;
                        }
                    }
                    if json["done"].as_bool().unwrap_or(false) {
                        break;
                    }
                }
            }
        }

        // Reset color and print summary
        let dur = start.elapsed();
        let tok_per_sec = if dur.as_secs_f64() > 0.0 {
            token_count as f64 / dur.as_secs_f64()
        } else {
            0.0
        };

        println!("\x1b[0m"); // reset color
        println!(
            "   {} [{} lines, {} tokens, {:.1}s, {:.0} tok/s]",
            role,
            line_count,
            token_count,
            dur.as_secs_f64(),
            tok_per_sec
        );

        Ok(full_text)
    }

    /// Generate text with streaming — tokens sent via channel as they arrive.
    /// Returns the full accumulated text when done.
    pub async fn generate_streaming(
        &self,
        role: &str,
        system: &str,
        user_prompt: &str,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<String> {
        let start = Instant::now();

        let result = if self.provider != CloudProvider::None {
            // Cloud providers: use non-streaming for now, send as single chunk
            let text = self.generate(role, system, user_prompt).await?;
            let _ = tx.send(StreamEvent::Token(text.clone())).await;
            let _ = tx.send(StreamEvent::Done(text.clone())).await;
            Ok(text)
        } else {
            self.call_ollama_streaming(role, system, user_prompt, &tx)
                .await
        };

        match &result {
            Ok(text) => {
                let dur = start.elapsed();
                println!(
                    "   {} [streamed, {} lines, {:.1}s]",
                    role,
                    text.lines().count(),
                    dur.as_secs_f64()
                );
            }
            Err(e) => {
                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                eprintln!("   {} STREAM FAILED: {}", role, e);
            }
        }
        result
    }

    /// Ollama streaming: parse NDJSON lines as they arrive.
    async fn call_ollama_streaming(
        &self,
        role: &str,
        system: &str,
        user_prompt: &str,
        tx: &mpsc::Sender<StreamEvent>,
    ) -> Result<String> {
        println!("   {} -> Ollama streaming ({})", role, self.model);

        let body = json!({
            "model": &self.model,
            "system": system,
            "prompt": user_prompt,
            "stream": true,
            "options": {
                "temperature": 0.0,
                "num_ctx": self.context_size,
                "num_predict": self.max_predict
            }
        });

        let resp = self
            .http
            .post(format!("{}/api/generate", ollama_url()))
            .json(&body)
            .send()
            .await
            .context("Ollama streaming request failed")?;

        let mut full_text = String::new();
        let mut stream = resp.bytes_stream();

        use futures_util::StreamExt;
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Stream chunk error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete NDJSON lines
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.trim().is_empty() {
                    continue;
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(token) = json["response"].as_str() {
                        if !token.is_empty() {
                            full_text.push_str(token);
                            let _ = tx.send(StreamEvent::Token(token.to_string())).await;
                        }
                    }

                    if json["done"].as_bool().unwrap_or(false) {
                        let _ = tx.send(StreamEvent::Done(full_text.clone())).await;
                        return Ok(full_text);
                    }
                }
            }
        }

        // If we get here without a done signal, send what we have
        let _ = tx.send(StreamEvent::Done(full_text.clone())).await;
        Ok(full_text)
    }

    async fn call_claude(
        &self,
        api_key: &str,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<String> {
        println!("   {} -> Claude ({})", role, self.model);

        let body = json!({
            "model": &self.model,
            "max_tokens": self.max_predict,
            "system": system,
            "messages": [{"role": "user", "content": user_prompt}]
        });

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Claude API request failed")?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            anyhow::bail!(
                "Claude API error ({}): {}",
                status,
                text.chars().take(200).collect::<String>()
            );
        }

        let json: serde_json::Value = serde_json::from_str(&text)?;
        let content = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Log cost from usage data
        let input_tokens = json["usage"]["input_tokens"].as_u64().unwrap_or(0);
        let output_tokens = json["usage"]["output_tokens"].as_u64().unwrap_or(0);
        let _ =
            crate::enterprise::log_cost("mission", &self.model, role, input_tokens, output_tokens);

        Ok(content)
    }

    /// Claude streaming: SSE with content_block_delta events.
    async fn call_claude_live(
        &self,
        api_key: &str,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<String> {
        use futures_util::StreamExt;
        use std::io::Write;

        let start = Instant::now();
        println!("   {} -> Claude live ({})", role, self.model);
        print!("   \x1b[90m");

        let body = json!({
            "model": &self.model,
            "max_tokens": self.max_predict,
            "stream": true,
            "system": system,
            "messages": [{"role": "user", "content": user_prompt}]
        });

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Claude streaming request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            print!("\x1b[0m");
            anyhow::bail!(
                "Claude API error ({}): {}",
                status,
                body.chars().take(200).collect::<String>()
            );
        }

        let mut full_text = String::new();
        let mut token_count = 0u64;
        let mut line_count = 0u64;
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Stream chunk error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(nl) = buffer.find('\n') {
                let line = buffer[..nl].to_string();
                buffer = buffer[nl + 1..].to_string();

                let line = line.trim();
                if !line.starts_with("data: ") {
                    continue;
                }
                let data = &line[6..];
                if data == "[DONE]" {
                    break;
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    // Claude SSE: content_block_delta with delta.text
                    if json["type"].as_str() == Some("content_block_delta") {
                        if let Some(text) = json["delta"]["text"].as_str() {
                            full_text.push_str(text);
                            token_count += 1;
                            line_count += text.matches('\n').count() as u64;
                            print!("{}", text);
                            let _ = std::io::stdout().flush();
                        }
                    }
                    // Claude SSE: message_delta has usage.output_tokens
                    if json["type"].as_str() == Some("message_delta") {
                        output_tokens = json["usage"]["output_tokens"]
                            .as_u64()
                            .unwrap_or(token_count);
                    }
                    // Claude SSE: message_start has usage.input_tokens
                    if json["type"].as_str() == Some("message_start") {
                        input_tokens = json["message"]["usage"]["input_tokens"]
                            .as_u64()
                            .unwrap_or(0);
                    }
                }
            }
        }

        let dur = start.elapsed();
        if output_tokens == 0 {
            output_tokens = token_count;
        }
        let tok_per_sec = if dur.as_secs_f64() > 0.0 {
            output_tokens as f64 / dur.as_secs_f64()
        } else {
            0.0
        };
        println!("\x1b[0m");
        println!(
            "   {} [{} lines, {} tokens, {:.1}s, {:.0} tok/s]",
            role,
            line_count,
            output_tokens,
            dur.as_secs_f64(),
            tok_per_sec
        );

        let _ =
            crate::enterprise::log_cost("mission", &self.model, role, input_tokens, output_tokens);

        Ok(full_text)
    }

    /// Grok streaming: OpenAI-compatible SSE with choices[0].delta.content.
    async fn call_grok_live(
        &self,
        api_key: &str,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<String> {
        use futures_util::StreamExt;
        use std::io::Write;

        let start = Instant::now();
        println!("   {} -> Grok live ({})", role, self.model);
        print!("   \x1b[90m");

        let body = json!({
            "model": &self.model,
            "max_tokens": self.max_predict,
            "temperature": 0.0,
            "stream": true,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user_prompt}
            ]
        });

        let resp = self
            .http
            .post("https://api.x.ai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Grok streaming request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            print!("\x1b[0m");
            anyhow::bail!(
                "Grok API error ({}): {}",
                status,
                body.chars().take(200).collect::<String>()
            );
        }

        let mut full_text = String::new();
        let mut token_count = 0u64;
        let mut line_count = 0u64;
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Stream chunk error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(nl) = buffer.find('\n') {
                let line = buffer[..nl].to_string();
                buffer = buffer[nl + 1..].to_string();

                let line = line.trim();
                if !line.starts_with("data: ") {
                    continue;
                }
                let data = &line[6..];
                if data == "[DONE]" {
                    break;
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(text) = json["choices"][0]["delta"]["content"].as_str() {
                        full_text.push_str(text);
                        token_count += 1;
                        line_count += text.matches('\n').count() as u64;
                        print!("{}", text);
                        let _ = std::io::stdout().flush();
                    }
                }
            }
        }

        let dur = start.elapsed();
        let tok_per_sec = if dur.as_secs_f64() > 0.0 {
            token_count as f64 / dur.as_secs_f64()
        } else {
            0.0
        };
        println!("\x1b[0m");
        println!(
            "   {} [{} lines, {} tokens, {:.1}s, {:.0} tok/s]",
            role,
            line_count,
            token_count,
            dur.as_secs_f64(),
            tok_per_sec
        );

        // Estimate input tokens (~4 chars/token), output tokens from stream count
        let est_input = (system.len() + user_prompt.len()) as u64 / 4;
        let _ = crate::enterprise::log_cost("mission", &self.model, role, est_input, token_count);

        Ok(full_text)
    }

    async fn call_grok(
        &self,
        api_key: &str,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<String> {
        println!("   {} -> Grok ({})", role, self.model);

        let body = json!({
            "model": &self.model,
            "max_tokens": self.max_predict,
            "temperature": 0.0,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user_prompt}
            ]
        });

        let resp = self
            .http
            .post("https://api.x.ai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Grok API request failed")?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            anyhow::bail!(
                "Grok API error ({}): {}",
                status,
                text.chars().take(200).collect::<String>()
            );
        }

        let json: serde_json::Value = serde_json::from_str(&text)?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Log cost from usage data
        let input_tokens = json["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        let output_tokens = json["usage"]["completion_tokens"].as_u64().unwrap_or(0);
        let _ =
            crate::enterprise::log_cost("mission", &self.model, role, input_tokens, output_tokens);

        Ok(content)
    }

    async fn call_ollama(&self, role: &str, system: &str, user_prompt: &str) -> Result<String> {
        println!("   {} -> Ollama ({})", role, self.model);

        let body = json!({
            "model": &self.model,
            "system": system,
            "prompt": user_prompt,
            "stream": false,
            "options": {
                "temperature": 0.0,
                "num_ctx": self.context_size,
                "num_predict": self.max_predict
            }
        });

        let resp = self
            .http
            .post(format!("{}/api/generate", ollama_url()))
            .json(&body)
            .send()
            .await
            .context("Ollama request failed — is `ollama serve` running?")?;

        let status = resp.status();
        let text = resp.text().await?;
        let json: serde_json::Value =
            serde_json::from_str(&text).context("Ollama returned invalid JSON")?;

        // Check for Ollama error (model not found, pull required, etc.)
        if !status.is_success() || json.get("error").is_some() {
            let err_msg = json["error"].as_str().unwrap_or("unknown error");
            anyhow::bail!("Ollama error for model '{}': {}", self.model, err_msg);
        }

        let response = json["response"].as_str().unwrap_or("").to_string();

        Ok(response)
    }

    /// Fallback: call Claude Opus when Ollama is unavailable.
    /// Uses claude-opus-4-6 with the same system/user prompt.
    async fn call_claude_fallback(
        &self,
        api_key: &str,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<String> {
        println!("   {} -> Claude Opus (fallback from {})", role, self.model);
        self.call_claude(api_key, role, system, user_prompt).await
    }

    /// Fallback with stats for live streaming functions.
    async fn generate_with_stats_claude_fallback(
        &self,
        api_key: &str,
        role: &str,
        system: &str,
        user_prompt: &str,
    ) -> Result<(String, LlmCallStats)> {
        let start = Instant::now();
        let text = self
            .call_claude_fallback(api_key, role, system, user_prompt)
            .await?;
        let dur = start.elapsed();
        let lines = text.lines().count() as u64;
        println!("   {} [{} lines, {:.1}s]", role, lines, dur.as_secs_f64());
        let stats = LlmCallStats {
            model: "claude-opus-4-6 (fallback)".to_string(),
            duration_secs: dur.as_secs_f64(),
            token_count: 0,
            tok_per_sec: 0.0,
            output_lines: lines,
        };
        Ok((text, stats))
    }

    // ── Chat with tools (Ollama /api/chat, Claude tool_use, Grok functions) ──

    /// Chat with native tool calling. Returns assistant content + tool calls.
    pub async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[OllamaTool],
    ) -> Result<ChatToolResponse> {
        match self.provider {
            CloudProvider::None => self.chat_tools_ollama(messages, tools).await,
            CloudProvider::Claude => self.chat_tools_claude(messages, tools).await,
            CloudProvider::Grok => self.chat_tools_grok(messages, tools).await,
        }
    }

    async fn chat_tools_ollama(
        &self,
        messages: &[ChatMessage],
        tools: &[OllamaTool],
    ) -> Result<ChatToolResponse> {
        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let mut msg = json!({ "role": m.role, "content": m.content });
                if let Some(ref tc) = m.tool_calls {
                    msg["tool_calls"] = serde_json::to_value(tc).unwrap_or_default();
                }
                if let Some(ref id) = m.tool_call_id {
                    msg["tool_call_id"] = json!(id);
                }
                msg
            })
            .collect();

        let body = json!({
            "model": &self.model,
            "messages": msgs,
            "tools": tools,
            "stream": false,
            "options": {
                "temperature": 0.0,
                "num_ctx": self.context_size,
                "num_predict": self.max_predict
            }
        });

        let url = format!("{}/api/chat", ollama_url());
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Ollama chat_with_tools request failed")?;
        let data: serde_json::Value = resp
            .json()
            .await
            .context("Ollama chat_with_tools parse failed")?;

        let content = data["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let tool_calls: Vec<OllamaToolCall> = data["message"]["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| serde_json::from_value(tc.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        // Fallback: if no native tool calls, check for TOOL_CALL: text pattern
        if tool_calls.is_empty() {
            if let Some(tc) = extract_text_tool_call(&content) {
                return Ok(ChatToolResponse {
                    content: content.clone(),
                    tool_calls: vec![tc],
                });
            }
        }

        Ok(ChatToolResponse {
            content,
            tool_calls,
        })
    }

    async fn chat_tools_claude(
        &self,
        messages: &[ChatMessage],
        tools: &[OllamaTool],
    ) -> Result<ChatToolResponse> {
        let api_key = self
            .claude_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY required for Claude tool calling"))?;

        // Convert messages (skip system, extract separately)
        let system_msg = messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let claude_msgs: Vec<serde_json::Value> = messages.iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let role = if m.role == "tool" { "user" } else { &m.role };
                let content = if m.role == "tool" {
                    json!([{ "type": "tool_result", "tool_use_id": m.tool_call_id.as_deref().unwrap_or(""), "content": m.content }])
                } else {
                    json!(m.content)
                };
                json!({ "role": role, "content": content })
            })
            .collect();

        // Convert tools to Claude format
        let claude_tools: Vec<serde_json::Value> = tools.iter().map(|t| {
            json!({ "name": t.function.name, "description": t.function.description, "input_schema": t.function.parameters })
        }).collect();

        let body = json!({
            "model": &self.model,
            "max_tokens": self.max_predict,
            "system": system_msg,
            "messages": claude_msgs,
            "tools": claude_tools,
        });

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Claude chat_with_tools failed")?;
        let data: serde_json::Value = resp.json().await?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();

        if let Some(blocks) = data["content"].as_array() {
            for block in blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        content.push_str(block["text"].as_str().unwrap_or(""));
                    }
                    Some("tool_use") => {
                        tool_calls.push(OllamaToolCall {
                            function: OllamaToolCallFunction {
                                name: block["name"].as_str().unwrap_or("").to_string(),
                                arguments: block["input"].clone(),
                            },
                        });
                    }
                    _ => {}
                }
            }
        }

        Ok(ChatToolResponse {
            content,
            tool_calls,
        })
    }

    async fn chat_tools_grok(
        &self,
        messages: &[ChatMessage],
        tools: &[OllamaTool],
    ) -> Result<ChatToolResponse> {
        let api_key = self
            .grok_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("XAI_API_KEY required for Grok tool calling"))?;

        let oai_msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect();

        let oai_tools: Vec<serde_json::Value> = tools.iter().map(|t| {
            json!({ "type": "function", "function": { "name": t.function.name, "description": t.function.description, "parameters": t.function.parameters } })
        }).collect();

        let body = json!({
            "model": &self.model,
            "messages": oai_msgs,
            "tools": oai_tools,
            "max_tokens": self.max_predict,
        });

        let resp = self
            .http
            .post("https://api.x.ai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Grok chat_with_tools failed")?;
        let data: serde_json::Value = resp.json().await?;

        let content = data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let tool_calls: Vec<OllamaToolCall> = data["choices"][0]["message"]["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let name = tc["function"]["name"].as_str()?.to_string();
                        let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                        let arguments = serde_json::from_str(args_str).unwrap_or(json!({}));
                        Some(OllamaToolCall {
                            function: OllamaToolCallFunction { name, arguments },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(ChatToolResponse {
            content,
            tool_calls,
        })
    }
}

/// Fallback: extract tool call from text pattern "TOOL_CALL: name args".
fn extract_text_tool_call(text: &str) -> Option<OllamaToolCall> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("TOOL_CALL:") {
            let rest = trimmed.strip_prefix("TOOL_CALL:")?.trim();
            let (name, args) = rest.split_once(' ').unwrap_or((rest, ""));
            return Some(OllamaToolCall {
                function: OllamaToolCallFunction {
                    name: name.to_string(),
                    arguments: json!({ "input": args.trim() }),
                },
            });
        }
    }
    None
}

/// Extract clean code from an LLM response that may contain markdown fences.
pub fn extract_code(raw: &str, language: &str) -> String {
    // Try ```language\n...\n```
    let fence = format!("```{}", language);
    if let Some(start) = raw.find(&fence) {
        let after_fence = &raw[start + fence.len()..];
        let code_start = if after_fence.starts_with('\n') { 1 } else { 0 };
        if let Some(end) = after_fence[code_start..].find("```") {
            return after_fence[code_start..code_start + end].trim().to_string();
        }
    }

    // Try generic ```\n...\n```
    if let Some(start) = raw.find("```\n") {
        let after = &raw[start + 4..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    // No fences found — return as-is (trimmed)
    raw.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_code_with_python_fence() {
        let raw = "Here is the code:\n```python\ndef hello():\n    print('hi')\n```\nDone!";
        assert_eq!(extract_code(raw, "python"), "def hello():\n    print('hi')");
    }

    #[test]
    fn test_extract_code_generic_fence() {
        let raw = "```\nconst x = 1;\n```";
        assert_eq!(extract_code(raw, "javascript"), "const x = 1;");
    }

    #[test]
    fn test_extract_code_no_fence() {
        let raw = "def hello():\n    print('hi')";
        assert_eq!(extract_code(raw, "python"), raw.trim());
    }

    #[test]
    fn test_stream_event_variants() {
        let token = StreamEvent::Token("hello".into());
        let done = StreamEvent::Done("full text".into());
        let err = StreamEvent::Error("oops".into());
        // Just verify they construct without panic
        match token {
            StreamEvent::Token(t) => assert_eq!(t, "hello"),
            other => unreachable!("unexpected variant: {:?}", other),
        }
        match done {
            StreamEvent::Done(t) => assert_eq!(t, "full text"),
            other => unreachable!("unexpected variant: {:?}", other),
        }
        match err {
            StreamEvent::Error(t) => assert_eq!(t, "oops"),
            other => unreachable!("unexpected variant: {:?}", other),
        }
    }
}
