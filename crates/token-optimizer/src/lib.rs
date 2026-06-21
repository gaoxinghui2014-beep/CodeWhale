//! Token Optimizer — 内容感知的工具输出压缩管道。
//!
//! 参考 headroom-main 的 token 优化方案，为 CodeWhale 提供：
//!
//! - **内容类型检测** — 自动识别 JSON、日志、diff、搜索、代码、文本
//! - **针对性压缩** — 每种类型使用最适合的压缩策略
//! - **CCR 可逆压缩** — 压缩时保存原始内容，需要时可检索
//! - **缓存感知策略** — 不破坏 LLM 的 KV 缓存前缀
//!
//! # 快速使用
//!
//! ```rust,ignore
//! use token_optimizer::{compress_tool_output, CompressResult};
//!
//! let result = compress_tool_output(&large_json_output, "read_file");
//! // result.compressed — 压缩后的内容
//! // result.content_type — 检测到的类型
//! // result.tokens_before / result.tokens_after — token 估算
//! ```

pub mod ccr;
pub mod compressors;
pub mod detector;
pub mod pipeline;
pub mod policy;
pub mod tokenizer;

use std::borrow::Cow;

/// 内容类型 —— 决定使用哪种压缩策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    /// JSON 数组或对象（工具输出、API 响应）
    Json,
    /// 构建/测试日志输出
    Log,
    /// git diff 输出
    Diff,
    /// grep/ripgrep 搜索结果
    Search,
    /// 源代码
    Code,
    /// 纯文本 / markdown
    Text,
    /// 无法识别
    Unknown,
}

/// 一次压缩操作的结果。
#[derive(Debug, Clone)]
pub struct CompressResult {
    /// 压缩后的内容。
    pub compressed: String,
    /// 检测到的内容类型。
    pub content_type: ContentType,
    /// 压缩前的估算 token 数。
    pub tokens_before: u32,
    /// 压缩后的估算 token 数。
    pub tokens_after: u32,
    /// 缩减的比例（0.0 = 无节省，1.0 = 100% 节省）。
    pub compression_ratio: f64,
    /// 应用的压缩策略名称。
    pub strategy: &'static str,
    /// CCR 存储键（如果启用了可逆压缩）。
    pub ccr_key: Option<String>,
}

impl CompressResult {
    /// 返回节省的 token 数。
    pub fn tokens_saved(&self) -> u32 {
        self.tokens_before.saturating_sub(self.tokens_after)
    }
}

/// 对工具输出应用内容感知压缩。
///
/// 这是 crate 的主要入口点。调用者只需传入工具输出的文本
/// 和工具名称，即可获得压缩后的结果。
///
/// # 参数
///
/// - `content`: 工具输出的原始文本
/// - `tool_name`: 工具名称（用于启发式类型推断）
///
/// # 返回
///
/// 压缩结果，包含压缩后的文本和统计信息。
pub fn compress_tool_output(content: &str, tool_name: &str) -> CompressResult {
    let pipeline = pipeline::default_pipeline();
    pipeline.compress(content, tool_name)
}

/// 应用保守的 token 预算截断（仅保留下界）。
///
/// 如果内容在预算内则原样返回，否则在合法 UTF-8 边界处截断。
/// 截断标记使用人类和模型都能理解的格式。
pub fn apply_token_budget(content: &str, budget_bytes: usize) -> Cow<'_, str> {
    if budget_bytes == 0 || content.len() <= budget_bytes {
        return Cow::Borrowed(content);
    }

    // 在预算边界附近找到合法 UTF-8 字符边界
    let mut end = budget_bytes - 128; // 留出截断标记的空间
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        end = budget_bytes.saturating_sub(256);
        if !content.is_char_boundary(end) {
            // 回退：从头找一个安全边界
            end = content
                .char_indices()
                .take_while(|(i, _)| *i < budget_bytes.saturating_sub(256))
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    let truncated = &content[..end];
    let omitted = content.len() - end;
    Cow::Owned(format!("{truncated}\n\n[… {omitted} bytes truncated …]"))
}
