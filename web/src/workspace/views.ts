export type WorkspaceView = "chat" | "home" | "overview" | "jobs" | "providers" | "skills" | "mcp"

export const WORKSPACE_VIEWS: WorkspaceView[] = [
  "chat",
  "home",
  "overview",
  "jobs",
  "providers",
  "skills",
  "mcp",
]

export function parseWorkspaceView(raw: string | null): WorkspaceView {
  if (raw === "tasks" || raw === "agent") return "chat"
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
  "": "home",
  home: "home",
  tasks: "chat",
  agent: "chat",
  overview: "overview",
  jobs: "jobs",
  providers: "providers",
  skills: "skills",
  mcp: "mcp",
}
