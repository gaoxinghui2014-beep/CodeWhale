use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::types::{derive_parent_id, estimate_tokens, level_from_node_id, node_id_to_path, NodeLevel, TreeNode, TreeStatus};

pub fn tree_dir(config: &Config, namespace: &str) -> PathBuf {
    config.workspace_dir.join("memory").join("namespaces").join(namespace).join("tree")
}

pub fn buffer_dir(config: &Config, namespace: &str) -> PathBuf {
    tree_dir(config, namespace).join("buffer")
}

pub fn node_file_path(config: &Config, namespace: &str, node_id: &str) -> PathBuf {
    tree_dir(config, namespace).join(node_id_to_path(node_id))
}

fn sanitize(namespace: &str) -> String {
    let trimmed = namespace.trim();
    trimmed.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '.'], "_").replace("__", "_")
}

pub fn validate_namespace(namespace: &str) -> Result<(), String> {
    let trimmed = namespace.trim();
    if trimmed.is_empty() { return Err("namespace must not be empty".to_string()); }
    if trimmed.contains("..") { return Err("namespace must not contain '..'".to_string()); }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') { return Err("namespace must not start with a path separator".to_string()); }
    Ok(())
}

pub fn write_node(config: &Config, node: &TreeNode) -> Result<()> {
    let path = node_file_path(config, &node.namespace, &node.node_id);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).with_context(|| format!("create dirs for {}", parent.display()))?; }
    let metadata_line = match &node.metadata { Some(m) => format!("metadata: {}\n", m), None => String::new() };
    let frontmatter = format!("---\nnode_id: \"{}\"\nnamespace: \"{}\"\nlevel: {}\nparent_id: {}\ntoken_count: {}\nchild_count: {}\ncreated_at: {}\nupdated_at: {}\n{}---\n\n",
        node.node_id,
        node.namespace,
        node.level.as_str(),
        match &node.parent_id { Some(pid) => format!("\"{}\"", pid), None => "~".to_string() },
        node.token_count,
        node.child_count,
        node.created_at.to_rfc3339(),
        node.updated_at.to_rfc3339(),
        metadata_line
    );
    let content = format!("{}\n", node.summary);
    std::fs::write(&path, format!("{}{}", frontmatter, content)).with_context(|| format!("write tree node {}", path.display()))?;
    Ok(())
}

pub fn read_node(config: &Config, namespace: &str, node_id: &str) -> Result<Option<TreeNode>> {
    let path = node_file_path(config, namespace, node_id);
    if !path.exists() { return Ok(None); }
    let raw = std::fs::read_to_string(&path).with_context(|| format!("read tree node {}", path.display()))?;
    parse_node_markdown(&raw, namespace, node_id).map(Some)
}

fn parse_node_markdown(raw: &str, namespace: &str, node_id: &str) -> Result<TreeNode> {
    let (front, body) = split_frontmatter(raw);
    let body = body.trim_end().to_string();
    let level = front.get("level").and_then(|v| crate::types::NodeLevel::from_str_label(v)).unwrap_or_else(|| level_from_node_id(node_id));
    let parent_id = front.get("parent_id").and_then(|v| { let t = v.trim().trim_matches('"'); if t=="~" || t.is_empty() { None } else { Some(t.to_string()) } }).or_else(|| derive_parent_id(node_id));
    let token_count = front.get("token_count").and_then(|v| v.parse::<u32>().ok()).unwrap_or_else(|| estimate_tokens(&body));
    let child_count = front.get("child_count").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let created_at = front.get("created_at").and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok()).map(|dt| dt.with_timezone(&Utc)).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
    let updated_at = front.get("updated_at").and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok()).map(|dt| dt.with_timezone(&Utc)).unwrap_or(created_at);
    let metadata = front.get("metadata").map(|v| v.to_string());
    Ok(TreeNode { node_id: node_id.to_string(), namespace: namespace.to_string(), level, parent_id, summary: body, token_count, child_count, created_at, updated_at, metadata })
}

fn split_frontmatter(raw: &str) -> (std::collections::HashMap<String, String>, String) {
    let mut map = std::collections::HashMap::new();
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") { return (map, raw.to_string()); }
    let after_open = &trimmed[3..];
    if let Some(close_pos) = after_open.find("\n---") {
        let fm_block = &after_open[..close_pos];
        let body_start = close_pos + 4;
        let body = after_open[body_start..].trim_start_matches('\n').to_string();
        for line in fm_block.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            if let Some(colon_pos) = line.find(':') {
                let key = line[..colon_pos].trim().to_string();
                let value = line[colon_pos+1..].trim().trim_matches('"').to_string();
                map.insert(key, value);
            }
        }
        (map, body)
    } else { (map, raw.to_string()) }
}

pub fn buffer_read(config: &Config, namespace: &str) -> Result<Vec<(String, String)>> {
    let dir = buffer_dir(config, namespace);
    if !dir.exists() { return Ok(vec![]); }
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e=="md").unwrap_or(false) {
            let name = entry.file_name().to_string_lossy().to_string();
            entries.push((name, path));
        }
    }
    entries.sort_by(|a,b| a.0.cmp(&b.0));
    let mut contents = Vec::new();
    for (name, path) in &entries {
        let raw = std::fs::read_to_string(path)?;
        let text = strip_buffer_frontmatter(&raw);
        contents.push((name.clone(), text));
    }
    Ok(contents)
}

pub fn buffer_delete(config: &Config, namespace: &str, filenames: &[String]) -> Result<()> {
    let dir = buffer_dir(config, namespace);
    for name in filenames {
        let path = dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path).with_context(|| format!("failed to remove buffer entry '{}' at {}", name, path.display()))?;
        }
    }
    Ok(())
}

pub fn read_children(config: &Config, namespace: &str, parent_id: &str) -> Result<Vec<TreeNode>> {
    let parent_level = level_from_node_id(parent_id);
    let base = tree_dir(config, namespace);
    match parent_level {
        NodeLevel::Root => read_subdirectory_summaries(&base, namespace, ""),
        NodeLevel::Year | NodeLevel::Month => read_subdirectory_summaries(&base, namespace, parent_id),
        NodeLevel::Day => read_hour_leaves(&base, namespace, parent_id),
        NodeLevel::Hour => Ok(vec![]),
    }
}

fn read_subdirectory_summaries(base: &Path, namespace: &str, parent_id: &str) -> Result<Vec<TreeNode>> {
    let scan_dir = if parent_id.is_empty() { base.to_path_buf() } else { base.join(parent_id) };
    if !scan_dir.exists() { return Ok(vec![]); }
    let mut children = Vec::new();
    for entry in std::fs::read_dir(&scan_dir)? {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
        let child_name = entry.file_name().to_string_lossy().to_string();
        if child_name == "buffer" || child_name == "buffer_backup" || child_name.chars().any(|c| !c.is_ascii_digit()) { continue; }
        let child_id = if parent_id.is_empty() { child_name } else { format!("{}/{}", parent_id, child_name) };
        let summary_path = entry.path().join("summary.md");
        if summary_path.exists() {
            let raw = std::fs::read_to_string(&summary_path)?;
            if let Ok(node) = parse_node_markdown(&raw, namespace, &child_id) {
                children.push(node);
            }
        }
    }
    children.sort_by(|a,b| a.node_id.cmp(&b.node_id));
    Ok(children)
}

fn read_hour_leaves(base: &Path, namespace: &str, day_id: &str) -> Result<Vec<TreeNode>> {
    let day_dir = base.join(day_id);
    if !day_dir.exists() { return Ok(vec![]); }
    let mut leaves = Vec::new();
    for entry in std::fs::read_dir(&day_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".md") || name == "summary.md" { continue; }
        let hour_part = name.trim_end_matches(".md");
        let node_id = format!("{}/{}", day_id, hour_part);
        let raw = std::fs::read_to_string(entry.path())?;
        if let Ok(node) = parse_node_markdown(&raw, namespace, &node_id) {
            leaves.push(node);
        }
    }
    leaves.sort_by(|a,b| a.node_id.cmp(&b.node_id));
    Ok(leaves)
}

fn strip_buffer_frontmatter(raw: &str) -> String {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") { return raw.to_string(); }
    let after_open = &trimmed[3..];
    if let Some(close_pos) = after_open.find("\n---") {
        let body_start = close_pos + 4;
        after_open[body_start..].trim_start_matches('\n').to_string()
    } else { raw.to_string() }
}

