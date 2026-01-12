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
// Skills
// ─────────────────────────────────────────────────────────────────────────────

export const list_skills = tool({
  description: "List all skills in the library with their names and descriptions",
  args: {},
  async execute() {
    const skills = await apiRequest("/skill")
    if (!skills || skills.length === 0) {
      return "No skills found in the library."
    }
    return skills.map((s: { name: string; description?: string }) =>
      `- ${s.name}: ${s.description || "(no description)"}`
    ).join("\n")
  },
})

export const get_skill = tool({
  description: "Get the full content of a skill by name, including SKILL.md and any additional files",
  args: {
    name: tool.schema.string().describe("The skill name (e.g., 'git-release')"),
  },
  async execute(args) {
    const skill = await apiRequest(`/skill/${encodeURIComponent(args.name)}`)
    let result = `# Skill: ${skill.name}\n\n`
    result += `**Path:** ${skill.path}\n`
    if (skill.description) {
      result += `**Description:** ${skill.description}\n`
    }
    result += `\n## SKILL.md Content\n\n${skill.content}`

    if (skill.files && skill.files.length > 0) {
      result += "\n\n## Additional Files\n"
      for (const file of skill.files) {
        result += `\n### ${file.path}\n\n${file.content}`
      }
    }

    if (skill.references && skill.references.length > 0) {
      result += "\n\n## Reference Files\n"
      result += skill.references.map((r: string) => `- ${r}`).join("\n")
    }

    return result
  },
})

export const save_skill = tool({
  description: "Create or update a skill in the library. Provide the full SKILL.md content including YAML frontmatter.",
  args: {
    name: tool.schema.string().describe("The skill name (lowercase, hyphens allowed, 1-64 chars)"),
    content: tool.schema.string().describe("Full SKILL.md content including YAML frontmatter with name and description"),
  },
  async execute(args) {
    await apiRequest(`/skill/${encodeURIComponent(args.name)}`, {
      method: "PUT",
      body: JSON.stringify({ content: args.content }),
    })
    return `Skill '${args.name}' saved successfully. Remember to commit and push your changes.`
  },
})

export const delete_skill = tool({
  description: "Delete a skill from the library",
  args: {
    name: tool.schema.string().describe("The skill name to delete"),
  },
  async execute(args) {
    await apiRequest(`/skill/${encodeURIComponent(args.name)}`, {
      method: "DELETE",
    })
    return `Skill '${args.name}' deleted. Remember to commit and push your changes.`
  },
})
