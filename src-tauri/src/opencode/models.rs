use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::domain::ModelInfo;

pub const DEFAULT_MODEL: &str = "opencode/nemotron-3-ultra-free";

pub const FALLBACK_FREE_MODELS: &[&str] = &[
    "opencode/nemotron-3-ultra-free",
    "opencode/big-pickle",
    "opencode/ling-3.0-flash-fin-free",
    "opencode/longcat-2.5-preview-free",
    "opencode/mimo-v2.6-flash-free",
    "opencode/muse-spark-1.3-contributor-free",
    "opencode/nemotron-3.5-lightning-free",
    "opencode/space-bunny-free",
];

pub fn fallback_models() -> Vec<ModelInfo> {
    FALLBACK_FREE_MODELS
        .iter()
        .map(|id| {
            let (provider_id, model_id) = id.split_once('/').unwrap_or(("opencode", id));
            ModelInfo {
                id: id.to_string(),
                name: model_id.to_string(),
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
                free: true,
                cost_input: 0.0,
                cost_output: 0.0,
            }
        })
        .collect()
}

pub fn discover_models(program: &Path) -> Vec<ModelInfo> {
    let output = match Command::new(program).args(["models", "--verbose"]).output() {
        Ok(output) => output,
        Err(_) => return fallback_models(),
    };
    let raw = String::from_utf8_lossy(&output.stdout);
    let models = parse_models(&raw);
    if models.is_empty() {
        fallback_models()
    } else {
        models
    }
}

pub fn parse_models(raw: &str) -> Vec<ModelInfo> {
    let mut models = Vec::new();
    let mut current: Option<String> = None;
    let mut buffer = String::new();

    for line in raw.lines() {
        if is_model_id(line) {
            if let Some(id) = current.take() {
                if let Some(model) = parse_block(&id, &buffer) {
                    models.push(model);
                }
            }
            current = Some(line.trim().to_string());
            buffer.clear();
        } else if current.is_some() {
            buffer.push_str(line);
            buffer.push('\n');
        }
    }

    if let Some(id) = current {
        if let Some(model) = parse_block(&id, &buffer) {
            models.push(model);
        }
    }

    models
}

#[allow(dead_code)]
pub fn free_models(models: &[ModelInfo]) -> Vec<ModelInfo> {
    models.iter().filter(|model| model.free).cloned().collect()
}

#[allow(dead_code)]
pub fn find_model<'a>(models: &'a [ModelInfo], id: &str) -> Option<&'a ModelInfo> {
    models.iter().find(|model| model.id == id)
}

#[allow(dead_code)]
pub fn is_free(models: &[ModelInfo], id: &str) -> bool {
    find_model(models, id).map(|model| model.free).unwrap_or(false)
}

pub fn inline_config(default_model: &str) -> String {
    serde_json::json!({
        "model": default_model,
        "small_model": DEFAULT_MODEL,
        "autoupdate": false,
        "share": "disabled",
    })
    .to_string()
}

fn is_model_id(line: &str) -> bool {
    if line.starts_with(char::is_whitespace) {
        return false;
    }
    let mut parts = line.splitn(2, '/');
    let (provider, model) = match (parts.next(), parts.next()) {
        (Some(provider), Some(model)) => (provider, model),
        _ => return false,
    };
    let valid = |value: &str| {
        !value.is_empty()
            && value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch))
    };
    valid(provider) && valid(model)
}

fn parse_block(line_id: &str, json: &str) -> Option<ModelInfo> {
    let value: Value = serde_json::from_str(json).ok()?;
    let (fallback_provider, fallback_model) = line_id.split_once('/').unwrap_or(("", line_id));
    let provider_id = value
        .get("providerID")
        .and_then(Value::as_str)
        .unwrap_or(fallback_provider);
    let model_id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(fallback_model);
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(model_id)
        .to_string();
    let cost_input = value
        .get("cost")
        .and_then(|cost| cost.get("input"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let cost_output = value
        .get("cost")
        .and_then(|cost| cost.get("output"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let free = cost_input == 0.0 && cost_output == 0.0;

    Some(ModelInfo {
        id: line_id.to_string(),
        name,
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        free,
        cost_input,
        cost_output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"opencode/big-pickle
{
  "id": "big-pickle",
  "providerID": "opencode",
  "name": "Big Pickle",
  "cost": { "input": 0, "output": 0 }
}
opencode/claude-sonnet-5
{
  "id": "claude-sonnet-5",
  "providerID": "opencode",
  "name": "Claude Sonnet 5",
  "cost": { "input": 3, "output": 15 }
}
deepseek/deepseek-v4-pro
{
  "id": "deepseek-v4-pro",
  "providerID": "deepseek",
  "name": "DeepSeek V4 Pro",
  "cost": { "input": 0.28, "output": 0.42 }
}
"#;

    #[test]
    fn parses_models_and_classifies_free() {
        let models = parse_models(SAMPLE);
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id, "opencode/big-pickle");
        assert!(models[0].free);
        assert_eq!(models[1].provider_id, "opencode");
        assert!(!models[1].free);
        assert_eq!(models[2].provider_id, "deepseek");
        assert!(!models[2].free);
    }

    #[test]
    fn free_filter_and_lookup() {
        let models = parse_models(SAMPLE);
        let free = free_models(&models);
        assert_eq!(free.len(), 1);
        assert!(is_free(&models, "opencode/big-pickle"));
        assert!(!is_free(&models, "deepseek/deepseek-v4-pro"));
        assert!(!is_free(&models, "missing/model"));
    }

    #[test]
    fn rejects_non_model_lines() {
        assert!(is_model_id("opencode/big-pickle"));
        assert!(!is_model_id("  \"id\": \"big-pickle\""));
        assert!(!is_model_id("{"));
        assert!(!is_model_id("just-a-word"));
    }

    #[test]
    fn inline_config_pins_free_models() {
        let config = inline_config(DEFAULT_MODEL);
        let value: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(value["small_model"], DEFAULT_MODEL);
        assert_eq!(value["autoupdate"], false);
        assert_eq!(value["share"], "disabled");
    }
}
