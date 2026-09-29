use crate::domain::PlanContent;
use crate::planning::TicketInput;

pub fn build_implementation_prompt(ticket: &TicketInput, plan: &PlanContent) -> String {
    let steps = plan
        .steps
        .iter()
        .map(|step| {
            if step.files.is_empty() {
                format!("{}. {}", step.order, step.description)
            } else {
                format!(
                    "{}. {} (files: {})",
                    step.order,
                    step.description,
                    step.files.join(", ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let tests = if plan.tests.is_empty() {
        "(none specified)".to_string()
    } else {
        plan.tests
            .iter()
            .map(|test| format!("- {test}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let risks = if plan.risks.is_empty() {
        "(none specified)".to_string()
    } else {
        plan.risks
            .iter()
            .map(|risk| format!("- {risk}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "You are implementing JIRA ticket {key}: {title}.\n\n\
JIRA TICKET\n\
===========\n\
{description}\n\n\
APPROVED IMPLEMENTATION PLAN\n\
============================\n\
Summary: {summary}\n\n\
Understanding:\n{understanding}\n\n\
Steps:\n{steps}\n\n\
Tests:\n{tests}\n\n\
Risks:\n{risks}\n\n\
INSTRUCTIONS\n\
============\n\
- The plan above was explicitly approved by the developer. Implement it.\n\
- Follow the existing repository conventions.\n\
- Do not make unrelated changes.\n\
- Run the relevant tests and fix failures.\n\
- Do not commit and do not push changes.",
        key = ticket.key,
        title = ticket.title,
        description = ticket.description,
        summary = plan.summary,
        understanding = plan.understanding,
        steps = steps,
        tests = tests,
        risks = risks,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PlanStep;

    fn ticket() -> TicketInput {
        TicketInput {
            key: "CC-142".to_string(),
            title: "Add pagination".to_string(),
            description: "The API returns all rows.".to_string(),
            acceptance_criteria: None,
        }
    }

    fn plan() -> PlanContent {
        PlanContent {
            ticket: "CC-142".to_string(),
            summary: "Add page/limit".to_string(),
            understanding: "Controller returns all rows".to_string(),
            steps: vec![PlanStep {
                order: 1,
                description: "Parse query params".to_string(),
                files: vec!["controller.ts".to_string()],
            }],
            tests: vec!["default page".to_string()],
            risks: vec!["compatibility".to_string()],
        }
    }

    #[test]
    fn prompt_includes_approved_plan() {
        let prompt = build_implementation_prompt(&ticket(), &plan());
        assert!(prompt.contains("CC-142"));
        assert!(prompt.contains("Add page/limit"));
        assert!(prompt.contains("Parse query params"));
        assert!(prompt.contains("controller.ts"));
        assert!(prompt.contains("default page"));
        assert!(prompt.contains("Do not commit"));
    }

    #[test]
    #[ignore = "spawns a real opencode server and spends tokens; run with --ignored"]
    fn live_implementation_writes_a_file() {
        use crate::opencode;

        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), "# Demo\n").unwrap();
        let _ = std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&dir)
            .status();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&dir)
            .status();
        let _ = std::process::Command::new("git")
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "commit",
                "-q",
                "-m",
                "init",
            ])
            .current_dir(&dir)
            .status();

        let program = opencode::resolve_program(opencode::DEFAULT_COMMAND).expect("opencode");
        let log_dir = temp.path().join("logs");
        let config = opencode::models::inline_config(opencode::models::DEFAULT_MODEL);
        let mut server = opencode::start_server(
            &program,
            &dir,
            &log_dir,
            opencode::IMPLEMENT_PERMISSION,
            &config,
        )
        .expect("start server");
        let client = server.client();
        let session = client.create_session("impl-test").expect("session");
        let _ = client
            .prompt(
                &session,
                "Create a file named hello.txt in the current directory containing exactly the text: hi. Do not commit or push.",
                Some("build"),
                Some(opencode::models::DEFAULT_MODEL),
                None,
            )
            .expect("prompt");
        let created = dir.join("hello.txt").exists();
        eprintln!("hello.txt exists: {created}");
        server.stop(Some(&session));
        assert!(created, "expected the agent to create hello.txt");
    }
}
