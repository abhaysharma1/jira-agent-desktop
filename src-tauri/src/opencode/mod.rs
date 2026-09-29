#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct OpenCodeManager {
    pub command: String,
    pub model: Option<String>,
}

impl Default for OpenCodeManager {
    fn default() -> Self {
        Self {
            command: "opencode".to_string(),
            model: None,
        }
    }
}

impl OpenCodeManager {
    pub fn new(command: impl Into<String>, model: Option<String>) -> Self {
        Self {
            command: command.into(),
            model,
        }
    }
}
