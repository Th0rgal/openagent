//! MCP Server for core host tools (filesystem + command execution).
//!
//! Exposes a minimal set of Open Agent tools to OpenCode via MCP.
//! Communicates over stdio using JSON-RPC 2.0.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use open_agent::tools;
use open_agent::tools::Tool;

// =============================================================================
// JSON-RPC Types
// =============================================================================

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

// =============================================================================
// MCP Types
// =============================================================================

#[derive(Debug, Serialize)]
struct ToolDefinition {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Debug, Serialize)]
struct ToolResult {
    content: Vec<ToolContent>,
    #[serde(rename = "isError")]
    is_error: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
}

// =============================================================================
// Tool Registry
// =============================================================================

fn working_dir() -> PathBuf {
    std::env::var("OPEN_AGENT_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn tool_set() -> HashMap<String, Arc<dyn Tool>> {
    let mut tools: HashMap<String, Arc<dyn Tool>> = HashMap::new();

    tools.insert("read_file".to_string(), Arc::new(tools::ReadFile));
    tools.insert(
        "write_file".to_string(),
        Arc::new(tools::WriteFile),
    );
    tools.insert(
        "delete_file".to_string(),
        Arc::new(tools::DeleteFile),
    );
    tools.insert(
        "list_directory".to_string(),
        Arc::new(tools::ListDirectory),
    );
    tools.insert(
        "search_files".to_string(),
        Arc::new(tools::SearchFiles),
    );
    tools.insert("grep_search".to_string(), Arc::new(tools::GrepSearch));
    tools.insert("run_command".to_string(), Arc::new(tools::RunCommand));
    tools.insert("git_status".to_string(), Arc::new(tools::GitStatus));
    tools.insert("git_diff".to_string(), Arc::new(tools::GitDiff));
    tools.insert("git_commit".to_string(), Arc::new(tools::GitCommit));
    tools.insert("git_log".to_string(), Arc::new(tools::GitLog));
    tools.insert("web_search".to_string(), Arc::new(tools::WebSearch));
    tools.insert("fetch_url".to_string(), Arc::new(tools::FetchUrl));

    tools
}

fn tool_definitions(tools: &HashMap<String, Arc<dyn Tool>>) -> Vec<ToolDefinition> {
    let mut defs = Vec::new();
    for tool in tools.values() {
        defs.push(ToolDefinition {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            input_schema: tool.parameters_schema(),
        });
    }
    defs.sort_by(|a, b| a.name.cmp(&b.name));
    defs
}

fn execute_tool(
    runtime: &tokio::runtime::Runtime,
    tools: &HashMap<String, Arc<dyn Tool>>,
    name: &str,
    args: &Value,
    working_dir: &Path,
) -> ToolResult {
    let Some(tool) = tools.get(name) else {
        return ToolResult {
            content: vec![ToolContent::Text {
                text: format!("Unknown tool: {}", name),
            }],
            is_error: true,
        };
    };

    let result = runtime.block_on(tool.execute(args.clone(), working_dir));
    match result {
        Ok(text) => ToolResult {
            content: vec![ToolContent::Text { text }],
            is_error: false,
        },
        Err(e) => ToolResult {
            content: vec![ToolContent::Text {
                text: format!("Tool error: {}", e),
            }],
            is_error: true,
        },
    }
}

fn handle_request(
    request: &JsonRpcRequest,
    runtime: &tokio::runtime::Runtime,
    tools: &HashMap<String, Arc<dyn Tool>>,
    working_dir: &Path,
) -> Option<JsonRpcResponse> {
    match request.method.as_str() {
        "initialize" => Some(JsonRpcResponse::success(
            request.id.clone(),
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "host-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                }
            }),
        )),
        "notifications/initialized" | "initialized" => None,
        "tools/list" => {
            let defs = tool_definitions(tools);
            Some(JsonRpcResponse::success(request.id.clone(), json!({ "tools": defs })))
        }
        "tools/call" => {
            let name = request
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(json!({}));
            let result = execute_tool(runtime, tools, name, &args, working_dir);
            Some(JsonRpcResponse::success(request.id.clone(), json!(result)))
        }
        _ => Some(JsonRpcResponse::error(
            request.id.clone(),
            -32601,
            format!("Method not found: {}", request.method),
        )),
    }
}

fn main() {
    eprintln!("[host-mcp] Starting MCP server for host tools...");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to start tokio runtime");

    let tools = tool_set();
    let workspace = working_dir();

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let response = JsonRpcResponse::error(Value::Null, -32700, e.to_string());
                let _ = writeln!(stdout, "{}", serde_json::to_string(&response).unwrap());
                let _ = stdout.flush();
                continue;
            }
        };

        if let Some(response) = handle_request(&request, &runtime, &tools, &workspace) {
            if let Ok(resp) = serde_json::to_string(&response) {
                let _ = writeln!(stdout, "{}", resp);
                let _ = stdout.flush();
            }
        }
    }
}
