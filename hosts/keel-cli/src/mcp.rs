//! Minimal MCP (Model Context Protocol) stdio client.
//!
//! One plugin is one MCP server process. The host spawns it, lists its tools
//! and registers each one as a kernel tool. The kernel never knows MCP exists;
//! it sees ordinary typed tools. The protocol is public and documented at
//! modelcontextprotocol.io.
//!
//! The trust boundary is the host: an MCP server is an external process the
//! user installed. The permission gate still runs for every call, but the
//! kernel cannot sandbox what the server does inside its own process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    AccessKind, ErrorCode, KernelError, ParamKind, ParamSpec, PermissionRequest, RiskLevel, Tool,
    ToolArgs, ToolCtx, ToolError, ToolResult, ToolSchema,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;

const PROTOCOL_VERSION: &str = "2024-11-05";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// One MCP server entry in the plugin file.
#[derive(Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Deserialize)]
struct PluginFile {
    #[serde(default)]
    servers: Vec<ServerConfig>,
}

/// A running MCP server and its JSON-RPC channel.
struct McpServer {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl McpServer {
    async fn spawn(command: &str, args: &[String]) -> Result<Self, String> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("cannot start `{command}`: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "server has no stdin".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "server has no stdout".to_owned())?;
        Ok(Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 1,
        })
    }

    async fn send(&mut self, message: Value) -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| format!("cannot write to server: {error}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| format!("cannot flush server stdin: {error}"))
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        loop {
            let line = timeout(REQUEST_TIMEOUT, self.lines.next_line())
                .await
                .map_err(|_| format!("`{method}` timed out"))?
                .map_err(|error| format!("cannot read from server: {error}"))?
                .ok_or_else(|| "server closed its stdout".to_owned())?;
            let value: Value = serde_json::from_str(&line)
                .map_err(|error| format!("invalid JSON from server: {error}"))?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                // Notifications and server-initiated requests are ignored in
                // this version; the tool path does not depend on them.
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(format!("server error: {error}"));
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "keel", "version": "0.1.0"}
            }),
        )
        .await?;
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await
    }

    async fn list_tools(&mut self) -> Result<Vec<Value>, String> {
        let result = self.request("tools/list", json!({})).await?;
        Ok(result
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
            .await
    }
}

/// A kernel tool backed by one MCP tool.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParamMode {
    /// The schema maps to flat kernel parameters.
    Flat,
    /// At least one property is nested; the model passes one JSON object.
    Json,
}

pub struct McpTool {
    server: Arc<Mutex<McpServer>>,
    name: String,
    remote_name: String,
    description: String,
    risk: RiskLevel,
    params: Vec<ParamSpec>,
    mode: ParamMode,
    root: PathBuf,
}

impl McpTool {
    fn new(
        server: Arc<Mutex<McpServer>>,
        server_name: &str,
        root: &Path,
        tool: &Value,
    ) -> Option<Self> {
        let remote_name = tool.get("name").and_then(Value::as_str)?.to_owned();
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("MCP tool")
            .to_owned();
        let read_only = tool
            .get("annotations")
            .and_then(|annotations| annotations.get("readOnlyHint"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let schema = tool.get("inputSchema").cloned().unwrap_or(Value::Null);
        let (params, mode, description) = match params_from_schema(&schema) {
            Some(params) => (params, ParamMode::Flat, description),
            None => {
                // The schema has nested objects or object arrays. A flat
                // parameter list would drop them or reject the model's
                // arguments, so the whole call becomes one JSON string.
                let compact = serde_json::to_string(&schema).unwrap_or_default();
                let compact = compact.chars().take(600).collect::<String>();
                (
                    vec![ParamSpec {
                        name: "arguments".to_owned(),
                        kind: ParamKind::String,
                        required: false,
                    }],
                    ParamMode::Json,
                    format!(
                        "{description} Pass one JSON object in `arguments` matching: {compact}"
                    ),
                )
            }
        };
        let name = format!("mcp_{}_{}", sanitize(server_name), sanitize(&remote_name));
        Some(Self {
            server,
            name,
            remote_name,
            description,
            risk: if read_only {
                RiskLevel::ReadOnly
            } else {
                RiskLevel::Mutating
            },
            params,
            mode,
            root: root.to_path_buf(),
        })
    }
}

#[async_trait]
impl Tool for McpTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.clone(),
            description: format!("{} (MCP tool `{}`)", self.description, self.remote_name),
            risk: self.risk,
            params: self.params.clone(),
        }
    }

    fn permission_requests(&self, _args: &ToolArgs) -> Result<Vec<PermissionRequest>, ToolError> {
        // The server is external; the kernel can only gate the call, not what
        // the server does. Read-only tools ask for Read, the rest for Write.
        Ok(vec![PermissionRequest {
            tool: self.name.clone(),
            access: if self.risk == RiskLevel::ReadOnly {
                AccessKind::Read
            } else {
                AccessKind::Write
            },
            path: Some(self.root.clone()),
            detail: Some(format!("MCP tool `{}`", self.remote_name)),
        }])
    }

    async fn call(&self, args: ToolArgs, ctx: &ToolCtx) -> ToolResult {
        if ctx.cancel.is_cancelled() {
            return ToolResult::failure(ctx.call_id, &self.name, ToolError::cancelled());
        }
        let arguments = match self.mode {
            ParamMode::Flat => arguments_json(&self.params, &args),
            ParamMode::Json => match args.get("arguments") {
                Some(keel_core::ArgValue::String(text)) if !text.trim().is_empty() => {
                    match serde_json::from_str(text) {
                        Ok(value) => value,
                        Err(error) => {
                            return ToolResult::failure(
                                ctx.call_id,
                                &self.name,
                                ToolError::invalid_arguments(format!(
                                    "`arguments` must be a JSON object: {error}"
                                )),
                            );
                        }
                    }
                }
                Some(_) => {
                    return ToolResult::failure(
                        ctx.call_id,
                        &self.name,
                        ToolError::invalid_arguments("`arguments` must be a JSON string"),
                    );
                }
                None => json!({}),
            },
        };
        let mut server = self.server.lock().await;
        match server.call_tool(&self.remote_name, arguments).await {
            Ok(result) => {
                let is_error = result
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let text = render_content(&result);
                if is_error {
                    ToolResult::failure(
                        ctx.call_id,
                        &self.name,
                        ToolError::new(ErrorCode::Internal, text, false),
                    )
                } else {
                    ToolResult::success(ctx.call_id, &self.name, text)
                }
            }
            Err(error) => ToolResult::failure(
                ctx.call_id,
                &self.name,
                ToolError::new(ErrorCode::Internal, error, false),
            ),
        }
    }
}

/// Loads the plugin file and registers every discovered MCP tool.
pub async fn load(
    path: &Path,
    root: &Path,
    registry: &mut keel_runtime::ToolRegistry,
) -> Result<Vec<String>, KernelError> {
    let raw = std::fs::read_to_string(path).map_err(|error| KernelError::Io {
        message: format!("cannot read plugins `{}`: {error}", path.display()),
    })?;
    let file: PluginFile = toml::from_str(&raw).map_err(|error| KernelError::InvalidConfig {
        message: format!("invalid plugins `{}`: {error}", path.display()),
    })?;
    let mut names = Vec::new();
    for server in file.servers {
        let mut running = match McpServer::spawn(&server.command, &server.args).await {
            Ok(server) => server,
            Err(error) => {
                eprintln!("plugin `{}` skipped: {error}", server.name);
                continue;
            }
        };
        if let Err(error) = running.initialize().await {
            eprintln!("plugin `{}` skipped: {error}", server.name);
            continue;
        }
        let tools = match running.list_tools().await {
            Ok(tools) => tools,
            Err(error) => {
                eprintln!("plugin `{}` skipped: {error}", server.name);
                continue;
            }
        };
        let shared = Arc::new(Mutex::new(running));
        for tool in tools {
            if let Some(tool) = McpTool::new(shared.clone(), &server.name, root, &tool) {
                names.push(tool.schema().name.clone());
                registry
                    .register(Arc::new(tool))
                    .map_err(|error| KernelError::InvalidConfig {
                        message: format!("cannot register MCP tool: {error}"),
                    })?;
            }
        }
    }
    Ok(names)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// Maps the flat properties of a JSON schema to kernel parameter specs.
/// Nested objects are skipped: the model cannot fill them through a flat tool
/// schema, and pretending otherwise would send broken arguments.
fn params_from_schema(schema: &Value) -> Option<Vec<ParamSpec>> {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Some(Vec::new());
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut params = Vec::new();
    for (name, property) in properties {
        let kind = match property.get("type").and_then(Value::as_str) {
            Some("string") => ParamKind::String,
            Some("integer") | Some("number") => ParamKind::Integer,
            Some("boolean") => ParamKind::Boolean,
            Some("array")
                if property
                    .get("items")
                    .and_then(|items| items.get("type"))
                    .and_then(Value::as_str)
                    == Some("string") =>
            {
                ParamKind::StringList
            }
            // Nested objects, object arrays and unknown types fall back to
            // the JSON argument mode; silently dropping them would send an
            // incomplete call.
            _ => return None,
        };
        params.push(ParamSpec {
            name: name.clone(),
            kind,
            required: required.contains(&name.as_str()),
        });
    }
    Some(params)
}

fn arguments_json(params: &[ParamSpec], args: &ToolArgs) -> Value {
    let mut object = serde_json::Map::new();
    for param in params {
        let Some(value) = args.get(&param.name) else {
            continue;
        };
        let value = match value {
            keel_core::ArgValue::String(text) => Value::String(text.clone()),
            keel_core::ArgValue::Integer(number) => json!(number),
            keel_core::ArgValue::Boolean(flag) => json!(flag),
            keel_core::ArgValue::StringList(items) => json!(items),
        };
        object.insert(param.name.clone(), value);
    }
    Value::Object(object)
}

fn render_content(result: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        for item in content {
            match item.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        parts.push(text.to_owned());
                    }
                }
                Some(other) => parts.push(format!("[{other} content omitted]")),
                None => {}
            }
        }
    }
    if parts.is_empty() {
        result.to_string()
    } else {
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flat_schema_properties_become_params() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "depth": {"type": "integer"},
                "hidden": {"type": "boolean"},
                "tags": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["path"]
        });
        let params = params_from_schema(&schema).unwrap();
        assert_eq!(params.len(), 4);
        let by_name = |name: &str| {
            params
                .iter()
                .find(|param| param.name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };
        assert_eq!(by_name("path").kind, ParamKind::String);
        assert!(by_name("path").required);
        assert_eq!(by_name("depth").kind, ParamKind::Integer);
        assert_eq!(by_name("hidden").kind, ParamKind::Boolean);
        assert_eq!(by_name("tags").kind, ParamKind::StringList);
    }

    #[test]
    fn nested_objects_switch_to_json_mode() {
        let schema = json!({
            "type": "object",
            "properties": {
                "filter": {"type": "object"},
                "path": {"type": "string"}
            }
        });
        assert!(params_from_schema(&schema).is_none());
    }

    #[test]
    fn object_arrays_switch_to_json_mode() {
        let schema = json!({
            "type": "object",
            "properties": {
                "entities": {"type": "array", "items": {"type": "object"}}
            }
        });
        assert!(params_from_schema(&schema).is_none());
    }

    #[test]
    fn arguments_keep_their_types() {
        let params = vec![
            ParamSpec {
                name: "path".to_owned(),
                kind: ParamKind::String,
                required: true,
            },
            ParamSpec {
                name: "depth".to_owned(),
                kind: ParamKind::Integer,
                required: false,
            },
        ];
        let mut args = ToolArgs::new();
        args.insert("path", keel_core::ArgValue::String(".".to_owned()));
        args.insert("depth", keel_core::ArgValue::Integer(2));
        let value = arguments_json(&params, &args);
        assert_eq!(value, json!({"path": ".", "depth": 2}));
    }

    #[test]
    fn content_render_marks_non_text_blocks() {
        let result = json!({
            "content": [
                {"type": "text", "text": "first"},
                {"type": "image", "data": "..."},
                {"type": "text", "text": "second"}
            ]
        });
        let text = render_content(&result);
        assert_eq!(text, "first\n[image content omitted]\nsecond");
    }

    #[test]
    fn tool_names_are_sanitized() {
        assert_eq!(sanitize("my-server.tool"), "my_server_tool");
    }
}
