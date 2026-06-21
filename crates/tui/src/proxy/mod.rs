//! Curl-based proxy/relay service support.
//!
//! When a user configures a `[[proxy_services]]` entry with a curl command
//! template in `config.toml`, this module parses the curl, substitutes the
//! user's prompt into the body, and sends the resulting HTTP request directly
//! — bypassing the normal provider/adapter machinery.
//!
//! This is useful for:
//! - Corporate proxy/relay services that already hold an API key
//! - Custom OpenAI-compatible endpoints with non-standard headers or body fields
//! - Fallback when the primary API key fails

pub mod curl_parser;
pub mod body_template;
pub mod client;

pub use client::{ProxyClient, ProxyMode};
