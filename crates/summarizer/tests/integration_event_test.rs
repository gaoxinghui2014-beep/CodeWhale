use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use chrono::Utc;

use summarizer::config::Config;
use summarizer::engine;
use summarizer::event::TreeEvent;
use summarizer::provider::{ChatResponse, Provider, UsageInfo};

struct MockProvider;

#[async_trait::async_trait]
impl Provider for MockProvider {
    async fn chat_with_system_with_usage(
        &self,
        _system_prompt: Option<&str>,
        message: &str,
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let summary = format!("SUMMARY: {}", &message.chars().take(200).collect::<String>());
        Ok(ChatResponse {
            text: Some(summary),
            usage: Some(UsageInfo {
                input_tokens: 12,
                output_tokens: 6,
                context_window: 4096,
                cached_input_tokens: 0,
                charged_amount_usd: 0.0,
            }),
        })
    }
}

#[tokio::test]
async fn integration_publishes_usage_events() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let workspace = tmp.path().to_path_buf();

    // create buffer dir and write a couple of entries
    let ns_dir = workspace.join("memory").join("namespaces").join("test_ns").join("tree").join("buffer");
    std::fs::create_dir_all(&ns_dir)?;
    let ts = Utc::now().timestamp_millis();
    let f1 = ns_dir.join(format!("{}_a.md", ts));
    std::fs::write(&f1, "first buffered note")?;
    let f2 = ns_dir.join(format!("{}_b.md", ts+1));
    std::fs::write(&f2, "second buffered note")?;

    let config = Config { workspace_dir: workspace.clone() };

    let events: Arc<Mutex<Vec<TreeEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_cloned = events.clone();
    let publish = move |ev: TreeEvent| {
        let events_cloned = events_cloned.clone();
        // push synchronously is fine for this test
        events_cloned.lock().unwrap().push(ev);
    };

    let provider = MockProvider;
    let result = engine::run_summarization(&config, &provider, "test-model", "test_ns", Utc::now(), publish).await?;
    // Should return the last hour node
    let last_node = result.expect("expected a last hour node");
    // Verify metadata frontmatter was written into the node when UsageInfo was returned
    let meta = last_node.metadata.expect("expected metadata in hour node");
    // Parse as JSON and check fields
    let v: serde_json::Value = serde_json::from_str(&meta).expect("metadata should be valid JSON");
    assert!(v.get("input_tokens").is_some(), "metadata missing input_tokens");
    assert!(v.get("output_tokens").is_some(), "metadata missing output_tokens");

    let locked = events.lock().unwrap();
    // Expect at least one HourCompleted and one LlmUsage event
    let mut found_hour = false;
    let mut found_usage = false;
    for ev in locked.iter() {
        match ev {
            TreeEvent::TreeSummarizerHourCompleted { namespace: _, node_id: _, token_count: _ } => found_hour = true,
            TreeEvent::TreeSummarizerLlmUsage { namespace: _, node_id: _, input_tokens: _, output_tokens: _ } => found_usage = true,
            _ => {}
        }
    }
    assert!(found_hour, "expected TreeSummarizerHourCompleted event");
    assert!(found_usage, "expected TreeSummarizerLlmUsage event");

    Ok(())
}

