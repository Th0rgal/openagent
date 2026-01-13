# Workspace API

All endpoints require authentication via `Authorization: Bearer <token>` header.

## List Workspaces

```
GET /api/workspaces
```

**Response**: Array of `Workspace` objects (see below).

## Create a Workspace

```
POST /api/workspaces
```

**Body**:
```json
{
  "name": "my-workspace",
  "workspace_type": "host",
  "path": "/path/to/workspace",
  "skills": ["skill-name"],
  "tools": ["tool-name"],
  "plugins": ["plugin-id"],
  "template": "template-name",
  "distro": "ubuntu-noble",
  "env_vars": {"KEY": "VALUE"},
  "init_script": "#!/bin/bash\napt install -y nodejs"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Human-readable workspace name |
| `workspace_type` | string | No | `host` or `chroot` (default: `host`) |
| `path` | string | No | Custom working directory path |
| `skills` | string[] | No | Library skill names to sync |
| `tools` | string[] | No | Library tool names to sync |
| `plugins` | string[] | No | Plugin identifiers for hooks |
| `template` | string | No | Template name (forces `chroot` type) |
| `distro` | string | No | Linux distro for containers |
| `env_vars` | object | No | Environment variables |
| `init_script` | string | No | Script to run on container build |

**Distro options**: `ubuntu-noble`, `ubuntu-jammy`, `debian-bookworm`, `arch-linux`

**Response**: `Workspace` object.

## Get Workspace Details

```
GET /api/workspaces/:id
```

**Response**: `Workspace` object.

## Update Workspace

```
PUT /api/workspaces/:id
```

**Body** (all optional):
```json
{
  "name": "new-name",
  "skills": ["skill-1", "skill-2"],
  "tools": ["tool-1"],
  "plugins": ["plugin-id"],
  "template": "template-name",
  "distro": "ubuntu-noble",
  "env_vars": {"KEY": "VALUE"},
  "init_script": "#!/bin/bash\napt install -y nodejs"
}
```

**Response**: `Workspace` object.

## Delete Workspace

```
DELETE /api/workspaces/:id
```

Deletes the workspace. For container workspaces, this also destroys the container.

**Note**: The default host workspace (nil UUID) cannot be deleted.

## Build Container

```
POST /api/workspaces/:id/build
```

Builds or rebuilds a container workspace. Only valid for `chroot` type workspaces.

**Body** (optional):
```json
{
  "distro": "ubuntu-noble",
  "rebuild": true
}
```

| Field | Type | Description |
|-------|------|-------------|
| `distro` | string | Override the distro for this build |
| `rebuild` | boolean | Force rebuild even if container exists |

Build runs in background. Poll workspace status to check completion.

**Response**: `Workspace` object with `status: "building"`.

## Sync Skills/Tools

```
POST /api/workspaces/:id/sync
```

Manually syncs the workspace's skills and tools from the library to the `.opencode/` directory.

**Response**: `Workspace` object.

## Execute Command

```
POST /api/workspaces/:id/exec
```

Execute a shell command in the workspace. For container workspaces, runs inside the container via systemd-nspawn.

**Body**:
```json
{
  "command": "ls -la",
  "cwd": "subdirectory",
  "timeout_secs": 60,
  "env": {"MY_VAR": "value"},
  "stdin": "input data"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `command` | string | Yes | Shell command to execute |
| `cwd` | string | No | Working directory (relative or absolute) |
| `timeout_secs` | number | No | Timeout in seconds (default: 300, max: 600) |
| `env` | object | No | Additional environment variables |
| `stdin` | string | No | Input to pass to stdin |

**Response**:
```json
{
  "exit_code": 0,
  "stdout": "file1.txt\nfile2.txt\n",
  "stderr": "",
  "timed_out": false
}
```

**Examples**:

```bash
# List files
curl -X POST "http://localhost:3000/api/workspaces/{id}/exec" \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"command": "ls -la"}'

# Run with custom environment
curl -X POST "http://localhost:3000/api/workspaces/{id}/exec" \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"command": "echo $MY_VAR", "env": {"MY_VAR": "hello"}}'

# Install packages in container
curl -X POST "http://localhost:3000/api/workspaces/{id}/exec" \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"command": "apt install -y nodejs", "timeout_secs": 120}'
```

## Open Shell (WebSocket)

```
GET /api/workspaces/:id/shell
```

Opens an interactive PTY shell session via WebSocket.

**Authentication**: Use `Sec-WebSocket-Protocol: openagent` header with JWT token.

**Note**: For programmatic command execution, prefer the `/exec` HTTP endpoint.

---

## Workspace Templates

Templates are stored in the library and define reusable workspace configurations.

### List Templates

```
GET /api/library/workspace-template
```

**Response**:
```json
[
  {
    "name": "nodejs-dev",
    "description": "Node.js development environment",
    "path": "workspace-template/nodejs-dev.json"
  }
]
```

### Get Template

```
GET /api/library/workspace-template/:name
```

**Response**:
```json
{
  "name": "nodejs-dev",
  "description": "Node.js development environment",
  "path": "workspace-template/nodejs-dev.json",
  "distro": "ubuntu-noble",
  "skills": ["typescript-dev"],
  "env_vars": {"NODE_ENV": "development"},
  "init_script": "#!/bin/bash\napt install -y nodejs npm"
}
```

### Save Template

```
PUT /api/library/workspace-template/:name
```

**Body**:
```json
{
  "description": "Node.js development environment",
  "distro": "ubuntu-noble",
  "skills": ["typescript-dev"],
  "env_vars": {"NODE_ENV": "development"},
  "init_script": "#!/bin/bash\napt install -y nodejs npm"
}
```

### Delete Template

```
DELETE /api/library/workspace-template/:name
```

---

## Workspace Object

```json
{
  "id": "uuid",
  "name": "my-workspace",
  "workspace_type": "chroot",
  "path": "/path/to/workspace",
  "status": "ready",
  "error_message": null,
  "created_at": "2025-01-13T10:00:00Z",
  "skills": ["skill-1"],
  "tools": ["tool-1"],
  "plugins": ["plugin-id"],
  "template": "nodejs-dev",
  "distro": "ubuntu-noble",
  "env_vars": {"KEY": "VALUE"},
  "init_script": "#!/bin/bash\n..."
}
```

### Workspace Types

| Type | Description |
|------|-------------|
| `host` | Executes commands directly on the host machine |
| `chroot` | Executes commands in an isolated container (systemd-nspawn) |

### Workspace Status

| Status | Description |
|--------|-------------|
| `pending` | Container not yet built |
| `building` | Container build in progress |
| `ready` | Workspace is ready for use |
| `error` | Build failed (see `error_message`) |

---

## Workflow Examples

### Create and Build a Container Workspace

```bash
# 1. Create the workspace
curl -X POST "http://localhost:3000/api/workspaces" \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "my-dev-env",
    "workspace_type": "chroot",
    "distro": "ubuntu-noble",
    "skills": ["python-dev"],
    "init_script": "#!/bin/bash\napt install -y python3 python3-pip"
  }'

# 2. Build the container
curl -X POST "http://localhost:3000/api/workspaces/{id}/build" \
  -H "Authorization: Bearer <token>"

# 3. Poll until ready
curl "http://localhost:3000/api/workspaces/{id}" \
  -H "Authorization: Bearer <token>"
# Wait for status: "ready"

# 4. Execute commands
curl -X POST "http://localhost:3000/api/workspaces/{id}/exec" \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"command": "python3 --version"}'
```

### Create Workspace from Template

```bash
# 1. List available templates
curl "http://localhost:3000/api/library/workspace-template" \
  -H "Authorization: Bearer <token>"

# 2. Create workspace using template
curl -X POST "http://localhost:3000/api/workspaces" \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "nodejs-project",
    "template": "nodejs-dev"
  }'

# 3. Build and use as shown above
```

### Update Workspace Skills

```bash
# Add new skills to workspace
curl -X PUT "http://localhost:3000/api/workspaces/{id}" \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"skills": ["python-dev", "rust-dev"]}'

# Force sync skills to .opencode/
curl -X POST "http://localhost:3000/api/workspaces/{id}/sync" \
  -H "Authorization: Bearer <token>"
```
