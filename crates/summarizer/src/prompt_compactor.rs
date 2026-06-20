use anyhow::Result;

use crate::provider::{Provider, UsageInfo};
use crate::types::estimate_tokens;
use crate::util::floor_char_boundary;

/// Compact a long text to fit within a token budget using a conservative char-per-token
/// heuristic (approx 4 chars per token). This is a best-effort truncation that
/// preserves character boundaries.
pub fn compact_text_to_budget(text: &str, max_tokens: u32) -> String {
    let tokens = estimate_tokens(text);
    if tokens <= max_tokens {
        return text.to_string();
    }
    let max_chars = (max_tokens as usize) * 4;
    let limit = max_chars.min(text.len());
    let cut = floor_char_boundary(text, limit);
    text[..cut].to_string()
}

/// Ask the provider to summarize `content` with an optional `system_prompt`,
/// enforcing a maximum token output. Returns the summary text and optional
/// usage metadata from the provider. If the provider does not return usage
/// information we conservatively estimate tokens from the prompt+response.
pub async fn summarize_with_provider(
    provider: &dyn Provider,
    system_prompt: Option<&str>,
    content: &str,
    model: &str,
    max_tokens: u32,
    temperature: f64,
) -> Result<(String, Option<UsageInfo>)> {
    let resp = provider
        .chat_with_system_with_usage(system_prompt, content, model, temperature)
        .await?;
    let text = resp.text.unwrap_or_default();
    let usage_opt = match resp.usage {
        Some(u) => Some(u),
        None => {
            // estimate from prompt+response
            let prompt_blob = match system_prompt {
                Some(s) => format!("{}\n\n{}", s, content),
                None => content.to_string(),
            };
            let input_tokens_est = estimate_tokens(&prompt_blob) as u64;
            let output_tokens_est = estimate_tokens(&text) as u64;
            Some(UsageInfo { input_tokens: input_tokens_est, output_tokens: output_tokens_est, context_window: 0, cached_input_tokens: 0, charged_amount_usd: 0.0 })
        }
    };
    // enforce char cap derived from token budget
    let max_chars = (max_tokens as usize) * 4;
    let char_limit = max_chars.min(text.len());
    let out = if text.len() > char_limit { let cut = floor_char_boundary(&text, char_limit); text[..cut].to_string() } else { text };
    Ok((out, usage_opt))
}

