//! 搜索压缩器 — 聚合 grep/ripgrep 搜索结果。
//!
//! 算法：
//! 1. 按文件分组匹配
//! 2. 限制文件数和每文件匹配数
//! 3. 添加摘要统计

use crate::{ContentType, compressors::Compressor};

/// 搜索压缩器的配置。
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// 最大保留文件数。
    pub max_files: usize,
    /// 每文件最大保留匹配数。
    pub max_matches_per_file: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_files: 30,
            max_matches_per_file: 5,
        }
    }
}

/// 搜索内容压缩器。
pub struct SearchCompressor {
    config: SearchConfig,
}

impl SearchCompressor {
    pub fn new(config: SearchConfig) -> Self {
        Self { config }
    }
}

impl Default for SearchCompressor {
    fn default() -> Self {
        Self::new(SearchConfig::default())
    }
}

impl Compressor for SearchCompressor {
    fn name(&self) -> &'static str {
        "search"
    }

    fn content_type(&self) -> ContentType {
        ContentType::Search
    }

    fn compress(&self, content: &str, _tool_name: &str) -> String {
        let grouped = group_by_file(content);
        let file_count = grouped.len();

        if file_count <= self.config.max_files {
            // 所有文件都在预算内，但可能仍需要裁剪每文件匹配数
            return format_grouped(&grouped, self.config.max_matches_per_file, None);
        }

        // 按匹配密度排序，保留匹配最多的文件
        let mut sorted: Vec<(&String, &Vec<SearchMatch>)> = grouped.iter().collect();
        sorted.sort_by_key(|(_, matches)| std::cmp::Reverse(matches.len()));

        let kept: Vec<(String, Vec<SearchMatch>)> = sorted
            .iter()
            .take(self.config.max_files)
            .map(|(file, matches)| ((*file).clone(), (*matches).clone()))
            .collect();

        let omitted_files = file_count - self.config.max_files;
        let kept_map: std::collections::HashMap<String, Vec<SearchMatch>> =
            kept.into_iter().collect();

        format_grouped(
            &kept_map,
            self.config.max_matches_per_file,
            Some(omitted_files),
        )
    }
}

/// 一条搜索匹配。
#[derive(Debug, Clone)]
struct SearchMatch {
    line_number: String,
    content: String,
}

/// 按文件分组搜索结果行（轻量解析 grep/ripgrep 格式）。
fn group_by_file(content: &str) -> std::collections::HashMap<String, Vec<SearchMatch>> {
    let mut result: std::collections::HashMap<String, Vec<SearchMatch>> =
        std::collections::HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // 尝试解析 "filename:line:content" 格式
        if let Some((file, rest)) = parse_file_prefix(line) {
            let (line_num, content_part) = parse_line_number(&rest);
            result.entry(file).or_default().push(SearchMatch {
                line_number: line_num.to_string(),
                content: content_part.to_string(),
            });
        } else {
            // 无法解析：放入 "results" 伪文件
            result
                .entry("results".to_string())
                .or_default()
                .push(SearchMatch {
                    line_number: String::new(),
                    content: line.to_string(),
                });
        }
    }

    result
}

/// 解析 "filename:rest" 前缀。
fn parse_file_prefix(line: &str) -> Option<(String, String)> {
    // 跳过 ANSI 颜色代码
    let clean = strip_ansi(line);
    let parts: Vec<&str> = clean.splitn(2, ':').collect();
    if parts.len() == 2 && !parts[0].is_empty() {
        // 检查第一部分看起来像文件路径（包含 / 或 .）
        if parts[0].contains('/') || parts[0].contains('\\') || parts[0].contains('.') {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    None
}

/// 简单去除 ANSI 颜色代码。
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // 跳过 ANSI 转义序列
            while let Some(&next) = chars.peek() {
                if next.is_alphabetic() {
                    chars.next();
                    break;
                }
                chars.next();
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// 解析行号前缀。
fn parse_line_number(rest: &str) -> (&str, &str) {
    let parts: Vec<&str> = rest.splitn(2, ':').collect();
    if parts.len() == 2 && parts[0].chars().all(|c| c.is_ascii_digit()) {
        (parts[0], parts[1])
    } else {
        ("", rest)
    }
}

/// 格式化分组结果。
fn format_grouped(
    grouped: &std::collections::HashMap<String, Vec<SearchMatch>>,
    max_per_file: usize,
    omitted_files: Option<usize>,
) -> String {
    let mut result = String::new();
    let mut sorted_keys: Vec<&String> = grouped.keys().collect();
    sorted_keys.sort();

    for file in sorted_keys {
        let matches = &grouped[file];
        result.push_str(&format!("{}", file));
        result.push('\n');

        let _show_count = matches.len().min(max_per_file);
        for m in matches.iter().take(max_per_file) {
            if m.line_number.is_empty() {
                result.push_str(&format!("  {}\n", m.content));
            } else {
                result.push_str(&format!("  {}: {}\n", m.line_number, m.content));
            }
        }

        if matches.len() > max_per_file {
            let omitted = matches.len() - max_per_file;
            result.push_str(&format!("  … {omitted} more matches …\n"));
        }
        result.push('\n');
    }

    if let Some(omitted) = omitted_files {
        if omitted > 0 {
            result.push_str(&format!(
                "\n… {omitted} files with fewer matches omitted …\n"
            ));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_file() {
        let content = "src/a.rs:10:fn foo()\nsrc/a.rs:20:fn bar()\nsrc/b.rs:5:mod test";
        let grouped = group_by_file(content);
        assert_eq!(grouped.len(), 2);
        assert!(grouped.contains_key("src/a.rs"));
        assert_eq!(grouped.get("src/a.rs").unwrap().len(), 2);
    }

    #[test]
    fn limits_files_and_matches() {
        let compressor = SearchCompressor::new(SearchConfig {
            max_files: 1,
            max_matches_per_file: 1,
        });
        let content = "src/a.rs:10:fn foo()\nsrc/a.rs:20:fn bar()\nsrc/b.rs:5:mod test";
        let result = compressor.compress(content, "");
        assert!(result.contains("more matches"));
    }

    #[test]
    fn small_search_unchanged() {
        let compressor = SearchCompressor::default();
        let content = "src/lib.rs:5:pub fn hello()";
        let result = compressor.compress(content, "");
        assert!(result.contains("pub fn hello()"));
    }

    #[test]
    fn strips_ansi_colors() {
        let result = strip_ansi("\x1b[32mgreen\x1b[0m text");
        assert_eq!(result, "green text");
    }
}
