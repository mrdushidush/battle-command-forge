/// Per-role model configuration with preset/TOML/env/CLI resolution.
///
/// Ported from battleclaw-v2 model_config.rs, adapted for forge's 9-stage pipeline.
/// Resolution order (highest priority last): preset → env → TOML → CLI.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ─── Model Provider ───

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelProvider {
    Local, // Ollama
    Cloud, // Anthropic API
}

impl std::fmt::Display for ModelProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelProvider::Local => write!(f, "local"),
            ModelProvider::Cloud => write!(f, "cloud"),
        }
    }
}

// ─── Role Config ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleConfig {
    pub model: String,
    pub provider: ModelProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_predict: Option<u32>,
}

impl RoleConfig {
    pub fn local(model: &str) -> Self {
        Self {
            model: model.to_string(),
            provider: ModelProvider::Local,
            context_size: None,
            max_predict: None,
        }
    }

    pub fn local_with_limits(model: &str, ctx: u32, predict: u32) -> Self {
        Self {
            model: model.to_string(),
            provider: ModelProvider::Local,
            context_size: Some(ctx),
            max_predict: Some(predict),
        }
    }

    pub fn cloud(model: &str) -> Self {
        Self {
            model: model.to_string(),
            provider: ModelProvider::Cloud,
            context_size: None,
            max_predict: None,
        }
    }

    pub fn cloud_with_limits(model: &str, ctx: u32, predict: u32) -> Self {
        Self {
            model: model.to_string(),
            provider: ModelProvider::Cloud,
            context_size: Some(ctx),
            max_predict: Some(predict),
        }
    }

    pub fn context_size(&self) -> u32 {
        self.context_size.unwrap_or(32768)
    }

    pub fn max_predict(&self) -> u32 {
        self.max_predict.unwrap_or(8192)
    }
}

// ─── Preset ───

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Fast,
    Balanced,
    Premium,
}

impl std::fmt::Display for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Preset::Fast => write!(f, "fast"),
            Preset::Balanced => write!(f, "balanced"),
            Preset::Premium => write!(f, "premium"),
        }
    }
}

impl std::str::FromStr for Preset {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "fast" => Ok(Preset::Fast),
            "balanced" => Ok(Preset::Balanced),
            "premium" => Ok(Preset::Premium),
            _ => Err(anyhow::anyhow!(
                "Unknown preset '{}'. Use: fast, balanced, premium",
                s
            )),
        }
    }
}

// ─── Model Config ───

/// Per-role model assignments for the 9-stage pipeline.
///
/// Pipeline roles:
///   architect  — Stage 2: spec + file manifest + TDD plan
///   tester     — Stage 3: write test suite before implementation
///   coder      — Stage 4: implement against tests
///   security   — Stage 6: OWASP review
///   critique   — Stage 7: 5-in-1 scoring (DEV/ARCH/TEST/SEC/DOCS)
///   cto        — Stage 8: mission-level coherence
///   complexity — Router: AI complexity scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub preset: Preset,
    pub architect: RoleConfig,
    pub tester: RoleConfig,
    pub coder: RoleConfig,
    pub fix_coder: RoleConfig,
    pub security: RoleConfig,
    pub critique: RoleConfig,
    pub cto: RoleConfig,
    pub complexity: RoleConfig,
}

impl ModelConfig {
    /// Build from preset defaults.
    pub fn from_preset(preset: Preset) -> Self {
        match preset {
            Preset::Fast => Self {
                preset,
                architect: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 8192),
                tester: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 16384),
                coder: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 16384),
                fix_coder: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 16384),
                security: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 1024),
                critique: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 1024),
                cto: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 1024),
                complexity: RoleConfig::local("qwen3.5:4b-q8_0"),
            },
            Preset::Balanced => Self {
                preset,
                architect: RoleConfig::local_with_limits("qwen2.5-coder:32b", 32768, 8192),
                tester: RoleConfig::local_with_limits("qwen2.5-coder:32b", 32768, 16384),
                coder: RoleConfig::local_with_limits("qwen2.5-coder:32b", 32768, 16384),
                fix_coder: RoleConfig::local_with_limits("qwen2.5-coder:32b", 32768, 16384),
                security: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 1024),
                critique: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 1024),
                cto: RoleConfig::local_with_limits("qwen2.5-coder:7b", 32768, 1024),
                complexity: RoleConfig::local("qwen3.5:4b-q8_0"),
            },
            Preset::Premium => Self {
                preset,
                architect: RoleConfig::local_with_limits("qwen2.5-coder:32b", 32768, 4096),
                tester: RoleConfig::cloud_with_limits("claude-opus-4-6", 200000, 8192),
                coder: RoleConfig::local_with_limits("qwen3-coder-next:q8_0", 65536, 32768),
                fix_coder: RoleConfig::cloud_with_limits("claude-sonnet-4-6", 200000, 16384),
                security: RoleConfig::local_with_limits("qwen3-coder:30b-a3b-q8_0", 65536, 1024),
                critique: RoleConfig::local_with_limits("qwen3-coder:30b-a3b-q8_0", 65536, 1024),
                cto: RoleConfig::cloud_with_limits("claude-sonnet-4-6", 200000, 1024),
                complexity: RoleConfig::local_with_limits("qwen3-coder:30b-a3b-q8_0", 32768, 1024),
            },
        }
    }

    /// Merge overrides from `.battlecommand/models.toml` (if it exists).
    pub fn merge_toml(mut self, workspace: &str) -> Self {
        let path = format!("{}/.battlecommand/models.toml", workspace);
        if !Path::new(&path).exists() {
            return self;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[model_config] Failed to read {}: {}", path, e);
                return self;
            }
        };

        let toml_val: TomlConfig = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[model_config] Failed to parse {}: {}", path, e);
                return self;
            }
        };

        // If toml specifies a different preset, rebuild from that preset first
        if let Some(preset_str) = &toml_val.preset {
            if let Ok(preset) = preset_str.parse::<Preset>() {
                if preset != self.preset {
                    self = Self::from_preset(preset);
                }
            }
        }

        // Apply per-role overrides
        if let Some(r) = toml_val.architect {
            apply_role_override(&mut self.architect, r);
        }
        if let Some(r) = toml_val.tester {
            apply_role_override(&mut self.tester, r);
        }
        if let Some(r) = toml_val.coder {
            apply_role_override(&mut self.coder, r);
        }
        if let Some(r) = toml_val.fix_coder {
            apply_role_override(&mut self.fix_coder, r);
        }
        if let Some(r) = toml_val.security {
            apply_role_override(&mut self.security, r);
        }
        if let Some(r) = toml_val.critique {
            apply_role_override(&mut self.critique, r);
        }
        if let Some(r) = toml_val.cto {
            apply_role_override(&mut self.cto, r);
        }
        if let Some(r) = toml_val.complexity {
            apply_role_override(&mut self.complexity, r);
        }

        println!("[CONFIG] Loaded model overrides from {}", path);
        self
    }

    /// Merge from environment variables.
    pub fn merge_env(mut self) -> Self {
        if let Ok(v) = std::env::var("ARCHITECT_MODEL") {
            self.architect.model = v.clone();
            self.architect.provider = infer_provider(&v);
        }
        if let Ok(v) = std::env::var("TESTER_MODEL") {
            self.tester.model = v.clone();
            self.tester.provider = infer_provider(&v);
        }
        if let Ok(v) = std::env::var("CODER_MODEL") {
            self.coder.model = v.clone();
            self.coder.provider = infer_provider(&v);
        }
        if let Ok(v) = std::env::var("FIX_CODER_MODEL") {
            self.fix_coder.model = v.clone();
            self.fix_coder.provider = infer_provider(&v);
        }
        if let Ok(v) = std::env::var("SECURITY_MODEL") {
            self.security.model = v.clone();
            self.security.provider = infer_provider(&v);
        }
        if let Ok(v) = std::env::var("CRITIQUE_MODEL") {
            self.critique.model = v.clone();
            self.critique.provider = infer_provider(&v);
        }
        if let Ok(v) = std::env::var("CTO_MODEL") {
            self.cto.model = v.clone();
            self.cto.provider = infer_provider(&v);
        }
        if let Ok(v) = std::env::var("COMPLEXITY_MODEL") {
            self.complexity.model = v.clone();
            self.complexity.provider = infer_provider(&v);
        }
        // Legacy: OLLAMA_MODEL sets coder
        if let Ok(v) = std::env::var("OLLAMA_MODEL") {
            self.coder.model = v;
            self.coder.provider = ModelProvider::Local;
        }
        // Convenience: REVIEWER_MODEL sets security+critique+cto
        if let Ok(v) = std::env::var("REVIEWER_MODEL") {
            let provider = infer_provider(&v);
            self.security.model = v.clone();
            self.security.provider = provider;
            self.critique.model = v.clone();
            self.critique.provider = provider;
            self.cto.model = v;
            self.cto.provider = provider;
        }
        self
    }

    /// Merge CLI flag overrides (highest priority).
    pub fn merge_cli(
        mut self,
        architect: Option<&str>,
        tester: Option<&str>,
        coder: Option<&str>,
        reviewer: Option<&str>,
    ) -> Self {
        if let Some(m) = architect {
            self.architect.model = m.to_string();
            self.architect.provider = infer_provider(m);
        }
        if let Some(m) = tester {
            self.tester.model = m.to_string();
            self.tester.provider = infer_provider(m);
        }
        if let Some(m) = coder {
            self.coder.model = m.to_string();
            self.coder.provider = infer_provider(m);
        }
        if let Some(m) = reviewer {
            let provider = infer_provider(m);
            self.security.model = m.to_string();
            self.security.provider = provider;
            self.critique.model = m.to_string();
            self.critique.provider = provider;
            self.cto.model = m.to_string();
            self.cto.provider = provider;
        }
        self
    }

    /// Full resolution: preset → env → TOML → CLI.
    pub fn resolve(
        preset: Preset,
        workspace: &str,
        architect: Option<&str>,
        tester: Option<&str>,
        coder: Option<&str>,
        reviewer: Option<&str>,
    ) -> Self {
        Self::from_preset(preset)
            .merge_env()
            .merge_toml(workspace)
            .merge_cli(architect, tester, coder, reviewer)
    }

    /// Generate a default models.toml content.
    pub fn generate_default_toml() -> String {
        r#"# BattleCommand Forge — Model Configuration
# Presets: fast, balanced, premium
preset = "premium"

# Premium dream team: Opus tester, local 80B coder, Sonnet fixer/CTO
# Cost: ~$0.30/mission (Opus tester + Sonnet fixes). C7+ auto-upgrades coder to Sonnet.
# Per-role overrides (uncomment to customize)

# [architect]
# model = "qwen2.5-coder:32b"           # Local 32B — concise specs, no overengineering
# context_size = 32768
# max_predict = 4096

# [tester]
# model = "claude-opus-4-6"             # Opus writes correct test fixtures (~$0.20)
# context_size = 200000
# max_predict = 8192

# [coder]
# model = "qwen3-coder-next:q8_0"       # Local 80B, single-shot generation
# context_size = 65536
# max_predict = 32768

# [fix_coder]
# model = "claude-sonnet-4-6"           # Sonnet for surgical fixes (~$0.05-0.10)
# context_size = 200000
# max_predict = 16384

# [security]
# model = "qwen3-coder:30b-a3b-q8_0"   # Most honest scorer
# context_size = 65536
# max_predict = 1024

# [critique]
# model = "qwen3-coder:30b-a3b-q8_0"   # DEV:3 SEC:1 for bad code
# context_size = 65536
# max_predict = 1024

# [cto]
# model = "claude-sonnet-4-6"           # Fast coherence checks (~$0.05)
# context_size = 200000
# max_predict = 1024
"#
        .to_string()
    }

    /// Print resolved config summary.
    pub fn print_summary(&self) {
        println!("Model Configuration (preset: {})", self.preset);
        println!("{:-<60}", "");
        println!(
            "  Architect:   {:<35} ({})",
            self.architect.model, self.architect.provider
        );
        println!(
            "  Tester:      {:<35} ({})",
            self.tester.model, self.tester.provider
        );
        println!(
            "  Coder:       {:<35} ({})",
            self.coder.model, self.coder.provider
        );
        if self.fix_coder.model != self.coder.model {
            println!(
                "  Fix Coder:   {:<35} ({})",
                self.fix_coder.model, self.fix_coder.provider
            );
        }
        println!(
            "  Security:    {:<35} ({})",
            self.security.model, self.security.provider
        );
        println!(
            "  Critique:    {:<35} ({})",
            self.critique.model, self.critique.provider
        );
        println!(
            "  CTO:         {:<35} ({})",
            self.cto.model, self.cto.provider
        );
        println!(
            "  Complexity:  {:<35} ({})",
            self.complexity.model, self.complexity.provider
        );
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self::from_preset(Preset::Premium)
    }
}

// ─── TOML Schema ───

#[derive(Debug, Deserialize)]
struct TomlConfig {
    preset: Option<String>,
    architect: Option<TomlRoleOverride>,
    tester: Option<TomlRoleOverride>,
    coder: Option<TomlRoleOverride>,
    fix_coder: Option<TomlRoleOverride>,
    security: Option<TomlRoleOverride>,
    critique: Option<TomlRoleOverride>,
    cto: Option<TomlRoleOverride>,
    complexity: Option<TomlRoleOverride>,
}

#[derive(Debug, Deserialize)]
struct TomlRoleOverride {
    model: Option<String>,
    provider: Option<ModelProvider>,
    context_size: Option<u32>,
    max_predict: Option<u32>,
}

fn apply_role_override(role: &mut RoleConfig, ov: TomlRoleOverride) {
    if let Some(m) = ov.model {
        // Auto-infer provider from model name unless explicitly set
        if ov.provider.is_none() {
            role.provider = infer_provider(&m);
        }
        role.model = m;
    }
    if let Some(p) = ov.provider {
        role.provider = p;
    }
    if let Some(c) = ov.context_size {
        role.context_size = Some(c);
    }
    if let Some(p) = ov.max_predict {
        role.max_predict = Some(p);
    }
}

/// Infer provider from model name.
fn infer_provider(model: &str) -> ModelProvider {
    if model.starts_with("claude-") || model.starts_with("grok-") {
        ModelProvider::Cloud
    } else {
        ModelProvider::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_fast() {
        let cfg = ModelConfig::from_preset(Preset::Fast);
        assert_eq!(cfg.architect.model, "qwen2.5-coder:7b");
        assert_eq!(cfg.coder.model, "qwen2.5-coder:7b");
    }

    #[test]
    fn preset_balanced() {
        let cfg = ModelConfig::from_preset(Preset::Balanced);
        assert_eq!(cfg.architect.model, "qwen2.5-coder:32b");
        assert_eq!(cfg.coder.model, "qwen2.5-coder:32b");
    }

    #[test]
    fn preset_premium() {
        let cfg = ModelConfig::from_preset(Preset::Premium);
        assert_eq!(cfg.architect.model, "qwen2.5-coder:32b");
        assert_eq!(cfg.tester.model, "claude-opus-4-6");
        assert_eq!(cfg.coder.model, "qwen3-coder-next:q8_0");
        assert_eq!(cfg.fix_coder.model, "claude-sonnet-4-6");
        assert_eq!(cfg.security.model, "qwen3-coder:30b-a3b-q8_0");
        assert_eq!(cfg.cto.model, "claude-sonnet-4-6");
    }

    #[test]
    fn cli_overrides() {
        let cfg = ModelConfig::from_preset(Preset::Premium).merge_cli(
            Some("nemotron-3-super"),
            None,
            Some("nemotron"),
            None,
        );
        assert_eq!(cfg.architect.model, "nemotron-3-super");
        assert_eq!(cfg.coder.model, "nemotron");
        // Unchanged
        assert_eq!(cfg.tester.model, "claude-opus-4-6");
    }

    #[test]
    fn reviewer_override_sets_all_three() {
        let cfg = ModelConfig::from_preset(Preset::Premium).merge_cli(
            None,
            None,
            None,
            Some("nemotron-3-nano"),
        );
        assert_eq!(cfg.security.model, "nemotron-3-nano");
        assert_eq!(cfg.critique.model, "nemotron-3-nano");
        assert_eq!(cfg.cto.model, "nemotron-3-nano");
    }

    #[test]
    fn infer_provider_cloud() {
        assert_eq!(infer_provider("claude-sonnet-4-6"), ModelProvider::Cloud);
        assert_eq!(infer_provider("grok-3"), ModelProvider::Cloud);
        assert_eq!(infer_provider("qwen3.5:35b-a3b"), ModelProvider::Local);
    }

    #[test]
    fn preset_parse() {
        assert_eq!("fast".parse::<Preset>().unwrap(), Preset::Fast);
        assert_eq!("balanced".parse::<Preset>().unwrap(), Preset::Balanced);
        assert_eq!("premium".parse::<Preset>().unwrap(), Preset::Premium);
        assert!("invalid".parse::<Preset>().is_err());
    }

    #[test]
    fn default_is_premium() {
        let cfg = ModelConfig::default();
        assert_eq!(cfg.preset, Preset::Premium);
    }

    #[test]
    fn role_config_defaults() {
        let r = RoleConfig::local("test-model");
        assert_eq!(r.context_size(), 32768);
        assert_eq!(r.max_predict(), 8192);

        let r = RoleConfig::local_with_limits("test-model", 16384, 4096);
        assert_eq!(r.context_size(), 16384);
        assert_eq!(r.max_predict(), 4096);
    }
}
