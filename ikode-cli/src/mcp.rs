//! Third-party Model Context Protocol (MCP) client support.
//!
//! Server definitions live in machine-local settings, while live connections and
//! model-facing tool routes are session-scoped. The official MCP Rust SDK owns the
//! protocol lifecycle and both standard transports (stdio and Streamable HTTP).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use gaise_core::contracts::{GaiseTool, GaiseToolParameter};
use http::{HeaderName, HeaderValue};
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_EXPOSED_TOOL_NAME: usize = 64;
const MAX_TOOL_RESULT_BYTES: usize = 512 * 1024;

fn default_enabled() -> bool {
    true
}

/// A machine-local MCP server registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpServerConfig {
    /// Launch a local server and exchange newline-delimited JSON-RPC over stdio.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
    /// Connect to a remote Streamable HTTP MCP endpoint.
    #[serde(rename = "http", alias = "streamable_http")]
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
}

impl McpServerConfig {
    pub fn enabled(&self) -> bool {
        match self {
            Self::Stdio { enabled, .. } | Self::Http { enabled, .. } => *enabled,
        }
    }

    pub fn set_enabled(&mut self, value: bool) {
        match self {
            Self::Stdio { enabled, .. } | Self::Http { enabled, .. } => *enabled = value,
        }
    }

    pub fn transport_label(&self) -> &'static str {
        match self {
            Self::Stdio { .. } => "stdio",
            Self::Http { .. } => "http",
        }
    }
}

/// Parse `/mcp add ...` arguments.
///
/// Supported forms mirror common MCP CLIs:
/// - `<name> [--env KEY=VALUE] -- <command> [args...]`
/// - `<name> --url <url> [--header NAME=VALUE]`
pub fn parse_add_server(input: &str) -> Result<(String, McpServerConfig)> {
    let words = split_cli_words(input)?;
    let Some(name) = words.first().cloned() else {
        bail!("usage: /mcp add <name> [--env KEY=VALUE] -- <command> [args...] | /mcp add <name> --url <url> [--header NAME=VALUE]");
    };
    validate_server_name(&name)?;

    if let Some(url_index) = words.iter().position(|word| word == "--url") {
        if url_index != 1 {
            bail!("--url must appear immediately after the server name");
        }
        let url = words
            .get(url_index + 1)
            .filter(|value| !value.is_empty())
            .cloned()
            .context("--url requires an MCP endpoint URL")?;
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("MCP HTTP URL must start with http:// or https://");
        }
        let mut headers = BTreeMap::new();
        let mut enabled = true;
        let mut index = url_index + 2;
        while index < words.len() {
            match words[index].as_str() {
                "--header" => {
                    let raw = words
                        .get(index + 1)
                        .context("--header requires NAME=VALUE")?;
                    let (key, value) = split_assignment(raw, "header")?;
                    headers.insert(key, value);
                    index += 2;
                }
                "--disabled" => {
                    enabled = false;
                    index += 1;
                }
                other => bail!("unknown HTTP MCP option '{other}'"),
            }
        }
        return Ok((
            name,
            McpServerConfig::Http {
                url,
                headers,
                enabled,
            },
        ));
    }

    let separator = words
        .iter()
        .position(|word| word == "--")
        .context("stdio registration requires '--' before the command")?;
    let command = words
        .get(separator + 1)
        .filter(|value| !value.is_empty())
        .cloned()
        .context("stdio registration requires a command after '--'")?;
    let args = words[(separator + 2)..].to_vec();
    let mut env = BTreeMap::new();
    let mut enabled = true;
    let mut index = 1;
    while index < separator {
        match words[index].as_str() {
            "--env" => {
                let raw = words.get(index + 1).context("--env requires KEY=VALUE")?;
                let (key, value) = split_assignment(raw, "environment variable")?;
                env.insert(key, value);
                index += 2;
            }
            "--disabled" => {
                enabled = false;
                index += 1;
            }
            other => bail!("unknown stdio MCP option '{other}'"),
        }
    }
    Ok((
        name,
        McpServerConfig::Stdio {
            command,
            args,
            env,
            enabled,
        },
    ))
}

fn split_assignment(raw: &str, label: &str) -> Result<(String, String)> {
    let (key, value) = raw
        .split_once('=')
        .with_context(|| format!("{label} must use KEY=VALUE syntax"))?;
    if key.trim().is_empty() {
        bail!("{label} name cannot be empty");
    }
    Ok((key.trim().to_string(), value.to_string()))
}

fn split_cli_words(input: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Some(active) if ch == active => quote = None,
            Some(active) if ch == '\\' && chars.peek() == Some(&active) => {
                current.push(chars.next().expect("peeked character exists"));
            }
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            None => current.push(ch),
        }
    }
    if let Some(active) = quote {
        bail!("unterminated {active} quote");
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

pub fn validate_server_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 48 {
        bail!("MCP server name must contain 1-48 characters");
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        bail!("MCP server name may contain only letters, digits, '.', '-' and '_'");
    }
    Ok(())
}

/// Whether a name has the provider-safe namespace used for exposed MCP tools.
pub fn is_exposed_tool_name(name: &str) -> bool {
    name.strip_prefix("mcp__").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

type McpService = RunningService<RoleClient, ()>;

struct ConnectedServer {
    service: McpService,
    tool_count: usize,
}

#[derive(Clone)]
struct ToolRoute {
    server: String,
    remote_name: String,
    spec: GaiseTool,
}

/// User-facing connection state for one configured server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerStatus {
    pub name: String,
    pub transport: String,
    pub enabled: bool,
    pub connected: bool,
    pub tool_count: usize,
    pub error: Option<String>,
}

/// Live MCP connections and their namespaced model-tool routes.
pub struct McpManager {
    root: PathBuf,
    connections: BTreeMap<String, ConnectedServer>,
    failures: BTreeMap<String, String>,
    routes: BTreeMap<String, ToolRoute>,
}

impl McpManager {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            connections: BTreeMap::new(),
            failures: BTreeMap::new(),
            routes: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty() && self.failures.is_empty()
    }

    pub fn tool_specs(&self) -> Vec<GaiseTool> {
        self.routes
            .values()
            .map(|route| route.spec.clone())
            .collect()
    }

    pub fn has_tool(&self, exposed_name: &str) -> bool {
        self.routes.contains_key(exposed_name)
    }

    pub fn tool_label(&self, exposed_name: &str) -> Option<String> {
        self.routes
            .get(exposed_name)
            .map(|route| format!("{}::{}", route.server, route.remote_name))
    }

    pub fn statuses(&self, configs: &BTreeMap<String, McpServerConfig>) -> Vec<McpServerStatus> {
        configs
            .iter()
            .map(|(name, config)| {
                let connection = self.connections.get(name);
                McpServerStatus {
                    name: name.clone(),
                    transport: config.transport_label().to_string(),
                    enabled: config.enabled(),
                    connected: connection.is_some(),
                    tool_count: connection.map_or(0, |server| server.tool_count),
                    error: self.failures.get(name).cloned(),
                }
            })
            .collect()
    }

    /// Reconnect every enabled registration and rebuild the namespaced tool list.
    /// A broken server is isolated: its error is recorded while other servers load.
    pub async fn reload(&mut self, configs: &BTreeMap<String, McpServerConfig>) {
        self.disconnect_all().await;
        self.failures.clear();
        self.routes.clear();

        let mut discovered = Vec::new();
        for (name, config) in configs {
            if !config.enabled() {
                continue;
            }
            match self.connect(name, config).await {
                Ok((connection, tools)) => {
                    self.connections.insert(name.clone(), connection);
                    discovered.push((name.clone(), tools));
                }
                Err(error) => {
                    self.failures.insert(name.clone(), concise_error(&error));
                }
            }
        }
        self.rebuild_routes(discovered);
    }

    async fn disconnect_all(&mut self) {
        let old = std::mem::take(&mut self.connections);
        for mut connection in old.into_values() {
            let _ = connection
                .service
                .close_with_timeout(Duration::from_secs(2))
                .await;
        }
    }

    async fn connect(
        &self,
        name: &str,
        config: &McpServerConfig,
    ) -> Result<(ConnectedServer, Vec<Tool>)> {
        let service = match config {
            McpServerConfig::Stdio {
                command, args, env, ..
            } => {
                let mut process = Command::new(command);
                process
                    .args(args)
                    .current_dir(&self.root)
                    .kill_on_drop(true);
                for (key, value) in env {
                    process.env(key, expand_environment(value)?);
                }
                let transport = TokioChildProcess::new(process)
                    .with_context(|| format!("launch stdio server '{name}'"))?;
                tokio::time::timeout(CONNECT_TIMEOUT, ().serve(transport))
                    .await
                    .with_context(|| format!("MCP server '{name}' initialization timed out"))?
                    .map_err(|error| anyhow!("initialize MCP server '{name}': {error}"))?
            }
            McpServerConfig::Http { url, headers, .. } => {
                let mut custom = HashMap::new();
                let mut bearer = None;
                for (raw_name, raw_value) in headers {
                    let value = expand_environment(raw_value)?;
                    if raw_name.eq_ignore_ascii_case("authorization") {
                        let (scheme, token) = value
                            .split_once(' ')
                            .context("Authorization header must use Bearer <token>")?;
                        if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
                            bail!("Authorization header must use Bearer <token>");
                        }
                        bearer = Some(token.to_string());
                        continue;
                    }
                    let header_name = HeaderName::from_bytes(raw_name.as_bytes())
                        .with_context(|| format!("invalid HTTP header name '{raw_name}'"))?;
                    let header_value = HeaderValue::from_str(&value)
                        .with_context(|| format!("invalid value for HTTP header '{raw_name}'"))?;
                    custom.insert(header_name, header_value);
                }
                let mut transport_config =
                    StreamableHttpClientTransportConfig::with_uri(url.clone())
                        .custom_headers(custom);
                if let Some(token) = bearer {
                    transport_config = transport_config.auth_header(token);
                }
                let transport = StreamableHttpClientTransport::from_config(transport_config);
                tokio::time::timeout(CONNECT_TIMEOUT, ().serve(transport))
                    .await
                    .with_context(|| format!("MCP server '{name}' initialization timed out"))?
                    .map_err(|error| anyhow!("initialize MCP server '{name}': {error}"))?
            }
        };

        let tools = tokio::time::timeout(CONNECT_TIMEOUT, service.list_all_tools())
            .await
            .with_context(|| format!("MCP server '{name}' tools/list timed out"))?
            .map_err(|error| anyhow!("list tools from MCP server '{name}': {error}"))?;
        let tool_count = tools.len();
        Ok((
            ConnectedServer {
                service,
                tool_count,
            },
            tools,
        ))
    }

    fn rebuild_routes(&mut self, mut discovered: Vec<(String, Vec<Tool>)>) {
        discovered.sort_by(|left, right| left.0.cmp(&right.0));
        let mut used = BTreeSet::new();
        for (server, mut tools) in discovered {
            tools.sort_by(|left, right| left.name.cmp(&right.name));
            let mut remote_names = BTreeSet::new();
            for tool in tools {
                let remote_name = tool.name.to_string();
                if !remote_names.insert(remote_name.clone()) {
                    continue;
                }
                let exposed = exposed_tool_name(&server, &remote_name, &used);
                used.insert(exposed.clone());
                let spec = gaise_tool(&server, &exposed, &tool);
                self.routes.insert(
                    exposed,
                    ToolRoute {
                        server: server.clone(),
                        remote_name,
                        spec,
                    },
                );
            }
        }
    }

    /// Invoke a namespaced MCP tool and render its mixed MCP content as a bounded
    /// text result suitable for the current GAISe tool-result contract.
    pub async fn call(&self, exposed_name: &str, arguments: Option<&str>) -> Result<String> {
        let route = self
            .routes
            .get(exposed_name)
            .with_context(|| format!("unknown MCP tool '{exposed_name}'"))?;
        let connection = self
            .connections
            .get(&route.server)
            .with_context(|| format!("MCP server '{}' is not connected", route.server))?;
        if connection.service.is_closed() {
            bail!(
                "MCP server '{}' disconnected; run /mcp refresh",
                route.server
            );
        }
        let value: Value = serde_json::from_str(arguments.unwrap_or("{}"))
            .with_context(|| format!("invalid arguments for MCP tool '{exposed_name}'"))?;
        let object = value
            .as_object()
            .cloned()
            .context("MCP tool arguments must be a JSON object")?;
        let params = CallToolRequestParams::new(route.remote_name.clone()).with_arguments(object);
        let result = tokio::time::timeout(CALL_TIMEOUT, connection.service.call_tool(params))
            .await
            .with_context(|| format!("MCP tool '{exposed_name}' timed out"))?
            .map_err(|error| anyhow!("MCP tool '{exposed_name}' failed: {error}"))?;
        Ok(render_tool_result(&result))
    }
}

fn gaise_tool(server: &str, exposed_name: &str, tool: &Tool) -> GaiseTool {
    let mut description = format!("Third-party MCP tool from server '{server}'.");
    if let Some(title) = &tool.title {
        description.push(' ');
        description.push_str(title.trim());
        description.push('.');
    }
    if let Some(detail) = &tool.description {
        description.push(' ');
        description.push_str(detail.trim());
    }
    if description.len() > 2_000 {
        description.truncate(2_000);
    }
    let schema = Value::Object(tool.input_schema.as_ref().clone());
    GaiseTool {
        name: exposed_name.to_string(),
        description: Some(description),
        parameters: Some(schema_to_parameter(&schema, true)),
    }
}

fn schema_to_parameter(schema: &Value, root: bool) -> GaiseToolParameter {
    let selected = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
        .and_then(|variants| variants.iter().find(|value| !value.is_null()))
        .unwrap_or(schema);
    let mut parameter = GaiseToolParameter {
        r#type: schema_type(selected)
            .or_else(|| schema_type(schema))
            .or_else(|| root.then(|| "object".to_string())),
        description: schema_description(schema),
        ..Default::default()
    };
    if let Some(properties) = schema
        .get("properties")
        .or_else(|| selected.get("properties"))
        .and_then(Value::as_object)
    {
        parameter.properties = Some(
            properties
                .iter()
                .map(|(name, value)| (name.clone(), schema_to_parameter(value, false)))
                .collect(),
        );
    }
    if let Some(items) = schema
        .get("items")
        .or_else(|| selected.get("items"))
        .filter(|value| value.is_object())
    {
        parameter.items = Some(Box::new(schema_to_parameter(items, false)));
    }
    if let Some(required) = schema
        .get("required")
        .or_else(|| selected.get("required"))
        .and_then(Value::as_array)
    {
        let names = required
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        if !names.is_empty() {
            parameter.required = Some(names);
        }
    }
    parameter
}

fn schema_type(schema: &Value) -> Option<String> {
    match schema.get("type") {
        Some(Value::String(value)) => Some(normalize_schema_type(value)),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .find(|value| *value != "null")
            .map(normalize_schema_type),
        _ if schema.get("properties").is_some() => Some("object".to_string()),
        _ => schema
            .get("enum")
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(infer_json_type)
            .map(str::to_string),
    }
}

fn normalize_schema_type(value: &str) -> String {
    if value == "null" {
        "string".to_string()
    } else {
        value.to_string()
    }
}

fn infer_json_type(value: &Value) -> Option<&'static str> {
    match value {
        Value::Bool(_) => Some("boolean"),
        Value::Number(number) if number.is_i64() || number.is_u64() => Some("integer"),
        Value::Number(_) => Some("number"),
        Value::String(_) => Some("string"),
        Value::Array(_) => Some("array"),
        Value::Object(_) => Some("object"),
        Value::Null => None,
    }
}

fn schema_description(schema: &Value) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(description) = schema.get("description").and_then(Value::as_str) {
        parts.push(description.trim().to_string());
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let rendered = values
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("Allowed values: {rendered}."));
    }
    if let Some(value) = schema.get("const") {
        parts.push(format!("Required value: {value}."));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn exposed_tool_name(server: &str, remote: &str, used: &BTreeSet<String>) -> String {
    let raw_identity = format!("{server}\0{remote}");
    let safe_server = sanitize_component(server);
    let safe_remote = sanitize_component(remote);
    let base = format!("mcp__{safe_server}__{safe_remote}");
    if base.len() <= MAX_EXPOSED_TOOL_NAME && !used.contains(&base) {
        return base;
    }
    for salt in 0u32.. {
        let identity = if salt == 0 {
            raw_identity.clone()
        } else {
            format!("{raw_identity}\0{salt}")
        };
        let suffix = format!("__{:08x}", stable_hash(&identity));
        let keep = MAX_EXPOSED_TOOL_NAME.saturating_sub(suffix.len());
        let mut shortened = base.clone();
        shortened.truncate(keep);
        shortened.push_str(&suffix);
        if !used.contains(&shortened) {
            return shortened;
        }
    }
    unreachable!("u32 collision space exhausted")
}

fn sanitize_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_underscore = false;
    for byte in value.bytes() {
        let valid = byte.is_ascii_alphanumeric() || byte == b'_';
        let character = if valid { byte as char } else { '_' };
        if character == '_' && previous_underscore {
            continue;
        }
        previous_underscore = character == '_';
        output.push(character);
    }
    let output = output.trim_matches('_');
    if output.is_empty() {
        "tool".to_string()
    } else {
        output.to_string()
    }
}

fn stable_hash(value: &str) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for byte in value.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

fn expand_environment(value: &str) -> Result<String> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[(start + 2)..];
        let end = after
            .find('}')
            .context("unterminated ${VAR} environment reference")?;
        let key = &after[..end];
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            bail!("invalid environment reference '${{{key}}}'");
        }
        let replacement =
            std::env::var(key).with_context(|| format!("environment variable {key} is not set"))?;
        output.push_str(&replacement);
        rest = &after[(end + 1)..];
    }
    output.push_str(rest);
    Ok(output)
}

fn render_tool_result(result: &CallToolResult) -> String {
    let mut parts = Vec::new();
    for content in &result.content {
        let value = serde_json::to_value(content).unwrap_or(Value::Null);
        match value.get("type").and_then(Value::as_str) {
            Some("text") | Some("json") => {
                if let Some(text) = value.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                } else if let Some(json) = value.get("json") {
                    parts.push(pretty_json(json));
                }
            }
            Some("resource") => {
                if let Some(resource) = value.get("resource") {
                    if let Some(text) = resource.get("text").and_then(Value::as_str) {
                        let uri = resource
                            .get("uri")
                            .and_then(Value::as_str)
                            .unwrap_or("embedded resource");
                        parts.push(format!("Resource {uri}:\n{text}"));
                    } else {
                        parts.push(format!("Embedded resource: {}", pretty_json(resource)));
                    }
                }
            }
            Some("resource_link") => {
                let uri = value
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown URI");
                let name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("resource");
                parts.push(format!("Resource link: {name} ({uri})"));
            }
            Some("image") | Some("audio") => {
                let kind = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("binary");
                let mime = value
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown MIME type");
                parts.push(format!(
                    "[{kind} result omitted from text transcript: {mime}]"
                ));
            }
            _ => parts.push(pretty_json(&value)),
        }
    }
    if let Some(structured) = &result.structured_content {
        let rendered = pretty_json(structured);
        if !parts.iter().any(|part| part.trim() == rendered.trim()) {
            parts.push(format!("Structured result:\n{rendered}"));
        }
    }
    let mut output = if parts.is_empty() {
        "MCP tool returned no content.".to_string()
    } else {
        parts.join("\n\n")
    };
    if result.is_error == Some(true) {
        output = format!("MCP tool reported an error:\n{output}");
    }
    bound_tool_result(output)
}

fn bound_tool_result(mut output: String) -> String {
    if output.len() <= MAX_TOOL_RESULT_BYTES {
        return output;
    }
    let mut end = MAX_TOOL_RESULT_BYTES;
    while !output.is_char_boundary(end) {
        end -= 1;
    }
    output.truncate(end);
    output.push_str("\n\n[MCP tool result truncated at 512 KiB]");
    output
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn concise_error(error: &anyhow::Error) -> String {
    let mut text = format!("{error:#}").replace(['\r', '\n'], " ");
    if text.len() > 500 {
        text.truncate(500);
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Json;
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use axum::Router;

    #[test]
    fn parses_stdio_and_http_registration_forms() {
        let (name, stdio) = parse_add_server(
            r#"filesystem --env TOKEN=${MCP_TOKEN} -- npx -y "@modelcontextprotocol/server-filesystem" ."#,
        )
        .unwrap();
        assert_eq!(name, "filesystem");
        assert_eq!(
            stdio,
            McpServerConfig::Stdio {
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                    ".".to_string(),
                ],
                env: BTreeMap::from([("TOKEN".to_string(), "${MCP_TOKEN}".to_string())]),
                enabled: true,
            }
        );

        let (_, http) = parse_add_server(
            r#"remote --url https://example.test/mcp --header "Authorization=Bearer ${MCP_TOKEN}""#,
        )
        .unwrap();
        assert_eq!(
            http,
            McpServerConfig::Http {
                url: "https://example.test/mcp".to_string(),
                headers: BTreeMap::from([(
                    "Authorization".to_string(),
                    "Bearer ${MCP_TOKEN}".to_string(),
                )]),
                enabled: true,
            }
        );
    }

    #[test]
    fn rejects_ambiguous_or_unsafe_registration_input() {
        assert!(parse_add_server("").is_err());
        assert!(parse_add_server("bad/name -- cmd").is_err());
        assert!(parse_add_server("x npx -y server").is_err());
        assert!(parse_add_server("x --url file:///tmp/server").is_err());
        assert!(parse_add_server("x -- npx 'unterminated").is_err());
    }

    #[test]
    fn converts_common_json_schema_and_retains_enum_guidance() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "city": {"type": "string", "description": "A city"},
                "units": {"type": "string", "enum": ["c", "f"]},
                "days": {"type": "array", "items": {"type": "integer"}}
            },
            "required": ["city"]
        });
        let parameter = schema_to_parameter(&schema, true);
        assert_eq!(parameter.r#type.as_deref(), Some("object"));
        assert_eq!(parameter.required, Some(vec!["city".to_string()]));
        let properties = parameter.properties.unwrap();
        assert_eq!(properties["city"].r#type.as_deref(), Some("string"));
        assert!(properties["units"]
            .description
            .as_deref()
            .unwrap()
            .contains("Allowed values"));
        assert_eq!(
            properties["days"].items.as_ref().unwrap().r#type.as_deref(),
            Some("integer")
        );
    }

    #[test]
    fn tool_results_are_bounded_without_splitting_utf8() {
        let output = bound_tool_result("\u{20ac}".repeat(MAX_TOOL_RESULT_BYTES));
        assert!(output.is_char_boundary(output.len()));
        assert!(output.ends_with("[MCP tool result truncated at 512 KiB]"));
        assert!(output.len() < MAX_TOOL_RESULT_BYTES + 100);
    }

    #[test]
    fn exposed_names_are_provider_safe_bounded_and_collision_resistant() {
        let mut used = BTreeSet::new();
        let first = exposed_tool_name("git-hub", "issues.list", &used);
        assert_eq!(first, "mcp__git_hub__issues_list");
        used.insert(first);
        let collision = exposed_tool_name("git.hub", "issues-list", &used);
        assert_ne!(collision, "mcp__git_hub__issues_list");
        assert!(collision.starts_with("mcp__git_hub__issues_list__"));
        let long = exposed_tool_name(&"s".repeat(48), &"t".repeat(128), &used);
        assert!(long.len() <= MAX_EXPOSED_TOOL_NAME);
        assert!(long
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'));
    }

    #[test]
    fn server_config_round_trips_with_enabled_default() {
        let value = serde_json::json!({
            "transport": "stdio",
            "command": "npx",
            "args": ["server"]
        });
        let config: McpServerConfig = serde_json::from_value(value).unwrap();
        assert!(config.enabled());
        let round_trip: McpServerConfig =
            serde_json::from_value(serde_json::to_value(config).unwrap()).unwrap();
        assert!(round_trip.enabled());
    }

    async fn fake_http_mcp(Json(request): Json<Value>) -> Response {
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "notifications/initialized" {
            return StatusCode::ACCEPTED.into_response();
        }
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => serde_json::json!({
                "protocolVersion": request["params"]["protocolVersion"],
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "fake-mcp", "version": "1.0.0"}
            }),
            "tools/list" => serde_json::json!({
                "tools": [{
                    "name": "echo.value",
                    "description": "Echo a value",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"value": {"type": "string"}},
                        "required": ["value"]
                    }
                }]
            }),
            "tools/call" => serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!("echo: {}", request["params"]["arguments"]["value"])
                }],
                "isError": false
            }),
            _ => {
                return Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": "Method not found"}
                }))
                .into_response();
            }
        };
        Json(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
    }

    #[tokio::test]
    async fn streamable_http_registration_discovers_and_calls_tools() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/mcp", post(fake_http_mcp)))
                .await
                .unwrap();
        });
        let root = tempfile::tempdir().unwrap();
        let configs = BTreeMap::from([(
            "fake".to_string(),
            McpServerConfig::Http {
                url: format!("http://{address}/mcp"),
                headers: BTreeMap::new(),
                enabled: true,
            },
        )]);
        let mut manager = McpManager::new(root.path().to_path_buf());
        manager.reload(&configs).await;
        let tools = manager.tool_specs();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "mcp__fake__echo_value");
        let result = manager
            .call(&tools[0].name, Some(r#"{"value":"hello"}"#))
            .await
            .unwrap();
        assert_eq!(result, "echo: \"hello\"");
        server.abort();
    }
}
