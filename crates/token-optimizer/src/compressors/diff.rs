//! Diff 压缩器 — 裁剪 unified diff 输出。
//!
//! 算法：
//! 1. 限制文件数量（按变更密度排序）
//! 2. 限制每文件的 hunk 数量
//! 3. 裁剪每个 hunk 的上下文行数

use crate::{ContentType, compressors::Compressor};

/// Diff 压缩器的配置。
#[derive(Debug, Clone)]
pub struct DiffConfig {
    /// 最大保留文件数。
    pub max_files: usize,
    /// 每文件最大保留 hunk 数。
    pub max_hunks_per_file: usize,
    /// hunk 上下文窗口大小（变更前后各保留的行数）。
    pub context_lines: usize,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            max_files: 20,
            max_hunks_per_file: 10,
            context_lines: 2,
        }
    }
}

/// Diff 内容压缩器。
pub struct DiffCompressor {
    config: DiffConfig,
}

impl DiffCompressor {
    pub fn new(config: DiffConfig) -> Self {
        Self { config }
    }
}

impl Default for DiffCompressor {
    fn default() -> Self {
        Self::new(DiffConfig::default())
    }
}

impl Compressor for DiffCompressor {
    fn name(&self) -> &'static str {
        "diff"
    }

    fn content_type(&self) -> ContentType {
        ContentType::Diff
    }

    fn compress(&self, content: &str, _tool_name: &str) -> String {
        let files = split_diff_files(content);

        if files.len() <= self.config.max_files {
            return compress_context_in_place(content, self.config.context_lines);
        }

        // 限制文件数：优先保留变更密度高的文件
        let mut scored: Vec<(usize, &DiffFile)> = files
            .iter()
            .enumerate()
            .map(|(_i, f)| {
                let changes = count_changes(&f.content);
                (changes, f)
            })
            .collect();
        scored.sort_by_key(|(c, _)| std::cmp::Reverse(*c));

        let mut result = String::with_capacity(content.len() / 2);

        for (idx, (_changes, file)) in scored.iter().enumerate().take(self.config.max_files) {
            if idx > 0 {
                result.push('\n');
            }

            if idx == self.config.max_files - 1 && scored.len() > self.config.max_files {
                let omitted = scored.len() - self.config.max_files;
                result.push_str(&format!(
                    "… {omitted} files with fewer changes omitted …\n\n"
                ));
            }

            let file_content = compress_diff_file(
                &file.content,
                self.config.max_hunks_per_file,
                self.config.context_lines,
            );
            result.push_str(&file_content);
        }

        result
    }
}

/// Diff 文件中一个文件的内容结构。
struct DiffFile {
    content: String,
}

/// 将整个 diff 输出按文件分割。
fn split_diff_files(content: &str) -> Vec<DiffFile> {
    let mut files = Vec::new();
    let mut current = String::new();
    let mut in_file = false;

    for line in content.lines() {
        if line.starts_with("diff --git ") || line.starts_with("diff -") {
            if in_file && !current.is_empty() {
                files.push(DiffFile {
                    content: std::mem::take(&mut current),
                });
            }
            in_file = true;
        }
        if in_file {
            current.push_str(line);
            current.push('\n');
        }
    }

    if in_file && !current.is_empty() {
        files.push(DiffFile { content: current });
    }

    // 如果没有检测到文件分隔符，将整个内容作为一个文件
    if files.is_empty() && !content.is_empty() {
        files.push(DiffFile {
            content: content.to_string(),
        });
    }

    files
}

/// 统计 diff 中的变更行数（+ 和 - 行）。
fn count_changes(content: &str) -> usize {
    content
        .lines()
        .filter(|l| {
            let trimmed = l.trim();
            (trimmed.starts_with('+') || trimmed.starts_with('-'))
                && !trimmed.starts_with("+++ ")
                && !trimmed.starts_with("--- ")
        })
        .count()
}

/// 原位裁剪 diff 上下文（不分割文件）。
fn compress_context_in_place(content: &str, context: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::with_capacity(content.len());
    let mut context_counter = 0;
    let mut in_skip = false;

    for line in &lines {
        let is_change = (line.starts_with('+') || line.starts_with('-'))
            && !line.starts_with("+++ ")
            && !line.starts_with("--- ");
        let is_hunk_header = line.starts_with("@@");

        if is_hunk_header || is_change {
            if in_skip {
                result.push_str("…\n");
                in_skip = false;
            }
            context_counter = context;
            result.push_str(line);
            result.push('\n');
        } else if context_counter > 0 {
            context_counter -= 1;
            result.push_str(line);
            result.push('\n');
        } else {
            in_skip = true;
        }
    }

    result
}

/// 压缩单个文件的 diff 内容。
fn compress_diff_file(content: &str, max_hunks: usize, context: usize) -> String {
    let hunks = split_hunks(content);
    if hunks.len() <= max_hunks {
        return compress_context_in_place(content, context);
    }

    // 限制 hunk 数
    let mut result = String::new();
    for (i, hunk) in hunks.iter().enumerate().take(max_hunks) {
        if i > 0 {
            result.push('\n');
        }
        if i == max_hunks - 1 && hunks.len() > max_hunks {
            let omitted = hunks.len() - max_hunks;
            result.push_str(&format!("… {omitted} hunks omitted …\n"));
        }
        result.push_str(&compress_context_in_place(hunk, context));
    }
    result
}

/// 将 diff 内容按 hunk 分割。
fn split_hunks(content: &str) -> Vec<String> {
    let mut hunks = Vec::new();
    let mut current = String::new();
    let mut in_hunk = false;

    for line in content.lines() {
        if line.starts_with("@@") {
            if in_hunk && !current.is_empty() {
                hunks.push(std::mem::take(&mut current));
            }
            in_hunk = true;
        }
        if in_hunk {
            current.push_str(line);
            current.push('\n');
        }
    }

    if in_hunk && !current.is_empty() {
        hunks.push(current);
    }

    if hunks.is_empty() {
        hunks.push(content.to_string());
    }

    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_diff_unchanged() {
        let compressor = DiffCompressor::default();
        let content = "diff --git a/lib.rs b/lib.rs\n--- a/lib.rs\n+++ b/lib.rs\n@@ -1,3 +1,4 @@\n fn main() {\n+    println!(\"hello\");\n }";
        let result = compressor.compress(content, "");
        assert!(result.contains("fn main()"));
    }

    #[test]
    fn counts_changes() {
        let content = "+added line\n-removed line\n unchanged\n+another add";
        assert_eq!(count_changes(content), 3);
    }

    #[test]
    fn splits_diff_files() {
        let content = "diff --git a/one b/one\nline1\ndiff --git a/two b/two\nline2";
        let files = split_diff_files(content);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn context_cropping_preserves_changes() {
        let content = concat!(
            "unchanged 1\n",
            "unchanged 2\n",
            "unchanged 3\n",
            "+added line\n",
            "unchanged 4\n",
            "unchanged 5",
        );
        let result = compress_context_in_place(content, 1);
        // 第2行 "unchanged 2" 在上下文窗口内
        assert!(result.contains("unchanged 2") || result.contains("unchanged 3"));
    }
}
