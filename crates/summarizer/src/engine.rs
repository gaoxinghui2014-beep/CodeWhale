use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

use crate::config::Config;
use crate::event::TreeEvent;
use crate::provider::{ChatResponse, Provider, UsageInfo};
use crate::store;
use crate::types::{derive_node_ids, derive_parent_id, estimate_tokens, level_from_node_id, NodeLevel, TreeNode};
use crate::util::floor_char_boundary;

const SUMMARIZATION_TEMP: f64 = 0.3;
const MAX_SUMMARY_CHARS: usize = 20_000 * 4;

pub async fn run_summarization<F: Fn(TreeEvent) + Send + Sync + 'static>(
    config: &Config,
    provider: &dyn Provider,
    model: &str,
    namespace: &str,
    _ts: DateTime<Utc>,
    publish: F,
) -> Result<Option<TreeNode>> {
    let buffered = store::buffer_read(config, namespace)?;
    if buffered.is_empty() {
        return Ok(None);
    }
    let buffer_filenames: Vec<String> = buffered.iter().map(|(name, _)| name.clone()).collect();
    let hour_groups = group_by_hour(&buffered);
    let mut all_propagation_ids: Vec<(String, NodeLevel)> = Vec::new();
    let mut last_hour_node: Option<TreeNode> = None;
    for (hour_id, entries) in &hour_groups {
        let combined = entries.join("\n\n---\n\n");
        let (existing_summary, existing_created_at) = match store::read_node(config, namespace, hour_id)? { Some(existing) => (Some(existing.summary), Some(existing.created_at)), None => (None, None) };
        let to_summarize = if let Some(ref prev) = existing_summary { format!("{}\n\n---\n\n{}", prev, combined) } else { combined };
        let (hour_summary, hour_usage) = summarize_to_limit(provider, &to_summarize, NodeLevel::Hour.max_tokens(), "hour", hour_id, model, namespace).await.context("summarize hour leaf")?;
        let now = Utc::now();
        let hour_node = TreeNode {
            node_id: hour_id.clone(),
            namespace: namespace.to_string(),
            level: NodeLevel::Hour,
            parent_id: derive_parent_id(hour_id),
            summary: hour_summary.clone(),
            token_count: estimate_tokens(&hour_summary),
            child_count: 0,
            created_at: existing_created_at.unwrap_or(now),
            updated_at: now,
            metadata: hour_usage.as_ref().map(|u| format!("{{\"input_tokens\":{},\"output_tokens\":{}}}", u.input_tokens, u.output_tokens)),
        };
        store::write_node(config, &hour_node)?;
        publish(TreeEvent::TreeSummarizerHourCompleted { namespace: namespace.to_string(), node_id: hour_id.clone(), token_count: hour_node.token_count });
        // Publish LLM usage event when the provider returned precise usage info.
        if let Some(ref u) = hour_usage {
            // clamp into u32 for the event shape; UsageInfo may use u64.
            publish(TreeEvent::TreeSummarizerLlmUsage {
                namespace: namespace.to_string(),
                node_id: hour_id.clone(),
                input_tokens: u.input_tokens as u32,
                output_tokens: u.output_tokens as u32,
            });
        }
        let (_, day_id, month_id, year_id, root_id) = derive_node_ids_from_hour_id(hour_id);
        all_propagation_ids.push((day_id, NodeLevel::Day));
        all_propagation_ids.push((month_id, NodeLevel::Month));
        all_propagation_ids.push((year_id, NodeLevel::Year));
        all_propagation_ids.push((root_id, NodeLevel::Root));
        last_hour_node = Some(hour_node);
    }
    let mut seen = std::collections::HashSet::new();
    let mut failed: Vec<String> = Vec::new();
    let mut propagated: u32 = 0;
    for level in [NodeLevel::Day, NodeLevel::Month, NodeLevel::Year, NodeLevel::Root] {
        for (node_id, node_level) in &all_propagation_ids {
            if *node_level == level && seen.insert(node_id.clone()) {
                match propagate_node(config, provider, namespace, node_id, level, model, &publish).await {
                    Ok(()) => propagated += 1,
                    Err(e) => { log::warn!("[tree_summarizer] propagate failed (continuing) namespace='{namespace}' node={node_id} level={}: {e:#}", level.as_str()); failed.push(node_id.clone()); }
                }
            }
        }
    }
    if failed.is_empty() {
        store::buffer_delete(config, namespace, &buffer_filenames).context("delete buffer entries after successful summarization")?;
    }
    Ok(last_hour_node)
}

pub async fn propagate_node<F: Fn(TreeEvent) + Send + Sync + 'static>(
    config: &Config,
    provider: &dyn Provider,
    namespace: &str,
    node_id: &str,
    level: NodeLevel,
    model: &str,
    publish: &F,
) -> Result<()> {
    let children = crate::store::read_children(config, namespace, node_id)?;
    if children.is_empty() { return Ok(()); }
    let child_count = children.len() as u32;
    let combined: String = children.iter().map(|c| format!("## {} ({})\n\n{}", c.node_id, c.level.as_str(), c.summary)).collect::<Vec<_>>().join("\n\n---\n\n");
    let combined_tokens = estimate_tokens(&combined);
    let max_tokens = level.max_tokens();
    let (summary, summary_usage) = if combined_tokens <= max_tokens { (combined, None) } else { let (s, u) = summarize_to_limit(provider, &combined, max_tokens, level.as_str(), node_id, model, namespace).await?; (s, u) };
    let now = Utc::now();
    let existing = store::read_node(config, namespace, node_id)?;
    let created_at = existing.map(|n| n.created_at).unwrap_or(now);
    let node = TreeNode { node_id: node_id.to_string(), namespace: namespace.to_string(), level, parent_id: derive_parent_id(node_id), summary: summary.clone(), token_count: estimate_tokens(&summary), child_count, created_at, updated_at: now, metadata: summary_usage.as_ref().map(|u| format!("{{\"input_tokens\":{},\"output_tokens\":{}}}", u.input_tokens, u.output_tokens)) };
    store::write_node(config, &node)?;
    publish(TreeEvent::TreeSummarizerPropagated { namespace: namespace.to_string(), node_id: node_id.to_string(), level: level.as_str().to_string(), token_count: node.token_count });
    if let Some(ref u) = summary_usage {
        publish(TreeEvent::TreeSummarizerLlmUsage { namespace: namespace.to_string(), node_id: node_id.to_string(), input_tokens: u.input_tokens as u32, output_tokens: u.output_tokens as u32 });
    }
    Ok(())
}

async fn summarize_to_limit(
    provider: &dyn Provider,
    content: &str,
    max_tokens: u32,
    level_name: &str,
    node_id: &str,
    model: &str,
    namespace: &str,
) -> Result<(String, Option<UsageInfo>)> {
    let max_chars = (max_tokens as usize) * 4;
    let system_prompt = format!("You are a hierarchical summarizer... Context: You are summarizing at the {level_name} level for node '{node_id}'.");
    let chat_resp: ChatResponse = provider.chat_with_system_with_usage(Some(&system_prompt), content, model, SUMMARIZATION_TEMP).await.with_context(|| format!("LLM summarization failed for node {node_id} (level={level_name})"))?;
    let response_text = chat_resp.text.unwrap_or_default();
    let usage_opt: Option<UsageInfo> = match chat_resp.usage { Some(u) => Some(u), None => { let prompt_blob = format!("{}\n\n{}", system_prompt, content); let input_tokens_est = crate::types::estimate_tokens(&prompt_blob) as u64; let output_tokens_est = crate::types::estimate_tokens(&response_text) as u64; Some(UsageInfo { input_tokens: input_tokens_est, output_tokens: output_tokens_est, context_window: 0, cached_input_tokens: 0, charged_amount_usd: 0.0 }) } };
    if let Some(ref usage) = usage_opt { /* caller may publish via adapter */ }
    let char_limit = max_chars.min(MAX_SUMMARY_CHARS);
    let response = if response_text.len() > char_limit { let truncated = &response_text[..floor_char_boundary(&response_text, char_limit)]; truncated.to_string() } else { response_text.clone() };
    Ok((response, usage_opt))
}

fn group_by_hour(entries: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (filename, content) in entries {
        let hour_id = hour_id_from_buffer_filename(filename).unwrap_or_else(|| { let now = Utc::now(); let (hour, _, _, _, _) = derive_node_ids(&now); hour });
        groups.entry(hour_id).or_default().push(content.clone());
    }
    groups
}

fn hour_id_from_buffer_filename(filename: &str) -> Option<String> {
    let ts_str = filename.split('_').next()?;
    let millis: i64 = ts_str.parse().ok()?;
    let dt = DateTime::from_timestamp_millis(millis)?;
    let (hour, _, _, _, _) = derive_node_ids(&dt);
    Some(hour)
}

fn derive_node_ids_from_hour_id(hour_id: &str) -> (String, String, String, String, String) {
    let parts: Vec<&str> = hour_id.split('/').collect();
    if parts.len() == 4 {
        let year = parts[0].to_string();
        let month = format!("{}/{}", parts[0], parts[1]);
        let day = format!("{}/{}/{}", parts[0], parts[1], parts[2]);
        (hour_id.to_string(), day, month, year, "root".to_string())
    } else {
        (hour_id.to_string(), "unknown".to_string(), "unknown".to_string(), "unknown".to_string(), "root".to_string())
    }
}

