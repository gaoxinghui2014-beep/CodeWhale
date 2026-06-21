//! 内容类型检测器。
//!
//! 自动识别工具输出的内容类型，支持基于内容的检测和基于工具名称的启发式推断。
//!
//! 不使用正则表达式 —— 全部使用结构化解析器（serde_json、字节前缀检查）。

use crate::ContentType;

/// 检测内容类型 —— 综合内容分析和工具名称启发式。
pub fn detect(content: &str, tool_name: &str) -> ContentType {
    // 优先使用工具名称启发式（置信度最高）
    if let Some(ct) = detect_by_tool_name(tool_name) {
        return ct;
    }

    // 回退到基于内容的检测
    detect_by_content(content)
}

/// 基于工具名称的启发式类型推断。
///
/// 对于已知工具，内容类型可以可靠地从工具名称推断，
/// 无需分析内容。
fn detect_by_tool_name(tool_name: &str) -> Option<ContentType> {
    match tool_name {
        // diff / git 工具
        "git_diff" | "git_show" | "git_log" | "git_status" => Some(ContentType::Diff),

        // 搜索工具
        "grep_files" | "file_search" | "web_search" => Some(ContentType::Search),

        // 代码生成
        "write_file" | "edit_file" | "apply_patch" if false => Some(ContentType::Code),
        _ => None,
    }
}

/// 基于内容的类型检测。
///
/// 使用结构化方法（非正则表达式）识别内容类型：
/// - 尝试 JSON 解析
/// - 检查 diff 头部
/// - 检查搜索输出格式
/// - 检查日志级别前缀
fn detect_by_content(content: &str) -> ContentType {
    let trimmed = content.trim();

    if trimmed.is_empty() {
        return ContentType::Text;
    }

    // 尝试 JSON 解析 —— JSON 数组/对象应优先识别
    if looks_like_json(trimmed) {
        return ContentType::Json;
    }

    // 检查 diff 特征
    if looks_like_diff(trimmed) {
        return ContentType::Diff;
    }

    // 检查搜索输出特征
    if looks_like_search_results(trimmed) {
        return ContentType::Search;
    }

    // 检查日志输出特征
    if looks_like_log(trimmed) {
        return ContentType::Log;
    }

    // 检查代码特征
    if looks_like_code(trimmed) {
        return ContentType::Code;
    }

    ContentType::Text
}

/// 检查是否像 JSON 内容。
fn looks_like_json(content: &str) -> bool {
    let first_char = content.chars().next();
    match first_char {
        Some('[') | Some('{') => {
            // 尝试浅层解析（只检查开头结构，不解析全部）
            serde_json::from_str::<serde_json::Value>(content).is_ok()
        }
        _ => false,
    }
}

/// 检查是否像 unified diff 输出。
fn looks_like_diff(content: &str) -> bool {
    // diff 的特征行：
    // - "diff --git a/… b/…"
    // - "--- a/…" 或 "+++ b/…"
    // - "@@ -… +… @@"
    let lines: Vec<&str> = content.lines().take(10).collect();

    let has_diff_header = lines
        .iter()
        .any(|l| l.starts_with("diff --git ") || l.starts_with("diff -"));

    let has_hunk_header = lines.iter().any(|l| l.starts_with("@@ -"));

    let has_file_marker = lines
        .iter()
        .any(|l| l.starts_with("--- ") || l.starts_with("+++ "));

    has_diff_header || (has_hunk_header && has_file_marker) || has_hunk_header
}

/// 检查是否像 grep/ripgrep 搜索输出。
fn looks_like_search_results(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().take(5).collect();

    // grep 格式: "filename:line_number:content"
    // ripgrep 格式类似，带颜色代码
    let colon_pattern_count = lines
        .iter()
        .filter(|l| {
            let parts: Vec<&str> = l.splitn(3, ':').collect();
            parts.len() >= 3
                && !parts[0].is_empty()
                && parts[1].chars().all(|c| c.is_ascii_digit())
                && !parts[1].is_empty()
        })
        .count();

    // 如果大部分行匹配这种格式，则是搜索输出
    colon_pattern_count >= 3 || (colon_pattern_count >= 1 && lines.len() <= 3)
}

/// 检查是否像日志输出。
fn looks_like_log(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().take(20).collect();

    let log_prefixes = [
        "ERROR", "WARN", "WARNING", "INFO", "DEBUG", "TRACE", "FATAL", "error", "warn", "warning",
        "info", "debug", "trace", "fatal",
    ];

    let log_line_count = lines
        .iter()
        .filter(|l| log_prefixes.iter().any(|prefix| l.contains(prefix)))
        .count();

    // 如果超过 30% 的行包含已知日志前缀，则是日志
    !lines.is_empty() && (log_line_count as f64 / lines.len() as f64) >= 0.3
}

/// 检查是否像源代码。
fn looks_like_code(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().take(30).collect();

    if lines.is_empty() {
        return false;
    }

    let code_indicators = [
        "fn ",
        "def ",
        "class ",
        "import ",
        "from ",
        "use ",
        "pub ",
        "const ",
        "let ",
        "var ",
        "function ",
        "if ",
        "for ",
        "while ",
        "match ",
        "switch ",
        "#include",
        "package ",
        "module ",
        "#define",
        "#ifdef",
        "#ifndef",
    ];

    let code_line_count = lines
        .iter()
        .filter(|l| {
            let trimmed = l.trim();
            code_indicators.iter().any(|kw| trimmed.starts_with(kw))
        })
        .count();

    // 如果有一定比例的行以代码关键字开头，则可能是代码
    (code_line_count as f64 / lines.len() as f64) >= 0.15
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_json_array() {
        let content = r#"[{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]"#;
        assert_eq!(detect_by_content(content), ContentType::Json);
    }

    #[test]
    fn detects_json_object() {
        let content = r#"{"status": "ok", "count": 42}"#;
        assert_eq!(detect_by_content(content), ContentType::Json);
    }

    #[test]
    fn detects_diff() {
        let content = "diff --git a/lib.rs b/lib.rs\n--- a/lib.rs\n+++ b/lib.rs\n@@ -1,3 +1,4 @@";
        assert_eq!(detect_by_content(content), ContentType::Diff);
    }

    #[test]
    fn detects_search_results() {
        let content = "src/main.rs:10:fn main() {\nsrc/lib.rs:5:pub fn hello()";
        assert_eq!(detect_by_content(content), ContentType::Search);
    }

    #[test]
    fn detects_log() {
        let content =
            "ERROR: something failed\nWARN: retry attempt 1\nINFO: processing\nDEBUG: value=42";
        assert_eq!(detect_by_content(content), ContentType::Log);
    }

    #[test]
    fn detects_code() {
        let content = "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}";
        assert_eq!(detect_by_content(content), ContentType::Code);
    }

    #[test]
    fn detects_diff_by_tool_name() {
        assert_eq!(detect_by_tool_name("git_diff"), Some(ContentType::Diff));
    }

    #[test]
    fn detects_search_by_tool_name() {
        assert_eq!(detect_by_tool_name("grep_files"), Some(ContentType::Search));
    }

    #[test]
    fn unknown_tool_returns_none() {
        assert_eq!(detect_by_tool_name("unknown_tool"), None);
    }
}
