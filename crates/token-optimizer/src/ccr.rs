//! CCR (Compress-Cache-Retrieve) — 可逆压缩存储。
//!
//! 提供压缩内容的本地存储和检索能力。
//! 当内容被压缩后，原始内容存入 CCR 存储，
//! 压缩后的内容包含检索标记，LLM 可通过工具调用恢复原始数据。
//!
//! 当前实现为内存存储（`InMemoryCcrStore`），
//! 未来可扩展为 SQLite 持久化存储。

use sha2::Digest;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// CCR 存储 trait —— 可扩展为不同后端。
pub trait CcrStore: Send + Sync {
    /// 存储原始内容并返回检索键。
    fn store(&self, content: &str) -> String;

    /// 通过检索键获取原始内容。
    fn retrieve(&self, key: &str) -> Option<String>;

    /// 删除指定键的内容。
    fn remove(&self, key: &str) -> bool;

    /// 返回存储中的条目数。
    fn len(&self) -> usize;

    /// 是否为空。
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 内存中的 CCR 存储实现。
///
/// 线程安全，使用 `Arc<Mutex<HashMap>>`。
#[derive(Debug, Clone, Default)]
pub struct InMemoryCcrStore {
    entries: Arc<Mutex<HashMap<String, CcrEntry>>>,
}

#[derive(Debug, Clone)]
struct CcrEntry {
    content: String,
    #[allow(dead_code)]
    created_at: std::time::Instant,
}

impl InMemoryCcrStore {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 清理所有条目。
    pub fn clear(&self) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.clear();
        }
    }
}

impl CcrStore for InMemoryCcrStore {
    fn store(&self, content: &str) -> String {
        let key = ccr_key(content);
        if let Ok(mut guard) = self.entries.lock() {
            guard.insert(
                key.clone(),
                CcrEntry {
                    content: content.to_string(),
                    created_at: std::time::Instant::now(),
                },
            );
        }
        key
    }

    fn retrieve(&self, key: &str) -> Option<String> {
        self.entries
            .lock()
            .ok()
            .and_then(|guard| guard.get(key).map(|e| e.content.clone()))
    }

    fn remove(&self, key: &str) -> bool {
        self.entries
            .lock()
            .ok()
            .map_or(false, |mut guard| guard.remove(key).is_some())
    }

    fn len(&self) -> usize {
        self.entries.lock().ok().map_or(0, |g| g.len())
    }
}

/// 生成 CCR 键：内容 SHA-256 的前 12 个十六进制字符。
fn ccr_key(content: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    let hex: String = result[..6].iter().map(|b| format!("{b:02x}")).collect();
    format!("ccr:{hex}")
}

/// 生成 CCR 检索标记文本。
///
/// 插入到压缩后的内容中，告知 LLM 可通过 `headroom_retrieve` 工具
/// 检索原始数据。
pub fn make_ccr_marker(ccr_key: &str, item_count: usize) -> String {
    format!("\n\n<<ccr:{ccr_key} {item_count}_items_offloaded>>\n")
}

/// 检索标记前缀（用于匹配）。
pub const CCR_MARKER_PREFIX: &str = "<<ccr:";

/// 从文本中解析 CCR 键。
pub fn parse_ccr_key(text: &str) -> Option<String> {
    if let Some(start) = text.find(CCR_MARKER_PREFIX) {
        let after_prefix = &text[start + CCR_MARKER_PREFIX.len()..];
        if let Some(end) = after_prefix.find(' ') {
            return Some(after_prefix[..end].to_string());
        }
        if let Some(end) = after_prefix.find(">>") {
            return Some(after_prefix[..end].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_retrieve() {
        let store = InMemoryCcrStore::new();
        let key = store.store("hello world");
        assert!(key.starts_with("ccr:"));
        assert_eq!(store.retrieve(&key), Some("hello world".to_string()));
    }

    #[test]
    fn same_content_same_key() {
        let store = InMemoryCcrStore::new();
        let key1 = store.store("hello");
        let key2 = store.store("hello");
        assert_eq!(key1, key2);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn remove_entry() {
        let store = InMemoryCcrStore::new();
        let key = store.store("test");
        assert!(store.remove(&key));
        assert!(!store.remove(&key));
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn parse_ccr_key_from_text() {
        let text = "some text <<ccr:abc123 42_items_offloaded>> more text";
        let key = parse_ccr_key(text);
        assert_eq!(key, Some("abc123".to_string()));
    }

    #[test]
    fn no_ccr_key_returns_none() {
        assert_eq!(parse_ccr_key("plain text"), None);
    }

    #[test]
    fn ccr_marker_format() {
        let marker = make_ccr_marker("abc123", 42);
        assert!(marker.contains("ccr:abc123"));
        assert!(marker.contains("42_items_offloaded"));
    }
}
