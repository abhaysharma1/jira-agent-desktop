use crate::commands::repository::not_implemented;
use crate::domain::AgentEvent;

#[tauri::command]
pub fn start_planning(task_id: String) -> Result<String, String> {
    let _ = task_id;
    Err(not_implemented("start_planning (Phase 6)"))
}

#[tauri::command]
pub fn start_implementation(task_id: String, approved_plan_version: u32) -> Result<String, String> {
    let _ = (task_id, approved_plan_version);
    Err(not_implemented("start_implementation (Phase 10)"))
}

#[tauri::command]
pub fn send_agent_message(run_id: String, content: String) -> Result<(), String> {
    let _ = (run_id, content);
    Err(not_implemented("send_agent_message (Phase 8)"))
}

#[tauri::command]
pub fn stop_agent(run_id: String) -> Result<(), String> {
    let _ = run_id;
    Err(not_implemented("stop_agent (Phase 4)"))
}

#[tauri::command]
pub fn get_agent_status(run_id: String) -> Result<Vec<AgentEvent>, String> {
    let _ = run_id;
    Err(not_implemented("get_agent_status (Phase 4)"))
}
