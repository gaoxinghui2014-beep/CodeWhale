//! 轻量级 token 估算器。
//!
//! 基于启发式规则的 token 数估算（不依赖 tokenizer 库）。
//! 对英文文本，每 4 字符约 1 token（GPT 风格 tokenizer 的粗略近似）。

/// 估算文本的 token 数。
///
/// 使用简单的启发式规则：
/// - 英文/ASCII 文本：~4 字符/token
/// - CJK 字符：~1 字符/token
/// - 空白和标点不计入
pub fn estimate_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }

    let mut tokens: f64 = 0.0;
    let mut ascii_run: u32 = 0;

    for ch in text.chars() {
        if ch.is_whitespace() || ch == '\n' || ch == '\r' {
            // 空白也计入 token（实际 tokenizer 会计数）
            tokens += 1.0;
            ascii_run = 0;
        } else if ch.is_ascii() {
            ascii_run += 1;
            if ascii_run >= 4 {
                tokens += 1.0;
                ascii_run = 0;
            }
        } else {
            // CJK 或其他多字节字符：约 1-2 token/字符
            if ascii_run > 0 {
                tokens += 1.0; // 之前累积的 ASCII 序列
                ascii_run = 0;
            }
            tokens += 1.5;
        }
    }

    // 剩余的 ASCII 序列
    if ascii_run > 0 {
        tokens += 1.0;
    }

    tokens.ceil() as u32
}

/// 估算节省的 token 比例。
pub fn estimate_savings(original: u32, compressed: u32) -> f64 {
    if original == 0 {
        return 0.0;
    }
    let saved = original.saturating_sub(compressed) as f64;
    saved / original as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_returns_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn short_ascii() {
        let tokens = estimate_tokens("hello world");
        assert!(tokens > 0 && tokens < 10);
    }

    #[test]
    fn long_ascii() {
        let text = "this is a longer piece of text that should produce more tokens";
        let tokens = estimate_tokens(text);
        assert!(tokens > 5);
    }

    #[test]
    fn savings_calculation() {
        let ratio = estimate_savings(1000, 500);
        assert!((ratio - 0.5).abs() < 0.01);
    }

    #[test]
    fn no_savings_when_original_is_zero() {
        assert_eq!(estimate_savings(0, 0), 0.0);
    }
}
