use anyhow::Result;
use async_trait::async_trait;

/// Usage information returned by providers.
#[derive(Debug, Clone)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub context_window: u32,
    pub cached_input_tokens: u64,
    pub charged_amount_usd: f64,
}

/// Rich chat response including optional usage metadata.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub text: Option<String>,
    // tool_calls omitted in this minimal API
    pub usage: Option<UsageInfo>,
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat_with_system_with_usage(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> Result<ChatResponse>;
}

