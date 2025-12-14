# Open Agent

A minimal autonomous coding agent with full machine access, implemented in Rust.

## Features

- **HTTP API** for task submission and monitoring
- **Tool-based agent loop** following the "tools in a loop" pattern
- **Full toolset**: file operations, terminal, grep search, web access, git
- **OpenRouter integration** for LLM access (supports any model)
- **SSE streaming** for real-time task progress
- **AI-maintainable** Rust codebase with strong typing

## Quick Start

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- An OpenRouter API key ([get one here](https://openrouter.ai/))

### Installation

```bash
git clone <repo-url>
cd open_agent
cargo build --release
```

### Running

```bash
# Set your API key
export OPENROUTER_API_KEY="sk-or-v1-..."

# Optional: configure model (default: openai/gpt-4.1-mini)
export DEFAULT_MODEL="openai/gpt-4.1-mini"

# Start the server
cargo run --release
```

The server starts on `http://127.0.0.1:3000` by default.

## API Reference

### Submit a Task

```bash
curl -X POST http://localhost:3000/api/task \
  -H "Content-Type: application/json" \
  -d '{"task": "Create a Python script that prints Hello World"}'
```

Response:
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "pending"
}
```

### Get Task Status

```bash
curl http://localhost:3000/api/task/{id}
```

Response:
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "completed",
  "task": "Create a Python script that prints Hello World",
  "model": "openai/gpt-4.1-mini",
  "iterations": 3,
  "result": "I've created hello.py with a simple Hello World script...",
  "log": [...]
}
```

### Stream Task Progress (SSE)

```bash
curl http://localhost:3000/api/task/{id}/stream
```

Events:
- `log` - Execution log entries (tool calls, results)
- `done` - Task completion with final status

### Health Check

```bash
curl http://localhost:3000/api/health
```

## Available Tools

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents with optional line range |
| `write_file` | Write/create files |
| `delete_file` | Delete files |
| `list_directory` | List directory contents |
| `search_files` | Search files by name pattern |
| `run_command` | Execute shell commands |
| `grep_search` | Search file contents with regex |
| `web_search` | Search the web (DuckDuckGo) |
| `fetch_url` | Fetch URL contents |
| `git_status` | Get git status |
| `git_diff` | Show git diff |
| `git_commit` | Create git commits |
| `git_log` | Show git log |

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENROUTER_API_KEY` | (required) | Your OpenRouter API key |
| `DEFAULT_MODEL` | `openai/gpt-4.1-mini` | Default LLM model |
| `WORKSPACE_PATH` | `.` | Working directory for file operations |
| `HOST` | `127.0.0.1` | Server bind address |
| `PORT` | `3000` | Server port |
| `MAX_ITERATIONS` | `50` | Max agent loop iterations |

## Architecture

```
┌─────────────────┐     ┌─────────────────┐
│   HTTP Client   │────▶│   HTTP API      │
└─────────────────┘     │   (axum)        │
                        └────────┬────────┘
                                 │
                        ┌────────▼────────┐
                        │   Agent Loop    │◀──────┐
                        │                 │       │
                        └────────┬────────┘       │
                                 │                │
                   ┌─────────────┼─────────────┐  │
                   ▼             ▼             ▼  │
            ┌──────────┐  ┌──────────┐  ┌──────────┐
            │   LLM    │  │  Tools   │  │  Tools   │
            │(OpenRouter)│ │(file,git)│ │(term,web)│
            └──────────┘  └──────────┘  └──────────┘
                   │
                   └──────────────────────────────┘
                            (results fed back)
```

## Development

```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Run tests
cargo test

# Format code
cargo fmt

# Check for issues
cargo clippy
```

## License

MIT

