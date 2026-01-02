# Open Agent

Minimal autonomous coding agent in Rust with **full machine access** (not sandboxed).

## Quick Reference

| Component | Location | Purpose |
|-----------|----------|---------|
| Backend (Rust) | `src/` | HTTP API + OpenCode integration |
| Dashboard (Next.js) | `dashboard/` | Web UI (uses **Bun**, not npm) |
| iOS Dashboard | `ios_dashboard/` | Native iOS app (Swift/SwiftUI) |
| MCP configs | `.open_agent/mcp/config.json` | Model Context Protocol servers |
| Providers | `.open_agent/providers.json` | Provider configuration |

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
ssh root@95.216.112.253 'cd /root/open_agent && git pull && cargo build --release && cp target/release/open_agent /usr/local/bin/ && cp target/release/desktop-mcp /usr/local/bin/ && systemctl restart open_agent'
```

## Architecture

Open Agent uses OpenCode as its execution backend, enabling Claude Max subscription usage.

```
Dashboard → Open Agent API → OpenCode Server → Anthropic API (Claude Max)
```

### Module Map

```
src/
├── agents/           # Agent system
│   └── opencode.rs   # OpenCodeAgent (delegates to OpenCode server)
├── budget/           # Cost tracking, pricing
│   ├── benchmarks.rs # Model capability scores
│   ├── pricing.rs    # Model pricing
│   └── resolver.rs   # Model family auto-upgrade system
├── memory/           # Supabase + pgvector persistence
│   ├── supabase.rs   # Database client
│   ├── context.rs    # ContextBuilder, SessionContext
│   ├── retriever.rs  # Semantic search
│   └── writer.rs     # Event recording
├── mcp/              # MCP server registry + config
├── opencode/         # OpenCode client
├── tools/            # Desktop MCP tools
├── task/             # Task types + verification
├── config.rs         # Config + env vars
└── api/              # HTTP routes (axum)
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

1. Build the MCP server: `cargo build --release --bin desktop-mcp`
2. Ensure `opencode.json` is in the project root with the desktop MCP config
3. OpenCode will automatically load the tools from the MCP server

The `opencode.json` configures MCP servers for desktop and browser automation:
```json
{
  "mcp": {
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

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/api/task` | Submit task |
| `GET` | `/api/task/{id}` | Get status |
| `GET` | `/api/task/{id}/stream` | SSE progress |
| `GET` | `/api/health` | Health check |
| `POST` | `/api/control/message` | Send message to agent |
| `GET` | `/api/control/stream` | SSE event stream |
| `GET` | `/api/models` | List available models |
| `GET` | `/api/providers` | List available providers |

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
2. Implement `Tool` trait: `name()`, `description()`, `parameters()`, `call()`
3. Register in `src/tools/mod.rs` → `create_tools()`
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
| Env file | `/etc/open_agent/open_agent.env` |
| Service | `systemctl status open_agent` |

**SSH Key:** Use `~/.ssh/cursor` key for production server access.

## Adding New Components

### New API Endpoint
1. Add handler in `src/api/`
2. Register route in `src/api/mod.rs`
3. Update this doc

### After Significant Changes
- Update `.cursor/rules/` if architecture changes
- Update `CLAUDE.md` for new env vars or commands
