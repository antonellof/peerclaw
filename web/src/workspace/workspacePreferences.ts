const STORAGE_KEY = "peerclaw_workspace_prefs_v1"

export type WorkspaceChatPreferences = {
  model: string
  temperature: number
  maxTokens: number
  distributed: boolean
}

const DEFAULTS: WorkspaceChatPreferences = {
  model: "llama-3.2-3b",
  temperature: 0.7,
  maxTokens: 500,
  distributed: false,
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
