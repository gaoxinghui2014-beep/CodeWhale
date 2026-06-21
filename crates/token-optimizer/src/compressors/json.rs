//! JSON 压缩器 — 对 JSON 数组/对象进行轻量统计压缩。
//!
//! 不实现完整的 headroom SmartCrusher（复杂的统计分析 + 锚点选择），
//! 而是提供两个层次的压缩：
//! 1. **Minify** — 去除空白，无损压缩
//! 2. **Truncate** — 对大型数组做智能采样

use crate::{ContentType, compressors::Compressor};

/// JSON 压缩器的配置。
#[derive(Debug, Clone)]
pub struct JsonConfig {
    /// 数组最大保留条目数（0 = 不限制）。
    pub max_array_items: usize,
    /// 是否执行 JSON minify（去除空白）。
    pub minify: bool,
    /// 截断标记文本。
    pub truncation_marker: String,
}

impl Default for JsonConfig {
    fn default() -> Self {
        Self {
            max_array_items: 30,
            minify: true,
            truncation_marker: String::from("\n// … N items omitted …\n"),
        }
    }
}

/// JSON 内容压缩器。
pub struct JsonCompressor {
    config: JsonConfig,
}

impl JsonCompressor {
    pub fn new(config: JsonConfig) -> Self {
        Self { config }
    }
}

impl Default for JsonCompressor {
    fn default() -> Self {
        Self::new(JsonConfig::default())
    }
}

impl Compressor for JsonCompressor {
    fn name(&self) -> &'static str {
        "json"
    }

    fn content_type(&self) -> ContentType {
        ContentType::Json
    }

    fn compress(&self, content: &str, _tool_name: &str) -> String {
        // Step 1: 尝试解析 JSON
        let parsed: serde_json::Value = match serde_json::from_str(content) {
            Ok(v) => v,
            Err(_) => return content.to_string(), // 非有效 JSON，不压缩
        };

        // Step 2: 压缩（对数组做截断，对对象做 minify）
        let compressed = match &parsed {
            serde_json::Value::Array(arr) => compress_array(
                arr,
                self.config.max_array_items,
                &self.config.truncation_marker,
            ),
            _ => {
                // 非数组：仅 minify
                if self.config.minify {
                    parsed.clone()
                } else {
                    return content.to_string();
                }
            }
        };

        // Step 3: 序列化
        if self.config.minify {
            serde_json::to_string(&compressed).unwrap_or_else(|_| content.to_string())
        } else {
            serde_json::to_string_pretty(&compressed).unwrap_or_else(|_| content.to_string())
        }
    }
}

/// 压缩 JSON 数组：保留前 N 项，截断其余。
fn compress_array(arr: &[serde_json::Value], max_items: usize, marker: &str) -> serde_json::Value {
    if max_items == 0 || arr.len() <= max_items {
        return serde_json::Value::Array(arr.to_vec());
    }

    let omitted = arr.len() - max_items;
    let mut kept: Vec<serde_json::Value> = arr[..max_items].to_vec();

    // 添加截断标记
    let marker_value = serde_json::Value::String(marker.replace(
        "… N items omitted …",
        &format!("… {omitted} items omitted …"),
    ));
    kept.push(marker_value);

    serde_json::Value::Array(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_array_unchanged() {
        let compressor = JsonCompressor::default();
        let content = r#"["a","b","c"]"#;
        let result = compressor.compress(content, "");
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    #[test]
    fn large_array_truncated() {
        let compressor = JsonCompressor::new(JsonConfig {
            max_array_items: 2,
            ..Default::default()
        });
        let arr: Vec<serde_json::Value> = (0..100)
            .map(|i| serde_json::Value::Number((i).into()))
            .collect();
        let content = serde_json::to_string(&arr).unwrap();
        let result = compressor.compress(&content, "");
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        // 2 items + 1 marker = 3
        assert!(parsed.as_array().unwrap().len() <= 3);
    }

    #[test]
    fn non_json_passthrough() {
        let compressor = JsonCompressor::default();
        let content = "not json at all";
        let result = compressor.compress(content, "");
        assert_eq!(result, content);
    }

    #[test]
    fn minify_removes_whitespace() {
        let compressor = JsonCompressor::default();
        let content = "{\n  \"key\":  \"value\"\n}";
        let result = compressor.compress(content, "");
        // minify 后不应有换行
        assert!(!result.contains('\n'));
    }
}
