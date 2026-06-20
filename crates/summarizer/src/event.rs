/// Lightweight summarizer events emitted by the summarizer engine.
#[derive(Debug, Clone)]
pub enum TreeEvent {
    TreeSummarizerHourCompleted {
        namespace: String,
        node_id: String,
        token_count: u32,
    },
    TreeSummarizerPropagated {
        namespace: String,
        node_id: String,
        level: String,
        token_count: u32,
    },
    TreeSummarizerLlmUsage {
        namespace: String,
        node_id: String,
        input_tokens: u32,
        output_tokens: u32,
    },
    TreeSummarizerRebuildCompleted { namespace: String, total_nodes: u64 },
}

