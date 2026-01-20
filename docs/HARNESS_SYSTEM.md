# Harness System

Open Agent supports multiple execution backends ("harnesses") for running agent
missions. The current architecture is **per-workspace execution**: OpenCode and
Claude Code run inside the selected workspace (host, container, or remote).

This document explains the harness architecture, configuration, and how to add
new backends.

## Overview

A **harness** (also called a backend) is an execution engine that runs agent
missions. Open Agent currently supports:

| Harness | Description | Configuration Model |
|---------|-------------|---------------------|
| **OpenCode** | OpenCode CLI executed inside each workspace | Per-workspace (`opencode.json`, `.opencode/`) |
| **Claude Code** | Claude CLI executed inside each workspace | Per-workspace (`CLAUDE.md`, `.claude/settings.local.json`) |

## Architecture (per-workspace)

```
┌─────────────────────────────────────────────────────────────────┐
│                         Mission Runner                          │
│                   (src/api/mission_runner.rs)                   │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Workspace Execution Layer                   │
│                 (src/workspace_exec.rs)                         │
│  - host: spawn process directly                                 │
│  - chroot: systemd-nspawn                                        │
│  - ssh: remote exec                                             │
└──────────────┬───────────────────────────────┬──────────────────┘
               │                               │
               ▼                               ▼
┌──────────────────────────┐    ┌──────────────────────────────────┐
│     OpenCode CLI          │    │      Claude Code CLI            │
│  (oh-my-opencode run)     │    │      (claude --stream-json)     │
│  - embedded server        │    │      - built-in agents          │
│  - per-workspace config   │    │      - per-workspace config     │
└──────────────────────────┘    └──────────────────────────────────┘
```

### Key properties

- **Native bash works** because the harness runs inside the workspace.
- **No host proxy bash tools** are required for standard missions.
- **Per-workspace isolation** prevents cross-workspace file effects.

## Backend registry (metadata)

Open Agent still maintains a backend registry for:

- listing agents
- backend configuration UI
- provider/auth settings

Execution itself is handled by the mission runner via the workspace execution
layer, not by a centralized OpenCode server.

## OpenCode harness

OpenCode is executed **per workspace** using the CLI:

- Uses `oh-my-opencode run` to start an embedded OpenCode server.
- Reads config from `opencode.json` and `.opencode/opencode.json`.
- `oh-my-opencode.json` is synced into each workspace.
- Built-in `bash` is enabled; legacy `workspace_*` tools are disabled by default.

### Agents

OpenCode agents are defined in `oh-my-opencode.json`:

```json
{
  "agents": {
    "Sisyphus": {
      "model": "anthropic/claude-opus-4-5"
    },
    "document-writer": {
      "model": "google/gemini-3-flash-preview"
    }
  }
}
```

## Claude Code harness

Claude Code is executed **per workspace** using the CLI:

- `.claude/settings.local.json` defines MCP servers and tool permissions.
- `CLAUDE.md` provides per-workspace context (generated from Library skills).
- Built-in `Bash` is enabled in the permissions allowlist.

### Harness bootstrap (auto-install)

For **chroot workspaces**, Open Agent can automatically install the required
CLIs during container build (best-effort):

- `OPEN_AGENT_BOOTSTRAP_CLAUDECODE=true` (default)
- `OPEN_AGENT_BOOTSTRAP_OPENCODE=true` (default)

At runtime, Claude Code can also self-install on first use if missing:

- `OPEN_AGENT_AUTO_INSTALL_CLAUDECODE=true` (default)

OpenCode can also self-install on first use if missing:

- `OPEN_AGENT_AUTO_INSTALL_OPENCODE=true` (default)

OpenCode installation uses the official installer (`https://opencode.ai/install`)
and copies the binary to `/usr/local/bin/opencode`. This requires `curl` inside
the workspace. If `curl` is unavailable, the mission fails with a clear error
message instructing you to add it to the workspace template.

Claude Code/oh-my-opencode installation uses `npm` in the workspace. If `npm`
is unavailable, the mission fails with a clear error message instructing you to
add Node/npm to the workspace template.

### CLI protocol (NDJSON)

Claude Code communicates via NDJSON streaming:

```bash
echo "prompt" | claude \
  --print \
  --output-format stream-json \
  --verbose \
  --include-partial-messages \
  --model "claude-sonnet-4-20250514" \
  --session-id "uuid"
```

Event types:
- `system` (init)
- `stream_event` (deltas)
- `assistant` (final content + tool calls)
- `user` (tool results)
- `result` (completion)

## Tool policy

Default per-workspace tool settings:

- **OpenCode**: built-in `bash` enabled; `workspace_*` disabled by default.
- **Claude Code**: built-in `Bash` enabled via permissions.

MCP tools (desktop/playwright/workspace) can be enabled when needed.

## Adding a new backend

To add a new backend (e.g., Codex):

1. Create a backend module under `src/backend/<backend>/`.
2. Register it in `src/api/routes.rs` for metadata/UI.
3. Implement a per-workspace execution path in the mission runner.
4. Update the dashboard to expose backend-specific settings.

## Mission runner integration

The mission runner selects the harness based on `backend_id` and spawns the CLI
inside the workspace execution context:

```rust
let result = match backend_id.as_str() {
    "claudecode" => run_claudecode_turn(...).await,
    "opencode" => run_opencode_turn(...).await,
    _ => Err(anyhow!("Unknown backend")),
};
```
