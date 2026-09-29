use serde_json::{json, Value};

use crate::domain::{PlanContent, PlanStep};

pub const RESPONSE_FORMAT: &str = "RESPONSE FORMAT\n\
===============\n\
Respond with ONLY a single JSON object and nothing else (no prose, no markdown fences):\n\
{\n  \"summary\": \"one-line summary\",\n  \"understanding\": \"what the code does and what must change\",\n  \"steps\": [{ \"order\": 1, \"description\": \"...\", \"files\": [\"path\"] }],\n  \"tests\": [\"...\"],\n  \"risks\": [\"...\"]\n}";

#[derive(Debug, Clone)]
pub struct TicketInput {
    pub key: String,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Option<String>,
}

#[allow(dead_code)]
pub fn plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": {
                "type": "string",
                "description": "One-line summary of the implementation approach"
            },
            "understanding": {
                "type": "string",
                "description": "What the code currently does and what must change"
            },
            "steps": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "order": { "type": "integer" },
                        "description": { "type": "string" },
                        "files": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["order", "description", "files"]
                }
            },
            "tests": { "type": "array", "items": { "type": "string" } },
            "risks": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["summary", "understanding", "steps", "tests", "risks"]
    })
}

pub fn build_prompt(ticket: &TicketInput, repo_path: &str) -> String {
    let criteria = ticket
        .acceptance_criteria
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("(none provided)");

    format!(
        "Analyze this JIRA ticket against the repository at {repo_path}.\n\n\
TICKET\n\
======\n\
{key}: {title}\n\n\
DESCRIPTION\n\
===========\n\
{description}\n\n\
ACCEPTANCE CRITERIA\n\
===================\n\
{criteria}\n\n\
INSTRUCTIONS\n\
============\n\
- Inspect the repository: architecture, relevant files, existing implementation, and tests.\n\
- Determine exactly which files must change and how.\n\
- Do NOT modify any files, commit, push, or create pull requests. You are read-only.\n\n\
{format_block}",
        repo_path = repo_path,
        key = ticket.key,
        title = ticket.title,
        description = ticket.description,
        criteria = criteria,
        format_block = RESPONSE_FORMAT,
    )
}

pub fn build_revision_prompt(feedback: &str) -> String {
    format!(
        "The developer reviewed the implementation plan and gave this feedback:\n\n\
{feedback}\n\n\
Revise the implementation plan to incorporate the feedback. You are read-only: do not modify files.\n\n\
{format_block}",
        feedback = feedback,
        format_block = RESPONSE_FORMAT,
    )
}

pub fn build_revision_seed_prompt(
    ticket: &TicketInput,
    current_plan: &PlanContent,
    repo_path: &str,
    feedback: &str,
) -> String {
    let plan_json = serde_json::to_string_pretty(current_plan).unwrap_or_default();
    format!(
        "You previously produced an implementation plan for JIRA ticket {key} ({title}) in the repository at {repo_path}.\n\n\
CURRENT PLAN (JSON)\n\
===================\n\
{plan_json}\n\n\
DEVELOPER FEEDBACK\n\
==================\n\
{feedback}\n\n\
Revise the plan to incorporate the feedback. You are read-only: do not modify files, commit, or push.\n\n\
{format_block}",
        key = ticket.key,
        title = ticket.title,
        repo_path = repo_path,
        plan_json = plan_json,
        feedback = feedback,
        format_block = RESPONSE_FORMAT,
    )
}

pub fn parse_plan(ticket_key: &str, value: &Value) -> Result<PlanContent, String> {
    let structured = value
        .get("info")
        .and_then(|info| info.get("structured_output"));

    let content_value = match structured {
        Some(structured) if !structured.is_null() => structured.clone(),
        _ => extract_json(value)
            .ok_or_else(|| "the agent did not return a structured plan".to_string())?,
    };

    content_from_value(ticket_key, &content_value)
}

fn content_from_value(ticket_key: &str, value: &Value) -> Result<PlanContent, String> {
    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let understanding = value
        .get("understanding")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let steps = value
        .get("steps")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(index, step)| PlanStep {
                    order: step
                        .get("order")
                        .and_then(Value::as_u64)
                        .unwrap_or(index as u64 + 1) as u32,
                    description: step
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    files: step
                        .get("files")
                        .and_then(Value::as_array)
                        .map(string_array)
                        .unwrap_or_default(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let tests = value.get("tests").and_then(Value::as_array).map(string_array).unwrap_or_default();
    let risks = value.get("risks").and_then(Value::as_array).map(string_array).unwrap_or_default();

    if summary.is_empty() && steps.is_empty() {
        return Err("the agent returned an empty plan".to_string());
    }

    Ok(PlanContent {
        ticket: ticket_key.to_string(),
        summary,
        understanding,
        steps,
        tests,
        risks,
    })
}

fn string_array(items: &Vec<Value>) -> Vec<String> {
    items
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn extract_json(value: &Value) -> Option<Value> {
    let text = crate::opencode::extract_text(value);
    let stripped = text.replace("```json", "").replace("```", "");
    let start = stripped.find('{')?;
    let slice = &stripped[start..];
    let end = balanced_end(slice)?;
    serde_json::from_str(&slice[..end]).ok()
}

fn balanced_end(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ticket() -> TicketInput {
        TicketInput {
            key: "CC-142".to_string(),
            title: "Add pagination".to_string(),
            description: "The API returns all rows.".to_string(),
            acceptance_criteria: Some("Support page and limit".to_string()),
        }
    }

    #[test]
    fn prompt_includes_ticket_fields() {
        let prompt = build_prompt(&sample_ticket(), "C:/repo");
        assert!(prompt.contains("CC-142"));
        assert!(prompt.contains("Add pagination"));
        assert!(prompt.contains("Support page and limit"));
        assert!(prompt.contains("C:/repo"));
        assert!(prompt.contains("read-only"));
    }

    #[test]
    fn parses_structured_output() {
        let value = json!({
            "info": {
                "structured_output": {
                    "summary": "Add pagination",
                    "understanding": "Endpoint returns all rows",
                    "steps": [
                        { "order": 1, "description": "Parse query", "files": ["controller.ts"] }
                    ],
                    "tests": ["default page"],
                    "risks": ["compatibility"]
                }
            }
        });
        let plan = parse_plan("CC-142", &value).unwrap();
        assert_eq!(plan.ticket, "CC-142");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].files[0], "controller.ts");
        assert_eq!(plan.tests, vec!["default page"]);
    }

    #[test]
    fn falls_back_to_text_json() {
        let value = json!({
            "parts": [
                { "type": "text", "text": "Here is the plan:\n{\"summary\":\"S\",\"understanding\":\"U\",\"steps\":[],\"tests\":[],\"risks\":[]}" }
            ]
        });
        let plan = parse_plan("CC-9", &value).unwrap();
        assert_eq!(plan.summary, "S");
        assert_eq!(plan.ticket, "CC-9");
    }

    #[test]
    fn errors_when_no_plan_present() {
        let value = json!({ "parts": [{ "type": "text", "text": "no json here" }] });
        assert!(parse_plan("CC-1", &value).is_err());
    }

    #[test]
    fn revision_prompt_includes_feedback() {
        let prompt = build_revision_prompt("Do not change the response format");
        assert!(prompt.contains("Do not change the response format"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn revision_seed_prompt_includes_prior_plan() {
        let plan = PlanContent {
            ticket: "CC-1".to_string(),
            summary: "Prior summary".to_string(),
            understanding: "u".to_string(),
            steps: vec![],
            tests: vec![],
            risks: vec![],
        };
        let prompt =
            build_revision_seed_prompt(&sample_ticket(), &plan, "C:/repo", "Use a query param");
        assert!(prompt.contains("CC-142"));
        assert!(prompt.contains("Prior summary"));
        assert!(prompt.contains("Use a query param"));
        assert!(prompt.contains("C:/repo"));
    }

    #[test]
    #[ignore = "spawns a real opencode server and spends tokens; run with --ignored"]
    fn live_planning_produces_structured_plan() {
        use crate::opencode;

        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("repo");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("README.md"),
            "# Demo\n\nA tiny demo project with a single binary.\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        let _ = std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&dir)
            .status();

        let program = opencode::resolve_program(opencode::DEFAULT_COMMAND).expect("opencode");
        let log_dir = temp.path().join("logs");
        let model = std::env::var("PLAN_TEST_MODEL")
            .unwrap_or_else(|_| opencode::models::DEFAULT_MODEL.to_string());
        let agent = std::env::var("PLAN_TEST_AGENT").ok();
        let config = opencode::models::inline_config(&model);
        let mut server = opencode::start_server(
            &program,
            &dir,
            &log_dir,
            opencode::READ_ONLY_PERMISSION,
            &config,
        )
        .expect("start server");
        let client = server.client();
        let session = client.create_session("plan-test").expect("session");

        let ticket = TicketInput {
            key: "CC-1".to_string(),
            title: "Add a greeting".to_string(),
            description: "Print a greeting from main.rs".to_string(),
            acceptance_criteria: Some("Running the binary prints a greeting".to_string()),
        };
        let prompt = build_prompt(&ticket, &dir.to_string_lossy());
        let value = client
            .prompt(
                &session,
                &prompt,
                agent.as_deref(),
                Some(model.as_str()),
                None,
            )
            .expect("prompt");
        let cost = value
            .get("info")
            .and_then(|info| info.get("cost"))
            .and_then(Value::as_f64)
            .unwrap_or(-1.0);
        eprintln!("model={model} cost={cost}");
        let plan = parse_plan("CC-1", &value).expect("parsed plan");
        eprintln!(
            "live plan: {} steps, {} tests, cost {cost}",
            plan.steps.len(),
            plan.tests.len()
        );
        assert!(!plan.steps.is_empty(), "expected at least one step");
        assert_eq!(cost, 0.0, "expected a free run");
        server.stop(Some(&session));
    }
}
