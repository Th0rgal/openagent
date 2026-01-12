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
// Library Agents
// ─────────────────────────────────────────────────────────────────────────────

export const list_agents = tool({
  description: "List all agents in the library with their names, descriptions, modes, and models",
  args: {},
  async execute() {
    const agents = await apiRequest("/agent")
    if (!agents || agents.length === 0) {
      return "No agents found in the library."
    }
    return agents.map((a: { name: string; description?: string; model?: string }) => {
      let line = `- ${a.name}`
      if (a.description) line += `: ${a.description}`
      if (a.model) line += ` (model: ${a.model})`
      return line
    }).join("\n")
  },
})

export const get_agent = tool({
  description: "Get the full content of a library agent by name, including frontmatter and system prompt",
  args: {
    name: tool.schema.string().describe("The agent name"),
  },
  async execute(args) {
    const agent = await apiRequest(`/agent/${encodeURIComponent(args.name)}`)
    let result = `# Agent: ${agent.name}\n\n`
    result += `**Path:** ${agent.path}\n`
    if (agent.description) result += `**Description:** ${agent.description}\n`
    if (agent.model) result += `**Model:** ${agent.model}\n`

    if (agent.tools && Object.keys(agent.tools).length > 0) {
      result += `**Tools:** ${JSON.stringify(agent.tools)}\n`
    }
    if (agent.permissions && Object.keys(agent.permissions).length > 0) {
      result += `**Permissions:** ${JSON.stringify(agent.permissions)}\n`
    }
    if (agent.rules && agent.rules.length > 0) {
      result += `**Rules:** ${agent.rules.join(", ")}\n`
    }

    result += `\n## Full Content (markdown file)\n\n${agent.content}`
    return result
  },
})

export const save_agent = tool({
  description: "Create or update a library agent. Provide the full markdown content including YAML frontmatter.",
  args: {
    name: tool.schema.string().describe("The agent name"),
    content: tool.schema.string().describe("Full markdown content with YAML frontmatter (description, mode, model, tools, permissions, etc.)"),
  },
  async execute(args) {
    await apiRequest(`/agent/${encodeURIComponent(args.name)}`, {
      method: "PUT",
      body: JSON.stringify({ content: args.content }),
    })
    return `Agent '${args.name}' saved successfully. Remember to commit and push your changes.`
  },
})

export const delete_agent = tool({
  description: "Delete a library agent",
  args: {
    name: tool.schema.string().describe("The agent name to delete"),
  },
  async execute(args) {
    await apiRequest(`/agent/${encodeURIComponent(args.name)}`, {
      method: "DELETE",
    })
    return `Agent '${args.name}' deleted. Remember to commit and push your changes.`
  },
})
