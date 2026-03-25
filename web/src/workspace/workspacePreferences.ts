const STORAGE_KEY = "peerclaw_workspace_prefs_v1"

export type WorkspaceChatPreferences = {
  model: string
  temperature: number
  maxTokens: number
  distributed: boolean
  /** When true (default), chat uses the node ToolRegistry ReAct loop (job_submit, shell, …). */
  useAgentic: boolean
  /** When true, chat and agent-goal tasks send `use_mcp` so the node runs the MCP tool loop. */
  useMcp: boolean
}

const DEFAULTS: WorkspaceChatPreferences = {
  model: "llama-3.2-3b",
  temperature: 0.7,
  maxTokens: 500,
  distributed: false,
  useAgentic: true,
  useMcp: false,
}

export function loadWorkspaceChatPreferences(): WorkspaceChatPreferences {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULTS }
    const j = JSON.parse(raw) as Record<string, unknown>
    return {
      model: typeof j.model === "string" && j.model ? j.model : DEFAULTS.model,
      temperature:
        typeof j.temperature === "number" && Number.isFinite(j.temperature) ? j.temperature : DEFAULTS.temperature,
      maxTokens: typeof j.maxTokens === "number" && Number.isFinite(j.maxTokens) ? j.maxTokens : DEFAULTS.maxTokens,
      distributed: Boolean(j.distributed),
      useAgentic: typeof j.useAgentic === "boolean" ? j.useAgentic : DEFAULTS.useAgentic,
      useMcp: Boolean(j.useMcp),
    }
  } catch {
    return { ...DEFAULTS }
  }
}

export function persistWorkspaceChatPreferences(p: WorkspaceChatPreferences) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(p))
  } catch {
    /* quota / private mode */
  }
}
