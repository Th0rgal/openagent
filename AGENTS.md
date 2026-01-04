# Open Agent

Minimal autonomous coding agent in Rust with **full machine access** (not sandboxed).

## Quick Reference

| Component | Location | Purpose |
|-----------|----------|---------|
| Backend (Rust) | `src/` | HTTP API + OpenCode integration |
| Dashboard (Next.js) | `dashboard/` | Web UI (uses **Bun**, not npm) |
| iOS Dashboard | `ios_dashboard/` | Native iOS app (Swift/SwiftUI) |
| MCP configs | `.openagent/mcp/config.json` | Model Context Protocol servers |
| Providers | `.openagent/providers.json` | Provider configuration |

## Commands

```bash
# Backend
cargo build --release           # Build
cargo run --release             # Run server (port 3000)
RUST_LOG=debug cargo run        # Debug mode
cargo test                      # Run tests
cargo fmt                       # Format code
cargo clippy                    # Lint

# Dashboard (uses Bun, NOT npm/yarn/pnpm)
cd dashboard
bun install                     # Install deps (NEVER use npm install)
bun dev                         # Dev server (port 3001)
bun run build                   # Production build

# IMPORTANT: Always use bun for dashboard, never npm
# - bun install (not npm install)
# - bun add <pkg> (not npm install <pkg>)
# - bun run <script> (not npm run <script>)

# Deployment
ssh root@95.216.112.253 'cd /root/open_agent && git pull && cargo build --release && cp target/release/open_agent /usr/local/bin/ && cp target/release/desktop-mcp /usr/local/bin/ && cp target/release/host-mcp /usr/local/bin/ && systemctl restart open_agent'
```

## Architecture

Open Agent uses OpenCode as its execution backend, enabling Claude Max subscription usage.

```
Dashboard → Open Agent API → OpenCode Server → Anthropic API (Claude Max)
```

### Module Map

```
src/
├── agents/              # Agent system
│   ├── mod.rs           # Agent trait, OrchestratorAgent trait, LeafCapability enum
│   ├── opencode.rs      # OpenCodeAgent (delegates to OpenCode server)
│   ├── context.rs       # AgentContext with LLM, tools, memory
│   ├── improvements.rs  # Blocker detection, tool failure tracking, smart truncation
│   ├── tree.rs          # AgentRef, AgentTree for hierarchy
│   ├── tuning.rs        # TuningParams for agent behavior
│   └── types.rs         # AgentError, AgentId, AgentResult, AgentType, Complexity
├── api/                 # HTTP routes (axum)
│   ├── mod.rs           # Endpoint registry
│   ├── routes.rs        # Core handlers, AppState, serve()
│   ├── auth.rs          # JWT authentication
│   ├── control.rs       # Global interactive control session (SSE streaming)
│   ├── console.rs       # WebSocket console
│   ├── desktop_stream.rs # WebSocket desktop stream (VNC-style)
│   ├── fs.rs            # Remote file explorer (list, upload, download, mkdir, rm)
│   ├── mcp.rs           # MCP server management endpoints
│   ├── mission_runner.rs # Background mission execution
│   ├── providers.rs     # Provider/model listing
│   ├── ssh_util.rs      # SSH utilities for remote connections
│   └── types.rs         # Request/response types
├── budget/              # Cost tracking, pricing, model selection
│   ├── mod.rs           # Budget type, SharedBenchmarkRegistry, SharedModelResolver
│   ├── benchmarks.rs    # Model capability scores from benchmarks
│   ├── pricing.rs       # Model pricing (cents per token)
│   ├── resolver.rs      # Model family auto-upgrade system
│   ├── allocation.rs    # Budget allocation strategies
│   ├── compatibility.rs # Model compatibility checks
│   ├── learned.rs       # Self-improving model selection from task outcomes
│   ├── budget.rs        # Budget tracking implementation
│   └── retry.rs         # Retry strategies with backoff
├── llm/                 # LLM client abstraction
│   ├── mod.rs           # OpenRouterClient, ToolDefinition, FunctionDefinition
│   ├── openrouter.rs    # OpenRouter API client
│   └── error.rs         # LLM-specific errors
├── mcp/                 # Model Context Protocol server registry
│   ├── mod.rs           # McpRegistry
│   ├── config.rs        # MCP configuration loading
│   ├── registry.rs      # Server discovery and management
│   └── types.rs         # MCP message types
├── memory/              # Supabase + pgvector persistence
│   ├── mod.rs           # MemorySystem, init_memory()
│   ├── supabase.rs      # Database client, learned stats queries
│   ├── context.rs       # ContextBuilder, SessionContext
│   ├── retriever.rs     # Semantic search, run/event retrieval
│   ├── writer.rs        # Event recording, run management
│   ├── embed.rs         # Embedding generation
│   └── types.rs         # Memory event types
├── opencode/            # OpenCode client
│   └── mod.rs           # OpenCode server communication
├── task/                # Task types + verification
│   ├── mod.rs           # Task exports
│   ├── task.rs          # Task struct, TaskAnalysis
│   ├── subtask.rs       # Subtask breakdown
│   ├── deliverables.rs  # Expected deliverables
│   └── verification.rs  # VerificationCriteria enum
├── tools/               # Tool system (agent's "hands and eyes")
│   ├── mod.rs           # Tool trait, ToolRegistry, PathResolution
│   ├── file_ops.rs      # read_file, write_file, delete_file
│   ├── directory.rs     # list_directory, search_files
│   ├── index.rs         # index_files, search_file_index (performance optimization)
│   ├── terminal.rs      # run_command (shell execution)
│   ├── search.rs        # grep_search
│   ├── web.rs           # web_search, fetch_url
│   ├── git.rs           # git_status, git_diff, git_commit, git_log
│   ├── github.rs        # github_clone, github_list_repos, github_get_file, github_search_code
│   ├── browser.rs       # Browser automation (conditional on BROWSER_ENABLED)
│   ├── desktop.rs       # Desktop automation via i3/Xvfb (conditional on DESKTOP_ENABLED)
│   ├── composite.rs     # High-level workflows: analyze_codebase, deep_search, prepare_project, debug_error
│   ├── ui.rs            # Frontend tool UI schemas (ui_optionList, ui_dataTable)
│   ├── storage.rs       # upload_image (Supabase storage)
│   ├── memory.rs        # search_memory, store_fact (shared memory tools)
│   └── mission.rs       # complete_mission (agent task completion)
├── bin/
│   └── desktop_mcp.rs   # Standalone MCP server for desktop tools
├── config.rs            # Config struct, environment variable loading
├── lib.rs               # Library exports
└── main.rs              # Server entry point
```

## OpenCode Configuration

OpenCode is required for task execution. It connects to an OpenCode server that handles LLM interactions.

```bash
# Optional configuration (defaults shown)
OPENCODE_BASE_URL=http://127.0.0.1:4096
OPENCODE_AGENT=build
OPENCODE_PERMISSIVE=true
```

**Desktop Tools with OpenCode:**
To enable desktop tools (i3, Xvfb, screenshots):

1. Build the MCP servers: `cargo build --release --bin desktop-mcp --bin host-mcp`
2. Workspace `opencode.json` files are generated automatically under `workspaces/`
   from `.openagent/mcp/config.json` (override by editing MCP configs via the UI).
3. OpenCode will automatically load the tools from the MCP server

The `opencode.json` configures MCP servers for desktop and browser automation:
```json
{
  "mcp": {
    "host": {
      "type": "local",
      "command": ["./target/release/host-mcp"],
      "enabled": true
    },
    "desktop": {
      "type": "local",
      "command": ["./target/release/desktop-mcp"],
      "enabled": true
    },
    "playwright": {
      "type": "local",
      "command": ["npx", "@playwright/mcp@latest"],
      "enabled": true
    }
  }
}
```

**Available MCP Tools:**
- **Desktop tools** (i3/Xvfb): `desktop_start_session`, `desktop_screenshot`, `desktop_click`, `desktop_type`, `desktop_i3_command`, etc.
- **Playwright tools**: `browser_navigate`, `browser_snapshot`, `browser_click`, `browser_type`, `browser_screenshot`, etc.

## Model Preferences

Use Claude models via your Claude Max subscription:
- `claude-opus-4-5-20251101` - Most capable, recommended
- `claude-sonnet-4-20250514` - Good balance of speed/capability (default)
- `claude-3-5-haiku-20241022` - Fastest, most economical

## API Endpoints

### Core Task Endpoints
| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/api/task` | Submit task |
| `GET` | `/api/task/{id}` | Get status |
| `GET` | `/api/task/{id}/stream` | SSE progress |
| `POST` | `/api/task/{id}/stop` | Cancel task |
| `GET` | `/api/tasks` | List all tasks |

### Control Session (Global Interactive)
| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/api/control/message` | Send message to agent |
| `POST` | `/api/control/tool_result` | Submit tool result |
| `GET` | `/api/control/stream` | SSE event stream |
| `POST` | `/api/control/cancel` | Cancel current operation |
| `GET` | `/api/control/tree` | Get state tree snapshot |
| `GET` | `/api/control/progress` | Get progress snapshot |

### Mission Management
| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/control/missions` | List missions |
| `POST` | `/api/control/missions` | Create mission |
| `GET` | `/api/control/missions/current` | Get current mission |
| `GET` | `/api/control/missions/{id}` | Get mission details |
| `GET` | `/api/control/missions/{id}/tree` | Get mission tree |
| `POST` | `/api/control/missions/{id}/load` | Load mission |
| `POST` | `/api/control/missions/{id}/status` | Set mission status |
| `POST` | `/api/control/missions/{id}/cancel` | Cancel mission |
| `POST` | `/api/control/missions/{id}/resume` | Resume mission |
| `POST` | `/api/control/missions/{id}/parallel` | Start parallel execution |
| `DELETE` | `/api/control/missions/{id}` | Delete mission |
| `POST` | `/api/control/missions/cleanup` | Cleanup empty missions |
| `GET` | `/api/control/running` | List running missions |
| `GET` | `/api/control/parallel/config` | Get parallel config |

### Memory Endpoints
| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/runs` | List archived runs |
| `GET` | `/api/runs/{id}` | Get run details |
| `GET` | `/api/runs/{id}/events` | Get run events |
| `GET` | `/api/runs/{id}/tasks` | Get run tasks |
| `GET` | `/api/memory/search` | Search memory |

### File System (Remote Explorer)
| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/fs/list` | List directory |
| `GET` | `/api/fs/download` | Download file |
| `POST` | `/api/fs/upload` | Upload file |
| `POST` | `/api/fs/upload-chunk` | Chunked upload |
| `POST` | `/api/fs/upload-finalize` | Finalize upload |
| `POST` | `/api/fs/download-url` | Download from URL |
| `POST` | `/api/fs/mkdir` | Create directory |
| `POST` | `/api/fs/rm` | Remove file/dir |

### MCP Management
| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/mcp` | List MCP servers |
| `POST` | `/api/mcp` | Add MCP server |
| `POST` | `/api/mcp/refresh` | Refresh all MCPs |
| `GET` | `/api/mcp/{id}` | Get MCP details |
| `DELETE` | `/api/mcp/{id}` | Remove MCP |
| `POST` | `/api/mcp/{id}/enable` | Enable MCP |
| `POST` | `/api/mcp/{id}/disable` | Disable MCP |
| `POST` | `/api/mcp/{id}/refresh` | Refresh MCP |
| `GET` | `/api/tools` | List all tools |
| `POST` | `/api/tools/{name}/toggle` | Toggle tool |

### Model Management
| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/providers` | List providers |
| `GET` | `/api/models` | List models |
| `POST` | `/api/models/refresh` | Refresh model data |
| `GET` | `/api/models/families` | List model families |
| `GET` | `/api/models/performance` | Get learned performance stats |

### System
| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/health` | Health check |
| `GET` | `/api/stats` | System statistics |
| `POST` | `/api/auth/login` | Authenticate |
| `GET` | `/api/console/ws` | WebSocket console |
| `GET` | `/api/desktop/stream` | WebSocket desktop stream |

## Environment Variables

### Production Auth
| Variable | Description |
|----------|-------------|
| `DEV_MODE` | `true` bypasses auth |
| `DASHBOARD_PASSWORD` | Password for dashboard login |
| `JWT_SECRET` | HMAC secret for JWT signing |

### Optional
| Variable | Default | Description |
|----------|---------|-------------|
| `DEFAULT_MODEL` | `claude-sonnet-4-20250514` | Default LLM model |
| `OPENCODE_BASE_URL` | `http://127.0.0.1:4096` | OpenCode server URL |
| `OPENCODE_AGENT` | - | OpenCode agent name |
| `OPENCODE_PERMISSIVE` | `true` | Auto-allow OpenCode permissions |
| `WORKING_DIR` | `/root` (prod), `.` (dev) | Working directory |
| `HOST` | `127.0.0.1` | Bind address |
| `PORT` | `3000` | Server port |
| `MAX_ITERATIONS` | `50` | Max agent loop iterations |
| `SUPABASE_URL` | - | Supabase project URL |
| `SUPABASE_SERVICE_ROLE_KEY` | - | Service role key |
| `OPENROUTER_API_KEY` | - | Only needed for memory embeddings |
| `BROWSER_ENABLED` | `false` | Enable browser automation tools |
| `DESKTOP_ENABLED` | `false` | Enable desktop automation tools |

## Secrets

Use `secrets.json` (gitignored) for local development. Template: `secrets.json.example`

```bash
# Read secrets
jq -r '.openrouter.api_key' secrets.json
```

**Rules:**
- Never paste secret values into code, comments, or docs
- Read secrets from environment variables at runtime

## Code Conventions

### Rust - Provability-First Design

Code should be written as if we want to **formally prove it correct later**. This means:

1. **Never panic** - always return `Result<T, E>`
2. **Exhaustive matches** - no `_` catch-all patterns in enums (forces handling new variants)
3. **Document invariants** as `/// Precondition:` and `/// Postcondition:` comments
4. **Pure functions** - separate pure logic from IO where possible
5. **Algebraic types** - prefer enums with exhaustive matching over stringly-typed data
6. Costs are in **cents (u64)** - never use floats for money

```rust
// Use thiserror for error types
#[derive(Debug, Error)]
pub enum MyError {
    #[error("description: {0}")]
    Variant(String),
}

// Propagate with ?
pub fn do_thing() -> Result<T, MyError> {
    let x = fallible_op()?;
    Ok(x)
}
```

### Adding a New Tool

1. Add to `src/tools/` (new file or extend existing)
2. Implement `Tool` trait: `name()`, `description()`, `parameters_schema()`, `execute()`
3. Register in `src/tools/mod.rs` → `ToolRegistry::with_options()`
4. Tool parameters use serde_json schema format
5. Document pre/postconditions for provability

### Dashboard (Next.js + Bun)
- Package manager: **Bun** (not npm/yarn/pnpm)
- Icons: **Lucide React** (`lucide-react`)
- API base: `process.env.NEXT_PUBLIC_API_URL ?? 'http://127.0.0.1:3000'`
- Auth: JWT stored in `sessionStorage`

### Design System - "Quiet Luxury + Liquid Glass"
- **Dark-first** aesthetic (dark mode is default)
- No pure black - use deep charcoal (#121214)
- Elevation via color, not shadows
- Use `white/[opacity]` for text (e.g., `text-white/80`)
- Accent color: indigo-500 (#6366F1)
- Borders: very subtle (0.06-0.08 opacity)
- No bounce animations, use `ease-out`

## Production

| Property | Value |
|----------|-------|
| Host | `95.216.112.253` |
| SSH | `ssh -i ~/.ssh/cursor root@95.216.112.253` |
| Backend URL | `https://agent-backend.thomas.md` |
| Dashboard URL | `https://agent.thomas.md` |
| Binary | `/usr/local/bin/open_agent` |
| Desktop MCP | `/usr/local/bin/desktop-mcp` |
| Host MCP | `/usr/local/bin/host-mcp` |
| Env file | `/etc/open_agent/open_agent.env` |
| Service | `systemctl status open_agent` |

**SSH Key:** Use `~/.ssh/cursor` key for production server access.

## Adding New Components

### New API Endpoint
1. Add handler in `src/api/`
2. Register route in `src/api/routes.rs`
3. Update this doc

### New Tool
1. Implement `Tool` trait in `src/tools/`
2. Register in `ToolRegistry::with_options()` in `src/tools/mod.rs`
3. Update this doc

### After Significant Changes
- Update `.cursor/rules/` if architecture changes
- Update `AGENTS.md` and `.claude/CLAUDE.md` for new env vars or commands
