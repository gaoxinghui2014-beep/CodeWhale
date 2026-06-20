//! Summarizer crate: extracted memory-tree summarization logic.
pub mod config;
pub mod event;
pub mod provider;
pub mod types;
pub mod util;
pub mod store;
pub mod engine;
pub mod prompt_compactor;

pub use types::*;
pub use event::TreeEvent;
pub use prompt_compactor::{compact_text_to_budget, summarize_with_provider};

