/// Campbell's Complexity Theory — Dual Assessment Router.
///
/// Ported from battleclaw-v2 router.rs. Dual-factor scoring:
/// 1. Rule-based keyword + structural analysis (fast, deterministic)
/// 2. AI-assisted scoring via configured complexity model (nuanced)
/// Blended with disagreement handling.
use crate::llm::LlmClient;

// ─── Types ───

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tier {
    Trivial,  // C1-C3
    Moderate, // C4-C6
    Complex,  // C7-C8
    Expert,   // C9-C10
}

impl Tier {
    pub fn label(&self) -> &'static str {
        match self {
            Tier::Trivial => "C1-C3 Trivial",
            Tier::Moderate => "C4-C6 Moderate",
            Tier::Complex => "C7-C8 Complex",
            Tier::Expert => "C9-C10 Expert",
        }
    }

    pub fn from_score(score: f32) -> Self {
        if score >= 9.0 {
            Tier::Expert
        } else if score >= 7.0 {
            Tier::Complex
        } else if score >= 4.0 {
            Tier::Moderate
        } else {
            Tier::Trivial
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComplexitySource {
    Rules, // Rule-based only
    Ai,    // AI assessment only
    Dual,  // Combined rule + AI
}

impl std::fmt::Display for ComplexitySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComplexitySource::Rules => write!(f, "rules"),
            ComplexitySource::Ai => write!(f, "ai"),
            ComplexitySource::Dual => write!(f, "dual"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoutingResult {
    pub complexity: u32,
    pub source: ComplexitySource,
    pub tier: Tier,
    pub reasoning: String,
    pub rule_score: f32,
    pub ai_score: Option<f32>,
}

// ─── Public API ───

/// Fast rule-based complexity assessment (no LLM needed).
pub fn assess_complexity(prompt: &str) -> Tier {
    let score = rule_score(prompt);
    Tier::from_score(score as f32)
}

/// Full dual-factor assessment: rules + AI, returns detailed result.
pub async fn assess_complexity_dual(prompt: &str, llm: &LlmClient) -> RoutingResult {
    let rules = rule_score(prompt);
    let ai = ai_complexity_score(prompt, llm).await;

    let (final_score, source, reasoning) = match ai {
        Some(ai_score) => {
            let diff = ai_score as i32 - rules as i32;
            if diff >= 2 {
                // AI sees more complexity — trust AI
                (
                    ai_score,
                    ComplexitySource::Ai,
                    format!(
                        "Rules: C{}, AI: C{} (using AI — semantic complexity)",
                        rules, ai_score
                    ),
                )
            } else if diff <= -2 {
                // Rules see more complexity — weighted average favoring rules
                let avg = (rules as f64 * 0.6 + ai_score as f64 * 0.4).round() as u32;
                (
                    avg,
                    ComplexitySource::Dual,
                    format!(
                        "Rules: C{}, AI: C{} (weighted avg, rules dominant)",
                        rules, ai_score
                    ),
                )
            } else {
                // Agreement — average, floor at rule score
                let avg = ((rules + ai_score) / 2).max(rules);
                (
                    avg,
                    ComplexitySource::Dual,
                    format!("Rules: C{}, AI: C{} (agreement)", rules, ai_score),
                )
            }
        }
        None => (
            rules,
            ComplexitySource::Rules,
            format!("Rule-based only (AI unavailable). C{}", rules),
        ),
    };

    let final_score = final_score.clamp(1, 10);
    let tier = Tier::from_score(final_score as f32);

    println!(
        "   Rules=C{} AI={} Final=C{} => {}",
        rules,
        ai.map(|s| format!("C{}", s))
            .unwrap_or_else(|| "N/A".to_string()),
        final_score,
        tier.label()
    );

    RoutingResult {
        complexity: final_score,
        source,
        tier,
        reasoning,
        rule_score: rules as f32,
        ai_score: ai.map(|s| s as f32),
    }
}

// ─── Rule-based scoring (ported from v2 assess_complexity_rules) ───

fn rule_score(prompt: &str) -> u32 {
    let text = prompt.to_lowercase();
    let word_count = prompt.split_whitespace().count();
    let mut score: f64 = 1.0; // base score (matched to v2)

    // ── 1. STRUCTURAL COMPLEXITY ──

    // Count explicit steps
    let steps = text.matches("step ").count();
    if steps >= 7 {
        score += 3.0;
    } else if steps >= 5 {
        score += 2.0;
    } else if steps >= 3 {
        score += 1.0;
    }

    // Count file extensions mentioned
    let file_exts = [
        ".py", ".ts", ".js", ".tsx", ".jsx", ".json", ".css", ".html", ".go", ".php",
    ];
    let file_count: usize = file_exts.iter().filter(|ext| text.contains(*ext)).count();
    if file_count >= 3 {
        score += 2.0;
    } else if file_count >= 2 {
        score += 1.0;
    }

    // Count function/class definitions expected
    let def_keywords = ["function", "class", "def ", "interface", "struct"];
    let def_count: usize = def_keywords.iter().map(|k| text.matches(k).count()).sum();
    if def_count >= 3 {
        score += 1.0;
    }

    // ── 2. SEMANTIC COMPLEXITY (4-tier keywords, matched to v2) ──

    let trivial = [
        "simple",
        "basic",
        "single",
        "just",
        "only",
        "straightforward",
    ];
    let moderate = [
        "handle",
        "validate",
        "check",
        "multiple",
        "combine",
        "integrate",
        "parse",
        "convert",
        "transform",
        "edge case",
        "error handling",
    ];
    let high = [
        "refactor",
        "optimize",
        "async",
        "concurrent",
        "parallel",
        "nested",
        "recursive",
        "complex",
        "algorithm",
        "data structure",
        "database",
        "api",
        "service",
        "module",
        "component",
        "cache",
        "lru",
        "linked list",
        "hash map",
        "tree",
        "graph",
        "queue",
        "stack",
        "heap",
        "binary",
        "sorting",
        "searching",
        "o(1)",
        "o(n)",
        "o(log",
        "time complexity",
    ];
    let extreme = [
        "architect",
        "design system",
        "framework",
        "infrastructure",
        "distributed",
        "microservice",
        "migration",
        "legacy",
        "security",
        "authentication",
        "authorization",
        "real-time",
        "multiple files",
        "full application",
        "project",
    ];

    let mut max_tier: f64 = 0.0;

    let extreme_hits = extreme.iter().filter(|k| text.contains(*k)).count();
    if extreme_hits >= 2 {
        max_tier = max_tier.max(4.0);
    } else if extreme_hits == 1 {
        max_tier = max_tier.max(3.0);
    }

    let high_hits = high.iter().filter(|k| text.contains(*k)).count();
    if high_hits >= 3 {
        max_tier = max_tier.max(3.0);
    } else if high_hits >= 1 {
        max_tier = max_tier.max(2.0);
    }

    let mod_hits = moderate.iter().filter(|k| text.contains(*k)).count();
    if mod_hits >= 2 {
        max_tier = max_tier.max(2.0);
    } else if mod_hits >= 1 {
        max_tier = max_tier.max(1.0);
    }

    let trivial_hits = trivial.iter().filter(|k| text.contains(*k)).count();
    if trivial_hits >= 2 && max_tier <= 1.0 {
        score -= 0.5;
    }

    score += max_tier;

    // ── 3. LENGTH FACTOR ──
    if word_count > 100 {
        score += 2.0;
    } else if word_count > 50 {
        score += 1.0;
    } else if word_count < 10 {
        score -= 0.5;
    }

    // ── 4. LANGUAGE MODIFIER ──
    let lang = detect_language_hint(&text);
    match lang {
        "go" | "rust" => score += 0.5,
        "typescript" => score += 0.5,
        _ => {}
    }

    // Web project boost (HTML/CSS usually need more files/context)
    if text.contains("html") || text.contains("landing page") || text.contains("website") {
        score = score.max(7.0);
    }

    (score.round() as u32).clamp(1, 10)
}

/// Quick language hint from prompt text (for scoring only).
fn detect_language_hint(lower: &str) -> &str {
    if lower.contains("rust") || lower.contains("cargo") {
        "rust"
    } else if lower.contains("golang") || lower.contains(" go ") {
        "go"
    } else if lower.contains("typescript") || lower.contains("next.js") {
        "typescript"
    } else {
        "python"
    }
}

// ─── AI complexity scoring ───

async fn ai_complexity_score(prompt: &str, llm: &LlmClient) -> Option<u32> {
    let system = "/no_think\nYou are a task complexity assessor for a coding agent system.\n\
        Rate the complexity of this programming task on a scale of 1-10:\n\
        - 1-3: Simple (single function, basic logic, no dependencies)\n\
        - 4-5: Medium (multiple functions, some validation, basic tests)\n\
        - 6-7: Moderate (multiple files, external APIs, error handling)\n\
        - 8-9: Complex (architecture design, multiple systems, advanced patterns)\n\
        - 10: Very Complex (distributed systems, complex algorithms, extensive testing)\n\n\
        Respond with ONLY a JSON object:\n\
        {\"complexity\": <number>, \"reasoning\": \"<1 sentence>\"}";

    let response = llm.generate("  AI-SCORE", system, prompt).await.ok()?;

    // Try JSON parse first
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response[start..=end]) {
                if let Some(c) = json["complexity"].as_u64() {
                    return Some((c as u32).clamp(1, 10));
                }
            }
        }
    }

    // Fallback: parse bare number
    for word in response.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| !c.is_numeric() && c != '.');
        if let Ok(n) = cleaned.parse::<f32>() {
            if (1.0..=10.0).contains(&n) {
                return Some(n.round() as u32);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Rule-based scoring ───

    #[test]
    fn test_trivial() {
        assert_eq!(assess_complexity("print hello world"), Tier::Trivial);
    }

    #[test]
    fn test_trivial_simple() {
        let c = rule_score("Simple basic function that prints a number");
        assert!(c <= 3, "simple function should be C1-C3, got C{}", c);
    }

    #[test]
    fn test_moderate() {
        let c = rule_score(
            "Build a REST API for a todo app with database integration and form validation",
        );
        assert!(
            (3..=6).contains(&c),
            "REST API todo app should be C3-C6, got C{}",
            c
        );
    }

    #[test]
    fn test_moderate_validation() {
        let c =
            rule_score("Write a function that validates email addresses and handles edge cases");
        assert!(
            (3..=6).contains(&c),
            "validation task should be C3-C6, got C{}",
            c
        );
    }

    #[test]
    fn test_complex() {
        let c = rule_score("Build a production-ready FastAPI user authentication endpoint with JWT, rate limiting, and security headers");
        assert!(c >= 5, "auth endpoint should be C5+, got C{}", c);
        assert!(c <= 8, "auth endpoint should be <=C8, got C{}", c);
    }

    #[test]
    fn test_complex_data_structure() {
        let c = rule_score(
            "Implement an LRU cache with O(1) get and put using a hash map and linked list",
        );
        assert!(c >= 3, "LRU cache should be C3+ (rule-based), got C{}", c);
        assert!(c <= 7, "LRU cache should be <=C7, got C{}", c);
    }

    #[test]
    fn test_expert() {
        let c = rule_score("Build a distributed consensus algorithm for a microservice infrastructure with real-time replication");
        assert!(
            c >= 5,
            "distributed system should be C5+ (rule-based), got C{}",
            c
        );
    }

    #[test]
    fn test_extreme_architecture() {
        let c = rule_score("Design a distributed microservice authentication system with real-time WebSocket notifications and multiple files for the full application");
        assert!(
            c >= 5,
            "distributed system should be C5+ (rule-based), got C{}",
            c
        );
    }

    #[test]
    fn test_web_project_boost() {
        let c = rule_score("Create an HTML landing page with sections");
        assert!(
            c >= 7,
            "HTML landing page should get web boost to C7+, got C{}",
            c
        );
    }

    #[test]
    fn test_keyword_score() {
        assert!(rule_score("hello world") <= 3);
        assert!(rule_score("Build a REST API for a todo app with database") >= 3);
        assert!(
            rule_score("Build a JWT authentication system with rate limiting and security") >= 4
        );
        assert!(
            rule_score("Build a distributed compiler with multiple files for the full application")
                >= 5
        );
    }

    #[test]
    fn test_tier_from_score() {
        assert_eq!(Tier::from_score(2.0), Tier::Trivial);
        assert_eq!(Tier::from_score(5.0), Tier::Moderate);
        assert_eq!(Tier::from_score(7.5), Tier::Complex);
        assert_eq!(Tier::from_score(9.5), Tier::Expert);
    }

    #[test]
    fn test_complexity_always_in_range() {
        assert!(rule_score("") >= 1);
        assert!(rule_score("") <= 10);
        assert!(rule_score("x") >= 1);
        assert!(rule_score(&"word ".repeat(200)) <= 10);
    }

    #[test]
    fn test_language_modifier() {
        let py = rule_score("Build a module");
        let go = rule_score("Build a golang module");
        assert!(go >= py, "Go should be >= Python complexity");
    }

    #[test]
    fn test_length_factor() {
        let short = rule_score("add numbers");
        let long = rule_score(&format!(
            "Build a system that {}",
            "handles complex logic and ".repeat(10)
        ));
        assert!(long > short, "longer prompt should score higher");
    }
}
