//! Domain types for the tree summarizer (extracted).

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Node level ─────────────────────────────────────────────────────────

/// Hierarchical level of a tree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeLevel {
    Root,
    Year,
    Month,
    Day,
    Hour,
}

impl NodeLevel {
    /// Maximum number of tokens allowed at this level.
    pub fn max_tokens(&self) -> u32 {
        match self {
            Self::Hour => 1_000,
            Self::Day => 2_000,
            Self::Month => 4_000,
            Self::Year => 8_000,
            Self::Root => 20_000,
        }
    }

    pub fn parent_level(&self) -> Option<NodeLevel> {
        match self {
            Self::Hour => Some(Self::Day),
            Self::Day => Some(Self::Month),
            Self::Month => Some(Self::Year),
            Self::Year => Some(Self::Root),
            Self::Root => None,
        }
    }

    pub fn is_leaf(&self) -> bool {
        matches!(self, Self::Hour)
    }

    pub fn from_str_label(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "root" => Some(Self::Root),
            "year" => Some(Self::Year),
            "month" => Some(Self::Month),
            "day" => Some(Self::Day),
            "hour" => Some(Self::Hour),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Year => "year",
            Self::Month => "month",
            Self::Day => "day",
            Self::Hour => "hour",
        }
    }
}

// ── Tree node ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub node_id: String,
    pub namespace: String,
    pub level: NodeLevel,
    pub parent_id: Option<String>,
    pub summary: String,
    pub token_count: u32,
    pub child_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeStatus {
    pub namespace: String,
    pub total_nodes: u64,
    pub depth: u32,
    pub oldest_entry: Option<DateTime<Utc>>,
    pub newest_entry: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRequest {
    pub namespace: String,
    pub content: String,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub node: TreeNode,
    pub children: Vec<TreeNode>,
}

pub fn estimate_tokens(text: &str) -> u32 {
    (text.len() as u32).div_ceil(4)
}

pub fn derive_parent_id(node_id: &str) -> Option<String> {
    if node_id == "root" {
        return None;
    }
    match node_id.rfind('/') {
        Some(pos) => Some(node_id[..pos].to_string()),
        None => Some("root".to_string()),
    }
}

pub fn level_from_node_id(node_id: &str) -> NodeLevel {
    if node_id == "root" {
        return NodeLevel::Root;
    }
    match node_id.matches('/').count() {
        0 => NodeLevel::Year,
        1 => NodeLevel::Month,
        2 => NodeLevel::Day,
        _ => NodeLevel::Hour,
    }
}

pub fn derive_node_ids(ts: &DateTime<Utc>) -> (String, String, String, String, String) {
    let year = format!("{}", ts.year());
    let month = format!("{}/{:02}", ts.year(), ts.month());
    let day = format!("{}/{:02}/{:02}", ts.year(), ts.month(), ts.day());
    let hour = format!("{}/{:02}/{:02}/{:02}", ts.year(), ts.month(), ts.day(), ts.hour());
    (hour, day, month, year, "root".to_string())
}

pub fn node_id_to_path(node_id: &str) -> PathBuf {
    if node_id == "root" {
        return PathBuf::from("root.md");
    }
    let level = level_from_node_id(node_id);
    if level.is_leaf() {
        PathBuf::from(format!("{}.md", node_id))
    } else {
        PathBuf::from(node_id).join("summary.md")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn node_level_max_tokens() {
        assert_eq!(NodeLevel::Hour.max_tokens(), 1_000);
        assert_eq!(NodeLevel::Day.max_tokens(), 2_000);
        assert_eq!(NodeLevel::Month.max_tokens(), 4_000);
        assert_eq!(NodeLevel::Year.max_tokens(), 8_000);
        assert_eq!(NodeLevel::Root.max_tokens(), 20_000);
    }

    #[test]
    fn derive_node_ids_from_timestamp() {
        let ts = Utc.with_ymd_and_hms(2024, 3, 15, 14, 30, 0).unwrap();
        let (hour, day, month, year, root) = derive_node_ids(&ts);
        assert_eq!(hour, "2024/03/15/14");
        assert_eq!(day, "2024/03/15");
        assert_eq!(month, "2024/03");
        assert_eq!(year, "2024");
        assert_eq!(root, "root");
    }
}

