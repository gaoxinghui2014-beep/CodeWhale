//! 工具结果预算截断（Tool-result budget）。
//!
//! 参考 openhuman-main 的 tool_result_budget 模块设计，在工具结果
//! 进入对话历史之前按字节预算截断大型输出。这是最便宜的分层减少阶段，
//! 因为操作针对的是尚未发送给 LLM 的新字节 —— 不会破坏 KV 缓存前缀。
//!
//! 截断后的内容包含一个人类和模型都可识别的标记：
//! `[… N bytes truncated by tool_result_budget …]`
//!
//! 当模型需要完整输出时，可以通过重新运行工具并指定更窄的查询来获取。

use std::fmt::Write as _;

/// 默认每个工具结果的字节预算（16 KiB）。
/// 与 openhuman-main 的 DEFAULT_TOOL_RESULT_BUDGET_BYTES 保持一致。
pub const DEFAULT_TOOL_RESULT_BUDGET_BYTES: usize = 16 * 1024;

/// 为截断标记保留的尾部字节数。
/// 实际头部容量为 `budget - TRAILER_RESERVED`。
#[allow(dead_code)]
const TRAILER_RESERVED: usize = 256;

/// 预算应用的结果，用于追踪日志。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetOutcome {
    /// 原始内容的字节长度。
    pub original_bytes: usize,
    /// 返回内容的字节长度（`== original_bytes` 当内容在预算内时）。
    pub final_bytes: usize,
    /// 内容是否被截断。
    pub truncated: bool,
}

impl BudgetOutcome {
    #[allow(dead_code)]
    pub fn unchanged(len: usize) -> Self {
        Self {
            original_bytes: len,
            final_bytes: len,
            truncated: false,
        }
    }
}

/// 对 `content` 应用工具结果预算。
///
/// 如果 `content` 长度不超过 `budget_bytes`，则原样返回。
/// 否则返回截断后的前缀，并附加人类可读的标记：
/// `\n\n[… N bytes truncated by tool_result_budget — re-run with a narrower query to see the rest …]`
///
/// 截断边界始终在合法的 UTF-8 字符边界处。
#[allow(dead_code)]
pub fn apply_tool_result_budget(content: &str, budget_bytes: usize) -> (String, BudgetOutcome) {
    let original_bytes = content.len();
    if budget_bytes == 0 || original_bytes <= budget_bytes {
        return (
            content.to_string(),
            BudgetOutcome::unchanged(original_bytes),
        );
    }

    // 为尾部标记保留空间。如果预算小于保留空间，我们仍然会输出标记；
    // 唯一保证是最终字符串比原始字符串短。
    let head_capacity = budget_bytes.saturating_sub(TRAILER_RESERVED).max(1);

    // 按字符索引向前找到不超过 head_capacity 的截断点。
    let mut cut = floor_char_boundary(content, head_capacity);

    // 极短内容（单个多字节字符）—— 保证至少保留一个字符。
    if cut == 0 {
        cut = content
            .char_indices()
            .next()
            .map(|(_, c)| c.len_utf8())
            .unwrap_or(0);
    }

    let dropped_bytes = original_bytes.saturating_sub(cut);
    let mut out = String::with_capacity(cut + TRAILER_RESERVED);
    out.push_str(&content[..cut]);
    // 硬分隔符，便于人类和模型识别。
    let _ = write!(
        out,
        "\n\n[… {dropped_bytes} bytes truncated by tool_result_budget — re-run with a narrower query to see the rest …]"
    );

    let final_bytes = out.len();
    (
        out,
        BudgetOutcome {
            original_bytes,
            final_bytes,
            truncated: true,
        },
    )
}

/// 找到不超过 `max_bytes` 的最近合法 UTF-8 字符边界。
/// 如果 `max_bytes` 落在多字节字符中间，返回前一个字符的结束位置。
#[allow(dead_code)]
fn floor_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    // 从 max_bytes 位置向前找到最近的字符边界
    let mut idx = max_bytes;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_content_passes_through_unchanged() {
        let input = "hello world";
        let (out, outcome) = apply_tool_result_budget(input, 1024);
        assert_eq!(out, input);
        assert!(!outcome.truncated);
        assert_eq!(outcome.original_bytes, outcome.final_bytes);
    }

    #[test]
    fn content_at_exact_budget_is_unchanged() {
        let input = "x".repeat(100);
        let (out, outcome) = apply_tool_result_budget(&input, 100);
        assert_eq!(out, input);
        assert!(!outcome.truncated);
    }

    #[test]
    fn oversized_content_is_truncated_with_marker() {
        let input = "x".repeat(20_000);
        let (out, outcome) = apply_tool_result_budget(&input, 1024);
        assert!(outcome.truncated);
        assert!(out.len() < 20_000);
        assert!(out.contains("truncated by tool_result_budget"));
        assert!(out.contains("bytes truncated"));
    }

    #[test]
    fn truncation_respects_utf8_boundaries() {
        // 每个 "é" 是 2 字节。600 个 = 1200 字节。
        let input: String = "é".repeat(600);
        let (out, outcome) = apply_tool_result_budget(&input, 500);
        assert!(outcome.truncated);
        // 必须是合法的 UTF-8。
        let _ = out.as_str();
        // 头部应该只包含完整的 "é" 字符（没有半个字节）。
        let head_end = out.find("\n\n[").unwrap();
        let head = &out[..head_end];
        assert!(head.chars().all(|c| c == 'é'));
    }

    #[test]
    fn zero_budget_is_noop() {
        let input = "keep me".to_string();
        let (out, outcome) = apply_tool_result_budget(&input, 0);
        assert_eq!(out, input);
        assert!(!outcome.truncated);
    }

    #[test]
    fn outcome_reports_correct_byte_counts() {
        let input = "x".repeat(5_000);
        let (out, outcome) = apply_tool_result_budget(&input, 1024);
        assert_eq!(outcome.original_bytes, 5_000);
        assert_eq!(outcome.final_bytes, out.len());
        assert!(outcome.truncated);
    }

    #[test]
    fn chinese_text_truncation() {
        let input = "你好世界".repeat(500);
        let (out, outcome) = apply_tool_result_budget(&input, 200);
        assert!(outcome.truncated);
        // 截断点应该落在 UTF-8 边界上
        let head_end = out.find("\n\n[").unwrap();
        let head = &out[..head_end];
        // 验证头部是合法 UTF-8
        let _ = head;
    }
}
