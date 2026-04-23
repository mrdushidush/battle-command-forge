//! Context compaction — prevents token limit crashes.
//! Ported from battleclaw-v2 context_compact.rs.

const MAX_CONTEXT_CHARS: usize = 120_000; // ~30K tokens
const COMPACT_THRESHOLD: f64 = 0.95;
const TARGET_AFTER_COMPACT: f64 = 0.60;

#[derive(Debug, Clone)]
pub struct ContextMessage {
    pub role: String,
    pub content: String,
    pub compactable: bool,
}

pub struct ContextManager {
    messages: Vec<ContextMessage>,
    total_chars: usize,
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            total_chars: 0,
        }
    }

    pub fn add(&mut self, role: &str, content: &str, compactable: bool) {
        self.total_chars += content.len();
        self.messages.push(ContextMessage {
            role: role.to_string(),
            content: content.to_string(),
            compactable,
        });
        if self.usage_ratio() >= COMPACT_THRESHOLD {
            self.compact();
        }
    }

    pub fn usage_ratio(&self) -> f64 {
        self.total_chars as f64 / MAX_CONTEXT_CHARS as f64
    }

    pub fn usage_percent(&self) -> u32 {
        (self.usage_ratio() * 100.0) as u32
    }

    pub fn to_string(&self) -> String {
        self.messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn compact(&mut self) {
        let target_chars = (MAX_CONTEXT_CHARS as f64 * TARGET_AFTER_COMPACT) as usize;

        // Phase 1: Truncate long compactable messages
        for msg in &mut self.messages {
            if msg.compactable && msg.content.len() > 500 {
                let end = msg.content.len().min(200);
                let truncated = msg.content.len() - end;
                msg.content = format!("{}...[truncated {} chars]", &msg.content[..end], truncated);
            }
        }
        self.recalc();
        if self.total_chars <= target_chars {
            return;
        }

        // Phase 2: Drop old compactable messages (keep last 20)
        if self.messages.len() > 20 {
            let to_remove = self.messages.len() - 20;
            let summary = format!(
                "[Compacted {} earlier messages at {}% capacity]",
                to_remove,
                self.usage_percent()
            );
            self.messages.drain(..to_remove);
            self.messages.insert(
                0,
                ContextMessage {
                    role: "system".into(),
                    content: summary,
                    compactable: false,
                },
            );
        }
        self.recalc();
    }

    fn recalc(&mut self) {
        self.total_chars = self.messages.iter().map(|m| m.content.len()).sum();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_add() {
        let mut cm = ContextManager::new();
        cm.add("user", "hello", false);
        assert_eq!(cm.len(), 1);
        assert_eq!(cm.total_chars, 5);
    }

    #[test]
    fn test_auto_compact() {
        let mut cm = ContextManager::new();
        // Add enough to trigger compaction
        for _ in 0..200 {
            cm.add("user", &"x".repeat(1000), true);
        }
        // Should have compacted
        assert!(cm.total_chars < MAX_CONTEXT_CHARS);
    }

    #[test]
    fn test_usage_ratio() {
        let mut cm = ContextManager::new();
        cm.add("user", &"a".repeat(60_000), false);
        assert!((cm.usage_ratio() - 0.5).abs() < 0.01);
    }
}
