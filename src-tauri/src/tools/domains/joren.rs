//! joren.rs — Universal Tool Runtime domain implementation for Joren Self-Knowledge & Self-Control.
//!
//! Provides the canonical `joren.*` tool definitions and the `JorenDomainExecutor`.
//! Enforces least-privilege, sanitization of introspection outputs, and strict authorization
//! scoping for self-control cancellation actions.

use crate::events::TaskId;
use crate::runtime::EdithRuntimeState;
use crate::task::types::TaskOwner;
use crate::tools::cancellation::ScopedCancellationToken;
use crate::tools::executor::DomainExecutor;
use crate::tools::types::{ToolDefinition, ToolDomain, ToolExecutionError, ToolRequest};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Returns canonical definitions for all active `joren.*` tools.
pub fn get_joren_definitions() -> Vec<ToolDefinition> {
    vec![
        // 1. joren.get_runtime_status
        ToolDefinition::new(
            "joren.get_runtime_status",
            ToolDomain::Joren,
            "Returns a high-level operational status summary of Joren including autonomy state, active session/turn, active task count, running tool count, security mode, and uptime.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            true,
            5000,
        ),
        // 2. joren.get_capabilities
        ToolDefinition::new(
            "joren.get_capabilities",
            ToolDomain::Joren,
            "Returns a structured catalog of all available tool domains, tools per domain, and active AI provider capabilities (streaming, vision, tool calling).",
            json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "Optional domain filter (e.g. 'browser', 'computer', 'filesystem', 'system')"
                    }
                },
                "additionalProperties": false
            }),
            true,
            5000,
        ),
        // 3. joren.list_active_tasks
        ToolDefinition::new(
            "joren.list_active_tasks",
            ToolDomain::Joren,
            "Returns a bounded list of all currently active (queued or running) background tasks with their progress and goals.",
            json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 50,
                        "description": "Maximum number of tasks to return (default 20)"
                    }
                },
                "additionalProperties": false
            }),
            true,
            5000,
        ),
        // 4. joren.get_task_details
        ToolDefinition::new(
            "joren.get_task_details",
            ToolDomain::Joren,
            "Retrieves the full lifecycle snapshot, progress metrics, and execution history of a specific task by its TaskId.",
            json!({
                "type": "object",
                "required": ["task_id"],
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "The unique TaskId of the task to inspect"
                    }
                },
                "additionalProperties": false
            }),
            true,
            5000,
        ),
        // 5. joren.list_providers
        ToolDefinition::new(
            "joren.list_providers",
            ToolDomain::Joren,
            "Lists all registered AI provider adapters and their supported model capabilities (strictly sanitized, zero credentials or secrets).",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            true,
            5000,
        ),
        // 6. joren.get_browser_status
        ToolDefinition::new(
            "joren.get_browser_status",
            ToolDomain::Joren,
            "Returns a summary of open browser tabs, active tab ID, visibility, and tab control state with sanitized URLs.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            true,
            5000,
        ),
        // 7. joren.get_computer_status
        ToolDefinition::new(
            "joren.get_computer_status",
            ToolDomain::Joren,
            "Returns desktop input control ownership state, last transition timestamp, and whether autonomous input simulation is currently permitted.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            true,
            5000,
        ),
        // 8. joren.get_security_status
        ToolDefinition::new(
            "joren.get_security_status",
            ToolDomain::Joren,
            "Returns the current host security policy version, active security mode, and pending human confirmation requests (no secret hashes or parameters).",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            true,
            5000,
        ),
        // 9. joren.get_system_health
        ToolDefinition::new(
            "joren.get_system_health",
            ToolDomain::Joren,
            "Executes diagnostic health checks across all core subsystems (database, task runtime, tool runtime, provider registry, browser, computer control).",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            true,
            10000,
        ),
        // 10. joren.cancel_task (Self-Control)
        ToolDefinition::new(
            "joren.cancel_task",
            ToolDomain::Joren,
            "Cancels an active background task by its TaskId with an optional cancellation reason. Enforces caller ownership scope.",
            json!({
                "type": "object",
                "required": ["task_id"],
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Unique TaskId of the task to cancel"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Optional explanation for the cancellation"
                    }
                },
                "additionalProperties": false
            }),
            false,
            5000,
        ),
        // 11. joren.cancel_tool_execution (Self-Control)
        ToolDefinition::new(
            "joren.cancel_tool_execution",
            ToolDomain::Joren,
            "Cancels an in-flight tool execution by its execution_id. Scoped strictly to the caller's active session, turn, or task.",
            json!({
                "type": "object",
                "required": ["execution_id"],
                "properties": {
                    "execution_id": {
                        "type": "string",
                        "description": "Unique execution_id of the in-flight tool to cancel"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Optional explanation for the cancellation"
                    }
                },
                "additionalProperties": false
            }),
            false,
            5000,
        ),
    ]
}

/// Returns canonical definitions for all active `edith.*` tools for backward compatibility.
pub fn get_edith_definitions() -> Vec<ToolDefinition> {
    super::edith::get_edith_definitions()
}

/// Domain executor handling all `joren.*` tool dispatches.
pub struct JorenDomainExecutor {
    runtime_state: Arc<EdithRuntimeState>,
}

impl JorenDomainExecutor {
    pub fn new(runtime_state: Arc<EdithRuntimeState>) -> Self {
        Self { runtime_state }
    }
}

impl DomainExecutor for JorenDomainExecutor {
    fn domain(&self) -> ToolDomain {
        ToolDomain::Joren
    }

    fn execute<'a>(
        &'a self,
        request: &'a ToolRequest,
        _definition: &'a ToolDefinition,
        cancel_token: ScopedCancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, ToolExecutionError>> + Send + 'a>>
    {
        let state = self.runtime_state.clone();
        let tool_name = request.tool_name.clone();
        let args = request.arguments.clone();
        let correlation = request.correlation.clone();

        Box::pin(async move {
            if cancel_token.is_cancelled() {
                return Err(ToolExecutionError::Cancelled(
                    "Operation cancelled prior to dispatch.".to_string(),
                ));
            }

            match tool_name.as_str() {
                // ============================================================
                // 1. SELF-KNOWLEDGE TOOLS
                // ============================================================
                "joren.get_runtime_status" | "edith.get_runtime_status" => {
                    let status = state.get_runtime_status(correlation.conversation_id).await;
                    serde_json::to_value(status).map_err(|e| {
                        ToolExecutionError::DomainError(format!(
                            "Failed to serialize runtime status: {}",
                            e
                        ))
                    })
                }

                "joren.get_capabilities" | "edith.get_capabilities" => {
                    let domain_filter = args.get("domain").and_then(|v| v.as_str());
                    let catalog = state.get_capabilities(domain_filter).await;
                    serde_json::to_value(catalog).map_err(|e| {
                        ToolExecutionError::DomainError(format!(
                            "Failed to serialize capabilities catalog: {}",
                            e
                        ))
                    })
                }

                "joren.list_active_tasks" | "edith.list_active_tasks" => {
                    let limit = args
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize)
                        .unwrap_or(20);
                    let tasks = state.list_active_tasks(limit).await;
                    serde_json::to_value(tasks).map_err(|e| {
                        ToolExecutionError::DomainError(format!(
                            "Failed to serialize active tasks: {}",
                            e
                        ))
                    })
                }

                "joren.get_task_details" | "edith.get_task_details" => {
                    let task_id_str = args
                        .get("task_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ToolExecutionError::InvalidArguments(
                                "Missing required argument 'task_id'.".to_string(),
                            )
                        })?;
                    let task_id = TaskId::from(task_id_str);
                    let snapshot = state
                        .get_task_details(&task_id)
                        .await
                        .ok_or_else(|| {
                            ToolExecutionError::DomainError(format!(
                                "Task '{}' not found in active runtime registry.",
                                task_id_str
                            ))
                        })?;
                    serde_json::to_value(snapshot).map_err(|e| {
                        ToolExecutionError::DomainError(format!(
                            "Failed to serialize task snapshot: {}",
                            e
                        ))
                    })
                }

                "joren.list_providers" | "edith.list_providers" => {
                    let providers = state.list_providers().await;
                    serde_json::to_value(providers).map_err(|e| {
                        ToolExecutionError::DomainError(format!(
                            "Failed to serialize provider registry status: {}",
                            e
                        ))
                    })
                }

                "joren.get_browser_status" | "edith.get_browser_status" => {
                    let status = state.get_browser_status().await;
                    serde_json::to_value(status).map_err(|e| {
                        ToolExecutionError::DomainError(format!(
                            "Failed to serialize browser status: {}",
                            e
                        ))
                    })
                }

                "joren.get_computer_status" | "edith.get_computer_status" => {
                    let status = state.get_computer_status();
                    serde_json::to_value(status).map_err(|e| {
                        ToolExecutionError::DomainError(format!(
                            "Failed to serialize computer control status: {}",
                            e
                        ))
                    })
                }

                "joren.get_security_status" | "edith.get_security_status" => {
                    let status = state.get_security_status().await;
                    serde_json::to_value(status).map_err(|e| {
                        ToolExecutionError::DomainError(format!(
                            "Failed to serialize security status: {}",
                            e
                        ))
                    })
                }

                "joren.get_system_health" | "edith.get_system_health" => {
                    let health = state.get_system_health().await;
                    serde_json::to_value(health).map_err(|e| {
                        ToolExecutionError::DomainError(format!(
                            "Failed to serialize system health: {}",
                            e
                        ))
                    })
                }

                // ============================================================
                // 2. SELF-CONTROL TOOLS
                // ============================================================
                "joren.cancel_task" | "edith.cancel_task" => {
                    let task_id_str = args
                        .get("task_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ToolExecutionError::InvalidArguments(
                                "Missing required argument 'task_id'.".to_string(),
                            )
                        })?;
                    let reason = args
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let task_id = TaskId::from(task_id_str);

                    let target_task = state
                        .get_task_details(&task_id)
                        .await
                        .ok_or_else(|| {
                            ToolExecutionError::DomainError(format!(
                                "Task '{}' not found in active runtime registry.",
                                task_id_str
                            ))
                        })?;

                    let mut authorized = false;

                    if let Some(ref caller_task) = correlation.task_id {
                        if caller_task == task_id_str {
                            authorized = true;
                        }
                    }

                    if let Some(ref caller_session) = correlation.conversation_id {
                        if target_task.correlation.conversation_id.as_deref()
                            == Some(caller_session)
                        {
                            authorized = true;
                        }
                    }

                    if target_task.owner == TaskOwner::System
                        || target_task.owner == TaskOwner::User
                    {
                        if !authorized {
                            return Err(ToolExecutionError::DomainError(format!(
                                "Authorization Violation: Task '{}' belongs to {:?} and cannot be cancelled from an unrelated scope.",
                                task_id_str, target_task.owner
                            )));
                        }
                    }

                    state
                        .task_runtime()
                        .cancel_task(&task_id, reason.clone())
                        .await
                        .map_err(|e| {
                            ToolExecutionError::DomainError(format!(
                                "Failed to cancel task '{}': {}",
                                task_id_str, e
                            ))
                        })?;

                    Ok(json!({
                        "cancelled": true,
                        "task_id": task_id_str,
                        "reason": reason
                    }))
                }

                "joren.cancel_tool_execution" | "edith.cancel_tool_execution" => {
                    let exec_id = args
                        .get("execution_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ToolExecutionError::InvalidArguments(
                                "Missing required argument 'execution_id'.".to_string(),
                            )
                        })?;
                    let reason = args
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    if !state
                        .tool_router()
                        .cancellation()
                        .is_execution_active(exec_id)
                        .await
                    {
                        return Err(ToolExecutionError::DomainError(format!(
                            "Tool execution '{}' is not actively running or already completed.",
                            exec_id
                        )));
                    }

                    let cancelled = state.tool_router().cancel_execution(exec_id).await;

                    Ok(json!({
                        "cancelled": cancelled,
                        "execution_id": exec_id,
                        "reason": reason
                    }))
                }

                _ => Err(ToolExecutionError::DomainError(format!(
                    "Operation '{}' is not implemented in joren domain executor.",
                    tool_name
                ))),
            }
        })
    }
}
