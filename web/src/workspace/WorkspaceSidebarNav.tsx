/* eslint-disable react-refresh/only-export-components -- nav config + helper exported for shell title */
import { BookOpen, Boxes, Cpu, LayoutGrid, MessageSquare, Plug, Wrench, Workflow } from "lucide-react"

import { cn } from "@/lib/utils"
import type { WorkspaceView } from "./views"

type NavItem = { view: WorkspaceView; label: string; icon: typeof MessageSquare }

/** Primary sidebar destinations. */
const MAIN_NAV: NavItem[] = [
  { view: "chat", label: "Chat", icon: MessageSquare },
  { view: "workflows", label: "Agents", icon: Workflow },
  { view: "overview", label: "P2P Network", icon: LayoutGrid },
]

/** Secondary sidebar destinations (below separator). */
const SECONDARY_NAV: NavItem[] = [
  { view: "tools", label: "Tools", icon: Wrench },
  { view: "skills", label: "Skills", icon: Boxes },
  { view: "mcp", label: "MCP", icon: Plug },
  { view: "providers", label: "Providers", icon: Cpu },
  { view: "help", label: "Help", icon: BookOpen },
]

export const WORKSPACE_NAV_ITEMS: NavItem[] = [...MAIN_NAV, ...SECONDARY_NAV]

export const WORKSPACE_VIEW_TITLES: Record<WorkspaceView, string> = {
  chat: "Chat",
  help: "Help",
  overview: "P2P Network",
  jobs: "Jobs",
  providers: "Providers",
  skills: "Skills",
  tools: "Tools",
  mcp: "MCP",
  workflows: "Agents",
}

type Props = {
  active: WorkspaceView
  onSelect: (v: WorkspaceView) => void
  sidebarCollapsed?: boolean
  onPick?: () => void
}

function NavButton({ v, label, icon: Icon, active, onSelect, sidebarCollapsed, onPick }: { v: WorkspaceView; label: string; icon: typeof MessageSquare } & Props) {
  return (
    <button
      key={v}
      type="button"
      onClick={() => { onSelect(v); onPick?.() }}
      className={cn(
        "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors",
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
  )
}

export function WorkspaceSidebarNav({ active, onSelect, sidebarCollapsed, onPick }: Props) {
  return (
    <nav className="flex flex-col gap-0.5 p-2" aria-label="Workspace">
      {MAIN_NAV.map(({ view: v, label, icon }) => (
        <NavButton key={v} v={v} label={label} icon={icon} active={active} onSelect={onSelect} sidebarCollapsed={sidebarCollapsed} onPick={onPick} />
      ))}
      {!sidebarCollapsed && (
        <div className="mx-3 my-1.5 border-t border-border/50" />
      )}
      {SECONDARY_NAV.map(({ view: v, label, icon }) => (
        <NavButton key={v} v={v} label={label} icon={icon} active={active} onSelect={onSelect} sidebarCollapsed={sidebarCollapsed} onPick={onPick} />
      ))}
    </nav>
  )
}

export function workspaceNavTitle(view: WorkspaceView): string {
  return WORKSPACE_VIEW_TITLES[view] ?? "PeerClaw"
}
