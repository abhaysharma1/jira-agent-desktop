use crate::db;
use crate::domain::{AgentRun, OpencodeServer};
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
        // Persist the spawned process so a crash can clean it up next launch.
        // The password is a credential, so it is stored encrypted.
        let password = crate::secrets::encrypt_value(&state.secure_store, &server.password)?;
        db::record_opencode_server(
            &connection,
            &OpencodeServer {
                run_id: run.id.clone(),
                task_id: run.task_id.clone(),
                pid: server.pid(),
                port: server.port,
                password,
                started_at: run.started_at.clone(),
            },
        )?;
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
