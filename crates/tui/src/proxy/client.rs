//! Proxy HTTP client that sends requests based on parsed curl configuration.
//!
//! Supports both streaming (SSE / line-delimited JSON) and non-streaming modes.
//! Response parsing adapts based on the detected format.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;

use super::curl_parser::{ParsedCurl, parse_curl_command};
use super::body_template::BodyTemplate;

use crate::llm_client::StreamEventBox;
use crate::models::{ContentBlockStart, Delta, MessageDelta, MessageResponse, StreamEvent, Usage};

/// Proxy operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    /// Always use the proxy (bypass API key entirely).
    Always,
    /// Only use the proxy when the normal API key path fails.
    Fallback,
}

impl ProxyMode {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "always" => Self::Always,
            _ => Self::Fallback,
        }
    }
}

/// Rate limiter token bucket (mirrors the one in client.rs).
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill: std::time::Instant,
}

impl TokenBucket {
    fn from_env() -> Self {
        let rps: f64 = std::env::var("DEEPSEEK_RATE_LIMIT_RPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8.0);
        let burst: f64 = std::env::var("DEEPSEEK_RATE_LIMIT_BURST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(16.0);
        Self {
            tokens: burst,
            max_tokens: burst,
            refill_rate: rps,
            last_refill: std::time::Instant::now(),
        }
    }

    async fn wait_for_token(&mut self) {
        loop {
            let now = std::time::Instant::now();
            let elapsed = now.duration_since(self.last_refill).as_secs_f64();
            self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
            self.last_refill = now;

            if self.tokens >= 1.0 {
                self.tokens -= 1.0;
                return;
            }

            let wait = Duration::from_secs_f64((1.0 - self.tokens) / self.refill_rate);
            tokio::time::sleep(wait).await;
        }
    }
}

/// A ready-to-use proxy client for a single curl template.
#[derive(Clone)]
pub struct ProxyClient {
    /// Human-readable name for this proxy service.
    name: String,
    http_client: reqwest::Client,
    pub(crate) parsed_curl: ParsedCurl,
    body_template: BodyTemplate,
    mode: ProxyMode,
    rate_limiter: Arc<AsyncMutex<TokenBucket>>,
}

impl ProxyClient {
    /// Create a new proxy client from a curl template string.
    pub fn new(
        name: &str,
        curl_template: &str,
        mode_str: &str,
        insecure_skip_tls_verify: bool,
    ) -> Result<Self> {
        let parsed_curl = parse_curl_command(curl_template)
            .context("Failed to parse curl_template for proxy service")?;

        let body_template = parsed_curl
            .body_json
            .as_ref()
            .map(BodyTemplate::new)
            .unwrap_or_else(|| BodyTemplate::new(&Value::Null));

        let mut header_map = HeaderMap::new();
        for (hdr_name, value) in &parsed_curl.headers {
            let hn = HeaderName::from_bytes(hdr_name.as_bytes())
                .with_context(|| format!("Invalid header name: {hdr_name}"))?;
            let hv = HeaderValue::from_str(value)
                .with_context(|| format!("Invalid header value for {hdr_name}: {value}"))?;
            header_map.insert(hn, hv);
        }

        let mut builder = crate::tls::reqwest_client_builder()
            .default_headers(header_map)
            .connect_timeout(Duration::from_secs(30))
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .min_tls_version(reqwest::tls::Version::TLS_1_2);

        if insecure_skip_tls_verify {
            builder = builder.danger_accept_invalid_certs(true);
        }

        let http_client = builder.build()?;

        Ok(Self {
            name: name.to_string(),
            http_client,
            parsed_curl,
            body_template,
            mode: ProxyMode::from_str(mode_str),
            rate_limiter: Arc::new(AsyncMutex::new(TokenBucket::from_env())),
        })
    }

    /// Human-readable name for this proxy.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The operating mode for this proxy.
    pub fn mode(&self) -> ProxyMode {
        self.mode
    }

    /// Check if this proxy's URL looks OpenAI-compatible.
    pub fn is_openai_compatible(&self) -> bool {
        let url_lower = self.parsed_curl.url.to_ascii_lowercase();
        url_lower.contains("chat/completions")
    }

    /// Send a non-streaming request through the proxy.
    pub async fn send(
        &self,
        user_prompt: &str,
        system_prompt: Option<&str>,
    ) -> Result<String> {
        self.rate_limiter.lock().await.wait_for_token().await;

        let body_str = self.body_template.substitute(user_prompt, system_prompt);
        let body_value: Value = serde_json::from_str(&body_str)
            .context("Failed to serialize substituted body as JSON")?;

        let mut request = match self.parsed_curl.method.to_uppercase().as_str() {
            "POST" => self.http_client.post(&self.parsed_curl.url),
            "GET" => self.http_client.get(&self.parsed_curl.url),
            "PUT" => self.http_client.put(&self.parsed_curl.url),
            m => bail!("Unsupported HTTP method in proxy curl: {m}"),
        };

        request = request.json(&body_value);

        if let Some(ref cookies) = self.parsed_curl.cookies {
            request = request.header("Cookie", cookies.as_str());
        }

        let response = request.send().await.context("Proxy request failed")?;
        let status = response.status();
        let text = response.text().await.context("Failed to read proxy response")?;

        if !status.is_success() {
            bail!("Proxy returned HTTP {status}: {text}");
        }

        Ok(text)
    }

    /// Send a streaming request through the proxy.
    ///
    /// Supports two response formats:
    /// - OpenAI-compatible SSE (`data: {...}\n\n`)
    /// - Line-delimited JSON (`{...}\n{...}\n`, Volvo Group GPT format)
    pub async fn send_stream(
        &self,
        user_prompt: &str,
        system_prompt: Option<&str>,
    ) -> Result<StreamEventBox> {
        self.rate_limiter.lock().await.wait_for_token().await;

        let body_str = self.body_template.substitute(user_prompt, system_prompt);
        let body_value: Value = serde_json::from_str(&body_str)
            .context("Failed to serialize substituted body as JSON")?;

        let mut request = self.http_client.post(&self.parsed_curl.url);
        request = request.json(&body_value);

        if let Some(ref cookies) = self.parsed_curl.cookies {
            request = request.header("Cookie", cookies.as_str());
        }

        let response = request.send().await.context("Proxy stream request failed")?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            bail!("Proxy stream returned HTTP {status}: {text}");
        }

        let mut byte_stream = response.bytes_stream();
        let is_openai = self.is_openai_compatible();

        let stream = async_stream::stream! {
            use futures_util::StreamExt;

            yield Ok(StreamEvent::MessageStart {
                message: MessageResponse {
                    id: String::new(),
                    r#type: "message".to_string(),
                    role: "assistant".to_string(),
                    content: Vec::new(),
                    model: String::new(),
                    stop_reason: None,
                    stop_sequence: None,
                    container: None,
                    usage: Usage::default(),
                },
            });

            let mut byte_buf: Vec<u8> = Vec::new();
            let mut content_idx: u32 = 0;
            let mut text_started = false;
            let idle_timeout = Duration::from_secs(300);

            'stream: loop {
                let chunk_result = match tokio::time::timeout(idle_timeout, byte_stream.next()).await {
                    Ok(Some(result)) => result,
                    Ok(None) => break,
                    Err(_) => {
                        yield Err(anyhow::anyhow!("Proxy SSE stream idle timeout"));
                        break;
                    }
                };

                let chunk = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        yield Err(anyhow::anyhow!("Proxy stream read error: {e}"));
                        break;
                    }
                };

                byte_buf.extend_from_slice(&chunk);

                while let Some(newline_pos) = byte_buf.iter().position(|&b| b == b'\n') {
                    let mut end = newline_pos;
                    if end > 0 && byte_buf[end - 1] == b'\r' {
                        end -= 1;
                    }
                    // Copy the line bytes before draining (avoids borrow conflict)
                    let line_bytes = byte_buf[..end].to_vec();
                    byte_buf.drain(..newline_pos + 1);

                    let line = String::from_utf8_lossy(&line_bytes);
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }

                    if is_openai {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                break 'stream;
                            }
                            let parsed: Value = match serde_json::from_str(data) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            if let Some(events) = parse_openai_sse_chunk(
                                &parsed, &mut content_idx, &mut text_started,
                            ) {
                                for event in events {
                                    yield Ok(event);
                                }
                            }
                        }
                    } else {
                        let parsed: Value = match serde_json::from_str(&line) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        if let Some(content) = extract_content_line_json(&parsed) {
                            if !text_started {
                                text_started = true;
                                yield Ok(StreamEvent::ContentBlockStart {
                                    index: content_idx,
                                    content_block: ContentBlockStart::Text {
                                        text: String::new(),
                                    },
                                });
                            }
                            yield Ok(StreamEvent::ContentBlockDelta {
                                index: content_idx,
                                delta: Delta::TextDelta { text: content },
                            });
                        }
                    }
                }
            }

            if text_started {
                yield Ok(StreamEvent::ContentBlockStop { index: content_idx });
            }
            yield Ok(StreamEvent::MessageStop);
        };

        Ok(Pin::from(Box::new(stream)
            as Box<dyn futures_util::Stream<Item = Result<StreamEvent>> + Send>))
    }
}

/// Parse an OpenAI-compatible SSE chunk JSON and extract stream events.
fn parse_openai_sse_chunk(
    value: &Value,
    content_idx: &mut u32,
    text_started: &mut bool,
) -> Option<Vec<StreamEvent>> {
    let mut events = Vec::new();

    // Check for usage-only chunk
    if let Some(usage) = value.get("usage") {
        events.push(StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: None,
                stop_sequence: None,
            },
            usage: Some(Usage {
                input_tokens: usage
                    .get("prompt_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                output_tokens: usage
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                ..Usage::default()
            }),
        });
        return Some(events);
    }

    // Parse choices
    let choices = value.get("choices")?.as_array()?;
    for choice in choices {
        if let Some(delta) = choice.get("delta") {
            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                if !*text_started {
                    *text_started = true;
                    events.push(StreamEvent::ContentBlockStart {
                        index: *content_idx,
                        content_block: ContentBlockStart::Text {
                            text: String::new(),
                        },
                    });
                }
                events.push(StreamEvent::ContentBlockDelta {
                    index: *content_idx,
                    delta: Delta::TextDelta {
                        text: content.to_string(),
                    },
                });
            }
        } else if let Some(message) = choice.get("message") {
            if let Some(content) = message.get("content").and_then(Value::as_str) {
                if !*text_started {
                    *text_started = true;
                    events.push(StreamEvent::ContentBlockStart {
                        index: *content_idx,
                        content_block: ContentBlockStart::Text {
                            text: String::new(),
                        },
                    });
                }
                events.push(StreamEvent::ContentBlockDelta {
                    index: *content_idx,
                    delta: Delta::TextDelta {
                        text: content.to_string(),
                    },
                });
            }
        }
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

/// Extract content from a line-delimited JSON object (Volvo Group GPT format).
///
/// Format: `{"choices": [{"messages": [{"content": "..."}]}]}`
fn extract_content_line_json(value: &Value) -> Option<String> {
    let content = value
        .get("choices")?
        .get(0)?
        .get("messages")?
        .get(0)?
        .get("content")?
        .as_str()?;
    Some(content.to_string())
}
