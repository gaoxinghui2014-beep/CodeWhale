//! Parse a curl command string into structured components.
//!
//! Supports:
//! - URL extraction (first positional argument, or `--url`)
//! - Method extraction (`-X` / `--request`)
//! - Header extraction (`-H` / `--header`)
//! - Cookie extraction (`-b` / `--cookie`)
//! - Body extraction (`--data-raw` / `--data`)
//! - JSON body parsing
//!
//! This is a pragmatic best-effort parser — it handles the typical curl
//! commands pasted from browser DevTools, not every edge case in the curl
//! manpage.

use anyhow::{Context, Result};

/// Parsed components of a curl command.
#[derive(Debug, Clone)]
pub struct ParsedCurl {
    /// The full request URL.
    pub url: String,
    /// HTTP method (GET, POST, PUT, etc.).
    pub method: String,
    /// Request headers as (name, value) pairs.
    pub headers: Vec<(String, String)>,
    /// Parsed JSON body, if `--data-raw` or `--data` was present.
    pub body_json: Option<serde_json::Value>,
    /// Raw body string (before JSON parsing), for templates.
    pub body_raw: Option<String>,
    /// Cookie header value, if `-b` was present.
    pub cookies: Option<String>,
}

/// Parse a curl command string into [`ParsedCurl`].
///
/// # Arguments
/// * `input` — A raw curl command string, e.g. `curl 'https://...' -H '...' --data-raw '...'`
pub fn parse_curl_command(input: &str) -> Result<ParsedCurl> {
    let input = input.trim();
    // Strip leading "curl " if present
    let input = input
        .strip_prefix("curl ")
        .or_else(|| input.strip_prefix("curl\t"))
        .unwrap_or(input);

    let tokens = tokenize(input);

    let mut url: Option<String> = None;
    let mut method: Option<String> = None;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut data_raw: Option<String> = None;
    let mut cookies: Option<String> = None;

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];

        match token.as_str() {
            "-X" | "--request" => {
                i += 1;
                if i < tokens.len() {
                    method = Some(tokens[i].clone());
                }
            }
            "-H" | "--header" => {
                i += 1;
                if i < tokens.len() {
                    if let Some((name, value)) = parse_header(&tokens[i]) {
                        headers.push((name, value));
                    }
                }
            }
            "-b" | "--cookie" => {
                i += 1;
                if i < tokens.len() {
                    cookies = Some(tokens[i].clone());
                }
            }
            "--data-raw" => {
                i += 1;
                if i < tokens.len() {
                    data_raw = Some(tokens[i].clone());
                }
            }
            "--data" => {
                i += 1;
                if i < tokens.len() && data_raw.is_none() {
                    data_raw = Some(tokens[i].clone());
                }
            }
            // Skip flags we don't care about
            t if t.starts_with("--") || t.starts_with('-') => {
                // Check if next token is a value for single-dash flags like `-H`
                // Already handled above for known flags.
            }
            // First non-flag token is the URL
            _ if url.is_none() && !token.starts_with('-') => {
                url = Some(token.clone());
            }
            _ => {}
        }
        i += 1;
    }

    let url = url
        .ok_or_else(|| anyhow::anyhow!("No URL found in curl command"))?;

    // Auto-detect POST when body is present
    let method = method.unwrap_or_else(|| {
        if data_raw.is_some() {
            "POST".to_string()
        } else {
            "GET".to_string()
        }
    });

    // Parse JSON body
    let body_json = data_raw
        .as_deref()
        .map(|s| {
            serde_json::from_str(s)
                .with_context(|| format!("Failed to parse --data-raw body as JSON: {s}"))
        })
        .transpose()?;

    Ok(ParsedCurl {
        url,
        method,
        headers,
        body_json,
        body_raw: data_raw,
        cookies,
    })
}

/// Split a curl command string into tokens, respecting single/double quotes.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Skip whitespace
        while i < chars.len() && chars[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }

        if chars[i] == '\'' {
            // Single-quoted token
            i += 1; // skip opening quote
            let mut s = String::new();
            while i < chars.len() && chars[i] != '\'' {
                s.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1; // skip closing quote
            }
            tokens.push(s);
        } else if chars[i] == '"' {
            // Double-quoted token (with basic escape support)
            i += 1; // skip opening quote
            let mut s = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1; // skip backslash
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        c => {
                            s.push('\\');
                            s.push(c);
                        }
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i < chars.len() {
                i += 1; // skip closing quote
            }
            tokens.push(s);
        } else {
            // Unquoted token
            let mut s = String::new();
            while i < chars.len() && !chars[i].is_ascii_whitespace() {
                s.push(chars[i]);
                i += 1;
            }
            tokens.push(s);
        }
    }

    tokens
}

/// Parse a `-H` header value like `"Content-Type: application/json"` into (name, value).
fn parse_header(header: &str) -> Option<(String, String)> {
    let colon_pos = header.find(':')?;
    let name = header[..colon_pos].trim().to_string();
    let value = header[colon_pos + 1..].trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some((name, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_get() {
        let parsed = parse_curl_command("curl 'https://example.com/api'").unwrap();
        assert_eq!(parsed.url, "https://example.com/api");
        assert_eq!(parsed.method, "GET");
        assert!(parsed.headers.is_empty());
        assert!(parsed.body_json.is_none());
    }

    #[test]
    fn parse_post_with_headers_and_body() {
        let input = r###"curl 'https://api.example.com/v1/chat' -H 'Content-Type: application/json' -H 'Authorization: Bearer token123' --data-raw '{"model":"gpt","messages":[{"role":"user","content":"hello"}]}'"###;
        let parsed = parse_curl_command(input).unwrap();
        assert_eq!(parsed.url, "https://api.example.com/v1/chat");
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.headers.len(), 2);
        assert_eq!(parsed.headers[0].0, "Content-Type");
        assert_eq!(parsed.headers[0].1, "application/json");
        assert_eq!(parsed.headers[1].0, "Authorization");
        assert!(parsed.body_json.is_some());
    }

    #[test]
    fn parse_cookies() {
        let input = "curl 'https://example.com' -b 'session=abc123'";
        let parsed = parse_curl_command(input).unwrap();
        assert_eq!(parsed.cookies.unwrap(), "session=abc123");
    }
}
