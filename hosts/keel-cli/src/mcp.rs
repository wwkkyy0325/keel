//! Minimal MCP (Model Context Protocol) client.
//!
//! A plugin is either a local MCP server process (stdio) or a remote MCP
//! endpoint (streamable HTTP / SSE). The host connects it, lists its tools and
//! registers each one as a kernel tool. The kernel never knows MCP exists; it
//! sees ordinary typed tools. The protocol is public and documented at
//! modelcontextprotocol.io.
//!
//! The trust boundary is the host: a server is an external process or service
//! the user chose. The permission gate still runs for every call, but the
//! kernel cannot sandbox what the server does on its own side.

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
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;

const PROTOCOL_VERSION: &str = "2024-11-05";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// One server message. A larger line or HTTP body is refused instead of being
/// buffered; the kernel clips a tool result before it reaches the model anyway.
const MAX_MCP_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

/// One MCP server entry in the plugin file.
#[derive(Deserialize)]
pub struct ServerConfig {
    pub name: String,
    /// Local stdio server command. Exactly one of `command` and `url` is set.
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Remote streamable-HTTP endpoint.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Deserialize)]
struct PluginFile {
    #[serde(default)]
    servers: Vec<ServerConfig>,
}

struct StdioTransport {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

enum Transport {
    Stdio(Box<StdioTransport>),
    Http {
        client: reqwest::Client,
        url: String,
        session: Option<String>,
    },
}

impl Drop for Transport {
    fn drop(&mut self) {
        if let Transport::Stdio(inner) = self {
            kill_tree(&mut inner.child);
        }
    }
}

/// Reads one newline-delimited message with a hard size cap. `Lines` would
/// grow the buffer until a newline arrives, so a broken server could exhaust
/// memory before the request timeout.
async fn read_message<R>(reader: &mut R) -> Result<Option<String>, String>
where
    R: AsyncBufRead + Unpin,
{
    let mut message = Vec::new();
    loop {
        let (chunk, consumed, done) = {
            let available = reader.fill_buf().await.map_err(|error| error.to_string())?;
            if available.is_empty() {
                (Vec::new(), 0, true)
            } else if let Some(position) = available.iter().position(|byte| *byte == b'\n') {
                (available[..=position].to_vec(), position + 1, true)
            } else {
                (available.to_vec(), available.len(), false)
            }
        };
        message.extend_from_slice(&chunk);
        reader.consume(consumed);
        if message.len() > MAX_MCP_MESSAGE_BYTES {
            return Err(format!(
                "server message exceeds {MAX_MCP_MESSAGE_BYTES} bytes"
            ));
        }
        if done {
            break;
        }
    }
    if message.is_empty() {
        return Ok(None);
    }
    String::from_utf8(message)
        .map(Some)
        .map_err(|_| "server message is not UTF-8".to_owned())
}

/// Kills the server and its descendants.
///
/// On Windows the direct child is often `cmd /c npx ...`; killing only it
/// leaves the runtime it started alive, holding files or ports. The same
/// applies to wrapper scripts on Unix, so the server runs in its own process
/// group and the whole group is signalled.
fn kill_tree(child: &mut Child) {
    let Some(pid) = child.id() else {
        return;
    };
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.start_kill();
    let _ = child.try_wait();
}

/// A connected MCP server and its JSON-RPC channel.
struct McpServer {
    transport: Transport,
    next_id: u64,
    /// Roots advertised through the `roots` capability; the first is the
    /// workspace and the spawn working directory.
    roots: Vec<PathBuf>,
}

impl McpServer {
    async fn spawn(command: &str, args: &[String], roots: &[PathBuf]) -> Result<Self, String> {
        let primary = roots.first().cloned().unwrap_or_default();
        let program = command;
        let mut command = Command::new(program);
        command
            .args(args)
            // Relative paths in a server should resolve against the workspace
            // it serves, matching the `roots` reply below.
            .current_dir(&primary)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start `{program}`: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "server has no stdin".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "server has no stdout".to_owned())?;
        Ok(Self {
            transport: Transport::Stdio(Box::new(StdioTransport {
                child,
                stdin,
                reader: BufReader::new(stdout),
            })),
            next_id: 1,
            roots: roots.to_vec(),
        })
    }

    fn http(url: String, roots: &[PathBuf]) -> Self {
        Self {
            transport: Transport::Http {
                client: reqwest::Client::new(),
                url,
                session: None,
            },
            next_id: 1,
            roots: roots.to_vec(),
        }
    }

    /// Sends one JSON-RPC message. For HTTP it returns the response body.
    async fn send(&mut self, message: &Value) -> Result<Option<String>, String> {
        match &mut self.transport {
            Transport::Stdio(inner) => {
                let mut line = message.to_string();
                line.push('\n');
                inner
                    .stdin
                    .write_all(line.as_bytes())
                    .await
                    .map_err(|error| format!("cannot write to server: {error}"))?;
                inner
                    .stdin
                    .flush()
                    .await
                    .map_err(|error| format!("cannot flush server stdin: {error}"))?;
                Ok(None)
            }
            Transport::Http {
                client,
                url,
                session,
            } => {
                let mut request = client
                    .post(url.as_str())
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json, text/event-stream");
                if let Some(session) = session.as_deref() {
                    request = request.header("Mcp-Session-Id", session);
                }
                let mut response = timeout(REQUEST_TIMEOUT, request.json(message).send())
                    .await
                    .map_err(|_| "HTTP request timed out".to_owned())?
                    .map_err(|error| format!("HTTP request failed: {error}"))?;
                if let Some(id) = response.headers().get("mcp-session-id") {
                    if let Ok(id) = id.to_str() {
                        *session = Some(id.to_owned());
                    }
                }
                let status = response.status();
                let body = timeout(REQUEST_TIMEOUT, async {
                    let mut body = Vec::new();
                    while let Some(chunk) = response
                        .chunk()
                        .await
                        .map_err(|error| format!("cannot read HTTP body: {error}"))?
                    {
                        if body.len() + chunk.len() > MAX_MCP_MESSAGE_BYTES {
                            return Err(format!("HTTP body exceeds {MAX_MCP_MESSAGE_BYTES} bytes"));
                        }
                        body.extend_from_slice(&chunk);
                    }
                    String::from_utf8(body).map_err(|_| "HTTP body is not UTF-8".to_owned())
                })
                .await
                .map_err(|_| "HTTP body timed out".to_owned())??;
                if !status.is_success() {
                    return Err(format!("HTTP {status}: {body}"));
                }
                Ok(Some(body))
            }
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let body = self.send(&message).await?;
        let roots = self.roots.clone();
        // HTTP: the response body may carry server requests next to our
        // response. Answer them with a POST before returning.
        if let Some(body) = body {
            let (response, requests) = split_http_response(&body, id)?;
            for request in requests {
                if let (Some(method), Some(request_id)) = (
                    request.get("method").and_then(Value::as_str),
                    request.get("id"),
                ) {
                    let reply = server_request_reply(method, request_id, &roots);
                    self.send(&reply).await?;
                }
            }
            return Ok(response);
        }
        let Transport::Stdio(inner) = &mut self.transport else {
            return Err("unexpected transport".to_owned());
        };
        loop {
            let line = timeout(REQUEST_TIMEOUT, read_message(&mut inner.reader))
                .await
                .map_err(|_| format!("`{method}` timed out"))?
                .map_err(|error| format!("cannot read from server: {error}"))?
                .ok_or_else(|| "server closed its stdout".to_owned())?;
            let value: Value = serde_json::from_str(&line)
                .map_err(|error| format!("invalid JSON from server: {error}"))?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                // A server request has a method and an id; a notification
                // has no id. Answering requests keeps servers that ask for
                // roots or ping from waiting; sampling is refused.
                if let (Some(method), Some(request_id)) =
                    (value.get("method").and_then(Value::as_str), value.get("id"))
                {
                    let reply = server_request_reply(method, request_id, &roots);
                    let mut line = reply.to_string();
                    line.push('\n');
                    inner
                        .stdin
                        .write_all(line.as_bytes())
                        .await
                        .map_err(|error| format!("cannot answer server request: {error}"))?;
                    inner
                        .stdin
                        .flush()
                        .await
                        .map_err(|error| format!("cannot flush server stdin: {error}"))?;
                }
                continue;
            }
            return response_result(value);
        }
    }

    async fn notify(&mut self, method: &str) -> Result<(), String> {
        let message = json!({"jsonrpc": "2.0", "method": method});
        self.send(&message).await.map(|_| ())
    }

    async fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"roots": {"listChanged": false}},
                "clientInfo": {"name": "keel", "version": "0.1.0"}
            }),
        )
        .await?;
        self.notify("notifications/initialized").await
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

/// Answers a request the server sent to this client.
///
/// `roots/list` returns the workspace and `ping` is acknowledged; anything else
/// is refused with a JSON-RPC error instead of being ignored. Sampling is
/// deliberately refused: it would spend the user's tokens and send context to
/// the server without going through the kernel's permission and audit path.
fn server_request_reply(method: &str, id: &Value, roots: &[PathBuf]) -> Value {
    match method {
        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        "roots/list" => {
            let roots: Vec<Value> = roots
                .iter()
                .enumerate()
                .map(|(index, root)| {
                    json!({
                        "uri": workspace_uri(root),
                        "name": if index == 0 { "workspace".to_owned() } else { format!("root-{index}") }
                    })
                })
                .collect();
            json!({"jsonrpc": "2.0", "id": id, "result": {"roots": roots}})
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("`{method}` is not supported by this client")
            }
        }),
    }
}

fn workspace_uri(root: &Path) -> String {
    let path = root
        .to_string_lossy()
        .replace('\\', "/")
        .replace(' ', "%20");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

fn response_result(value: Value) -> Result<Value, String> {
    if let Some(error) = value.get("error") {
        return Err(format!("server error: {error}"));
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

/// Parses a JSON or SSE response and returns the result for `id`.
/// Splits a JSON or SSE body into our response and any server requests.
///
/// A streamable-HTTP response may carry several JSON-RPC messages: the reply
/// to our request plus requests the server wants the client to answer.
fn split_http_response(body: &str, id: u64) -> Result<(Value, Vec<Value>), String> {
    let mut messages = Vec::new();
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Array(items)) => messages.extend(items),
        Ok(value) => messages.push(value),
        Err(_) => {
            for line in body.lines() {
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<Value>(data) {
                    messages.push(value);
                }
            }
        }
    }
    let response = messages
        .iter()
        .find(|value| value.get("id").and_then(Value::as_u64) == Some(id))
        .or_else(|| messages.iter().find(|value| value.get("result").is_some()))
        .cloned()
        .ok_or_else(|| "no matching JSON-RPC response".to_owned())?;
    let requests = messages
        .into_iter()
        .filter(|value| value.get("method").is_some() && value.get("id").is_some())
        .collect();
    Ok((response_result(response)?, requests))
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
                // arguments, so the whole call becomes one JSON string. The
                // description carries the structure instead of the raw schema,
                // which was clipped before it reached the fields.
                let shape = describe_schema(&schema);
                (
                    vec![ParamSpec {
                        name: "arguments".to_owned(),
                        kind: ParamKind::String,
                        required: false,
                    }],
                    ParamMode::Json,
                    format!("{description} Pass one JSON object in `arguments`. {shape}"),
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

/// One server probe, for `--plugins-check` and a configuration page.
#[derive(serde::Serialize)]
pub struct ServerStatus {
    pub name: String,
    pub tools: usize,
    pub error: Option<String>,
    pub duration_ms: u128,
}

/// Starts every configured server, performs the handshake and lists tools.
///
/// A failing server does not stop the others; the caller reports each line and
/// decides the exit code. The transports are dropped at the end of each probe,
/// which kills the server process tree.
pub async fn check(path: &Path, roots: &[PathBuf]) -> Result<Vec<ServerStatus>, KernelError> {
    let file = read_plugin_file(path)?;
    let mut statuses = Vec::new();
    for server in file.servers {
        let started = std::time::Instant::now();
        let result = async {
            let mut running = connect(&server, roots).await?;
            running.initialize().await?;
            Ok::<usize, String>(running.list_tools().await?.len())
        }
        .await;
        let tools = result.as_ref().copied().unwrap_or(0);
        statuses.push(ServerStatus {
            name: server.name,
            tools,
            error: result.err(),
            duration_ms: started.elapsed().as_millis(),
        });
    }
    Ok(statuses)
}

fn read_plugin_file(path: &Path) -> Result<PluginFile, KernelError> {
    let raw = std::fs::read_to_string(path).map_err(|error| KernelError::Io {
        message: format!("cannot read plugins `{}`: {error}", path.display()),
    })?;
    toml::from_str(&raw).map_err(|error| KernelError::InvalidConfig {
        message: format!("invalid plugins `{}`: {error}", path.display()),
    })
}

async fn connect(server: &ServerConfig, roots: &[PathBuf]) -> Result<McpServer, String> {
    if let Some(url) = &server.url {
        Ok(McpServer::http(url.clone(), roots))
    } else if let Some(command) = &server.command {
        McpServer::spawn(command, &server.args, roots).await
    } else {
        Err("set either `command` or `url`".to_owned())
    }
}

/// Loads the plugin file and registers every discovered MCP tool.
pub async fn load(
    path: &Path,
    roots: &[PathBuf],
    registry: &mut keel_runtime::ToolRegistry,
) -> Result<Vec<String>, KernelError> {
    let file = read_plugin_file(path)?;
    let primary = roots.first().cloned().unwrap_or_default();
    let mut names = Vec::new();
    for server in file.servers {
        let mut running = match connect(&server, roots).await {
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
            if let Some(tool) = McpTool::new(shared.clone(), &server.name, &primary, &tool) {
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
            Some("integer") => ParamKind::Integer,
            // `number` may be fractional, and the kernel models integers only.
            // Advertising it as an integer would silently drop the fraction,
            // so the tool falls back to one JSON argument like nested objects.
            Some("number") => return None,
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

/// Describes a nested schema for the JSON fallback parameter.
///
/// The model sees one `arguments` string, so the description must carry the
/// structure: field names, types, required names and a minimal example. A
/// clipped raw schema often lost the fields at the end.
fn describe_schema(schema: &Value) -> String {
    let mut parts = Vec::new();
    describe_fields(schema, "", &mut parts);
    let mut text = if parts.is_empty() {
        "It is an object with no declared fields.".to_owned()
    } else {
        parts.join("; ")
    };
    if let Ok(example) = serde_json::to_string(&example_value(schema)) {
        text.push_str(" Example: ");
        text.push_str(&example);
    }
    clip(&text, 900)
}

fn describe_fields(schema: &Value, prefix: &str, parts: &mut Vec<String>) {
    let label = type_label(schema);
    if label == "array of object" || label == "array of array" {
        parts.push(if prefix.is_empty() {
            label.to_owned()
        } else {
            format!("{prefix}: {label}")
        });
        if let Some(items) = schema.get("items") {
            describe_object(items, &format!("{prefix}[]."), parts);
        }
    } else if label == "object" {
        describe_object(schema, prefix, parts);
    }
}

fn describe_object(schema: &Value, prefix: &str, parts: &mut Vec<String>) {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (name, property) in properties {
        let label = type_label(property);
        let required = if required.contains(&name.as_str()) {
            " (required)"
        } else {
            ""
        };
        parts.push(format!("{prefix}{name}: {label}{required}"));
        if label == "object" {
            describe_object(property, &format!("{prefix}{name}."), parts);
        } else if label == "array of object" {
            if let Some(items) = property.get("items") {
                describe_object(items, &format!("{prefix}{name}[]."), parts);
            }
        }
    }
}

fn type_label(schema: &Value) -> &'static str {
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => "string",
        Some("integer") => "integer",
        Some("number") => "number",
        Some("boolean") => "boolean",
        Some("object") => "object",
        Some("array") => match schema
            .get("items")
            .and_then(|items| items.get("type"))
            .and_then(Value::as_str)
        {
            Some("object") => "array of object",
            Some("array") => "array of array",
            Some("string") => "array of string",
            Some("integer") | Some("number") => "array of number",
            Some("boolean") => "array of boolean",
            _ => "array",
        },
        _ => "value",
    }
}

fn example_value(schema: &Value) -> Value {
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let mut object = serde_json::Map::new();
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (name, property) in properties {
                    object.insert(name.clone(), example_value(property));
                }
            }
            Value::Object(object)
        }
        Some("array") => {
            let item = schema
                .get("items")
                .map(example_value)
                .unwrap_or_else(|| Value::String("text".to_owned()));
            Value::Array(vec![item])
        }
        Some("string") => Value::String("text".to_owned()),
        Some("integer") | Some("number") => Value::from(0),
        Some("boolean") => Value::Bool(false),
        _ => Value::String("text".to_owned()),
    }
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut clipped: String = text.chars().take(limit).collect();
    clipped.push_str("...");
    clipped
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
    fn a_fractional_number_switches_to_json_mode() {
        let schema = json!({
            "type": "object",
            "properties": {
                "temperature": {"type": "number"},
                "prompt": {"type": "string"}
            }
        });
        assert!(params_from_schema(&schema).is_none());
    }

    #[test]
    fn a_nested_schema_is_described_with_fields_and_an_example() {
        let schema = json!({
            "type": "object",
            "properties": {
                "entities": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "entityType": {"type": "string"},
                            "observations": {
                                "type": "array",
                                "items": {"type": "string"}
                            }
                        },
                        "required": ["name", "entityType"]
                    }
                }
            },
            "required": ["entities"]
        });
        let text = describe_schema(&schema);
        assert!(
            text.contains("entities: array of object (required)"),
            "{text}"
        );
        assert!(
            text.contains("entities[].name: string (required)"),
            "{text}"
        );
        assert!(
            text.contains("entities[].observations: array of string"),
            "{text}"
        );
        assert!(text.contains("Example: {\"entities\":[{\""), "{text}");
        assert!(text.contains("\"observations\":[\"text\"]"), "{text}");
        assert!(
            text.chars().count() <= 901,
            "the description must stay bounded"
        );
    }

    #[test]
    fn a_server_status_serializes_for_a_config_page() {
        let status = ServerStatus {
            name: "sqlite".to_owned(),
            tools: 2,
            error: Some("boom".to_owned()),
            duration_ms: 12,
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["name"], "sqlite");
        assert_eq!(value["tools"], 2);
        assert_eq!(value["error"], "boom");
        assert_eq!(value["duration_ms"], 12);
    }

    #[test]
    fn server_requests_are_answered_by_method() {
        let roots = [PathBuf::from("workspace root"), PathBuf::from("archive")];
        let ping = server_request_reply("ping", &json!(7), &roots);
        assert_eq!(ping["result"], json!({}));
        assert_eq!(ping["id"], json!(7));

        let reply = server_request_reply("roots/list", &json!("r1"), &roots);
        assert_eq!(reply["result"]["roots"][0]["name"], "workspace");
        assert_eq!(reply["result"]["roots"][1]["name"], "root-1");
        let uri = reply["result"]["roots"][0]["uri"].as_str().unwrap();
        assert!(uri.ends_with("/workspace%20root"), "{uri}");

        let refused = server_request_reply("sampling/createMessage", &json!(8), &roots);
        assert_eq!(refused["error"]["code"], json!(-32601));
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

    #[tokio::test]
    async fn a_stdio_message_is_read_up_to_its_newline() {
        let data = b"{\"jsonrpc\":\"2.0\"}\nnext";
        let mut reader = BufReader::new(&data[..]);

        assert_eq!(
            read_message(&mut reader).await.unwrap().as_deref(),
            Some("{\"jsonrpc\":\"2.0\"}\n")
        );
        assert_eq!(
            read_message(&mut reader).await.unwrap().as_deref(),
            Some("next")
        );
        assert_eq!(read_message(&mut reader).await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_oversized_stdio_message_is_refused() {
        let data = vec![b'x'; MAX_MCP_MESSAGE_BYTES + 1];
        let mut reader = BufReader::new(&data[..]);

        let error = read_message(&mut reader).await.unwrap_err();

        assert!(error.contains("exceeds"), "{error}");
    }

    #[tokio::test]
    async fn an_oversized_http_body_is_refused() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = [0u8; 4096];
            let _ = socket.read(&mut buffer).await;
            let header =
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
            if socket.write_all(header.as_bytes()).await.is_err() {
                return;
            }
            let chunk = vec![b'x'; 64 * 1024];
            for _ in 0..(MAX_MCP_MESSAGE_BYTES / chunk.len() + 2) {
                if socket.write_all(&chunk).await.is_err() {
                    break;
                }
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let mut server = McpServer::http(format!("http://{address}"), &[dir.path().to_path_buf()]);
        let error = server.request("tools/list", json!({})).await.unwrap_err();

        assert!(error.contains("exceeds"), "{error}");
    }

    #[tokio::test]
    async fn a_remote_http_server_is_listed_and_called() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        async fn mock_server(seen: Arc<Mutex<Vec<Value>>>) -> String {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    let seen = seen.clone();
                    tokio::spawn(async move {
                        let mut buffer = Vec::new();
                        let mut chunk = [0u8; 4096];
                        let end = loop {
                            let read = socket.read(&mut chunk).await.unwrap_or(0);
                            if read == 0 {
                                return;
                            }
                            buffer.extend_from_slice(&chunk[..read]);
                            if let Some(index) =
                                buffer.windows(4).position(|window| window == b"\r\n\r\n")
                            {
                                break index + 4;
                            }
                        };
                        let headers = String::from_utf8_lossy(&buffer[..end]).to_ascii_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        while buffer.len() < end + length {
                            let read = socket.read(&mut chunk).await.unwrap_or(0);
                            if read == 0 {
                                return;
                            }
                            buffer.extend_from_slice(&chunk[..read]);
                        }
                        let body = String::from_utf8_lossy(&buffer[end..end + length]);
                        let value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
                        let id = value.get("id").cloned().unwrap_or(Value::Null);
                        // A client response to a server request has no method.
                        if value.get("method").is_none() {
                            seen.lock().await.push(value);
                            let response = "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                            let _ = socket.write_all(response.as_bytes()).await;
                            return;
                        }
                        let result = match method {
                            "initialize" => json!({
                                "protocolVersion": "2024-11-05",
                                "capabilities": {},
                                "serverInfo": {"name": "mock", "version": "1"}
                            }),
                            "tools/list" => json!({
                                "tools": [{
                                    "name": "echo",
                                    "description": "echo text",
                                    "inputSchema": {
                                        "type": "object",
                                        "properties": {"text": {"type": "string"}},
                                        "required": ["text"]
                                    }
                                }]
                            }),
                            "tools/call" => json!({
                                "content": [{"type": "text", "text": "echo: mock"}]
                            }),
                            _ => json!({}),
                        };
                        let (status, content_type, body) = if id.is_null() {
                            ("202 Accepted", "application/json", String::new())
                        } else if method == "tools/list" {
                            // The SSE stream carries a server request next to
                            // the response to the client's request.
                            let roots = json!({
                                "jsonrpc": "2.0",
                                "id": 500,
                                "method": "roots/list",
                                "params": {}
                            });
                            let response = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": result
                            });
                            (
                                "200 OK",
                                "text/event-stream",
                                format!("data: {roots}\n\ndata: {response}\n\n"),
                            )
                        } else {
                            (
                                "200 OK",
                                "application/json",
                                json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string(),
                            )
                        };
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nMcp-Session-Id: test\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                    });
                }
            });
            format!("http://{address}/mcp")
        }

        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut server = McpServer::http(mock_server(seen.clone()).await, &[PathBuf::from(".")]);
        server.initialize().await.unwrap();
        let tools = server.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "echo");
        let replies = seen.lock().await;
        assert_eq!(replies.len(), 1, "the server request must be answered");
        assert_eq!(replies[0]["id"], 500);
        assert_eq!(replies[0]["result"]["roots"][0]["name"], "workspace");
        drop(replies);
        let result = server
            .call_tool("echo", json!({"text": "hi"}))
            .await
            .unwrap();
        assert_eq!(render_content(&result), "echo: mock");
    }

    #[test]
    fn tool_names_are_sanitized() {
        assert_eq!(sanitize("my-server.tool"), "my_server_tool");
    }

    /// A third server family on a non-Node runtime. Python is not a project
    /// dependency, so the test reports success without running when it is
    /// missing; the fixture itself uses only the standard library.
    #[tokio::test]
    async fn a_python_sqlite_server_is_probed_and_called() {
        let python = std::process::Command::new("python")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if !python {
            return;
        }
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../evals/harness/fixtures/mcp_sqlite_server.py");
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let plugins = dir.path().join("plugins.toml");
        std::fs::write(
            &plugins,
            format!(
                "[[servers]]\nname = \"sqlite\"\ncommand = \"python\"\nargs = ['{}', '{}']\n",
                fixture.display(),
                db.display()
            ),
        )
        .unwrap();

        let statuses = check(&plugins, &[dir.path().to_path_buf()]).await.unwrap();
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].error.is_none(), "{:?}", statuses[0].error);
        assert_eq!(statuses[0].tools, 2);

        let mut server = McpServer::spawn(
            "python",
            &[fixture.display().to_string(), db.display().to_string()],
            &[dir.path().to_path_buf()],
        )
        .await
        .unwrap();
        server.initialize().await.unwrap();
        let roots = server.call_tool("roots_seen", json!({})).await.unwrap();
        assert_eq!(render_content(&roots), workspace_uri(dir.path()));
        server
            .call_tool("sqlite_query", json!({"sql": "CREATE TABLE t (n INTEGER)"}))
            .await
            .unwrap();
        server
            .call_tool("sqlite_query", json!({"sql": "INSERT INTO t VALUES (7)"}))
            .await
            .unwrap();
        let result = server
            .call_tool("sqlite_query", json!({"sql": "SELECT n FROM t"}))
            .await
            .unwrap();
        assert_eq!(render_content(&result), "[[7]]");
    }
}
