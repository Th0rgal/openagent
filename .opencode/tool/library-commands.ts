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
// Commands
// ─────────────────────────────────────────────────────────────────────────────

export const list_commands = tool({
  description: "List all commands in the library (slash commands like /commit, /test)",
  args: {},
  async execute() {
    const commands = await apiRequest("/command")
    if (!commands || commands.length === 0) {
      return "No commands found in the library."
    }
    return commands.map((c: { name: string; description?: string }) =>
      `- /${c.name}: ${c.description || "(no description)"}`
    ).join("\n")
  },
})

export const get_command = tool({
  description: "Get the full content of a command by name",
  args: {
    name: tool.schema.string().describe("The command name (without the leading /)"),
  },
  async execute(args) {
    const command = await apiRequest(`/command/${encodeURIComponent(args.name)}`)
    let result = `# Command: /${command.name}\n\n`
    result += `**Path:** ${command.path}\n`
    if (command.description) result += `**Description:** ${command.description}\n`
    result += `\n## Content\n\n${command.content}`
    return result
  },
})

export const save_command = tool({
  description: "Create or update a command. Provide the full markdown content including YAML frontmatter.",
  args: {
    name: tool.schema.string().describe("The command name (without the leading /)"),
    content: tool.schema.string().describe("Full markdown content with YAML frontmatter (description, model, subtask, agent)"),
  },
  async execute(args) {
    await apiRequest(`/command/${encodeURIComponent(args.name)}`, {
      method: "PUT",
      body: JSON.stringify({ content: args.content }),
    })
    return `Command '/${args.name}' saved successfully. Remember to commit and push your changes.`
  },
})

export const delete_command = tool({
  description: "Delete a command from the library",
  args: {
    name: tool.schema.string().describe("The command name to delete (without the leading /)"),
  },
  async execute(args) {
    await apiRequest(`/command/${encodeURIComponent(args.name)}`, {
      method: "DELETE",
    })
    return `Command '/${args.name}' deleted. Remember to commit and push your changes.`
  },
})

// ─────────────────────────────────────────────────────────────────────────────
// Library Tools
// ─────────────────────────────────────────────────────────────────────────────

export const list_tools = tool({
  description: "List all custom tools in the library (TypeScript tool definitions)",
  args: {},
  async execute() {
    const tools = await apiRequest("/tool")
    if (!tools || tools.length === 0) {
      return "No custom tools found in the library."
    }
    return tools.map((t: { name: string; description?: string }) =>
      `- ${t.name}: ${t.description || "(no description)"}`
    ).join("\n")
  },
})

export const get_tool = tool({
  description: "Get the full TypeScript code of a custom tool by name",
  args: {
    name: tool.schema.string().describe("The tool name"),
  },
  async execute(args) {
    const t = await apiRequest(`/tool/${encodeURIComponent(args.name)}`)
    let result = `# Tool: ${t.name}\n\n`
    result += `**Path:** ${t.path}\n`
    if (t.description) result += `**Description:** ${t.description}\n`
    result += `\n## Code\n\n\`\`\`typescript\n${t.content}\n\`\`\``
    return result
  },
})

export const save_tool = tool({
  description: "Create or update a custom tool in the library. Provide TypeScript code using the @opencode-ai/plugin tool() helper.",
  args: {
    name: tool.schema.string().describe("The tool name"),
    content: tool.schema.string().describe("Full TypeScript code for the tool"),
  },
  async execute(args) {
    await apiRequest(`/tool/${encodeURIComponent(args.name)}`, {
      method: "PUT",
      body: JSON.stringify({ content: args.content }),
    })
    return `Tool '${args.name}' saved successfully. Remember to commit and push your changes.`
  },
})

export const delete_tool = tool({
  description: "Delete a custom tool from the library",
  args: {
    name: tool.schema.string().describe("The tool name to delete"),
  },
  async execute(args) {
    await apiRequest(`/tool/${encodeURIComponent(args.name)}`, {
      method: "DELETE",
    })
    return `Tool '${args.name}' deleted. Remember to commit and push your changes.`
  },
})

// ─────────────────────────────────────────────────────────────────────────────
// Rules
// ─────────────────────────────────────────────────────────────────────────────

export const list_rules = tool({
  description: "List all rules in the library (reusable instruction sets for agents)",
  args: {},
  async execute() {
    const rules = await apiRequest("/rule")
    if (!rules || rules.length === 0) {
      return "No rules found in the library."
    }
    return rules.map((r: { name: string; description?: string }) =>
      `- ${r.name}: ${r.description || "(no description)"}`
    ).join("\n")
  },
})

export const get_rule = tool({
  description: "Get the full content of a rule by name",
  args: {
    name: tool.schema.string().describe("The rule name"),
  },
  async execute(args) {
    const rule = await apiRequest(`/rule/${encodeURIComponent(args.name)}`)
    let result = `# Rule: ${rule.name}\n\n`
    result += `**Path:** ${rule.path}\n`
    if (rule.description) result += `**Description:** ${rule.description}\n`
    result += `\n## Content\n\n${rule.content}`
    return result
  },
})

export const save_rule = tool({
  description: "Create or update a rule in the library. Provide markdown content with optional YAML frontmatter.",
  args: {
    name: tool.schema.string().describe("The rule name"),
    content: tool.schema.string().describe("Full markdown content, optionally with YAML frontmatter (description)"),
  },
  async execute(args) {
    await apiRequest(`/rule/${encodeURIComponent(args.name)}`, {
      method: "PUT",
      body: JSON.stringify({ content: args.content }),
    })
    return `Rule '${args.name}' saved successfully. Remember to commit and push your changes.`
  },
})

export const delete_rule = tool({
  description: "Delete a rule from the library",
  args: {
    name: tool.schema.string().describe("The rule name to delete"),
  },
  async execute(args) {
    await apiRequest(`/rule/${encodeURIComponent(args.name)}`, {
      method: "DELETE",
    })
    return `Rule '${args.name}' deleted. Remember to commit and push your changes.`
  },
})
