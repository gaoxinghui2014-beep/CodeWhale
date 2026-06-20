//! 文本压缩器 — 对纯文本做截断 + 摘要。
//!
//! 这是内容类型检测失败时的后备压缩器，以及对于大块纯文本的简单截断。

use crate::{ContentType, compressors::Compressor};

/// 文本压缩器的配置。
#[derive(Debug, Clone)]
pub struct TextConfig {
    /// 最大保留字节数。
    pub max_bytes: usize,
    /// 是否保留首尾（各 50% 预算）。
    pub keep_head_tail: bool,
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            max_bytes: 16384, // 16 KiB
            keep_head_tail: true,
        }
    }
}

/// 纯文本压缩器。
pub struct TextCompressor {
    config: TextConfig,
}

impl TextCompressor {
    pub fn new(config: TextConfig) -> Self {
        Self { config }
    }
}

impl Default for TextCompressor {
    fn default() -> Self {
        Self::new(TextConfig::default())
    }
}

impl Compressor for TextCompressor {
    fn name(&self) -> &'static str {
        "text"
    }

    fn content_type(&self) -> ContentType {
        ContentType::Text
    }

    fn compress(&self, content: &str, _tool_name: &str) -> String {
        if content.len() <= self.config.max_bytes {
            return content.to_string();
        }

        if self.config.keep_head_tail {
            let half = self.config.max_bytes / 2;
            let head = find_char_boundary(content, half);
            let tail_start = find_char_boundary_rev(content, content.len().saturating_sub(half));
            let omitted = tail_start - head;

            let head_part = &content[..head];
            let tail_part = &content[tail_start..];

            format!(
                "{head_part}\n\n[… {omitted} bytes omitted …]\n\n{tail_part}"
            )
        } else {
            let end = find_char_boundary(content, self.config.max_bytes - 128);
            let omitted = content.len() - end;
            let head = &content[..end];
            format!("{head}\n\n[… {omitted} bytes omitted …]")
        }
    }
}

/// 在合法 UTF-8 边界处截断。
fn find_char_boundary(s: &str, target: usize) -> usize {
    let mut pos = target.min(s.len());
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// 从末尾向前找合法 UTF-8 边界。
fn find_char_boundary_rev(s: &str, target: usize) -> usize {
    let mut pos = target.min(s.len());
    while pos < s.len() && !s.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_unchanged() {
        let compressor = TextCompressor::default();
        let content = "hello world";
        let result = compressor.compress(content, "");
        assert_eq!(result, content);
    }

    #[test]
    fn long_text_truncated() {
        let compressor = TextCompressor::new(TextConfig {
            max_bytes: 100,
            keep_head_tail: true,
        });
        let content = "a".repeat(500);
        let result = compressor.compress(&content, "");
        assert!(result.len() < 300);
        assert!(result.contains("omitted"));
    }

    #[test]
    fn preserves_head_and_tail() {
        let compressor = TextCompressor::new(TextConfig {
            max_bytes: 200,
            keep_head_tail: true,
        });
        let content = "START\n".to_string() + &"m".repeat(1000) + "\nEND";
        let result = compressor.compress(&content, "");
        assert!(result.contains("START"));
        assert!(result.contains("END"));
    }

    #[test]
    fn head_only_mode() {
        let compressor = TextCompressor::new(TextConfig {
            max_bytes: 100,
            keep_head_tail: false,
        });
        let content = "BEGIN\n".to_string() + &"x".repeat(500) + "\nFINISH";
        let result = compressor.compress(&content, "");
        assert!(result.contains("BEGIN"));
        assert!(!result.contains("FINISH"));
    }

    #[test]
    fn char_boundary_respect_utf8() {
        // 在汉字边界处不截断
        let s = "你好世界hello";
        let pos = find_char_boundary(s, 7);
        assert!(s.is_char_boundary(pos));
    }
}
