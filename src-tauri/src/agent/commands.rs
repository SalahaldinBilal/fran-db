use std::sync::Arc;

use tauri::State;

use super::approvals::ApprovalRequest;
use super::config::AgentConfig;
use super::history::HistoryEntry;
use super::host::{AgentHost, CloseReason, ServerStatus};
use super::sessions::AgentSessionInfo;
use crate::error::{AppError, AppResult};

type Host<'a> = State<'a, Arc<AgentHost>>;

#[tauri::command]
pub async fn get_agent_config(host: Host<'_>) -> AppResult<AgentConfig> {
    host.config()
}

#[tauri::command]
pub async fn set_agent_config(host: Host<'_>, config: AgentConfig) -> AppResult<ServerStatus> {
    host.inner().set_config(config).await?;
    Ok(host.status())
}

#[tauri::command]
pub async fn get_agent_status(host: Host<'_>) -> AppResult<ServerStatus> {
    Ok(host.status())
}

#[tauri::command]
pub async fn list_agent_sessions(host: Host<'_>) -> AppResult<Vec<AgentSessionInfo>> {
    Ok(host.sessions.list())
}

#[tauri::command]
pub async fn close_agent_session(host: Host<'_>, id: String) -> AppResult<()> {
    host.close_session(&id, CloseReason::User).await;
    Ok(())
}

#[tauri::command]
pub async fn close_all_agent_sessions(host: Host<'_>) -> AppResult<()> {
    host.close_all(CloseReason::User).await;
    Ok(())
}

#[tauri::command]
pub async fn list_agent_approvals(host: Host<'_>) -> AppResult<Vec<ApprovalRequest>> {
    Ok(host.approvals.list())
}

#[tauri::command]
pub async fn answer_agent_request(host: Host<'_>, id: String, approved: bool) -> AppResult<()> {
    if host.answer(&id, approved) {
        Ok(())
    } else {
        Err(AppError::not_found("The agent stopped waiting for this change"))
    }
}

#[tauri::command]
pub async fn list_agent_history(host: Host<'_>, from: u64, to: u64) -> AppResult<Vec<HistoryEntry>> {
    host.history.unlock(&host.vault)?;
    host.history.list(from, to)
}

#[tauri::command]
pub async fn get_agent_history_entry(host: Host<'_>, id: String, at: u64) -> AppResult<Option<HistoryEntry>> {
    host.history.unlock(&host.vault)?;
    host.history.get(&id, at)
}

#[tauri::command]
pub async fn get_agent_history_size(host: Host<'_>) -> AppResult<u64> {
    host.history.size()
}

#[tauri::command]
pub async fn clear_agent_history(host: Host<'_>) -> AppResult<()> {
    host.history.clear()
}
