const STORAGE_KEY = "peerclaw_workspace_prefs_v2"
const LEGACY_STORAGE_KEY = "peerclaw_workspace_prefs_v1"

export type WorkspaceChatPreferences = {
  model: string
  temperature: number
  maxTokens: number
  distributed: boolean
  /** When true (default), chat uses the node ToolRegistry ReAct loop (job_submit, shell, …). */
  useAgentic: boolean
  /** When true, chat and agent-goal tasks send `use_mcp` so the node runs the MCP tool loop. */
  useMcp: boolean
  /**
   * When set, messages run that saved agent: `flow` → `/api/flows/kickoff`, `task` → `/api/tasks`.
   * Built-ins and Agent builder saves live in the node `agent_library.json`.
   */
  selectedAgentLibraryId: string | null
}

const DEFAULTS: WorkspaceChatPreferences = {
  model: "llama-3.2-3b",
  temperature: 0.7,
  maxTokens: 500,
  distributed: false,
  useAgentic: true,
  useMcp: false,
  selectedAgentLibraryId: null,
}

function parsePrefs(raw: string): WorkspaceChatPreferences {
  const j = JSON.parse(raw) as Record<string, unknown>
  return {
    model: typeof j.model === "string" && j.model ? j.model : DEFAULTS.model,
    temperature:
      typeof j.temperature === "number" && Number.isFinite(j.temperature) ? j.temperature : DEFAULTS.temperature,
    maxTokens: typeof j.maxTokens === "number" && Number.isFinite(j.maxTokens) ? j.maxTokens : DEFAULTS.maxTokens,
    distributed: Boolean(j.distributed),
    useAgentic: typeof j.useAgentic === "boolean" ? j.useAgentic : DEFAULTS.useAgentic,
    useMcp: Boolean(j.useMcp),
    selectedAgentLibraryId:
      typeof j.selectedAgentLibraryId === "string" && j.selectedAgentLibraryId
        ? j.selectedAgentLibraryId
        : null,
  }
}

export function loadWorkspaceChatPreferences(): WorkspaceChatPreferences {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) {
      const legacy = localStorage.getItem(LEGACY_STORAGE_KEY)
      if (legacy) {
        try {
          const merged = { ...parsePrefs(legacy), selectedAgentLibraryId: null as string | null }
          localStorage.setItem(STORAGE_KEY, JSON.stringify(merged))
          return merged
        } catch {
          /* fall through */
        }
      }
      return { ...DEFAULTS }
    }
    return parsePrefs(raw)
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
