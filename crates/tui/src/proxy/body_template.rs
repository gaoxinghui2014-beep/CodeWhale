//! Body template substitution for proxy services.
//!
//! Two substitution strategies are supported:
//!
//! 1. **Explicit placeholder**: `{{PROMPT}}` in the curl `--data-raw` body.
//! 2. **Auto-detect**: When no `{{PROMPT}}` is present, automatically find and
//!    replace `messages[*].content` fields (the last `content` in the messages
//!    array is treated as the user prompt; others are treated as system prompt).

use serde_json::Value;

/// A prepared body template ready for substitution.
#[derive(Debug, Clone)]
pub struct BodyTemplate {
    /// The parsed JSON body template (may contain placeholder values).
    template: Value,
    /// Whether we're using explicit `{{PROMPT}}` placeholder mode.
    has_explicit_placeholder: bool,
}

impl BodyTemplate {
    /// Create a new body template from a parsed JSON body.
    pub fn new(body_json: &Value) -> Self {
        let has_explicit = has_prompt_placeholder(body_json);
        Self {
            template: body_json.clone(),
            has_explicit_placeholder: has_explicit,
        }
    }

    /// Substitute user prompt (and optional system prompt) into the template.
    ///
    /// Returns the final JSON body as a string.
    pub fn substitute(&self, user_prompt: &str, system_prompt: Option<&str>) -> String {
        let mut body = self.template.clone();

        if self.has_explicit_placeholder {
            substitute_placeholder(&mut body, "{{PROMPT}}", user_prompt);
            if let Some(sys) = system_prompt {
                substitute_placeholder(&mut body, "{{SYSTEM}}", sys);
            }
            // Remove remaining placeholders
            substitute_placeholder(&mut body, "{{SYSTEM}}", "");
        } else {
            // Auto-detect: find messages array and replace content
            auto_substitute_messages(&mut body, user_prompt, system_prompt);
        }

        serde_json::to_string(&body).unwrap_or_else(|_| body.to_string())
    }

    /// Whether this template has explicit placeholders.
    pub fn has_explicit_placeholder(&self) -> bool {
        self.has_explicit_placeholder
    }
}

/// Check if a JSON value contains `{{PROMPT}}` anywhere.
fn has_prompt_placeholder(value: &Value) -> bool {
    match value {
        Value::String(s) => s.contains("{{PROMPT}}"),
        Value::Array(arr) => arr.iter().any(has_prompt_placeholder),
        Value::Object(map) => map.values().any(has_prompt_placeholder),
        _ => false,
    }
}

/// Recursively substitute `{{KEY}}` placeholders in a JSON value.
fn substitute_placeholder(value: &mut Value, placeholder: &str, replacement: &str) {
    match value {
        Value::String(s) if s.contains(placeholder) => {
            *s = s.replace(placeholder, replacement);
        }
        Value::Array(arr) => {
            for item in arr {
                substitute_placeholder(item, placeholder, replacement);
            }
        }
        Value::Object(map) => {
            for (_, v) in map {
                substitute_placeholder(v, placeholder, replacement);
            }
        }
        _ => {}
    }
}

/// Auto-detect `messages` array and substitute the last message's content with
/// the user prompt. Earlier messages with `role: "system"` or `role: "user"`
/// may also be substituted.
fn auto_substitute_messages(body: &mut Value, user_prompt: &str, system_prompt: Option<&str>) {
    // Try "messages" first, then "data-raw.messages" (nested)
    let messages = if let Some(msgs) = body.get_mut("messages") {
        msgs.as_array_mut()
    } else if let Some(data_raw) = body.get_mut("data-raw") {
        data_raw.get_mut("messages").and_then(Value::as_array_mut)
    } else {
        None
    };

    let Some(messages) = messages else {
        // No messages array found — try to find content field at any level
        substitute_content_deep(body, user_prompt);
        return;
    };

    if messages.is_empty() {
        return;
    }

    // Find system and user messages
    let mut system_idx: Option<usize> = None;
    let mut user_idx: Option<usize> = None;

    for (i, msg) in messages.iter().enumerate() {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
        match role {
            "system" => {
                if system_idx.is_none() {
                    system_idx = Some(i);
                }
            }
            "user" => {
                user_idx = Some(i); // always take the LAST user message
            }
            _ => {}
        }
    }

    // Substitute system prompt if configured
    if let (Some(sys), Some(idx)) = (system_prompt, system_idx) {
        if let Some(msg) = messages.get_mut(idx) {
            msg["content"] = Value::String(sys.to_string());
        }
    }

    // Substitute user prompt — always the last user message
    if let Some(idx) = user_idx {
        if let Some(msg) = messages.get_mut(idx) {
            msg["content"] = Value::String(user_prompt.to_string());
        }
    }
}

/// Fallback: find any "content" field deep in the JSON and substitute.
fn substitute_content_deep(value: &mut Value, content: &str) {
    match value {
        Value::Object(map) => {
            if map.contains_key("content") {
                map["content"] = Value::String(content.to_string());
            }
            for (_, v) in map {
                substitute_content_deep(v, content);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                substitute_content_deep(item, content);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_placeholder_substitution() {
        let body = json!({
            "model": "gpt",
            "messages": [{"role": "user", "content": "{{PROMPT}}"}]
        });
        let template = BodyTemplate::new(&body);
        assert!(template.has_explicit_placeholder());

        let result = template.substitute("What is force?", None);
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(
            parsed["messages"][0]["content"].as_str().unwrap(),
            "What is force?"
        );
    }

    #[test]
    fn auto_detect_substitution() {
        let body = json!({
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "original question"}
            ]
        });
        let template = BodyTemplate::new(&body);
        assert!(!template.has_explicit_placeholder());

        let result = template.substitute("What is force?", Some("Be concise."));
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let messages = parsed["messages"].as_array().unwrap();
        assert_eq!(messages[0]["content"].as_str().unwrap(), "Be concise.");
        assert_eq!(messages[1]["content"].as_str().unwrap(), "What is force?");
    }

    #[test]
    fn auto_detect_no_system() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "hello"}
            ]
        });
        let template = BodyTemplate::new(&body);
        let result = template.substitute("What is force?", None);
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(
            parsed["messages"][0]["content"].as_str().unwrap(),
            "What is force?"
        );
    }
}
