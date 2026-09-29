use crate::db;
use crate::domain::AgentRun;
use crate::opencode::ManagedServer;
use crate::state::{AgentRuntime, AppState};

pub fn register_run(
    state: &AppState,
    run: AgentRun,
    output: String,
    cost: f64,
    model: Option<String>,
    server: ManagedServer,
) -> Result<(), String> {
    {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::upsert_agent_run(&connection, &run, &output, cost, model.as_deref())?;
    }
    state
        .agents
        .lock()
        .map_err(|error| error.to_string())?
        .insert(
            run.id.clone(),
            AgentRuntime {
                run,
                events: Vec::new(),
                output,
                cost,
                model,
                server,
            },
        );
    Ok(())
}
