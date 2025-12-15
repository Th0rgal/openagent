# OpenAgent Dashboard

Developer-focused UI for monitoring and controlling the OpenAgent backend.

## Prerequisites
- **Bun** (required): `bun@1.x`

## Getting started (Bun only)

```bash
cd dashboard
bun install

# If the backend is on :3000, run the dashboard on :3001 to avoid port conflicts.
PORT=3001 bun dev
```

Configure the backend URL via:
- `NEXT_PUBLIC_API_URL` (defaults to `http://127.0.0.1:3000`)

## Auth

If the backend reports `auth_required=true` from `GET /api/health`, the dashboard will prompt for a password and store a JWT in `sessionStorage`.
