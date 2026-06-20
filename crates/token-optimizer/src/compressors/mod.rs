//! 内容感知的压缩器集合。
//!
//! 每种内容类型都有对应的专用压缩器：
//! - `JsonCompressor` — JSON 数组统计压缩
//! - `LogCompressor` — 日志输出优先级压缩
//! - `DiffCompressor` — diff 上下文裁剪
//! - `SearchCompressor` — 搜索结果聚合
//! - `TextCompressor` — 纯文本截断

pub mod json;
pub mod log;
pub mod diff;
pub mod search;
pub mod text;

use crate::{ContentType, CompressResult, tokenizer};

/// 压缩器 trait —— 所有压缩器实现此接口。
pub trait Compressor: Send + Sync {
    /// 压缩器名称（用于日志和统计）。
    fn name(&self) -> &'static str;

    /// 此压缩器处理的内容类型。
    fn content_type(&self) -> ContentType;

    /// 检查此压缩器是否应处理给定内容。
    /// 默认匹配 content_type。
    fn should_handle(&self, _content: &str, _tool_name: &str) -> bool {
        true
    }

    /// 压缩内容并返回结果。
    fn compress(&self, content: &str, tool_name: &str) -> String;
}

/// 使用压缩器列表和内容创建 CompressResult。
pub fn make_result(
    compressor: &dyn Compressor,
    original: &str,
    compressed: String,
    ccr_key: Option<String>,
) -> CompressResult {
    let tokens_before = tokenizer::estimate_tokens(original);
    let tokens_after = tokenizer::estimate_tokens(&compressed);
    let ratio = tokenizer::estimate_savings(tokens_before, tokens_after);

    CompressResult {
        compressed,
        content_type: compressor.content_type(),
        tokens_before,
        tokens_after,
        compression_ratio: ratio,
        strategy: compressor.name(),
        ccr_key,
    }
}
