//! 压缩管道编排器。
//!
//! 将内容类型检测和压缩器路由连接成一个完整的管道。
//! 参考 headroom 的两阶段架构（CacheAligner → ContentRouter），
//! 简化为单阶段路由。

use crate::{
    CompressResult, ContentType,
    compressors::{
        Compressor, diff::DiffCompressor, json::JsonCompressor, log::LogCompressor,
        search::SearchCompressor, text::TextCompressor,
    },
    detector,
};

/// 压缩管道的配置。
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// 是否启用 JSON 压缩。
    pub enable_json: bool,
    /// 是否启用日志压缩。
    pub enable_log: bool,
    /// 是否启用 diff 压缩。
    pub enable_diff: bool,
    /// 是否启用搜索压缩。
    pub enable_search: bool,
    /// 是否启用文本压缩。
    pub enable_text: bool,
    /// 最低压缩字节阈值（内容小于此值不压缩）。
    pub min_content_bytes: usize,
    /// 是否启用 CCR。
    pub enable_ccr: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            enable_json: true,
            enable_log: true,
            enable_diff: true,
            enable_search: true,
            enable_text: true,
            min_content_bytes: 256,
            enable_ccr: false, // 默认关闭，需要显式配置
        }
    }
}

/// 压缩管道 —— 将内容路由到合适的压缩器。
pub struct CompressionPipeline {
    config: PipelineConfig,
    json_compressor: JsonCompressor,
    log_compressor: LogCompressor,
    diff_compressor: DiffCompressor,
    search_compressor: SearchCompressor,
    text_compressor: TextCompressor,
}

impl CompressionPipeline {
    /// 创建新管道。
    pub fn new(config: PipelineConfig) -> Self {
        Self {
            config,
            json_compressor: JsonCompressor::default(),
            log_compressor: LogCompressor::default(),
            diff_compressor: DiffCompressor::default(),
            search_compressor: SearchCompressor::default(),
            text_compressor: TextCompressor::default(),
        }
    }

    /// 压缩内容。这是管道的主要入口。
    pub fn compress(&self, content: &str, tool_name: &str) -> CompressResult {
        // 短内容不需要压缩
        if content.len() < self.config.min_content_bytes {
            return CompressResult {
                compressed: content.to_string(),
                content_type: ContentType::Text,
                tokens_before: crate::tokenizer::estimate_tokens(content),
                tokens_after: crate::tokenizer::estimate_tokens(content),
                compression_ratio: 0.0,
                strategy: "passthrough",
                ccr_key: None,
            };
        }

        // 检测内容类型
        let content_type = detector::detect(content, tool_name);

        // 路由到对应的压缩器
        let (compressed, ccr_key) = self.route(content, tool_name, content_type);

        // 构建结果
        let tokens_before = crate::tokenizer::estimate_tokens(content);
        let tokens_after = crate::tokenizer::estimate_tokens(&compressed);

        // 膨胀保护：如果压缩后更大，回退到原始内容
        if tokens_after > tokens_before && !compressed.is_empty() {
            return CompressResult {
                compressed: content.to_string(),
                content_type,
                tokens_before,
                tokens_after: tokens_before,
                compression_ratio: 0.0,
                strategy: "inflation_guard",
                ccr_key: None,
            };
        }

        let ratio = crate::tokenizer::estimate_savings(tokens_before, tokens_after);

        // 确定策略名称
        let strategy = match content_type {
            ContentType::Json => "json",
            ContentType::Log => "log",
            ContentType::Diff => "diff",
            ContentType::Search => "search",
            ContentType::Code => "passthrough", // 代码不压缩
            ContentType::Text => "text",
            ContentType::Unknown => "text",
        };

        CompressResult {
            compressed,
            content_type,
            tokens_before,
            tokens_after,
            compression_ratio: ratio,
            strategy,
            ccr_key,
        }
    }

    /// 根据类型路由到压缩器。
    fn route(
        &self,
        content: &str,
        tool_name: &str,
        content_type: ContentType,
    ) -> (String, Option<String>) {
        let (compressed, strategy_name) = match content_type {
            ContentType::Json if self.config.enable_json => {
                (self.json_compressor.compress(content, tool_name), "json")
            }
            ContentType::Log if self.config.enable_log => {
                (self.log_compressor.compress(content, tool_name), "log")
            }
            ContentType::Diff if self.config.enable_diff => {
                (self.diff_compressor.compress(content, tool_name), "diff")
            }
            ContentType::Search if self.config.enable_search => (
                self.search_compressor.compress(content, tool_name),
                "search",
            ),
            ContentType::Code => {
                // 代码不压缩（只做无损 minify 太危险）
                return (content.to_string(), None);
            }
            _ => {
                if self.config.enable_text {
                    (self.text_compressor.compress(content, tool_name), "text")
                } else {
                    return (content.to_string(), None);
                }
            }
        };

        // CCR 存储（当前用哈希占位，实际存储由集成方决定）
        let ccr_key = if self.config.enable_ccr && compressed != content {
            Some(format!("ccr:{}", sha2_hash(content)))
        } else {
            None
        };

        let _unused = strategy_name;
        (compressed, ccr_key)
    }
}

impl Default for CompressionPipeline {
    fn default() -> Self {
        Self::new(PipelineConfig::default())
    }
}

/// 创建默认管道（全局单例）。
pub fn default_pipeline() -> CompressionPipeline {
    CompressionPipeline::default()
}

/// 计算内容的 SHA-256 哈希（前 16 个十六进制字符）。
fn sha2_hash(content: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    let hex: String = result[..8].iter().map(|b| format!("{b:02x}")).collect();
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_content_passthrough() {
        let pipeline = CompressionPipeline::default();
        let result = pipeline.compress("short", "unknown");
        assert_eq!(result.strategy, "passthrough");
        assert_eq!(result.compressed, "short");
    }

    #[test]
    fn json_array_compressed() {
        let pipeline = CompressionPipeline::new(PipelineConfig {
            min_content_bytes: 10,
            ..Default::default()
        });
        let items: Vec<serde_json::Value> = (0..100)
            .map(|i| serde_json::Value::Number(i.into()))
            .collect();
        let content = serde_json::to_string(&items).unwrap();
        let result = pipeline.compress(&content, "unknown");
        assert_eq!(result.content_type, ContentType::Json);
        assert_eq!(result.strategy, "json");
    }

    #[test]
    fn inflation_guard_reverts() {
        // 创建一个内容在压缩后不会变小的情况
        let pipeline = CompressionPipeline::new(PipelineConfig {
            min_content_bytes: 10,
            enable_text: true,
            ..Default::default()
        });
        let content = "a".repeat(200); // 200 字节全相同字符
        let result = pipeline.compress(&content, "unknown");
        // 压缩后不应比原始更大
        assert!(
            result.tokens_after <= result.tokens_before || result.strategy == "inflation_guard"
        );
    }
}
