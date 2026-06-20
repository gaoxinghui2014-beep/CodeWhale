//! 日志压缩器 — 按优先级保留关键日志行。
//!
//! 算法：
//! 1. 按行分类（ERROR > WARN > INFO > DEBUG）
//! 2. 优先保留高优先级行
//! 3. 在预算内按分类选择

use crate::{ContentType, compressors::Compressor};

/// 日志行的优先级类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Error = 0,
    Warning = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
    Unknown = 5,
}

/// 日志压缩器的配置。
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// 最大保留行数。
    pub max_lines: usize,
    /// 错误和警告的上下文窗口大小（前后各保留 N 行）。
    pub context_window: usize,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            max_lines: 80,
            context_window: 3,
        }
    }
}

/// 日志内容压缩器。
pub struct LogCompressor {
    config: LogConfig,
}

impl LogCompressor {
    pub fn new(config: LogConfig) -> Self {
        Self { config }
    }
}

impl Default for LogCompressor {
    fn default() -> Self {
        Self::new(LogConfig::default())
    }
}

impl Compressor for LogCompressor {
    fn name(&self) -> &'static str {
        "log"
    }

    fn content_type(&self) -> ContentType {
        ContentType::Log
    }

    fn compress(&self, content: &str, _tool_name: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();

        if lines.len() <= self.config.max_lines {
            return content.to_string();
        }

        // 分类每一行
        let classified: Vec<(LogLevel, bool)> = lines
            .iter()
            .map(|line| {
                let level = classify_line(line);
                let is_key = matches!(level, LogLevel::Error | LogLevel::Warning);
                (level, is_key)
            })
            .collect();

        // 构建选择索引：错误/警告 + 上下文窗口 + 首尾行
        let selected = select_lines(&classified, self.config.max_lines, self.config.context_window);

        // 构建输出
        let mut result = String::with_capacity(content.len() / 2);
        let mut last_idx: Option<usize> = None;
        for (idx, line) in lines.iter().enumerate() {
            if selected.contains(&idx) {
                if let Some(prev) = last_idx {
                    if idx > prev + 1 {
                        // 插入省略标记
                        let skipped = idx - prev - 1;
                        result.push_str(&format!("… {skipped} lines omitted …\n"));
                    }
                }
                result.push_str(line);
                result.push('\n');
                last_idx = Some(idx);
            }
        }

        // 如果尾部有省略
        if let Some(prev) = last_idx {
            let tail_skipped = lines.len() - prev - 1;
            if tail_skipped > 0 {
                result.push_str(&format!("\n… {tail_skipped} trailing lines omitted …\n"));
            }
        }

        result
    }
}

/// 分类日志行。
fn classify_line(line: &str) -> LogLevel {
    let upper = line.to_uppercase();
    if upper.contains("ERROR") || upper.contains("FATAL") || upper.contains("FAIL") {
        LogLevel::Error
    } else if upper.contains("WARN") || upper.contains("WARNING") {
        LogLevel::Warning
    } else if upper.contains("INFO") {
        LogLevel::Info
    } else if upper.contains("DEBUG") {
        LogLevel::Debug
    } else if upper.contains("TRACE") {
        LogLevel::Trace
    } else {
        LogLevel::Unknown
    }
}

/// 选择保留的行索引。
fn select_lines(
    classified: &[(LogLevel, bool)],
    max_lines: usize,
    context: usize,
) -> Vec<usize> {
    let total = classified.len();
    let mut selected = Vec::with_capacity(max_lines);

    // 始终保留首行和尾行（每个各几行）
    let head_keep = 3.min(total);
    let tail_keep = 3.min(total - head_keep);

    for i in 0..head_keep {
        if !selected.contains(&i) {
            selected.push(i);
        }
    }

    // 保留关键行（错误/警告）及其上下文
    let mut key_indices: Vec<usize> = Vec::new();
    for (i, (_level, is_key)) in classified.iter().enumerate() {
        if *is_key {
            key_indices.push(i);
            // 添加上下文窗口
            let start = i.saturating_sub(context);
            let end = (i + context + 1).min(total);
            for j in start..end {
                if !selected.contains(&j) {
                    selected.push(j);
                }
            }
        }
    }

    // 添加尾部行
    for i in (total - tail_keep)..total {
        if !selected.contains(&i) {
            selected.push(i);
        }
    }

    // 如果超过预算，按优先级裁剪（保留关键行，去掉上下文中的低优先级行）
    if selected.len() > max_lines {
        // 按日志级别排序，保留最高优先级的
        let mut indexed: Vec<(usize, LogLevel)> = selected
            .iter()
            .filter(|&&i| i < total)
            .map(|&i| (i, classified[i].0))
            .filter(|(i, _)| !key_indices.contains(i) || key_indices.is_empty())
            .collect();
        // 按级别排序（Error 优先）
        indexed.sort_by(|a, b| a.1.cmp(&b.1));
        indexed.truncate(max_lines - key_indices.len().min(max_lines));

        let mut final_selected: Vec<usize> = key_indices
            .iter()
            .take(max_lines)
            .copied()
            .chain(indexed.iter().map(|(i, _)| *i))
            .collect();
        final_selected.sort();
        final_selected.dedup();
        final_selected
    } else {
        selected.sort();
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_log_unchanged() {
        let compressor = LogCompressor::default();
        let content = "INFO: start\nERROR: fail\nINFO: done";
        let result = compressor.compress(content, "");
        assert!(result.contains("ERROR: fail"));
    }

    #[test]
    fn long_log_compressed() {
        let compressor = LogCompressor::new(LogConfig {
            max_lines: 20,
            context_window: 2,
        });
        // 生成 200 行日志，第 50 行有一个错误
        let mut lines = Vec::new();
        for i in 0..200 {
            if i == 50 {
                lines.push(format!("ERROR: critical failure at line {i}"));
            } else {
                lines.push(format!("INFO: processing line {i}"));
            }
        }
        let content = lines.join("\n");
        let result = compressor.compress(&content, "");
        assert!(result.contains("ERROR: critical"));
        assert!(result.contains("omitted"));
    }

    #[test]
    fn preserves_warnings() {
        let compressor = LogCompressor::new(LogConfig {
            max_lines: 15,
            context_window: 1,
        });
        let content = "INFO: a\nWARN: something\nINFO: b\nINFO: c";
        let result = compressor.compress(content, "");
        assert!(result.contains("WARN: something"));
    }

    #[test]
    fn log_level_ordering() {
        assert!(LogLevel::Error < LogLevel::Warning);
        assert!(LogLevel::Warning < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
    }
}
