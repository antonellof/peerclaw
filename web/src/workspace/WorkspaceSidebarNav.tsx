/* eslint-disable react-refresh/only-export-components -- nav config + helper exported for shell title */
import { Home, LayoutGrid, MessageSquare } from "lucide-react"

import { cn } from "@/lib/utils"
import type { WorkspaceView } from "./views"

/** Primary sidebar destinations; Jobs, Providers, Skills are under Settings. */
export const WORKSPACE_NAV_ITEMS: { view: WorkspaceView; label: string; icon: typeof MessageSquare }[] = [
  { view: "chat", label: "Chat", icon: MessageSquare },
  { view: "home", label: "Home", icon: Home },
  { view: "overview", label: "Overview", icon: LayoutGrid },
]

export const WORKSPACE_VIEW_TITLES: Record<WorkspaceView, string> = {
  chat: "Chat",
  home: "Home",
  overview: "Overview",
  jobs: "Jobs",
  providers: "Providers",
  skills: "Skills",
  mcp: "MCP",
}

type Props = {
  active: WorkspaceView
  onSelect: (v: WorkspaceView) => void
  sidebarCollapsed?: boolean
  onPick?: () => void
}

export function WorkspaceSidebarNav({ active, onSelect, sidebarCollapsed, onPick }: Props) {
  return (
    <nav className="flex flex-col gap-0.5 p-2" aria-label="Workspace">
      {WORKSPACE_NAV_ITEMS.map(({ view: v, label, icon: Icon }) => (
        <button
          key={v}
          type="button"
          onClick={() => {
            onSelect(v)
            onPick?.()
          }}
          className={cn(
            "flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm transition-colors",
            active === v
              ? "bg-secondary text-foreground"
              : "text-muted-foreground hover:bg-muted/80 hover:text-foreground",
            sidebarCollapsed && "justify-center px-0",
          )}
          title={sidebarCollapsed ? label : undefined}
        >
          <Icon className="size-[18px] shrink-0 opacity-90" />
          {!sidebarCollapsed && <span className="truncate">{label}</span>}
        </button>
      ))}
    </nav>
  )
}

export function workspaceNavTitle(view: WorkspaceView): string {
  return WORKSPACE_VIEW_TITLES[view] ?? "PeerClaw"
}
