export type WorkspaceView =
  | "chat"
  | "help"
  | "overview"
  | "jobs"
  | "providers"
  | "skills"
  | "tools"
  | "mcp"
  | "workflows"

export const WORKSPACE_VIEWS: WorkspaceView[] = [
  "chat",
  "help",
  "overview",
  "jobs",
  "providers",
  "skills",
  "tools",
  "mcp",
  "workflows",
]

export function parseWorkspaceView(raw: string | null): WorkspaceView {
  if (raw === "tasks" || raw === "agent") return "chat"
  if (raw === "agent-builder" || raw === "agent_builder" || raw === "crews") return "workflows"
  /** Legacy Home panel → full Help page. */
  if (raw === "home") return "help"
  /** Legacy sidebar URL; shell redirects to P2P Network with hash. */
  if (raw === "join") return "overview"
  if (raw && WORKSPACE_VIEWS.includes(raw as WorkspaceView)) return raw as WorkspaceView
  return "chat"
}

/** URL for a workspace panel. Chat uses a clean `/` with no query. */
export function workspaceHref(view: WorkspaceView, hash?: string): string {
  const h = hash ? (hash.startsWith("#") ? hash : `#${hash}`) : ""
  if (view === "chat") return `/${h}`
  return `/?view=${view}${h}`
}

export const LEGACY_CONSOLE_REDIRECT: Record<string, WorkspaceView> = {
  "": "chat",
  home: "help",
  tasks: "chat",
  agent: "chat",
  overview: "overview",
  join: "overview",
  jobs: "jobs",
  providers: "providers",
  skills: "skills",
  mcp: "mcp",
  crews: "workflows",
  "agent-builder": "workflows",
}
