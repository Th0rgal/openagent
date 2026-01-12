import { tool } from "@opencode-ai/plugin"

// The Open Agent API URL - the backend handles library configuration internally
const API_BASE = "http://127.0.0.1:3000"

async function apiRequest(endpoint: string, options: RequestInit = {}) {
  const url = `${API_BASE}/api/library${endpoint}`
  const response = await fetch(url, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      ...options.headers,
    },
  })

  if (!response.ok) {
    const text = await response.text()
    throw new Error(`API error ${response.status}: ${text}`)
  }

  const contentType = response.headers.get("content-type")
  if (contentType?.includes("application/json")) {
    return response.json()
  }
  return response.text()
}

// ─────────────────────────────────────────────────────────────────────────────
// Git Operations
// ─────────────────────────────────────────────────────────────────────────────

export const status = tool({
  description: "Get the git status of the library: current branch, commits ahead/behind, and modified files",
  args: {},
  async execute() {
    const status = await apiRequest("/status")
    let result = `# Library Git Status\n\n`
    result += `**Branch:** ${status.branch || "unknown"}\n`
    result += `**Remote:** ${status.remote || "not configured"}\n`

    if (status.commits_ahead !== undefined) {
      result += `**Commits ahead:** ${status.commits_ahead}\n`
    }
    if (status.commits_behind !== undefined) {
      result += `**Commits behind:** ${status.commits_behind}\n`
    }

    if (status.modified_files && status.modified_files.length > 0) {
      result += `\n## Modified Files\n`
      result += status.modified_files.map((f: string) => `- ${f}`).join("\n")
    } else {
      result += `\nNo uncommitted changes.`
    }

    return result
  },
})

export const sync = tool({
  description: "Pull latest changes from the library remote (git pull)",
  args: {},
  async execute() {
    await apiRequest("/sync", { method: "POST" })
    return "Library synced successfully. Latest changes pulled from remote."
  },
})

export const commit = tool({
  description: "Commit all changes in the library with a message",
  args: {
    message: tool.schema.string().describe("Commit message describing what changed"),
  },
  async execute(args) {
    await apiRequest("/commit", {
      method: "POST",
      body: JSON.stringify({ message: args.message }),
    })
    return `Changes committed with message: "${args.message}"\n\nUse library-git_push to push to remote.`
  },
})

export const push = tool({
  description: "Push committed changes to the library remote (git push)",
  args: {},
  async execute() {
    await apiRequest("/push", { method: "POST" })
    return "Changes pushed to remote successfully."
  },
})

// ─────────────────────────────────────────────────────────────────────────────
// MCP Servers
// ─────────────────────────────────────────────────────────────────────────────

export const get_mcps = tool({
  description: "Get all MCP server configurations from the library",
  args: {},
  async execute() {
    const mcps = await apiRequest("/mcps")
    if (!mcps || Object.keys(mcps).length === 0) {
      return "No MCP servers configured in the library."
    }

    let result = "# MCP Servers\n\n"
    for (const [name, config] of Object.entries(mcps)) {
      const c = config as { type: string; command?: string[]; url?: string; enabled?: boolean }
      result += `## ${name}\n`
      result += `- Type: ${c.type}\n`
      if (c.type === "local" && c.command) {
        result += `- Command: \`${c.command.join(" ")}\`\n`
      }
      if (c.type === "remote" && c.url) {
        result += `- URL: ${c.url}\n`
      }
      result += `- Enabled: ${c.enabled !== false}\n\n`
    }
    return result
  },
})

export const save_mcps = tool({
  description: "Save MCP server configurations to the library. Provide the full JSON object with all servers.",
  args: {
    servers: tool.schema.string().describe("JSON object with MCP server configurations. Each server has type (local/remote), command/url, env/headers, and enabled fields."),
  },
  async execute(args) {
    let parsed: Record<string, unknown>
    try {
      parsed = JSON.parse(args.servers)
    } catch (e) {
      throw new Error(`Invalid JSON: ${e}`)
    }

    await apiRequest("/mcps", {
      method: "PUT",
      body: JSON.stringify(parsed),
    })
    return "MCP server configurations saved successfully. Remember to commit and push your changes."
  },
})
