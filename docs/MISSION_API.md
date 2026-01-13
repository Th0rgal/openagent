# Mission API

All endpoints require authentication via `Authorization: Bearer <token>` header.

## Create a Mission

```
POST /api/control/missions
```

**Body** (all optional):
```json
{
  "title": "My Mission",
  "workspace_id": "uuid",
  "agent": "code-reviewer",
  "model_override": "anthropic/claude-sonnet-4-20250514"
}
```

**Response**: `Mission` object (see below).

## Load/Switch to a Mission

```
POST /api/control/missions/:id/load
```

Loads the mission into the active control session. Required before sending messages.

## Send a Message

```
POST /api/control/message
```

**Body**:
```json
{
  "content": "Your message here",
  "agent": "optional-agent-override"
}
```

**Response**:
```json
{
  "id": "uuid",
  "queued": false
}
```

`queued: true` means another message is being processed.

## Cancel Current Execution

```
POST /api/control/cancel
```

Cancels the currently running agent task.

## Cancel a Specific Mission

```
POST /api/control/missions/:id/cancel
```

## Set Mission Status

```
POST /api/control/missions/:id/status
```

**Body**:
```json
{
  "status": "completed"
}
```

Statuses: `pending`, `active`, `completed`, `failed`, `interrupted`.

## Get Mission Events (History)

```
GET /api/control/missions/:id/events?types=user_message,assistant_message&limit=100&offset=0
```

**Query params** (all optional):
- `types`: comma-separated event types to filter
- `limit`: max events to return
- `offset`: pagination offset

**Response**: Array of `StoredEvent`:
```json
[
  {
    "id": 1,
    "mission_id": "uuid",
    "sequence": 1,
    "event_type": "user_message",
    "timestamp": "2025-01-13T10:00:00Z",
    "content": "...",
    "metadata": {}
  }
]
```

## Stream Events (SSE)

```
GET /api/control/stream
```

Server-Sent Events stream for real-time updates. Events have `event:` and `data:` fields.

**Event types**:
- `status` — control state changed (`idle`, `running`, `tool_waiting`)
- `user_message` — user message received
- `assistant_message` — agent response complete
- `thinking` — agent reasoning (streaming)
- `tool_call` — tool invocation
- `tool_result` — tool result
- `error` — error occurred
- `mission_status_changed` — mission status updated

**Example SSE event**:
```
event: assistant_message
data: {"id":"uuid","content":"Done!","success":true,"cost_cents":5,"model":"claude-sonnet-4-20250514"}
```

## Other Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/control/missions` | GET | List missions |
| `/api/control/missions/:id` | GET | Get mission details |
| `/api/control/missions/:id` | DELETE | Delete mission |
| `/api/control/missions/:id/tree` | GET | Get agent tree for mission |
| `/api/control/missions/current` | GET | Get current active mission |
| `/api/control/missions/:id/resume` | POST | Resume interrupted mission |
| `/api/control/tree` | GET | Get live agent tree |
| `/api/control/progress` | GET | Get execution progress |

## Mission Object

```json
{
  "id": "uuid",
  "status": "active",
  "title": "My Mission",
  "workspace_id": "uuid",
  "workspace_name": "my-workspace",
  "agent": "code-reviewer",
  "model_override": null,
  "history": [],
  "created_at": "2025-01-13T10:00:00Z",
  "updated_at": "2025-01-13T10:05:00Z"
}
```

