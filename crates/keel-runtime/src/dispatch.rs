use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use keel_core::{
    redact_secrets, resolve_path, simplify_path, AccessKind, CancelToken, ErrorCode, Event,
    GrantScope, InteractionKind, InteractionOption, InteractionRequest, InteractionResponse,
    PermissionDecision, PermissionGate, PermissionLevel, PermissionRequest, RiskLevel, Tool,
    ToolArgs, ToolCallId, ToolCallRequest, ToolCtx, ToolError, ToolResult, TurnId,
};
use tokio::task::JoinSet;

use crate::event_log::EventLog;
use crate::interaction::InteractionBroker;
use crate::permission::PermissionEngine;
use crate::registry::ToolRegistry;

/// Read-only calls run in parallel, but a round that asks for fifty files must
/// not start fifty processes at once.
const MAX_CONCURRENT_READS: usize = 8;

struct PreparedCall {
    index: usize,
    tool: Arc<dyn Tool>,
    call_id: ToolCallId,
    name: String,
    args: ToolArgs,
    args_preview: String,
    risk: RiskLevel,
}

/// Dispatches tool calls: read-only calls run concurrently, mutating and
/// dangerous calls run one at a time. Every call is audited.
pub struct ToolDispatcher {
    registry: Arc<ToolRegistry>,
    events: Arc<EventLog>,
    permission: Arc<PermissionEngine>,
    interactions: Option<Arc<InteractionBroker>>,
    timeout: Duration,
}

impl ToolDispatcher {
    pub fn new(
        registry: Arc<ToolRegistry>,
        events: Arc<EventLog>,
        permission: Arc<PermissionEngine>,
        timeout: Duration,
    ) -> Self {
        Self {
            registry,
            events,
            permission,
            interactions: None,
            timeout,
        }
    }

    pub fn with_interactions(mut self, interactions: Arc<InteractionBroker>) -> Self {
        self.interactions = Some(interactions);
        self
    }

    /// Host-facing mode switch: allow every access for `ttl` without a prompt.
    pub fn escalate_full_auto(&self, ttl: Duration) {
        self.permission.escalate_full_auto(ttl);
    }

    /// Drops session grants and returns to the configured base level.
    pub fn revoke_grants(&self) {
        self.permission.revoke_all();
    }

    /// Switches the base level without reopening the session.
    pub fn set_permission_level(&self, level: PermissionLevel) {
        self.permission.set_base_level(level);
    }

    /// The effective level, including an active full-auto grant.
    pub fn permission_level(&self) -> PermissionLevel {
        self.permission.level()
    }

    /// The configured level, without a session grant.
    pub fn base_permission_level(&self) -> PermissionLevel {
        self.permission.base_level()
    }

    /// Adds a session scope for the rest of the session (host `/add-dir`).
    pub fn add_root(
        &self,
        path: impl AsRef<std::path::Path>,
        access: AccessKind,
    ) -> Result<crate::permission::RootScope, keel_core::KernelError> {
        self.permission.add_root(path, access)
    }

    pub async fn dispatch(
        &self,
        turn_id: TurnId,
        calls: Vec<ToolCallRequest>,
        visible: &BTreeSet<String>,
        cancel: &CancelToken,
    ) -> Vec<ToolResult> {
        let call_ids: Vec<ToolCallId> = calls.iter().map(|call| call.id).collect();
        let call_names: Vec<String> = calls.iter().map(|call| call.name.clone()).collect();
        let mut results: Vec<Option<ToolResult>> = vec![None; calls.len()];
        let mut reads = Vec::new();
        let mut writes = Vec::new();

        for (index, call) in calls.into_iter().enumerate() {
            let tool = self.registry.get(&call.name);
            let risk = tool
                .as_ref()
                .map_or(RiskLevel::Dangerous, |tool| tool.schema().risk);
            // A rejected call still needs its arguments in the transcript. In
            // particular a denied `shell` must show the command the user was
            // asked to approve.
            let args_preview = redact_secrets(&crate::summary::compact_args(&call.args));

            if !visible.contains(&call.name) {
                results[index] = Some(self.reject(
                    turn_id,
                    call.id,
                    call.name.clone(),
                    risk,
                    Some(args_preview.clone()),
                    ToolError::permission_denied(format!(
                        "tool `{}` is not visible for this turn",
                        call.name
                    )),
                ));
                continue;
            }

            let Some(tool) = tool else {
                results[index] = Some(self.reject(
                    turn_id,
                    call.id,
                    call.name.clone(),
                    risk,
                    Some(args_preview.clone()),
                    ToolError::not_found(format!("tool `{}` is not registered", call.name)),
                ));
                continue;
            };

            let schema = tool.schema();
            if let Err(error) = schema.validate(&call.args) {
                results[index] = Some(self.reject(
                    turn_id,
                    call.id,
                    call.name.clone(),
                    schema.risk,
                    Some(args_preview.clone()),
                    error,
                ));
                continue;
            }

            if let Err(error) = self
                .check_permissions(turn_id, call.id, &call.name, &tool, &call.args, cancel)
                .await
            {
                results[index] = Some(self.reject(
                    turn_id,
                    call.id,
                    call.name.clone(),
                    schema.risk,
                    Some(args_preview.clone()),
                    error,
                ));
                continue;
            }

            let prepared = PreparedCall {
                index,
                tool,
                call_id: call.id,
                name: call.name,
                args_preview,
                args: call.args,
                risk: schema.risk,
            };
            if schema.risk == RiskLevel::ReadOnly || prepared.tool.concurrent() {
                reads.push(prepared);
            } else {
                writes.push(prepared);
            }
        }

        let read_indices: VecDeque<usize> = reads.iter().map(|prepared| prepared.index).collect();
        let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_READS));
        let mut join = JoinSet::new();
        for prepared in reads {
            let ctx = self.tool_ctx(turn_id, prepared.call_id, cancel.clone());
            let events = self.events.clone();
            let timeout = self.timeout;
            let cancel = cancel.clone();
            let permits = permits.clone();
            join.spawn(async move {
                // A permit is released when the call finishes, so the next
                // queued read starts without waiting for the whole batch.
                let _permit = permits.acquire_owned().await.ok();
                let start = Instant::now();
                events.append(Event::ToolStarted {
                    turn_id,
                    call_id: prepared.call_id,
                    tool: prepared.name.clone(),
                    risk: prepared.risk,
                    args_preview: Some(prepared.args_preview.clone()),
                });
                let result = run_tool(
                    prepared.tool,
                    prepared.name.clone(),
                    prepared.args,
                    ctx,
                    timeout,
                    &cancel,
                )
                .await;
                events.append(Event::ToolFinished {
                    turn_id,
                    call_id: prepared.call_id,
                    tool: prepared.name,
                    ok: result.is_success(),
                    error: result.error().map(|error| error.code),
                    duration_ms: elapsed_ms(start.elapsed()),
                    output_preview: preview_of(&result),
                });
                (prepared.index, result)
            });
        }

        let mut pending = read_indices;

        while let Some(joined) = join.join_next().await {
            match joined {
                Ok((index, result)) => {
                    pending.retain(|candidate| *candidate != index);
                    results[index] = Some(result);
                }
                Err(error) => {
                    let index = pending.pop_front().unwrap_or(0);
                    let call_id = call_ids.get(index).copied().unwrap_or(ToolCallId::new(0));
                    let name = call_names
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_owned());
                    results[index] = Some(ToolResult::failure(
                        call_id,
                        name,
                        ToolError::new(
                            ErrorCode::Internal,
                            format!("tool task failed: {error}"),
                            false,
                        ),
                    ));
                }
            }
        }

        for prepared in writes {
            let start = Instant::now();
            self.events.append(Event::ToolStarted {
                turn_id,
                call_id: prepared.call_id,
                tool: prepared.name.clone(),
                risk: prepared.risk,
                args_preview: Some(prepared.args_preview.clone()),
            });
            let ctx = self.tool_ctx(turn_id, prepared.call_id, cancel.clone());
            let result = if cancel.is_cancelled() {
                ToolResult::failure(
                    prepared.call_id,
                    prepared.name.clone(),
                    ToolError::cancelled(),
                )
            } else {
                run_tool(
                    prepared.tool,
                    prepared.name.clone(),
                    prepared.args,
                    ctx,
                    self.timeout,
                    cancel,
                )
                .await
            };
            self.events.append(Event::ToolFinished {
                turn_id,
                call_id: prepared.call_id,
                tool: prepared.name,
                ok: result.is_success(),
                error: result.error().map(|error| error.code),
                duration_ms: elapsed_ms(start.elapsed()),
                output_preview: preview_of(&result),
            });
            results[prepared.index] = Some(result);
        }

        results
            .into_iter()
            .map(|slot| {
                slot.unwrap_or_else(|| {
                    ToolResult::failure(
                        ToolCallId::new(0),
                        "unknown",
                        ToolError::new(ErrorCode::Internal, "tool call produced no result", false),
                    )
                })
            })
            .collect()
    }

    async fn check_permissions(
        &self,
        turn_id: TurnId,
        call_id: ToolCallId,
        tool_name: &str,
        tool: &Arc<dyn Tool>,
        args: &ToolArgs,
        cancel: &CancelToken,
    ) -> Result<(), ToolError> {
        let risk = tool.schema().risk;
        for mut request in tool.permission_requests(args)? {
            // Follow symlinks before the gate compares paths: a link inside
            // the workspace must not be approved for a target outside it.
            if let Some(path) = request.path.take() {
                request.path = Some(confine_path(self.permission.root(), &path).await?);
            }
            // The registered name wins; a tool must not rename itself in an
            // audit record.
            request.tool = tool_name.to_owned();
            // A read-only tool runs concurrently with other reads, so it must
            // not declare a write or execute request. Enforce the pairing the
            // dispatcher's concurrency relies on instead of trusting the label.
            if risk == RiskLevel::ReadOnly && request.access != AccessKind::Read {
                let message = format!(
                    "tool `{tool_name}` is declared read-only but requests {} access",
                    request.access.as_str()
                );
                self.record_permission(
                    turn_id,
                    call_id,
                    tool_name,
                    &request,
                    "deny",
                    Some(&message),
                );
                return Err(ToolError::permission_denied(message));
            }
            match self.permission.check(&request) {
                PermissionDecision::Allow => {
                    self.record_permission(turn_id, call_id, tool_name, &request, "allow", None);
                }
                PermissionDecision::Deny { code, message } => {
                    self.record_permission(
                        turn_id,
                        call_id,
                        tool_name,
                        &request,
                        "deny",
                        Some(&message),
                    );
                    return Err(ToolError::new(code, message, false));
                }
                PermissionDecision::Confirm { reason } => {
                    self.record_permission(
                        turn_id,
                        call_id,
                        tool_name,
                        &request,
                        "confirm",
                        Some(&reason),
                    );
                    let Some(interactions) = &self.interactions else {
                        return Err(ToolError::permission_denied(format!(
                            "confirmation required: {reason}"
                        )));
                    };

                    let prompt = permission_prompt(tool_name, &request, &reason);
                    let interaction = InteractionRequest::new(
                        turn_id,
                        InteractionKind::PermissionApproval,
                        prompt,
                    )
                    .with_options(vec![
                        InteractionOption::new("deny", "Deny"),
                        InteractionOption::new("allow_once", "Allow once"),
                        InteractionOption::new("allow_scope", "Allow this path for the session"),
                        InteractionOption::new("full_auto", "Allow and switch to full auto"),
                    ]);
                    let mut interaction = interaction;
                    interaction.correlation = Some(call_id);
                    interaction.path = request.path.clone();

                    match interactions.request(interaction, cancel).await {
                        InteractionResponse::Approved {
                            scope: GrantScope::Once,
                        } => {}
                        InteractionResponse::Approved {
                            scope: GrantScope::PathPrefix { prefix, ttl },
                        } => {
                            self.permission
                                .grant_path_prefix(&prefix, request.access, ttl);
                            self.events.append(Event::PermissionGranted {
                                turn_id,
                                scope: "path".to_owned(),
                                path: Some(prefix.display().to_string()),
                                access: Some(request.access),
                                ttl_ms: Some(ttl.as_millis().min(u128::from(u64::MAX)) as u64),
                            });
                        }
                        InteractionResponse::Approved {
                            scope: GrantScope::FullAutoSession { ttl },
                        } => {
                            self.permission.escalate_full_auto(ttl);
                            self.events.append(Event::PermissionGranted {
                                turn_id,
                                scope: "full_auto".to_owned(),
                                path: None,
                                access: None,
                                ttl_ms: Some(ttl.as_millis().min(u128::from(u64::MAX)) as u64),
                            });
                        }
                        InteractionResponse::Cancelled => {
                            return Err(ToolError::cancelled());
                        }
                        InteractionResponse::Rejected { reason } => {
                            return Err(ToolError::permission_denied(reason));
                        }
                        InteractionResponse::Deferred
                        | InteractionResponse::TimedOut
                        | InteractionResponse::Selected { .. }
                        | InteractionResponse::Freeform { .. } => {
                            return Err(ToolError::permission_denied(
                                "permission confirmation was not granted",
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Records one gate decision for a UI or an audit log.
    #[allow(clippy::too_many_arguments)]
    fn record_permission(
        &self,
        turn_id: TurnId,
        call_id: ToolCallId,
        tool: &str,
        request: &PermissionRequest,
        decision: &str,
        reason: Option<&str>,
    ) {
        self.events.append(Event::PermissionDecided {
            turn_id,
            call_id,
            tool: tool.to_owned(),
            access: request.access,
            path: request.path.as_ref().map(|path| path.display().to_string()),
            decision: decision.to_owned(),
            reason: reason.map(str::to_owned),
        });
    }

    fn tool_ctx(&self, turn_id: TurnId, call_id: ToolCallId, cancel: CancelToken) -> ToolCtx {
        ToolCtx {
            turn_id,
            call_id,
            workspace_root: self.permission.root().to_path_buf(),
            cancel,
        }
    }

    fn reject(
        &self,
        turn_id: TurnId,
        call_id: ToolCallId,
        name: String,
        risk: RiskLevel,
        args_preview: Option<String>,
        error: ToolError,
    ) -> ToolResult {
        self.events.append(Event::ToolStarted {
            turn_id,
            call_id,
            tool: name.clone(),
            risk,
            args_preview,
        });
        let result = ToolResult::failure(call_id, name, error);
        self.events.append(Event::ToolFinished {
            turn_id,
            call_id,
            tool: result.tool.clone(),
            ok: false,
            error: result.error().map(|error| error.code),
            duration_ms: 0,
            output_preview: preview_of(&result),
        });
        result
    }
}

async fn run_tool(
    tool: Arc<dyn Tool>,
    tool_name: String,
    args: ToolArgs,
    ctx: ToolCtx,
    timeout: Duration,
    cancel: &CancelToken,
) -> ToolResult {
    let call_id = ctx.call_id;
    tokio::select! {
        _ = cancel.cancelled() => {
            ToolResult::failure(call_id, tool_name, ToolError::cancelled())
        }
        result = tokio::time::timeout(timeout, tool.call(args, &ctx)) => {
            match result {
                Ok(result) => result,
                Err(_) => ToolResult::failure(call_id, tool_name, ToolError::timeout(timeout)),
            }
        }
    }
}

fn elapsed_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

/// Resolves a tool path and follows every symlink in the existing part of it.
///
/// The permission gate compares paths lexically, so a link inside the
/// workspace that points outside would otherwise be approved. Canonicalizing
/// the deepest existing ancestor also covers a write target that does not
/// exist yet; a dangling symlink is refused instead of followed.
async fn confine_path(root: &Path, path: &Path) -> Result<PathBuf, ToolError> {
    let candidate = resolve_path(root, path);
    let mut existing = candidate.as_path();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match tokio::fs::symlink_metadata(existing).await {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = existing.file_name() else {
                    return Err(ToolError::permission_denied(format!(
                        "cannot resolve `{}`",
                        candidate.display()
                    )));
                };
                tail.push(name.to_os_string());
                existing = existing.parent().ok_or_else(|| {
                    ToolError::permission_denied(format!(
                        "cannot resolve `{}`",
                        candidate.display()
                    ))
                })?;
            }
            Err(error) => {
                return Err(ToolError::permission_denied(format!(
                    "cannot resolve `{}`: {error}",
                    candidate.display()
                )))
            }
        }
    }

    let mut resolved = tokio::fs::canonicalize(existing).await.map_err(|error| {
        ToolError::permission_denied(format!("cannot resolve `{}`: {error}", existing.display()))
    })?;
    for part in tail.iter().rev() {
        resolved.push(part);
    }
    Ok(simplify_path(resolved))
}

/// Bytes of tool output kept for a live UI. The field is skipped during
/// serialization, so the preview never reaches the persisted event stream.
const OUTPUT_PREVIEW_BYTES: usize = 4 * 1024;

fn preview_of(result: &ToolResult) -> Option<String> {
    let text = match result.content() {
        Some(content) => content,
        None => result.error()?.message.as_str(),
    };
    if text.trim().is_empty() {
        return None;
    }
    let redacted = redact_secrets(text);
    if redacted.len() <= OUTPUT_PREVIEW_BYTES {
        return Some(redacted);
    }
    let mut end = OUTPUT_PREVIEW_BYTES;
    while end > 0 && !redacted.is_char_boundary(end) {
        end -= 1;
    }
    let mut preview = redacted[..end].to_owned();
    preview.push_str("\n… truncated");
    Some(preview)
}

/// The text a host shows for a permission request. A command line is labelled
/// as one; any other detail is a target the user should read as a sentence.
fn permission_prompt(tool: &str, request: &PermissionRequest, reason: &str) -> String {
    let scope = request
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "no path".to_owned());
    let detail = request
        .detail
        .as_deref()
        .map(|detail| {
            if request.access == AccessKind::Execute {
                format!("\ncommand: {detail}")
            } else {
                format!("\n{detail}")
            }
        })
        .unwrap_or_default();
    format!(
        "Allow `{tool}` to {} `{scope}`? {reason}{detail}",
        request.access.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(access: AccessKind) -> PermissionRequest {
        PermissionRequest {
            tool: "test".to_owned(),
            access,
            path: Some(std::path::PathBuf::from("/outside/folder")),
            detail: Some("window Notepad".to_owned()),
        }
    }

    #[test]
    fn a_shell_detail_is_labelled_as_a_command() {
        let prompt = permission_prompt("shell", &request(AccessKind::Execute), "outside");
        assert!(prompt.contains("\ncommand: window Notepad"), "{prompt}");
    }

    #[test]
    fn another_tool_detail_is_a_sentence() {
        let prompt = permission_prompt("screen_capture", &request(AccessKind::Write), "outside");
        assert!(prompt.contains("\nwindow Notepad"), "{prompt}");
        assert!(!prompt.contains("command:"), "{prompt}");
    }

    #[test]
    fn a_successful_call_keeps_its_output() {
        let result = ToolResult::success(ToolCallId::new(1), "shell", "all tests passed");
        assert_eq!(preview_of(&result).as_deref(), Some("all tests passed"));
    }

    #[test]
    fn a_failed_call_keeps_its_message() {
        let result = ToolResult::failure(
            ToolCallId::new(1),
            "shell",
            ToolError::invalid_arguments("program `bash` is not allowlisted"),
        );
        assert_eq!(
            preview_of(&result).as_deref(),
            Some("program `bash` is not allowlisted")
        );
    }

    #[test]
    fn a_preview_is_redacted_and_bounded() {
        let secret = ToolResult::success(
            ToolCallId::new(1),
            "read_file",
            "-----BEGIN RSA PRIVATE KEY-----\nabc",
        );
        assert_eq!(
            preview_of(&secret).as_deref(),
            Some("[REDACTED PRIVATE KEY]")
        );

        let long = ToolResult::success(ToolCallId::new(1), "shell", "a".repeat(5000));
        let preview = preview_of(&long).expect("preview");
        assert!(preview.len() < 5000);
        assert!(preview.ends_with("\n… truncated"));
    }

    #[test]
    fn an_empty_result_has_no_preview() {
        let result = ToolResult::success(ToolCallId::new(1), "shell", "  \n");
        assert!(preview_of(&result).is_none());
    }
}
