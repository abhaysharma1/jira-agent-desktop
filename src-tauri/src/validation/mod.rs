use std::path::Path;
use std::process::Command;

use crate::domain::{PlanContent, ValidationConfig};
use crate::planning::TicketInput;

pub fn build_validation_steps(config: &ValidationConfig) -> Vec<String> {
    let mut steps = vec![
        "git diff --check".to_string(),
        "git status --porcelain".to_string(),
    ];
    for value in [
        config.test.as_ref(),
        config.lint.as_ref(),
        config.build.as_ref(),
    ] {
        if let Some(command) = value.filter(|command| !command.trim().is_empty()) {
            steps.push(command.clone());
        }
    }
    steps
}

pub fn run_command(dir: &Path, command: &str) -> (i32, String) {
    #[cfg(windows)]
    let mut process = {
        let mut process = Command::new("cmd");
        process.arg("/C").arg(command);
        process
    };
    #[cfg(not(windows))]
    let mut process = {
        let mut process = Command::new("sh");
        process.arg("-c").arg(command);
        process
    };

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        process.creation_flags(0x0800_0000);
    }

    match process.current_dir(dir).output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&stderr);
            }
            let exit_code = output.status.code().unwrap_or(-1);
            (exit_code, truncate(&text, 8000))
        }
        Err(error) => (-1, format!("failed to run command: {error}")),
    }
}

pub fn build_repair_prompt(
    ticket: &TicketInput,
    plan: &PlanContent,
    failing_output: &str,
) -> String {
    let steps = plan
        .steps
        .iter()
        .map(|step| format!("{}. {}", step.order, step.description))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "The implementation for JIRA ticket {key} ({title}) is failing its tests.\n\n\
FAILING TEST OUTPUT\n\
===================\n\
{failing_output}\n\n\
APPROVED PLAN (for context)\n\
===========================\n\
Summary: {summary}\n\
Steps:\n{steps}\n\n\
Fix the code so the tests pass. Make only the changes needed. \
Do not commit and do not push changes.",
        key = ticket.key,
        title = ticket.title,
        failing_output = failing_output,
        summary = plan.summary,
        steps = steps,
    )
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let mut truncated: String = value.chars().take(max).collect();
        truncated.push_str("\n…(truncated)");
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PlanStep;

    fn ticket() -> TicketInput {
        TicketInput {
            key: "CC-1".to_string(),
            title: "Fix".to_string(),
            description: "d".to_string(),
            acceptance_criteria: None,
        }
    }

    fn plan() -> PlanContent {
        PlanContent {
            ticket: "CC-1".to_string(),
            summary: "s".to_string(),
            understanding: "u".to_string(),
            steps: vec![PlanStep {
                order: 1,
                description: "do".to_string(),
                files: vec![],
            }],
            tests: vec![],
            risks: vec![],
        }
    }

    #[test]
    fn repair_prompt_includes_failure_and_plan() {
        let prompt = build_repair_prompt(&ticket(), &plan(), "2 failed");
        assert!(prompt.contains("CC-1"));
        assert!(prompt.contains("2 failed"));
        assert!(prompt.contains("1. do"));
        assert!(prompt.contains("Do not commit"));
    }

    #[test]
    fn run_command_reports_success_and_failure() {
        let temp = tempfile::tempdir().unwrap();
        let (ok_code, _) = run_command(temp.path(), "exit 0");
        assert_eq!(ok_code, 0);

        let (fail_code, _) = run_command(temp.path(), "exit 3");
        assert_eq!(fail_code, 3);
    }

    #[test]
    fn builds_validation_steps_from_config() {
        let config = ValidationConfig {
            test: Some("npm test".to_string()),
            lint: Some("npm run lint".to_string()),
            build: None,
        };
        let steps = build_validation_steps(&config);
        assert_eq!(steps[0], "git diff --check");
        assert_eq!(steps[1], "git status --porcelain");
        assert!(steps.contains(&"npm test".to_string()));
        assert!(steps.contains(&"npm run lint".to_string()));
        assert!(!steps.iter().any(|step| step.contains("build")));

        let empty = build_validation_steps(&ValidationConfig::default());
        assert_eq!(empty.len(), 2);
    }
}
