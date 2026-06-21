//! Code generation compaction strategy for AI coding agents.
//!
//! Inspired by the [ponytail](https://github.com/DietrichGebert/ponytail)
//! "lazy senior developer" model, this module injects a decision ladder and
//! compaction rules into the system prompt to make the model generate less
//! code without sacrificing correctness or safety.
//!
//! # Strategy
//!
//! Before writing any code, the model is instructed to stop at the first
//! rung that holds:
//!
//! 1. Does this need to exist? (YAGNI) → skip it
//! 2. Standard library does it? → use stdlib
//! 3. Native platform feature? → use platform
//! 4. Already-installed dependency? → use existing dep
//! 5. One line? → one line
//! 6. Only then: minimum code that works

use serde::{Deserialize, Serialize};

/// Intensity level for code compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CompactLevel {
    /// Build what's asked, but name the lazier alternative. User decides.
    Lite,
    /// The ladder enforced: stdlib and native first. Shortest diff wins.
    /// This is the default.
    #[default]
    Full,
    /// YAGNI extremist. Deletion before addition. Challenge requirements.
    Ultra,
}

/// Configuration for code generation compaction.
///
/// When enabled (default), the ponytail-inspired decision ladder rules are
/// injected into the system prompt. The model is instructed to prefer
/// standard library, native platform features, one-liners, and existing
/// dependencies before writing custom code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeCompactionConfig {
    /// Enable or disable code compaction. Default: true.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Intensity level. Default: [`CompactLevel::Full`].
    #[serde(default)]
    pub level: CompactLevel,
}

fn default_enabled() -> bool {
    true
}

impl Default for CodeCompactionConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            level: CompactLevel::default(),
        }
    }
}

// ── Instruction blocks ───────────────────────────────────────────────────────

/// Core decision ladder shared by all levels.
const LADDER: &str = "\
## Code Compaction — Decision Ladder\n\n\
Before writing any code, stop at the first rung that holds:\n\n\
1. **Does this need to exist at all?** Speculative need = skip it. (YAGNI)\n\
2. **Stdlib does it?** Use the standard library — never hand-roll what's built in.\n\
3. **Native platform feature covers it?** `<input type=\"date\">` over a picker lib, \
CSS over JS, DB constraint over app code.\n\
4. **Already-installed dependency solves it?** Use it. Never add a new dep for \
what a few lines can do.\n\
5. **Can it be one line?** One line. Not golfed — one clear line.\n\
6. **Only then:** the minimum code that works.";

/// Shared safety rules applied at all levels.
const SAFETY_GUARDS: &str = "\
## Safety Guards (never simplify away)\n\n\
- Input validation at trust boundaries (path traversal, SQL injection, etc.)\n\
- Error handling that prevents data loss\n\
- Security measures (auth, encryption, access control)\n\
- Accessibility basics (aria labels, keyboard navigation)\n\
- Anything the user explicitly asked to keep\n\
- Hardware calibration knobs (real clocks drift, real sensors read off)\n\n\
Lazy code without a check is unfinished: non-trivial logic leaves ONE runnable \
check (assert-based self-check or one small test file; no frameworks). Trivial \
one-liners need no test.";

/// Deletion-first and simplicity rules.
const SIMPLICITY_RULES: &str = "\
## Simplicity Rules\n\n\
- No unrequested abstractions: no interface with one implementation, no factory \
for one product\n\
- No boilerplate, no scaffolding \"for later\" — later can scaffold for itself\n\
- Deletion over addition. Boring over clever.\n\
- Fewest files possible. Shortest working diff wins.\n\
- Complex request? Ship the lazy version and question it: \"Did X; Y covers it. \
Need full X? Say so.\"\n\
- Between two same-size stdlib options, pick the one correct on edge cases.\n\
- Mark deliberate simplifications with a `// ponytail:` comment: name the \
ceiling and upgrade path.\n\
  Example: `// ponytail: global lock, per-account locks if throughput matters`";

// ── Level-specific instruction blocks ────────────────────────────────────────

/// Lite level: build what's asked, name the lazier alternative.
const LITE_INSTRUCTIONS: &str = "\
## Code Compaction Level: Lite\n\n\
Build what the user asked for, but always name the lazier alternative in one \
line. Let the user decide.\n\n\
The shorter path: prefer stdlib, native platform features, and existing \
dependencies over custom code. When the lazier version is clearly sufficient, \
mention it alongside your implementation.";

/// Full level: the ladder enforced.
pub const FULL_INSTRUCTIONS: &str = "\
## Code Compaction Level: Full\n\n\
Write the laziest solution that actually works. Before every code block, run the \
decision ladder: YAGNI → stdlib → native → existing dep → one-line → minimum. \
The first lazy solution that works is the right one.";

/// Ultra level: YAGNI extremist.
const ULTRA_INSTRUCTIONS: &str = "\
## Code Compaction Level: Ultra\n\n\
YAGNI extremist. Deletion before addition. Ship the one-liner and challenge \
the rest of the requirement in the same breath. No cache until a profiler says \
so. No abstraction until a second caller exists. The best code is the code \
never written.";

impl CodeCompactionConfig {
    /// Return the full system-prompt instruction block, or `None` if disabled.
    #[must_use]
    pub fn instruction_block(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }

        let intro = match self.level {
            CompactLevel::Lite => LITE_INSTRUCTIONS,
            CompactLevel::Full => FULL_INSTRUCTIONS,
            CompactLevel::Ultra => ULTRA_INSTRUCTIONS,
        };

        Some(format!(
            "{intro}\n\n{LADDER}\n\n{SIMPLICITY_RULES}\n\n{SAFETY_GUARDS}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_enabled_full() {
        let cfg = CodeCompactionConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.level, CompactLevel::Full);
    }

    #[test]
    fn disabled_returns_none() {
        let cfg = CodeCompactionConfig {
            enabled: false,
            level: CompactLevel::Full,
        };
        assert!(cfg.instruction_block().is_none());
    }

    #[test]
    fn enabled_returns_some() {
        let cfg = CodeCompactionConfig::default();
        let block = cfg.instruction_block().unwrap();
        assert!(block.contains("Decision Ladder"));
        assert!(block.contains("Safety Guards"));
        assert!(block.contains("Simplicity Rules"));
    }

    #[test]
    fn each_level_produces_block() {
        for level in [CompactLevel::Lite, CompactLevel::Full, CompactLevel::Ultra] {
            let cfg = CodeCompactionConfig {
                enabled: true,
                level,
            };
            let block = cfg.instruction_block().unwrap();
            assert!(block.contains("Decision Ladder"));
            assert!(!block.is_empty());
        }
    }

    #[test]
    fn lite_names_alternative() {
        let cfg = CodeCompactionConfig {
            enabled: true,
            level: CompactLevel::Lite,
        };
        let block = cfg.instruction_block().unwrap();
        assert!(block.contains("Lite"));
    }

    #[test]
    fn ultra_challenges_requirements() {
        let cfg = CodeCompactionConfig {
            enabled: true,
            level: CompactLevel::Ultra,
        };
        let block = cfg.instruction_block().unwrap();
        assert!(block.contains("Ultra"));
        assert!(block.contains("YAGNI extremist"));
    }
}
